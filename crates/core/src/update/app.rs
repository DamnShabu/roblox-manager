//! The app's own updates: its newest GitHub release, and installing it the
//! way this copy was installed. Every file is checked against the release's
//! `SHA256SUMS` before anything is replaced.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use super::UpdateError;
use super::channel::{self, asset_name};
use super::packagekit::PackageInstaller;
use crate::cordial::Runner;
use crate::github::{GithubClient, GithubError, Release};
use crate::install::Install;
use crate::paths::Paths;
use crate::version::is_newer;

/// The app's repository.
pub const REPO: &str = "DamnShabu/roblox-manager";

/// The version running now.
pub const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// The release's checksums, in `sha256sum`'s format.
pub const SUMS: &str = "SHA256SUMS";

/// Where the app's releases come from. Adapters: [`GithubAppReleases`], and
/// a stand-in in the tests.
pub trait AppReleases: Send + Sync {
    fn latest(&self) -> Result<Release, GithubError>;
    /// The file at `url`, written to `to`.
    fn download(&self, url: &str, to: &Path) -> Result<(), GithubError>;
}

#[derive(Default)]
pub struct GithubAppReleases {
    github: GithubClient,
}

impl AppReleases for GithubAppReleases {
    fn latest(&self) -> Result<Release, GithubError> {
        self.github.latest(REPO)
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), GithubError> {
        self.github.download(url, to)
    }
}

/// The newest release, when it is newer than the running version. A
/// repository with no release yet has nothing newer.
pub fn newer(releases: &dyn AppReleases) -> Result<Option<Release>, UpdateError> {
    let latest = match releases.latest() {
        Err(GithubError::Status(404)) => return Ok(None),
        got => got?,
    };
    Ok(is_newer(&latest.version, RUNNING).then_some(latest))
}

/// Installing a release over this copy.
pub struct SelfUpdate<'a> {
    pub releases: &'a dyn AppReleases,
    pub runner: &'a dyn Runner,
    pub packages: &'a dyn PackageInstaller,
    pub paths: &'a Paths,
    pub install: &'a Install,
    /// `/.flatpak-info` inside a Flatpak: says which installation it is in.
    pub flatpak_info: &'a Path,
}

impl SelfUpdate<'_> {
    /// Download `release`'s file for this install, check it, and install
    /// it. The running app stays the old version until it is restarted.
    pub fn install(&self, release: &Release, log: &dyn Fn(String)) -> Result<(), UpdateError> {
        let version = &release.version;
        let name = asset_name(self.install, version).ok_or_else(|| {
            UpdateError::Unsupported(format!(
                "Roblox Manager {version} is out, but this copy cannot update itself: {}",
                channel::by_hand(self.install)
            ))
        })?;
        let asset = release
            .asset(&name)
            .ok_or_else(|| UpdateError::Install(format!("release {version} has no {name}")))?;
        let sums = release.asset(SUMS).ok_or_else(|| {
            UpdateError::Install(format!("release {version} has no {SUMS} to check it with"))
        })?;
        // Beside the AppImage, so the new one is renamed over it in one step.
        let dir = match self.install {
            Install::AppImage(image) => image.parent().map(Path::to_path_buf).unwrap_or_default(),
            _ => self.paths.updates(),
        };
        fs::create_dir_all(&dir).map_err(|e| io_error("could not create", &dir, e))?;
        let file = dir.join(format!(".{name}.part"));
        let sums_file = self.paths.updates().join(format!("{SUMS}-{version}"));
        if let Some(parent) = sums_file.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error("could not create", parent, e))?;
        }

        log(format!("Downloading Roblox Manager {version}..."));
        self.releases.download(&sums.url, &sums_file)?;
        let expected = fs::read_to_string(&sums_file)
            .map_err(|e| io_error("could not read", &sums_file, e))
            .map(|text| checksum_for(&text, &name));
        remove(&sums_file);
        let expected = expected?.ok_or_else(|| {
            UpdateError::Install(format!("{SUMS} of release {version} does not list {name}"))
        })?;
        let fetched = self.releases.download(&asset.url, &file).map_err(UpdateError::from);
        let checked = fetched.and_then(|()| self.check(&file, &expected));
        let applied = checked.and_then(|()| self.apply(&file, &name, version, log));
        // Kept only where it is the new AppImage itself, already moved.
        remove(&file);
        applied
    }

    fn check(&self, file: &Path, expected: &str) -> Result<(), UpdateError> {
        let argv = vec!["sha256sum".to_owned(), file.display().to_string()];
        let out = self
            .runner
            .run(&argv, Duration::from_secs(300))
            .map_err(|e| UpdateError::Install(format!("could not check the download: {e}")))?;
        let got = String::from_utf8_lossy(&out.stdout);
        match got.split_whitespace().next() {
            Some(sum) if out.success() && sum.eq_ignore_ascii_case(expected) => Ok(()),
            _ => Err(UpdateError::Checksum),
        }
    }

    fn apply(
        &self,
        file: &Path,
        name: &str,
        version: &str,
        log: &dyn Fn(String),
    ) -> Result<(), UpdateError> {
        log(format!("Installing Roblox Manager {version}..."));
        match self.install {
            Install::AppImage(image) => {
                fs::set_permissions(file, fs::Permissions::from_mode(0o755))
                    .map_err(|e| io_error("could not make runnable", file, e))?;
                fs::rename(file, image).map_err(|e| io_error("could not replace", image, e))
            }
            Install::Flatpak => {
                let info = fs::read_to_string(self.flatpak_info).unwrap_or_default();
                let argv = channel::flatpak_install(&info, file);
                let out = self.runner.run(&argv, Duration::from_secs(1800)).map_err(|e| {
                    UpdateError::Install(format!("could not run flatpak on the host: {e}"))
                })?;
                if out.success() {
                    return Ok(());
                }
                let why = crate::cordial::process::last_line(&out.stderr);
                Err(UpdateError::Install(format!("flatpak could not install it: {why}")))
            }
            Install::Package(_) => {
                // A copy where the package can be found again if PackageKit
                // fails: the user can install it by hand.
                let kept = self.paths.updates().join(name);
                fs::rename(file, &kept).map_err(|e| io_error("could not move", &kept, e))?;
                match self.packages.install_file(&kept) {
                    Ok(()) => {
                        remove(&kept);
                        Ok(())
                    }
                    Err(e) => Err(UpdateError::Install(format!(
                        "{e}. The package is at {}; install it with your package manager",
                        kept.display()
                    ))),
                }
            }
            Install::Native(_) => {
                Err(UpdateError::Unsupported(channel::by_hand(self.install).into()))
            }
        }
    }
}

/// The checksum `sums` (`sha256sum` output) lists for `name`.
fn checksum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        // `sha256sum -b` marks binary files with a leading `*`.
        (file.trim().trim_start_matches('*') == name).then(|| sum.to_owned())
    })
}

/// Best effort: a leftover download only takes room in the cache.
fn remove(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        if e.kind() != io::ErrorKind::NotFound {
            eprintln!("roblox-manager: could not delete {}: {e}", path.display());
        }
    }
}

fn io_error(what: &str, path: &Path, e: io::Error) -> UpdateError {
    UpdateError::Io(format!("{what} {}: {e}", path.display()))
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
