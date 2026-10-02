//! The relay: a macro-ready client's display, passed through on its way to
//! the client, so a recording can hear what its window receives.
//!
//! Cage runs `roblox-manager --relay DISPLAY_FILE -- cordial-run ...`. The
//! relay gives the client a socket of its own beside the display file and
//! links every connection on it through to cage; recorders arm it on its
//! report socket, also beside the display file. It ends with its client.

pub mod event;
mod forward;
mod objects;
pub mod report;

use std::fs;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use report::Hub;

/// The argument that starts the app as a relay, not a window.
pub const FLAG: &str = "--relay";

/// `argv` run behind a relay, `program`, which reaches cage's display at
/// `display_file`.
pub fn argv(program: &Path, display_file: &Path, argv: &[String]) -> Vec<String> {
    let relay = [program.display().to_string(), FLAG.to_owned()];
    let display = [display_file.display().to_string(), "--".to_owned()];
    relay.into_iter().chain(display).chain(argv.iter().cloned()).collect()
}

/// Where a relay beside `display_file` takes recorders.
pub fn record_file(display_file: &Path) -> PathBuf {
    display_file.with_extension("record")
}

/// The display a relay beside `display_file` gives its client.
fn client_file(display_file: &Path) -> PathBuf {
    display_file.with_extension("client")
}

/// The relay, given what followed [`FLAG`]: runs the client to its end and
/// returns its exit status. Without its sockets the client still runs, on
/// cage's own display, where only recording is lost.
pub fn run(args: &[String]) -> i32 {
    let Some((display, argv)) = parse(args) else {
        eprintln!("rbxmgr relay: expected {FLAG} DISPLAY_FILE -- PROGRAM [ARGUMENT...]");
        return 2;
    };
    let mut client = Command::new(&argv[0]);
    client.args(&argv[1..]);
    let relay = match Relay::open(&display).and_then(|relay| relay.serve().map(|()| relay)) {
        Ok(relay) => {
            client.env("WAYLAND_DISPLAY", client_file(&display));
            Some(relay)
        }
        Err(e) => {
            eprintln!("rbxmgr relay: this client cannot be recorded: {e}");
            None
        }
    };
    let status = client.status();
    drop(relay);
    match status {
        Ok(status) => status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!("rbxmgr relay: could not start {}: {e}", argv[0]);
            127
        }
    }
}

/// `DISPLAY_FILE -- PROGRAM [ARGUMENT...]`.
fn parse(args: &[String]) -> Option<(PathBuf, Vec<String>)> {
    match args {
        [display, dashes, argv @ ..]
            if dashes == "--" && !argv.is_empty() && !display.is_empty() =>
        {
            Some((PathBuf::from(display), argv.to_vec()))
        }
        _ => None,
    }
}

/// A relay's two sockets, beside the display file: the display it gives its
/// client, and its report socket. Both are removed when it ends.
struct Relay {
    display: PathBuf,
    clients: UnixListener,
    recorders: UnixListener,
}

impl Relay {
    fn open(display: &Path) -> io::Result<Self> {
        let clients = listen(&client_file(display))?;
        let recorders = match listen(&record_file(display)) {
            Ok(recorders) => recorders,
            Err(e) => {
                remove(&client_file(display));
                return Err(e);
            }
        };
        Ok(Relay { display: display.to_owned(), clients, recorders })
    }

    /// Serve both sockets, on threads of their own, for as long as this
    /// process runs: each client connection linked through to cage, each
    /// recorder armed.
    fn serve(&self) -> io::Result<()> {
        let hub = Arc::new(Hub::default());
        let (clients, display, links) =
            (self.clients.try_clone()?, self.display.clone(), Arc::clone(&hub));
        thread::spawn(move || {
            accept(&clients, |client| {
                let cage = UnixStream::connect(&display)?;
                forward::link(client, cage, Arc::clone(&links))
            });
        });
        let recorders = self.recorders.try_clone()?;
        thread::spawn(move || {
            accept(&recorders, |recorder| {
                // Its own thread: one slow to answer holds up no other.
                let hub = Arc::clone(&hub);
                thread::spawn(move || {
                    if let Err(e) = report::serve(recorder, &hub) {
                        eprintln!("rbxmgr relay: a recorder was turned away: {e}");
                    }
                });
                Ok(())
            });
        });
        Ok(())
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        remove(&client_file(&self.display));
        remove(&record_file(&self.display));
    }
}

/// Take each connection on `listener` with `take`, telling what fails.
fn accept(listener: &UnixListener, mut take: impl FnMut(UnixStream) -> io::Result<()>) {
    for conn in listener.incoming() {
        if let Err(e) = conn.and_then(&mut take) {
            eprintln!("rbxmgr relay: a connection was turned away: {e}");
            // An error that repeats (no file descriptors left) never spins.
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A socket at `path`: a stale one left there is replaced, a live one -- a
/// relay still serving it -- never.
fn listen(path: &Path) -> io::Result<UnixListener> {
    if UnixStream::connect(path).is_ok() {
        let taken = format!("{} is another relay's", path.display());
        return Err(io::Error::new(io::ErrorKind::AddrInUse, taken));
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    UnixListener::bind(path)
}

fn remove(path: &Path) {
    // Gone already is gone; anything else leaves only a stale file, which
    // the next relay replaces.
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests;
