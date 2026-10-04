//! Cage's connections to the desktop, passed through a [`Shade`]: cage's
//! requests and the desktop's events each a whole message at a time, on
//! two threads per connection, and hiding or showing all of them at once.

use std::io;
use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::Instant;

use super::shade::{Out, Shade};
use crate::macros::socket::{BUFFER, recv, send};
use crate::macros::wire::Framer;

/// Every connection cage has open, and whether its windows are hidden.
#[derive(Default)]
pub struct Windows {
    links: Mutex<Vec<Weak<Mutex<Link>>>>,
    hidden: Mutex<bool>,
    epoch: Mutex<Option<Instant>>,
}

/// One connection: what is known of it, and both its ends.
struct Link {
    shade: Shade,
    cage: UnixStream,
    desktop: UnixStream,
    /// File descriptors still to go with the next bytes each way.
    to_cage: Vec<OwnedFd>,
    to_desktop: Vec<OwnedFd>,
}

impl Windows {
    pub fn hidden(&self) -> bool {
        *lock(&self.hidden)
    }

    /// Hide or show every window cage has.
    pub fn set_hidden(&self, hide: bool) {
        let mut hidden = lock(&self.hidden);
        *hidden = hide;
        let now = self.now_ms();
        let mut links = lock(&self.links);
        links.retain(|l| l.strong_count() > 0);
        for link in links.iter().filter_map(Weak::upgrade) {
            let mut link = lock(&link);
            let out = link.shade.set_hidden(hide, now);
            // A connection that has gone takes its windows with it.
            if let Err(e) = link.deliver(out) {
                eprintln!("rbxmgr window relay: a display connection ended: {e}");
            }
        }
    }

    /// Pass cage's connection through to the desktop until either end
    /// closes. Cage makes its window as it starts, so a window is never
    /// made hidden: there is nothing to hide before then.
    pub fn link(self: &Arc<Self>, cage: UnixStream, desktop: UnixStream) -> io::Result<()> {
        let link = Arc::new(Mutex::new(Link {
            shade: Shade::default(),
            cage: cage.try_clone()?,
            desktop: desktop.try_clone()?,
            to_cage: Vec::new(),
            to_desktop: Vec::new(),
        }));
        lock(&self.links).push(Arc::downgrade(&link));
        let (cage_too, desktop_too) = (cage.try_clone()?, desktop.try_clone()?);
        let (windows, link_too) = (Arc::clone(self), Arc::clone(&link));
        thread::spawn(move || {
            let ended = pump(&cage, &link, &windows, Side::Cage);
            close(&cage, &desktop, ended);
        });
        let windows = Arc::clone(self);
        thread::spawn(move || {
            let ended = pump(&desktop_too, &link_too, &windows, Side::Desktop);
            close(&cage_too, &desktop_too, ended);
        });
        Ok(())
    }

    /// Milliseconds on the clock frame callbacks answered here carry.
    fn now_ms(&self) -> u32 {
        let start = *lock(&self.epoch).get_or_insert_with(Instant::now);
        start.elapsed().as_millis() as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Cage,
    Desktop,
}

impl Link {
    /// Each message on to its end, with the file descriptors waiting to go.
    fn deliver(&mut self, out: Vec<Out>) -> io::Result<()> {
        let (mut cage, mut desktop) = (Vec::new(), Vec::new());
        for o in out {
            match o {
                Out::Cage(m) => cage.extend(m),
                Out::Desktop(m) => desktop.extend(m),
            }
        }
        if !desktop.is_empty() {
            send(&self.desktop, &desktop, &mut self.to_desktop)?;
        }
        if !cage.is_empty() {
            send(&self.cage, &cage, &mut self.to_cage)?;
        }
        Ok(())
    }
}

/// Read one side's messages, each through the shade, until it closes. Once
/// the stream cannot be framed it goes on as it comes, unread.
fn pump(from: &UnixStream, link: &Mutex<Link>, windows: &Windows, side: Side) -> io::Result<()> {
    let mut buf = vec![0u8; BUFFER];
    let mut fds = Vec::new();
    let mut framer = Framer::default();
    let mut framed = true;
    loop {
        let n = recv(from, &mut buf, &mut fds)?;
        if n == 0 {
            return Ok(());
        }
        let now = windows.now_ms();
        let mut link = lock(link);
        let link = &mut *link;
        match side {
            Side::Cage => link.to_desktop.append(&mut fds),
            Side::Desktop => link.to_cage.append(&mut fds),
        }
        let mut out = Vec::new();
        if framed {
            framer.push(&buf[..n]);
            loop {
                match framer.next() {
                    Ok(Some(msg)) => out.extend(match side {
                        Side::Cage => link.shade.request(msg),
                        Side::Desktop => link.shade.event(msg, now),
                    }),
                    Ok(None) => break,
                    Err(_) => {
                        framed = false;
                        out.push(pass(side, framer.rest()));
                        break;
                    }
                }
            }
        } else {
            out.push(pass(side, buf[..n].to_vec()));
        }
        link.deliver(out)?;
    }
}

/// Bytes from `side`, on to the other unread.
fn pass(side: Side, bytes: Vec<u8>) -> Out {
    match side {
        Side::Cage => Out::Desktop(bytes),
        Side::Desktop => Out::Cage(bytes),
    }
}

fn close(cage: &UnixStream, desktop: &UnixStream, ended: io::Result<()>) {
    if let Err(e) = ended {
        eprintln!("rbxmgr window relay: a display connection ended: {e}");
    }
    for end in [cage, desktop] {
        // One already closed is closed all the same.
        let _ = end.shutdown(Shutdown::Both);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
