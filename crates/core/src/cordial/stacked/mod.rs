//! Stacked, the fork the engine comes from, at its newest release rather than
//! the commit this build pinned.
//!
//! Updating downloads the release's AppImage, unpacks it (without running
//! it) into `<data>/rbxmgr/stacked/<version>` and puts a small script at its
//! `bin/cordial-run` that starts the engine with the libraries the AppImage
//! bundles. A host that cannot run a downloaded program (NixOS) builds the
//! fork's flake with Nix instead, linked at `stacked/nix`. Either way
//! `stacked/current` then points at it, and launches run
//! `current/bin/cordial-run` instead of the bundled engine. Delete
//! `stacked/current` to go back.

pub mod appimage;
pub mod nix;
pub mod releases;

use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::CordialError;
use super::process::Runner;
use crate::paths::Paths;
pub use releases::{GithubReleases, Release, Releases};

/// What an update did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Updated {
    pub version: String,
    /// False when launches were already on this version.
    pub fresh: bool,
    /// Older versions that could not be deleted, and why.
    pub left_behind: Option<String>,
}

/// How this host can run a Stacked it did not come with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// It has the standard program loader: the release's AppImage runs.
    Portable,
    /// It has none, or NixOS's stand-in that only says it cannot run
    /// anything: only a Nix build runs.
    Nix,
}

#[cfg(target_arch = "aarch64")]
const LOADER: &str = "/lib/ld-linux-aarch64.so.1";
#[cfg(not(target_arch = "aarch64"))]
const LOADER: &str = "/lib64/ld-linux-x86-64.so.2";

/// In NixOS's stub-ld (its default `/lib64` loader since 24.05), which
/// explains that it runs nothing, and in no real loader.
const STUB_LD: &[u8] = b"nix.dev/permalink/stub-ld";

impl Host {
    pub fn detect() -> Self {
        Self::with_loader(Path::new(LOADER))
    }

    fn with_loader(loader: &Path) -> Self {
        match fs::read(loader) {
            Ok(bytes) if !bytes.windows(STUB_LD.len()).any(|w| w == STUB_LD) => Host::Portable,
            _ => Host::Nix,
        }
    }
}

const CURRENT: &str = "current";
const NIX_LINK: &str = "nix";

/// One at a time: two updates must not unpack into the same directory.
static UPDATING: Mutex<()> = Mutex::new(());

/// The engine launches run: the updated Stacked when there is one, else the
/// `cordial-run` this install came with, found on PATH.
pub fn engine_program(paths: &Paths) -> String {
    let updated = engine(&paths.stacked().join(CURRENT));
    if updated.is_file() { updated.display().to_string() } else { "cordial-run".into() }
}

fn engine(version_dir: &Path) -> PathBuf {
    version_dir.join("bin/cordial-run")
}

/// Install the newest Stacked and make launches use it.
pub fn update(
    runner: &dyn Runner,
    releases: &dyn Releases,
    paths: &Paths,
    host: Host,
    log: &dyn Fn(String),
) -> Result<Updated, CordialError> {
    let _one_at_a_time = UPDATING.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = paths.stacked();
    fs::create_dir_all(&dir).map_err(|e| io_error("could not create", &dir, e))?;
    let previous = current_target(&dir);
    // The version, the name `current` should point at, and whether it is a
    // new install.
    let (version, target, installed) = match host {
        Host::Portable => {
            let release = releases.latest()?;
            let target = release.version.clone();
            let have = previous.as_deref() == Some(target.as_str())
                && engine(&dir.join(&target)).is_file();
            if !have {
                log(format!("Downloading Stacked {}...", release.version));
                install_appimage(releases, &dir, &release)?;
            }
            (release.version, target, !have)
        }
        Host::Nix => {
            let link = dir.join(NIX_LINK);
            let before = fs::read_link(&link).ok();
            log("Building the newest Stacked with Nix; this can take a long while...".into());
            let version = nix::build(runner, &link)?;
            if !engine(&link).is_file() {
                return Err(CordialError::Stacked(format!(
                    "Stacked {version} has no bin/cordial-run"
                )));
            }
            // A new store path is a new version, though the link keeps its name.
            let rebuilt = before != fs::read_link(&link).ok();
            (version, NIX_LINK.to_owned(), rebuilt)
        }
    };
    let moved = previous.as_deref() != Some(target.as_str());
    if moved {
        switch(&dir, &target)?;
    }
    let fresh = installed || moved;
    let keep = [CURRENT, NIX_LINK, target.as_str(), previous.as_deref().unwrap_or(CURRENT)];
    let left_behind = prune(&dir, &keep);
    Ok(Updated { version, fresh, left_behind })
}

/// What `current` points at, by name.
fn current_target(dir: &Path) -> Option<String> {
    let target = fs::read_link(dir.join(CURRENT)).ok()?;
    Some(target.file_name()?.to_string_lossy().into_owned())
}

/// Download the release's AppImage and unpack it into `dir/<version>`.
fn install_appimage(
    releases: &dyn Releases,
    dir: &Path,
    release: &Release,
) -> Result<(), CordialError> {
    let appimage = dir.join(format!(".download-{}.AppImage", release.version));
    let work = dir.join(format!(".unpack-{}", release.version));
    remove(&work)?;
    releases.download(&release.url, &appimage)?;
    let unpacked = appimage::unpack(&appimage, &work);
    remove(&appimage)?;
    unpacked?;
    if !work.join("usr/bin/cordial-run").is_file() {
        return Err(CordialError::Stacked(format!(
            "Stacked {}'s AppImage has no usr/bin/cordial-run",
            release.version
        )));
    }
    let target = dir.join(&release.version);
    remove(&target)?;
    fs::rename(&work, &target).map_err(|e| io_error("could not move into", &target, e))?;
    write_launcher(&target)
}

/// `bin/cordial-run`: the engine, with what the AppImage's own AppRun gives
/// it. It execs the engine, so the running process is `.../usr/bin/cordial-run`
/// as the client list expects.
fn write_launcher(target: &Path) -> Result<(), CordialError> {
    let quoted = format!("'{}'", target.display().to_string().replace('\'', r"'\''"));
    let script = format!(
        r#"#!/bin/sh
# Stacked's engine, with the libraries its AppImage bundles. Written by the
# Roblox manager's Update Stacked.
d={quoted}
LD_LIBRARY_PATH="$d/usr/lib${{LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}}"
GSETTINGS_SCHEMA_DIR="$d/usr/share/glib-2.0/schemas"
XDG_DATA_DIRS="$d/usr/share:${{XDG_DATA_DIRS:-/usr/local/share:/usr/share}}"
WEBKIT_EXEC_PATH="$d/usr/libexec/webkitgtk-6.0"
WEBKIT_INJECTED_BUNDLE_PATH="$d/usr/lib/webkitgtk-6.0/injected-bundle"
export LD_LIBRARY_PATH GSETTINGS_SCHEMA_DIR XDG_DATA_DIRS WEBKIT_EXEC_PATH WEBKIT_INJECTED_BUNDLE_PATH
exec "$d/usr/bin/cordial-run" "$@"
"#
    );
    let path = engine(target);
    if let Some(bin) = path.parent() {
        fs::create_dir_all(bin).map_err(|e| io_error("could not create", bin, e))?;
    }
    fs::write(&path, script).map_err(|e| io_error("could not write", &path, e))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .map_err(|e| io_error("could not make runnable", &path, e))
}

/// Point `current` at `target`, in one step: a launch sees the old engine or
/// the new, never neither.
fn switch(dir: &Path, target: &str) -> Result<(), CordialError> {
    let next = dir.join(".current-next");
    remove(&next)?;
    symlink(target, &next).map_err(|e| io_error("could not link", &next, e))?;
    let current = dir.join(CURRENT);
    fs::rename(&next, &current).map_err(|e| io_error("could not replace", &current, e))
}

/// Delete everything in `dir` but `keep`: older versions, and what a broken-off
/// update left. The previous version stays, for clients still running on it.
fn prune(dir: &Path, keep: &[&str]) -> Option<String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return Some(format!("{}: {e}", dir.display())),
    };
    let failed: Vec<String> = entries
        .filter_map(|entry| {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(e) => return Some(e.to_string()),
            };
            let name = path.file_name()?.to_string_lossy().into_owned();
            if keep.contains(&name.as_str()) {
                return None;
            }
            remove(&path).err().map(|e| e.to_string())
        })
        .collect();
    (!failed.is_empty()).then(|| failed.join("; "))
}

/// Gone, whatever it was; already gone is fine.
fn remove(path: &Path) -> Result<(), CordialError> {
    let result = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    };
    result.map_err(|e| io_error("could not delete", path, e))
}

fn io_error(what: &str, path: &Path, e: io::Error) -> CordialError {
    CordialError::Io(format!("{what} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests;
