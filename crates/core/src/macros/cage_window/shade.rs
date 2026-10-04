//! One cage's connection to the desktop, as far as hiding its window needs
//! it followed: which surfaces are cage's windows, the buffers and frame
//! callbacks on them, and the configures they are sent.
//!
//! Hiding is the protocol's own unmap -- a null buffer committed on the
//! window's surface -- sent on cage's behalf, so the desktop drops the
//! window (and its taskbar entry) while cage, and the client inside it, go
//! on as before. Cage never learns of it, so what it would do that the
//! unmap forbids is kept from the desktop: a buffer it attaches is held, and
//! the frame the desktop answers is not passed on, since cage would only
//! draw for nobody.
//!
//! Showing is the protocol's remap, done here as well: a commit with no
//! buffer, the configure that answers it acked, and a buffer committed --
//! the one cage had up when it was hidden, kept from it for this, or a newer
//! one it attached since. It cannot be left to cage. A cage with nothing new
//! to draw draws nothing, and sends neither the ack nor a buffer: measured
//! with a still client in cage, the window never came back. Cage's own ack
//! of that configure, when it comes, is not passed on twice.

use std::collections::{HashMap, HashSet};

use crate::macros::wire::{DISPLAY, header, message, read_str, word, words};

/// wl_surface requests.
const ATTACH: u16 = 1;
const FRAME: u16 = 3;
const COMMIT: u16 = 6;
/// xdg_wm_base.get_xdg_surface, xdg_surface.get_toplevel and ack_configure.
const GET_XDG_SURFACE: u16 = 2;
const GET_TOPLEVEL: u16 = 1;
const ACK_CONFIGURE: u16 = 4;
/// xdg_toplevel.set_title and set_app_id: what the desktop forgets of a
/// window when it comes down.
const SET_TITLE: u16 = 2;
const SET_APP_ID: u16 = 3;
/// wp_linux_drm_syncobj_manager_v1.get_surface; the syncobj surface's
/// set_acquire_point and set_release_point, which go with a buffer.
const GET_SYNCOBJ_SURFACE: u16 = 1;
const SET_ACQUIRE_POINT: u16 = 1;
const SET_RELEASE_POINT: u16 = 2;
/// wl_display.delete_id, xdg_surface.configure, wl_callback.done and
/// wl_buffer.release: three events numbered 0 on their own objects.
const DELETE_ID: u16 = 1;
const CONFIGURE: u16 = 0;
const DONE: u16 = 0;
const RELEASE: u16 = 0;

/// Where a message goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Out {
    Desktop(Vec<u8>),
    Cage(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Shown,
    /// Hidden as soon as cage commits the buffer it has attached.
    Hiding,
    /// Unmapped, and since then the desktop's last configure, if any, and
    /// whether cage has acked it.
    Hidden {
        serial: Option<u32>,
        acked: bool,
    },
    /// Started over, waiting on the desktop's configure.
    Showing,
}

/// A buffer as cage puts it up: the attach and the sync points with it.
#[derive(Debug, Clone, Default)]
struct Buffer {
    id: u32,
    msgs: Vec<Vec<u8>>,
}

/// One of cage's windows: a wl_surface with an xdg_toplevel.
#[derive(Debug)]
struct Toplevel {
    xdg_surface: u32,
    xdg_toplevel: u32,
    /// Its title and app id as cage last set them, said again on showing:
    /// sway showed a window put back without them as nameless.
    names: Vec<Vec<u8>>,
    syncobj: Option<u32>,
    phase: Phase,
    /// Attached since the last commit, while shown.
    pending: Option<Buffer>,
    /// The buffer the desktop has up, or had when the window came down.
    current: Buffer,
    /// That buffer, kept from cage for showing again: its release from the
    /// desktop is not passed on.
    kept: u32,
    /// What cage attached while the window was down, newest only.
    held: Option<Buffer>,
    /// The frame callback cage asked for last, the one it waits on.
    frame: Option<Frame>,
    /// Configures acked here, whose ack from cage is not passed on.
    acked_here: HashSet<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    id: u32,
    /// The desktop has sent its done.
    sent: bool,
    /// Cage has had it.
    answered: bool,
}

#[derive(Debug, Default)]
pub struct Shade {
    registries: HashSet<u32>,
    wm_bases: HashSet<u32>,
    syncobj_managers: HashSet<u32>,
    /// xdg_surface: its wl_surface, until it gets a role.
    xdg_surfaces: HashMap<u32, u32>,
    /// wl_surface: syncobj surface, made before it is a toplevel's.
    syncobjs: HashMap<u32, u32>,
    toplevels: HashMap<u32, Toplevel>,
    /// Frame callbacks answered here for a copy of the frame, whose done
    /// from the desktop, if it ever comes, is not passed on twice.
    drawn: HashSet<u32>,
}

impl Shade {
    /// A request from cage, and what it becomes.
    pub fn request(&mut self, msg: Vec<u8>) -> Vec<Out> {
        let (obj, op) = header(&msg);
        let body = msg.get(8..).unwrap_or_default();
        self.note(obj, op, body);
        // Attach's buffer, frame's callback, ack_configure's serial.
        let first = word(body, 0);
        if let Some(t) = self.toplevels.get_mut(&obj) {
            return t.surface_request(obj, op, first, msg);
        }
        if let Some(t) = self.toplevels.values_mut().find(|t| t.syncobj == Some(obj)) {
            if matches!(op, SET_ACQUIRE_POINT | SET_RELEASE_POINT) {
                return t.sync_point(msg);
            }
        } else if let Some(t) = self.toplevels.values_mut().find(|t| t.xdg_toplevel == obj) {
            if matches!(op, SET_TITLE | SET_APP_ID) {
                t.names.retain(|m| header(m).1 != op);
                t.names.push(msg.clone());
            }
        } else if let Some(t) = self.toplevels.values_mut().find(|t| t.xdg_surface == obj) {
            if op == ACK_CONFIGURE {
                if t.acked_here.remove(&first) {
                    return Vec::new();
                }
                if let Phase::Hidden { serial: Some(s), .. } = t.phase {
                    t.phase = Phase::Hidden { serial: Some(s), acked: s == first };
                }
            }
        }
        vec![Out::Desktop(msg)]
    }

    /// An event from the desktop, and what it becomes.
    pub fn event(&mut self, msg: Vec<u8>, now_ms: u32) -> Vec<Out> {
        let (obj, op) = header(&msg);
        let body = msg.get(8..).unwrap_or_default();
        if op == DONE && self.drawn.remove(&obj) {
            return Vec::new();
        }
        if obj == DISPLAY && op == DELETE_ID {
            self.forget(word(body, 0));
            return vec![Out::Cage(msg)];
        }
        for (surface, t) in &mut self.toplevels {
            let down = !t.up();
            if let Some(f) = t.frame.as_mut().filter(|f| op == DONE && f.id == obj) {
                f.sent = true;
                if down {
                    // Cage would draw a frame for a window that is down.
                    return Vec::new();
                }
                f.answered = true;
            } else if op == RELEASE && obj == t.kept && t.kept != 0 {
                // The desktop letting go of it as the window came down.
                return Vec::new();
            } else if op == CONFIGURE && obj == t.xdg_surface {
                let serial = word(body, 0);
                match t.phase {
                    Phase::Hidden { .. } => {
                        t.phase = Phase::Hidden { serial: Some(serial), acked: false };
                    }
                    Phase::Showing => {
                        let mut out = vec![Out::Cage(msg)];
                        out.extend(t.remap(*surface, Some(serial), now_ms));
                        return out;
                    }
                    Phase::Shown | Phase::Hiding => {}
                }
            }
        }
        vec![Out::Cage(msg)]
    }

    /// Hide every window, or show them again.
    pub fn set_hidden(&mut self, hide: bool, now_ms: u32) -> Vec<Out> {
        let mut out = Vec::new();
        for (surface, t) in &mut self.toplevels {
            out.extend(if hide { t.hide(*surface) } else { t.show(*surface, now_ms) });
        }
        out
    }

    /// Have cage draw its next frame now: the frame callback it waits on,
    /// answered here. Asked for by a macro copying the frame out, which a
    /// cage left waiting -- hidden, or out of the desktop's sight -- never
    /// draws.
    pub fn draw(&mut self, now_ms: u32) -> Vec<Out> {
        let mut out = Vec::new();
        for t in self.toplevels.values_mut() {
            if let Some(f) = t.frame.as_mut().filter(|f| !f.answered) {
                f.answered = true;
                self.drawn.insert(f.id);
                out.push(Out::Cage(message(f.id, DONE, &words(&[now_ms]))));
            }
        }
        out
    }

    /// Follow the objects hiding has to know: registries, the globals that
    /// make windows and sync points, and which surfaces become windows.
    fn note(&mut self, obj: u32, op: u16, body: &[u8]) {
        if obj == DISPLAY && op == 1 {
            self.registries.insert(word(body, 0)); // get_registry
        } else if op == 0 && self.registries.contains(&obj) {
            // bind(name, interface, version, id): the id after the string.
            let padded = (word(body, 4) as usize + 3) & !3;
            let id = word(body, 12 + padded);
            match read_str(body, 4).as_deref() {
                Some("xdg_wm_base") => self.wm_bases.insert(id),
                Some("wp_linux_drm_syncobj_manager_v1") => self.syncobj_managers.insert(id),
                _ => false,
            };
        } else if op == GET_XDG_SURFACE && self.wm_bases.contains(&obj) {
            self.xdg_surfaces.insert(word(body, 0), word(body, 4));
        } else if op == GET_SYNCOBJ_SURFACE && self.syncobj_managers.contains(&obj) {
            let (syncobj, surface) = (word(body, 0), word(body, 4));
            self.syncobjs.insert(surface, syncobj);
            if let Some(t) = self.toplevels.get_mut(&surface) {
                t.syncobj = Some(syncobj);
            }
        } else if op == GET_TOPLEVEL {
            if let Some(&surface) = self.xdg_surfaces.get(&obj) {
                self.toplevels.insert(
                    surface,
                    Toplevel {
                        xdg_surface: obj,
                        xdg_toplevel: word(body, 0),
                        names: Vec::new(),
                        syncobj: self.syncobjs.get(&surface).copied(),
                        phase: Phase::Shown,
                        pending: None,
                        current: Buffer::default(),
                        kept: 0,
                        held: None,
                        frame: None,
                        acked_here: HashSet::new(),
                    },
                );
            }
        }
    }

    /// An id the desktop has let go of, and all that was known of it.
    fn forget(&mut self, id: u32) {
        self.registries.remove(&id);
        self.wm_bases.remove(&id);
        self.syncobj_managers.remove(&id);
        self.xdg_surfaces.remove(&id);
        self.syncobjs.remove(&id);
        self.syncobjs.retain(|_, s| *s != id);
        self.toplevels.remove(&id);
        self.drawn.remove(&id);
        for t in self.toplevels.values_mut() {
            if t.syncobj == Some(id) {
                t.syncobj = None;
            }
        }
    }
}

impl Toplevel {
    fn up(&self) -> bool {
        matches!(self.phase, Phase::Shown | Phase::Hiding)
    }

    fn surface_request(&mut self, surface: u32, op: u16, first: u32, msg: Vec<u8>) -> Vec<Out> {
        match op {
            ATTACH if self.up() => {
                self.pending = Some(Buffer { id: first, msgs: vec![msg.clone()] });
                vec![Out::Desktop(msg)]
            }
            ATTACH => self.hold(first, msg),
            FRAME => {
                self.frame = Some(Frame { id: first, sent: false, answered: false });
                vec![Out::Desktop(msg)]
            }
            COMMIT if self.up() => {
                if let Some(buffer) = self.pending.take() {
                    self.current = buffer;
                }
                let mut out = vec![Out::Desktop(msg)];
                if self.phase == Phase::Hiding {
                    out.extend(self.unmap(surface));
                }
                out
            }
            _ => vec![Out::Desktop(msg)],
        }
    }

    /// A sync point for the buffer being attached.
    fn sync_point(&mut self, msg: Vec<u8>) -> Vec<Out> {
        if self.up() {
            if let Some(b) = self.pending.as_mut() {
                b.msgs.push(msg.clone());
            }
            return vec![Out::Desktop(msg)];
        }
        if let Some(b) = self.held.as_mut() {
            b.msgs.push(msg);
        }
        Vec::new()
    }

    /// A buffer attached while the window is down, to go up when it is
    /// shown. Any it replaces -- the one kept, or an older one held -- is
    /// cage's again.
    fn hold(&mut self, id: u32, msg: Vec<u8>) -> Vec<Out> {
        let mut out = Vec::new();
        let replaced = match self.held.take() {
            Some(b) => b.id,
            None => std::mem::take(&mut self.kept),
        };
        if replaced != 0 && replaced != id {
            out.push(Out::Cage(message(replaced, RELEASE, &[])));
        }
        self.held = Some(Buffer { id, msgs: vec![msg] });
        out
    }

    fn hide(&mut self, surface: u32) -> Vec<Out> {
        match self.phase {
            Phase::Shown if self.pending.is_some() => {
                self.phase = Phase::Hiding;
                Vec::new()
            }
            Phase::Shown => self.unmap(surface),
            Phase::Showing => {
                // Its configure is still to come, and will be noted.
                self.phase = Phase::Hidden { serial: None, acked: false };
                Vec::new()
            }
            Phase::Hiding | Phase::Hidden { .. } => Vec::new(),
        }
    }

    fn show(&mut self, surface: u32, now_ms: u32) -> Vec<Out> {
        match self.phase {
            Phase::Hiding => {
                self.phase = Phase::Shown;
                Vec::new()
            }
            Phase::Hidden { serial: None, .. } => {
                // The commit that starts a window over, with its names; the
                // desktop answers it with a configure.
                self.phase = Phase::Showing;
                let mut out: Vec<Out> = self.names.iter().cloned().map(Out::Desktop).collect();
                out.push(commit(surface));
                out
            }
            // Cage committed while hidden, which started it over already.
            Phase::Hidden { serial: Some(s), acked } => {
                let mut out: Vec<Out> = self.names.iter().cloned().map(Out::Desktop).collect();
                out.extend(self.remap(surface, (!acked).then_some(s), now_ms));
                out
            }
            Phase::Shown | Phase::Showing => Vec::new(),
        }
    }

    /// A null buffer, committed: the window comes down, and the buffer it
    /// had is kept for when it goes up again.
    fn unmap(&mut self, surface: u32) -> Vec<Out> {
        self.phase = Phase::Hidden { serial: None, acked: false };
        self.kept = self.current.id;
        vec![Out::Desktop(message(surface, ATTACH, &words(&[0, 0, 0]))), commit(surface)]
    }

    /// Put the window back up: `serial` acked if cage has not, and the
    /// newest buffer committed. Then the frame cage waits on, if its done
    /// was kept from it while the window was down.
    fn remap(&mut self, surface: u32, serial: Option<u32>, now_ms: u32) -> Vec<Out> {
        self.phase = Phase::Shown;
        self.kept = 0;
        let mut out = Vec::new();
        if let Some(s) = serial {
            self.acked_here.insert(s);
            out.push(Out::Desktop(message(self.xdg_surface, ACK_CONFIGURE, &words(&[s]))));
        }
        if let Some(held) = self.held.take() {
            self.current = held;
        }
        out.extend(self.current.msgs.iter().cloned().map(Out::Desktop));
        out.push(commit(surface));
        if let Some(f) = self.frame.as_mut().filter(|f| f.sent && !f.answered) {
            f.answered = true;
            out.push(Out::Cage(message(f.id, DONE, &words(&[now_ms]))));
        }
        out
    }
}

fn commit(surface: u32) -> Out {
    Out::Desktop(message(surface, COMMIT, &[]))
}

#[cfg(test)]
mod tests;
