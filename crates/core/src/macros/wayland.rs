//! A virtual keyboard and pointer on one Wayland display: a nested cage.
//!
//! Not wtype and wlrctl: Roblox's in-game key path takes the raw evdev
//! keycode, and wtype makes up a keymap per run that numbers keys in the order
//! it meets them -- so `tap j` went out as code 1, Escape, and opened Roblox's
//! menu. This uploads a real US keymap once and presses the real code.
//!
//! It speaks the Wayland wire protocol itself: a handful of fixed messages,
//! less than a binding library would be to ship.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IoSlice, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use super::keys;
use super::player::Input;

/// Sent with the virtual keyboard. The compositor compiles it, so the
/// includes resolve against its own xkeyboard-config.
const KEYMAP: &str = "xkb_keymap {
  xkb_keycodes { include \"evdev+aliases(qwerty)\" };
  xkb_types { include \"complete\" };
  xkb_compat { include \"complete\" };
  xkb_symbols { include \"pc+us+inet(evdev)\" };
};
";

const DISPLAY: u32 = 1;

pub struct VirtualInput {
    sock: UnixStream,
    buf: Vec<u8>,
    last_id: u32,
    keyboard: u32,
    pointer: u32,
    mods: BTreeSet<u16>,
    epoch: Instant,
}

impl VirtualInput {
    /// Connect to the display at `path` and set up a keyboard and pointer on
    /// it. A compositor that refuses either says so here, not mid-macro.
    pub fn connect(path: &Path) -> io::Result<Self> {
        let sock = UnixStream::connect(path)?;
        let mut vi = VirtualInput {
            sock,
            buf: Vec::new(),
            last_id: DISPLAY,
            keyboard: 0,
            pointer: 0,
            mods: BTreeSet::new(),
            epoch: Instant::now(),
        };
        let registry = vi.new_id();
        vi.send(DISPLAY, 1, &words(&[registry]))?; // wl_display.get_registry
        let offered: BTreeMap<String, u32> = vi
            .roundtrip()?
            .into_iter()
            .filter(|(obj, op, _)| *obj == registry && *op == 0) // wl_registry.global
            .filter_map(|(_, _, body)| Some((read_str(&body, 4)?, word(&body, 0))))
            .collect();
        let bind = |vi: &mut VirtualInput, iface: &str| -> io::Result<u32> {
            let name = *offered
                .get(iface)
                .ok_or_else(|| refused(&format!("its display offers no {iface}")))?;
            let id = vi.new_id();
            let mut body = words(&[name]);
            body.extend(wire_str(iface));
            body.extend(words(&[1, id]));
            vi.send(registry, 0, &body)?; // wl_registry.bind
            Ok(id)
        };
        let seat = bind(&mut vi, "wl_seat")?;
        let keyboards = bind(&mut vi, "zwp_virtual_keyboard_manager_v1")?;
        let pointers = bind(&mut vi, "zwlr_virtual_pointer_manager_v1")?;
        vi.keyboard = vi.new_id();
        vi.pointer = vi.new_id();
        vi.send(keyboards, 0, &words(&[seat, vi.keyboard]))?; // create_virtual_keyboard
        vi.send(pointers, 0, &words(&[seat, vi.pointer]))?; // create_virtual_pointer
        vi.upload_keymap()?;
        vi.roundtrip()?; // a refusal shows up here, not mid-macro
        Ok(vi)
    }

    fn new_id(&mut self) -> u32 {
        self.last_id += 1;
        self.last_id
    }

    fn send(&mut self, obj: u32, op: u16, body: &[u8]) -> io::Result<()> {
        self.sock.write_all(&message(obj, op, body))
    }

    /// The keymap (xkb v1), in a memfd passed alongside the request.
    fn upload_keymap(&mut self) -> io::Result<()> {
        use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
        let keymap = [KEYMAP.as_bytes(), b"\0"].concat();
        let fd = rustix::fs::memfd_create("rbxmgr-keymap", rustix::fs::MemfdFlags::CLOEXEC)?;
        let mut file = std::fs::File::from(fd);
        file.write_all(&keymap)?;
        let msg = message(self.keyboard, 0, &words(&[1, keymap.len() as u32]));
        let fds = [file.as_fd()];
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        control.push(SendAncillaryMessage::ScmRights(&fds));
        sendmsg(&self.sock, &[IoSlice::new(&msg)], &mut control, SendFlags::empty())?;
        Ok(())
    }

    /// Events that have arrived; a protocol error is raised.
    fn pump(&mut self, block: bool) -> io::Result<Vec<(u32, u16, Vec<u8>)>> {
        let mut data = [0u8; 65536];
        self.sock.set_nonblocking(!block)?;
        let got = match self.sock.read(&mut data) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::BrokenPipe, "its display closed")),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => 0,
            Err(e) => return Err(e),
        };
        self.buf.extend_from_slice(&data[..got]);
        let mut events = Vec::new();
        while self.buf.len() >= 8 {
            let (obj, size_op) = (word(&self.buf, 0), word(&self.buf, 4));
            let size = (size_op >> 16) as usize;
            if size < 8 {
                return Err(refused("its display sent a malformed message"));
            }
            if self.buf.len() < size {
                break;
            }
            let body = self.buf[8..size].to_vec();
            self.buf.drain(..size);
            let op = (size_op & 0xffff) as u16;
            if obj == DISPLAY && op == 0 {
                // wl_display.error
                let why = read_str(&body, 8).unwrap_or_default();
                return Err(refused(&format!("its display refused input: {why}")));
            }
            events.push((obj, op, body));
        }
        Ok(events)
    }

    /// Every event before the display has handled all that was sent.
    fn roundtrip(&mut self) -> io::Result<Vec<(u32, u16, Vec<u8>)>> {
        let done = self.new_id();
        self.send(DISPLAY, 0, &words(&[done]))?; // wl_display.sync
        let mut events = Vec::new();
        loop {
            for ev in self.pump(true)? {
                if ev.0 == done {
                    return Ok(events);
                }
                events.push(ev);
            }
        }
    }

    /// A timestamp for an input event, surfacing any error the display sent.
    fn stamp(&mut self) -> io::Result<u32> {
        self.pump(false)?;
        Ok(self.epoch.elapsed().as_millis() as u32)
    }
}

impl Input for VirtualInput {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()> {
        let time = self.stamp()?;
        self.send(self.keyboard, 1, &words(&[time, u32::from(code), u32::from(down)]))?;
        if keys::modifier_mask(code).is_some() {
            // wlroots does not work modifier state out from a virtual
            // keyboard's keys; without this, Shift+h typed "h".
            if down {
                self.mods.insert(code);
            } else {
                self.mods.remove(&code);
            }
            let mask =
                self.mods.iter().filter_map(|c| keys::modifier_mask(*c)).fold(0, |m, b| m | b);
            self.send(self.keyboard, 2, &words(&[mask, 0, 0, 0]))?;
        }
        Ok(())
    }

    fn motion(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        let time = self.stamp()?;
        // wl_fixed: 24.8 fixed point.
        let body = words(&[time, (dx * 256) as u32, (dy * 256) as u32]);
        self.send(self.pointer, 0, &body)?;
        self.send(self.pointer, 4, &[]) // frame
    }

    fn button(&mut self, code: u16, down: bool) -> io::Result<()> {
        let time = self.stamp()?;
        self.send(self.pointer, 2, &words(&[time, u32::from(code), u32::from(down)]))?;
        self.send(self.pointer, 4, &[]) // frame
    }
}

impl Drop for VirtualInput {
    /// Destroy the keyboard, then the pointer, and wait for the display to
    /// have seen it. Past failing is nothing to report.
    fn drop(&mut self) {
        let _ = self.send(self.keyboard, 3, &[]);
        let _ = self.send(self.pointer, 8, &[]);
        let _ = self.roundtrip();
    }
}

/// One request: object, size and opcode, body.
fn message(obj: u32, op: u16, body: &[u8]) -> Vec<u8> {
    let mut msg = words(&[obj, ((8 + body.len() as u32) << 16) | u32::from(op)]);
    msg.extend_from_slice(body);
    msg
}

/// The display answered, but not as needed: an error of its own kind, never
/// mistaken for a socket nobody listens on.
fn refused(why: &str) -> io::Error {
    io::Error::other(why.to_owned())
}

/// Wire words: native-endian u32s.
fn words(vals: &[u32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

/// A wire string: length with its NUL, the bytes, NUL, padded to 4.
fn wire_str(s: &str) -> Vec<u8> {
    let mut out = words(&[s.len() as u32 + 1]);
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// The string at `off` in a message body.
fn read_str(body: &[u8], off: usize) -> Option<String> {
    let n = u32::from_ne_bytes(body.get(off..off + 4)?.try_into().ok()?) as usize;
    let bytes = body.get(off + 4..off + 3 + n)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn word(body: &[u8], off: usize) -> u32 {
    body.get(off..off + 4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_ne_bytes)
}

#[cfg(test)]
mod tests;
