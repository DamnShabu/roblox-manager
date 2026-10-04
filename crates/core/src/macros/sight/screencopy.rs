//! Copying an area of a nested cage's frame out: wlr-screencopy, which cage
//! offers to any client of its display, into shared memory of our own.
//!
//! A hidden window's cage draws nothing on its own -- the window relay keeps
//! the desktop's frame callbacks from it, since nobody sees what it draws --
//! so every copy asks the window relay for one frame as well
//! ([`cage_window::draw`]). The same holds for a window the desktop has put
//! out of sight on another workspace, which it stops sending frames to.
//!
//! Spoken on the wire, like [`super::super::wayland`]: a handful of fixed
//! messages.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, IoSlice, Read};
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{Area, Eyes, Image};
use crate::macros::cage_window;
use crate::macros::wire::{DISPLAY, Framer, header, message, read_str, wire_str, word, words};

/// How long a frame may take to come before the copy is given up on.
const FRAME_WAIT: Duration = Duration::from_secs(1);

/// wl_shm formats a copy may come in: 32 bits a pixel, little-endian.
const ARGB8888: u32 = 0;
const XRGB8888: u32 = 1;
const ABGR8888: u32 = 0x3432_4241;
const XBGR8888: u32 = 0x3432_4258;
const ARGB2101010: u32 = 0x3033_5241;
const XRGB2101010: u32 = 0x3033_5258;
const ABGR2101010: u32 = 0x3033_4241;
const XBGR2101010: u32 = 0x3033_4258;

/// zwlr_screencopy_frame_v1 events.
const BUFFER: u16 = 0;
const FLAGS: u16 = 1;
const READY: u16 = 2;
const FAILED: u16 = 3;
const BUFFER_DONE: u16 = 6;
/// Its flags: the copy is upside down.
const Y_INVERT: u32 = 1;

/// One display's screencopy, and the shared memory its copies land in.
pub struct Screencopy {
    sock: UnixStream,
    framer: Framer,
    last_id: u32,
    display_file: PathBuf,
    shm: u32,
    output: u32,
    manager: u32,
    version: u32,
    pool: Option<Pool>,
}

/// Shared memory, and the buffer made in it for copies of one size.
struct Pool {
    file: File,
    pool: u32,
    buffer: u32,
    shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shape {
    format: u32,
    width: u32,
    height: u32,
    stride: u32,
}

impl Screencopy {
    /// Connect to the display at `display_file`. A compositor that cannot
    /// copy its frame out says so here, not when a `when` first looks.
    pub fn connect(display_file: &Path) -> io::Result<Self> {
        let mut sc = Screencopy {
            sock: UnixStream::connect(display_file)?,
            framer: Framer::default(),
            last_id: DISPLAY,
            display_file: display_file.to_owned(),
            shm: 0,
            output: 0,
            manager: 0,
            version: 1,
            pool: None,
        };
        let registry = sc.new_id();
        sc.send(DISPLAY, 1, &words(&[registry]))?; // wl_display.get_registry
        // wl_registry.global: interface, (name, version); the first output.
        let mut offered: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for (obj, op, body) in sc.roundtrip()? {
            if obj != registry || op != 0 {
                continue;
            }
            let Some(iface) = read_str(&body, 4) else { continue };
            let padded = (word(&body, 4) as usize + 3) & !3;
            offered.entry(iface).or_insert((word(&body, 0), word(&body, 8 + padded)));
        }
        let bind = |sc: &mut Screencopy, iface: &str, most: u32| -> io::Result<(u32, u32)> {
            let &(name, version) = offered
                .get(iface)
                .ok_or_else(|| refused(&format!("its display offers no {iface}")))?;
            let version = version.min(most);
            let id = sc.new_id();
            let mut body = words(&[name]);
            body.extend(wire_str(iface));
            body.extend(words(&[version, id]));
            sc.send(registry, 0, &body)?; // wl_registry.bind
            Ok((id, version))
        };
        sc.shm = bind(&mut sc, "wl_shm", 1)?.0;
        sc.output = bind(&mut sc, "wl_output", 1)?.0;
        (sc.manager, sc.version) = bind(&mut sc, "zwlr_screencopy_manager_v1", 3)?;
        sc.roundtrip()?; // a refusal shows up here
        Ok(sc)
    }

    fn new_id(&mut self) -> u32 {
        self.last_id += 1;
        self.last_id
    }

    fn send(&mut self, obj: u32, op: u16, body: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.sock.write_all(&message(obj, op, body))
    }

    /// The next event, or None once `deadline` passes; a protocol error is
    /// raised.
    fn event(&mut self, deadline: Instant) -> io::Result<Option<(u32, u16, Vec<u8>)>> {
        loop {
            let malformed = |_| refused("its display sent a malformed message");
            if let Some(msg) = self.framer.next().map_err(malformed)? {
                let (obj, op) = header(&msg);
                let body = msg.get(8..).unwrap_or_default().to_vec();
                if obj == DISPLAY && op == 0 {
                    // wl_display.error
                    let why = read_str(&body, 8).unwrap_or_default();
                    return Err(refused(&format!("its display refused a copy: {why}")));
                }
                return Ok(Some((obj, op, body)));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.sock.set_read_timeout(Some(left))?;
            let mut data = [0u8; 4096];
            match self.sock.read(&mut data) {
                Ok(0) => {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "its display closed"));
                }
                Ok(n) => self.framer.push(&data[..n]),
                Err(e)
                    if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
                {
                    return Ok(None);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Every event before the display has handled all that was sent.
    fn roundtrip(&mut self) -> io::Result<Vec<(u32, u16, Vec<u8>)>> {
        let done = self.new_id();
        self.send(DISPLAY, 0, &words(&[done]))?; // wl_display.sync
        let deadline = Instant::now() + FRAME_WAIT * 5;
        let mut events = Vec::new();
        while let Some(ev) = self.event(deadline)? {
            if ev.0 == done {
                return Ok(events);
            }
            events.push(ev);
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "its display did not answer"))
    }

    /// A buffer of `shape`: the one there is, or a new pool for it.
    fn buffer(&mut self, shape: Shape) -> io::Result<u32> {
        if let Some(p) = self.pool.as_ref().filter(|p| p.shape == shape) {
            return Ok(p.buffer);
        }
        if let Some(old) = self.pool.take() {
            self.send(old.buffer, 0, &[])?; // wl_buffer.destroy
            self.send(old.pool, 1, &[])?; // wl_shm_pool.destroy
        }
        let size =
            shape.stride.checked_mul(shape.height).filter(|s| *s > 0 && *s < i32::MAX as u32);
        let size = size.ok_or_else(|| refused("its display offered a copy of no size"))?;
        let fd = rustix::fs::memfd_create("rbxmgr-sight", rustix::fs::MemfdFlags::CLOEXEC)?;
        let file = File::from(fd);
        file.set_len(u64::from(size))?;
        let (pool, buffer) = (self.new_id(), self.new_id());
        self.send_fd(self.shm, 0, &words(&[pool, size]), &file)?; // wl_shm.create_pool
        let body = words(&[buffer, 0, shape.width, shape.height, shape.stride, shape.format]);
        self.send(pool, 0, &body)?; // wl_shm_pool.create_buffer
        self.pool = Some(Pool { file, pool, buffer, shape });
        Ok(buffer)
    }

    /// A request with a file descriptor passed alongside it.
    fn send_fd(&mut self, obj: u32, op: u16, body: &[u8], file: &File) -> io::Result<()> {
        use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
        let msg = message(obj, op, body);
        let fds = [file.as_fd()];
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        control.push(SendAncillaryMessage::ScmRights(&fds));
        sendmsg(&self.sock, &[IoSlice::new(&msg)], &mut control, SendFlags::NOSIGNAL)?;
        Ok(())
    }

    /// One copy of `area`, start to end, on the frame object `frame`.
    fn copy(&mut self, frame: u32, area: Area, deadline: Instant) -> io::Result<Image> {
        // The buffers it may be copied into, then (from version 3) a done.
        let mut shape = None;
        loop {
            let Some((obj, op, body)) = self.event(deadline)? else { return Err(late()) };
            if obj != frame {
                continue;
            }
            match op {
                BUFFER => {
                    shape = Some(Shape {
                        format: word(&body, 0),
                        width: word(&body, 4),
                        height: word(&body, 8),
                        stride: word(&body, 12),
                    });
                    if self.version < 3 {
                        break;
                    }
                }
                BUFFER_DONE => break,
                FAILED => return Err(refused("its display could not copy that area")),
                _ => {}
            }
        }
        let shape = shape.ok_or_else(|| refused("its display offered nothing to copy into"))?;
        let buffer = self.buffer(shape)?;
        self.send(frame, 0, &words(&[buffer]))?; // copy
        // Asked after the copy, so the frame it draws is the one copied.
        cage_window::draw(&self.display_file)?;
        let mut flags = 0;
        loop {
            let Some((obj, op, body)) = self.event(deadline)? else { return Err(late()) };
            if obj != frame {
                continue;
            }
            match op {
                FLAGS => flags = word(&body, 0),
                READY => break,
                FAILED => return Err(refused("its display could not copy that area")),
                _ => {}
            }
        }
        let pool = self.pool.as_ref().ok_or_else(|| refused("the copy had nowhere to land"))?;
        let mut raw = vec![0u8; (shape.stride * shape.height) as usize];
        pool.file.read_exact_at(&mut raw, 0)?;
        let image = to_rgb(&raw, shape, flags & Y_INVERT != 0)?;
        Ok(fit(image, area))
    }
}

impl Eyes for Screencopy {
    fn look(&mut self, area: Area) -> io::Result<Image> {
        let area = area.on_display();
        if area.w == 0 || area.h == 0 {
            return Err(refused("that area is off the display"));
        }
        let frame = self.new_id();
        // capture_output_region(frame, overlay_cursor, output, x, y, w, h):
        // without the pointer, which is not part of the game.
        let body = words(&[frame, 0, self.output, area.x as u32, area.y as u32, area.w, area.h]);
        self.send(self.manager, 1, &body)?;
        let copied = self.copy(frame, area, Instant::now() + FRAME_WAIT);
        self.send(frame, 1, &[])?; // destroy
        copied
    }
}

impl Drop for Screencopy {
    /// Let go of the shared memory and the manager. Past failing is nothing
    /// to report: the connection closing frees the lot.
    fn drop(&mut self) {
        if let Some(p) = self.pool.take() {
            let _ = self.send(p.buffer, 0, &[]);
            let _ = self.send(p.pool, 1, &[]);
        }
        let _ = self.send(self.manager, 2, &[]);
    }
}

/// A copy, as red, green, blue bytes the right way up.
fn to_rgb(raw: &[u8], shape: Shape, flipped: bool) -> io::Result<Image> {
    let (w, h) = (shape.width as usize, shape.height as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for row in 0..h {
        let src = if flipped { h - 1 - row } else { row };
        let start = src * shape.stride as usize;
        let line = raw.get(start..start + w * 4).ok_or_else(|| refused("the copy came short"))?;
        for px in line.chunks_exact(4) {
            let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
            let (r, g, b) = match shape.format {
                ARGB8888 | XRGB8888 => (v >> 16, v >> 8, v),
                ABGR8888 | XBGR8888 => (v, v >> 8, v >> 16),
                ARGB2101010 | XRGB2101010 => (v >> 22, v >> 12, v >> 2),
                ABGR2101010 | XBGR2101010 => (v >> 2, v >> 12, v >> 22),
                other => return Err(refused(&format!("its display copies in format {other:#x}"))),
            };
            rgb.extend_from_slice(&[r as u8, g as u8, b as u8]);
        }
    }
    Ok(Image { width: shape.width, height: shape.height, rgb })
}

/// A copy as the area asked for: the display clips an area running off its
/// right or bottom edge, and the copy is the part on it -- which reads the
/// same from the top-left corner. One that comes bigger is a display drawn
/// at a scale, sampled back down to its own units.
fn fit(image: Image, area: Area) -> Image {
    if image.width <= area.w && image.height <= area.h {
        return image;
    }
    let (w, h) = (area.w.min(image.width), area.h.min(image.height));
    let (sx, sy) =
        (f64::from(image.width) / f64::from(area.w), f64::from(image.height) / f64::from(area.h));
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for y in 0..h {
        for x in 0..w {
            let px = (f64::from(x) * sx) as i64;
            let py = (f64::from(y) * sy) as i64;
            rgb.extend_from_slice(&image.pixel(px, py).unwrap_or_default());
        }
    }
    Image { width: w, height: h, rgb }
}

fn late() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "its window drew no frame to look at")
}

/// The display answered, but not as needed.
fn refused(why: &str) -> io::Error {
    io::Error::other(why.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_in_either_byte_order_read_as_rgb_the_right_way_up() {
        let shape = |format| Shape { format, width: 1, height: 2, stride: 4 };
        // Two pixels, a row each: red over blue, as XRGB8888 puts them.
        let xrgb = [0, 0, 255, 0, 255, 0, 0, 0];
        assert_eq!(to_rgb(&xrgb, shape(XRGB8888), false).unwrap().rgb, [255, 0, 0, 0, 0, 255]);
        assert_eq!(to_rgb(&xrgb, shape(XRGB8888), true).unwrap().rgb, [0, 0, 255, 255, 0, 0]);
        let xbgr = [255, 0, 0, 0, 0, 0, 255, 0];
        assert_eq!(to_rgb(&xbgr, shape(XBGR8888), false).unwrap().rgb, [255, 0, 0, 0, 0, 255]);
        let red_10bit = (1023u32 << 20).to_le_bytes();
        let one = Shape { format: XRGB2101010, width: 1, height: 1, stride: 4 };
        assert_eq!(to_rgb(&red_10bit, one, false).unwrap().rgb, [255, 0, 0]);
        let odd = Shape { format: 0x1234, width: 1, height: 1, stride: 4 };
        assert!(to_rgb(&[0; 4], odd, false).is_err());
    }

    #[test]
    fn a_copy_drawn_at_twice_the_scale_is_sampled_back_down() {
        let image = Image { width: 4, height: 2, rgb: (0..24).collect() };
        let got = fit(image.clone(), Area { x: 0, y: 0, w: 2, h: 1 });
        assert_eq!((got.width, got.height), (2, 1));
        assert_eq!(got.rgb, [0, 1, 2, 6, 7, 8]);
        // Clipped at the display's edge: as it came.
        assert_eq!(fit(image.clone(), Area { x: 0, y: 0, w: 9, h: 9 }), image);
    }
}
