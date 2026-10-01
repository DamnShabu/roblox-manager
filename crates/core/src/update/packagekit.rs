//! Installing a downloaded distribution package as root, through PackageKit
//! on the system bus. pkexec cannot do it: a packaged copy runs inside its
//! bundle's user namespace, where no set-uid program gains root. PackageKit
//! asks polkit, so the desktop's own password prompt appears.

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use super::UpdateError;

/// Installs a package file. Adapters: [`PackageKit`], and a stand-in in the
/// tests.
pub trait PackageInstaller: Send + Sync {
    fn install_file(&self, file: &Path) -> Result<(), UpdateError>;
}

const BUS: &str = "org.freedesktop.PackageKit";
const PATH: &str = "/org/freedesktop/PackageKit";
const TRANSACTION: &str = "org.freedesktop.PackageKit.Transaction";

/// `PK_EXIT_ENUM_SUCCESS`.
const EXIT_SUCCESS: u32 = 1;

/// Long enough for a password prompt left open and a slow disk.
const TIMEOUT: Duration = Duration::from_secs(1800);

/// The PackageKit daemon.
pub struct PackageKit;

impl PackageInstaller for PackageKit {
    fn install_file(&self, file: &Path) -> Result<(), UpdateError> {
        let conn = Connection::system().map_err(unreachable)?;
        let daemon = Proxy::new(&conn, BUS, PATH, BUS).map_err(unreachable)?;
        let path: OwnedObjectPath = daemon.call("CreateTransaction", &()).map_err(unreachable)?;
        let tx = Proxy::new(&conn, BUS, path.into_inner(), TRANSACTION).map_err(failed)?;
        // Subscribe before starting: a quick failure may come at once.
        let signals = tx.receive_all_signals().map_err(failed)?;
        let (send, done) = mpsc::channel();
        // The iterator has no timeout of its own; it waits on a thread that
        // ends with the transaction.
        thread::spawn(move || {
            let mut error = None;
            for msg in signals {
                let header = msg.header();
                match header.member().map(|m| m.as_str()) {
                    Some("ErrorCode") => {
                        error = msg.body().deserialize::<(u32, String)>().ok().map(|(_, d)| d);
                    }
                    Some("Finished") => {
                        let exit = msg.body().deserialize::<(u32, u32)>().ok().map(|(e, _)| e);
                        let _ = send.send((exit, error));
                        return;
                    }
                    _ => {}
                }
            }
        });
        // Without it the daemon may refuse to ask for the password.
        tx.call::<_, _, ()>("SetHints", &(vec!["interactive=true"],)).map_err(failed)?;
        let files = vec![file.to_string_lossy().into_owned()];
        // No flags: a local file is untrusted, which is what polkit asks about.
        tx.call::<_, _, ()>("InstallFiles", &(0u64, files)).map_err(failed)?;
        match done.recv_timeout(TIMEOUT) {
            Ok((Some(EXIT_SUCCESS), _)) => Ok(()),
            Ok((_, Some(why))) => Err(UpdateError::Install(format!("PackageKit: {why}"))),
            Ok((_, None)) => Err(UpdateError::Install("PackageKit did not install it".into())),
            Err(_) => Err(UpdateError::Install(format!(
                "PackageKit did not finish within {} minutes",
                TIMEOUT.as_secs() / 60
            ))),
        }
    }
}

fn unreachable(e: zbus::Error) -> UpdateError {
    UpdateError::Install(format!("PackageKit could not be reached ({e})"))
}

fn failed(e: zbus::Error) -> UpdateError {
    UpdateError::Install(format!("PackageKit: {e}"))
}
