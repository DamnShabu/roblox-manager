//! Hiding a macro-ready client's window: cage's, on the desktop, while the
//! client inside keeps its own window in cage -- where a macro's input
//! lands, and which hiding the client's window would take away.
//!
//! Cage runs behind the window relay, `roblox-manager --window-relay
//! DISPLAY_FILE -- cage ...`, which gives cage a display of its own beside
//! the display file and links every connection on it through to the
//! desktop's. Told to hide on its control socket, it unmaps cage's window
//! there with the protocol's own null buffer; see [`shade`]. Cage, the
//! client and any macro playing into it go on as before, and the window
//! comes back on Show. It ends with cage.
//!
//! The Wayland protocol, spoken on cage's behalf: nothing reaches into
//! cage, the client or the game.

mod link;
mod shade;

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use link::Windows;

use super::relay::{accept, listen, parse, remove};

/// The argument that starts the app as a window relay, not a window.
pub const FLAG: &str = "--window-relay";

/// `cage` (its whole command) run behind a window relay, `program`, beside
/// `display_file`.
pub fn argv(program: &Path, display_file: &Path, cage: &[String]) -> Vec<String> {
    let relay = [program.display().to_string(), FLAG.to_owned()];
    let display = [display_file.display().to_string(), "--".to_owned()];
    relay.into_iter().chain(display).chain(cage.iter().cloned()).collect()
}

/// Whether the window of the client beside `display_file` is hidden. None
/// when it has no window relay: it was launched by an earlier version,
/// without one, or not in a macro-ready window at all.
pub fn hidden(display_file: &Path) -> io::Result<Option<bool>> {
    ask(display_file, "state")
}

/// Hide or show the window of the client beside `display_file`. None, as
/// for [`hidden`], when there is no window relay to do it.
pub fn set_hidden(display_file: &Path, hide: bool) -> io::Result<Option<bool>> {
    ask(display_file, if hide { "hide" } else { "show" })
}

/// Have the cage beside `display_file` draw its next frame, hidden or not:
/// for a macro copying the frame out, which a cage whose window nobody can
/// see would otherwise never draw. Nothing to do without a window relay.
pub fn draw(display_file: &Path) -> io::Result<()> {
    ask(display_file, "draw").map(drop)
}

/// Where a window relay takes hide and show.
fn control_file(display_file: &Path) -> PathBuf {
    display_file.with_extension("window")
}

/// The display a window relay gives cage.
fn cage_file(display_file: &Path) -> PathBuf {
    display_file.with_extension("desktop")
}

/// One line to the window relay, and the state it answers with.
fn ask(display_file: &Path, line: &str) -> io::Result<Option<bool>> {
    let mut conn = match UnixStream::connect(control_file(display_file)) {
        Ok(conn) => conn,
        // No socket, or one its relay left behind.
        Err(e)
            if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    conn.write_all(format!("{line}\n").as_bytes())?;
    let mut answer = String::new();
    // A relay slow to answer is a timeout, whatever the platform calls it:
    // a macro looking through it waits out a timeout, and gives up on
    // anything else.
    BufReader::new(conn).read_line(&mut answer).map_err(|e| match e.kind() {
        io::ErrorKind::WouldBlock => {
            io::Error::new(io::ErrorKind::TimedOut, "the window relay did not answer")
        }
        _ => e,
    })?;
    match answer.trim() {
        "hidden" => Ok(Some(true)),
        "shown" => Ok(Some(false)),
        other => Err(io::Error::other(format!("the window relay said {other:?}"))),
    }
}

/// The window relay, given what followed [`FLAG`]: runs cage to its end and
/// returns its exit status. Without its sockets, or a desktop display to
/// link to, cage still runs straight on the desktop, where only hiding is
/// lost.
pub fn run(args: &[String]) -> i32 {
    let Some((display, argv)) = parse(args) else {
        eprintln!("rbxmgr window relay: expected {FLAG} DISPLAY_FILE -- PROGRAM [ARGUMENT...]");
        return 2;
    };
    let mut cage = Command::new(&argv[0]);
    cage.args(&argv[1..]);
    let relay = match desktop_display().and_then(|desktop| Relay::open(&display, desktop)) {
        Ok(relay) => {
            cage.env("WAYLAND_DISPLAY", cage_file(&display));
            Some(relay)
        }
        Err(e) => {
            eprintln!("rbxmgr window relay: this window cannot be hidden: {e}");
            None
        }
    };
    let status = cage.status();
    drop(relay);
    match status {
        Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!("rbxmgr window relay: could not start {}: {e}", argv[0]);
            127
        }
    }
}

/// The desktop's display socket, from this process's environment.
fn desktop_display() -> io::Result<PathBuf> {
    let name = std::env::var_os("WAYLAND_DISPLAY")
        .filter(|d| !d.is_empty())
        .ok_or_else(|| io::Error::other("no WAYLAND_DISPLAY"))?;
    let name = PathBuf::from(name);
    if name.is_absolute() {
        return Ok(name);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .ok_or_else(|| io::Error::other("no XDG_RUNTIME_DIR"))?;
    Ok(PathBuf::from(dir).join(name))
}

/// A window relay's two sockets, beside the display file: the display it
/// gives cage, and its control socket. Both are removed when it ends.
struct Relay {
    display: PathBuf,
}

impl Relay {
    fn open(display: &Path, desktop: PathBuf) -> io::Result<Self> {
        let cages = listen(&cage_file(display))?;
        let controls = match listen(&control_file(display)) {
            Ok(controls) => controls,
            Err(e) => {
                remove(&cage_file(display));
                return Err(e);
            }
        };
        serve(cages, controls, desktop);
        Ok(Relay { display: display.to_owned() })
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        remove(&cage_file(&self.display));
        remove(&control_file(&self.display));
    }
}

/// Serve both sockets, on threads of their own, for as long as this process
/// runs: each of cage's connections linked through to the desktop, each
/// control connection answered.
fn serve(cages: UnixListener, controls: UnixListener, desktop: PathBuf) {
    let windows = Arc::new(Windows::default());
    let links = Arc::clone(&windows);
    thread::spawn(move || {
        accept(&cages, |cage| {
            let to_desktop = UnixStream::connect(&desktop)?;
            links.link(cage, to_desktop)
        });
    });
    thread::spawn(move || {
        accept(&controls, |conn| control(conn, &windows));
    });
}

/// One control connection: a line -- hide, show, draw or state -- and the state
/// after it.
fn control(conn: UnixStream, windows: &Windows) -> io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    BufReader::new(&conn).read_line(&mut line)?;
    match line.trim() {
        "hide" => windows.set_hidden(true),
        "show" => windows.set_hidden(false),
        "draw" => windows.draw(),
        "state" => {}
        other => return Err(io::Error::other(format!("not a window command: {other:?}"))),
    }
    let state = if windows.hidden() { "hidden\n" } else { "shown\n" };
    (&conn).write_all(state.as_bytes())
}

#[cfg(test)]
mod tests;
