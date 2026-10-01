//! Keeping everything current with one Update: the Roblox build, Stacked
//! (the engine), and the app itself.
//!
//! [`check`] asks GitHub whether the app or Stacked has a newer release,
//! which is what shows the window's Update button. Roblox has no such
//! question to ask cheaply -- cordial-fetch learns the newest build by
//! fetching it -- so it is only brought up to date when [`update_all`] runs.
//! The three are independent: one failing leaves the others done.

pub mod app;
pub mod channel;
pub mod packagekit;

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::cordial::stacked::{self, Host, Releases, Updated};
use crate::cordial::{Runner, roblox_build};
use crate::github::GithubError;
use crate::install::Install;
use crate::paths::Paths;
pub use app::{AppReleases, GithubAppReleases, RUNNING, SelfUpdate};
pub use channel::{APP_ID, RESTARTED, Relaunch, relaunch};
pub use packagekit::{PackageInstaller, PackageKit};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UpdateError {
    #[error(transparent)]
    Github(#[from] GithubError),
    #[error("{0}")]
    Io(String),
    #[error("the download does not match the release's checksum")]
    Checksum,
    #[error("{0}")]
    Install(String),
    /// This copy cannot update itself; the text says what to do instead.
    #[error("{0}")]
    Unsupported(String),
}

/// What has a newer release than what runs now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Available {
    /// The app's newest version.
    pub app: Option<String>,
    /// Stacked's newest version.
    pub stacked: Option<String>,
}

impl Available {
    pub fn any(&self) -> bool {
        self.app.is_some() || self.stacked.is_some()
    }

    /// "Roblox Manager 0.3.0 and Stacked 0.21.0", for a tooltip.
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            self.app.as_ref().map(|v| format!("Roblox Manager {v}")),
            self.stacked.as_ref().map(|v| format!("Stacked {v}")),
        ]
        .into_iter()
        .flatten()
        .collect();
        parts.join(" and ")
    }
}

/// What a check found, and what it could not ask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checked {
    pub available: Available,
    pub problems: Vec<String>,
}

/// Ask whether the app or Stacked has a newer release.
pub fn check(app: &dyn AppReleases, stacked: &dyn Releases, paths: &Paths) -> Checked {
    let mut checked = Checked::default();
    match app::newer(app) {
        Ok(release) => checked.available.app = release.map(|r| r.version),
        Err(e) => checked.problems.push(format!("Could not check for a new Roblox Manager: {e}")),
    }
    match stacked::newer(stacked, paths) {
        Ok(version) => checked.available.stacked = version,
        Err(e) => checked.problems.push(format!("Could not check for a new Stacked: {e}")),
    }
    checked
}

/// What happened to the app in an [`update_all`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppOutcome {
    /// No newer release.
    Current,
    /// This version is installed; it runs from the next start.
    Installed(String),
    /// A newer version is out, but it was not (or could not be) installed.
    Failed(String),
}

/// What an [`update_all`] did, one part at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub roblox: Result<(), String>,
    pub stacked: Result<Updated, String>,
    pub app: AppOutcome,
}

/// Everything [`update_all`] uses.
pub struct Sources<'a> {
    pub runner: &'a dyn Runner,
    pub stacked: &'a dyn Releases,
    pub app: &'a dyn AppReleases,
    pub packages: &'a dyn PackageInstaller,
    pub paths: &'a Paths,
    pub install: &'a Install,
    pub host: Host,
}

/// Bring the Roblox build, Stacked and the app up to date, in that order:
/// the app last, since only a restart finishes it.
pub fn update_all(src: &Sources, log: &dyn Fn(String)) -> Report {
    let roblox = roblox_build(src.runner, log, true).map(drop).map_err(|e| e.to_string());
    let stacked = stacked::update(src.runner, src.stacked, src.paths, src.host, log)
        .map_err(|e| e.to_string());
    let app = match app::newer(src.app) {
        Ok(None) => AppOutcome::Current,
        Ok(Some(release)) => {
            let update = SelfUpdate {
                releases: src.app,
                runner: src.runner,
                packages: src.packages,
                paths: src.paths,
                install: src.install,
                flatpak_info: Path::new("/.flatpak-info"),
            };
            match update.install(&release, log) {
                Ok(()) => AppOutcome::Installed(release.version),
                Err(e) => AppOutcome::Failed(e.to_string()),
            }
        }
        Err(e) => AppOutcome::Failed(format!("could not check for a new Roblox Manager: {e}")),
    };
    Report { roblox, stacked, app }
}

/// Start the installed (new) version, which waits for this one to exit:
/// the first way of [`relaunch`] that starts.
pub fn start_installed(
    runner: &dyn Runner,
    install: &Install,
    paths: &Paths,
) -> Result<(), UpdateError> {
    let logs = paths.logs();
    fs::create_dir_all(&logs)
        .map_err(|e| UpdateError::Io(format!("could not create {}: {e}", logs.display())))?;
    let log = logs.join("restart.log");
    let mut why = String::from("there is no way to start it");
    for way in relaunch(install) {
        let file = File::create(&log)
            .map_err(|e| UpdateError::Io(format!("could not create {}: {e}", log.display())))?;
        match runner.spawn(&way.argv, file, &way.env) {
            Ok(_started) => return Ok(()),
            Err(e) => why = e.to_string(),
        }
    }
    Err(UpdateError::Install(format!("could not start the new version: {why}")))
}

/// Wait, up to `limit`, for the copy that restarted this one to give up the
/// app's name on the session bus; until it does, this one would only hand
/// it its arguments and exit. False when it did not go in time, or the bus
/// could not be asked (then there is nothing to wait for).
pub fn wait_for_predecessor(limit: Duration) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else { return false };
    let Ok(bus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return false };
    let Ok(name) = zbus::names::BusName::try_from(APP_ID) else { return false };
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        match bus.name_has_owner(name.clone()) {
            Ok(false) => return true,
            Ok(true) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_out_reads_as_one_line() {
        let both = Available { app: Some("0.3.0".into()), stacked: Some("0.21.0".into()) };
        assert_eq!(both.describe(), "Roblox Manager 0.3.0 and Stacked 0.21.0");
        assert!(both.any());
        assert!(!Available::default().any());
    }

    #[test]
    fn the_new_version_is_started_the_first_way_that_works() {
        let dir = tempfile::tempdir().unwrap();
        let runner = crate::cordial::process::recording::Recording::default();
        let paths = Paths::under(dir.path());
        let install = Install::Package(crate::install::PackageFormat::Rpm);
        start_installed(&runner, &install, &paths).unwrap();
        let spawned = runner.spawned();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].0, ["/usr/bin/roblox-manager", RESTARTED]);
    }
}
