//! Stacked, the fork the engine comes from, at its newest commit rather than
//! the one this build pinned. Updating builds the fork's own flake with Nix
//! into `<data>/rbxmgr/stacked` (a link Nix keeps as a GC root); from then on
//! launches run that `cordial-run` instead of the bundled one. Delete the link
//! to go back.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::CordialError;
use super::process::{ProcessView, Runner, last_line};
use crate::paths::Paths;

/// The fork's flake. Its engine builds from git submodules, which a flake
/// leaves out unless asked.
pub const FLAKE: &str = "github:DamnShabu/stacked?submodules=1";

/// A build from source: Rust, the bionic linker and the JNI layer.
const BUILD_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// The engine launches run: the updated Stacked when there is one, else the
/// `cordial-run` this install came with, found on PATH.
pub fn engine_program(paths: &Paths) -> String {
    let updated = updated_engine(&paths.stacked());
    if updated.is_file() { updated.display().to_string() } else { "cordial-run".into() }
}

fn updated_engine(link: &Path) -> PathBuf {
    link.join("bin/cordial-run")
}

/// Build the newest Stacked and make launches use it. Returns the version
/// built, as its store path names it.
pub fn update(
    runner: &dyn Runner,
    paths: &Paths,
    view: ProcessView,
) -> Result<String, CordialError> {
    if view == ProcessView::Host {
        return Err(CordialError::Stacked(
            "a Flatpak cannot run a Nix build; its Stacked comes with the Flatpak".into(),
        ));
    }
    let link = paths.stacked();
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir)
            .map_err(|e| CordialError::Io(format!("could not create {}: {e}", dir.display())))?;
    }
    let argv: Vec<String> = [
        "nix",
        "--extra-experimental-features",
        "nix-command flakes",
        "build",
        // Ask GitHub for the newest commit, not the one Nix saw last.
        "--refresh",
        "--print-out-paths",
        "--out-link",
    ]
    .into_iter()
    .map(String::from)
    .chain([link.display().to_string(), FLAKE.to_owned()])
    .collect();
    let out = runner
        .run(&argv, BUILD_TIMEOUT)
        .map_err(|e| CordialError::Stacked(format!("updating needs Nix ({e})")))?;
    if !out.success() {
        return Err(CordialError::Stacked(nix_error(&out.stderr, out.status)));
    }
    let built = last_line(&out.stdout);
    if !updated_engine(&link).is_file() {
        return Err(CordialError::Stacked(format!("{built} has no bin/cordial-run")));
    }
    Ok(version(&built))
}

/// `/nix/store/<hash>-stacked-0.21.0` is 0.21.0; anything else as it is.
fn version(store_path: &str) -> String {
    let name = store_path.rsplit('/').next().unwrap_or(store_path);
    match name.split_once("-stacked-") {
        Some((_, v)) if !v.is_empty() => v.to_owned(),
        _ => name.to_owned(),
    }
}

/// Nix ends a failure with context lines after its `error:` line; the error
/// is the reason.
fn nix_error(stderr: &[u8], status: i32) -> String {
    let text = String::from_utf8_lossy(stderr);
    let error = text.lines().rev().map(str::trim).find(|l| l.starts_with("error:"));
    match error {
        Some(line) => line.to_owned(),
        None => {
            let last = last_line(stderr);
            if last.is_empty() { format!("nix exited with status {status}") } else { last }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cordial::process::recording::Recording;

    fn sandbox() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        (dir, paths)
    }

    /// What a successful `nix build --out-link` leaves: the link, with the
    /// engine in it.
    fn built(paths: &Paths) {
        let bin = paths.stacked().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("cordial-run"), "").unwrap();
    }

    #[test]
    fn launches_use_the_bundled_engine_until_an_update() {
        let (_dir, paths) = sandbox();
        assert_eq!(engine_program(&paths), "cordial-run");
        built(&paths);
        assert_eq!(
            engine_program(&paths),
            paths.stacked().join("bin/cordial-run").display().to_string()
        );
    }

    #[test]
    fn updating_builds_the_newest_commit_of_the_fork_into_the_link() {
        let (_dir, paths) = sandbox();
        built(&paths);
        let r = Recording::default().answer(0, "/nix/store/abc-stacked-0.21.0\n", "");
        assert_eq!(update(&r, &paths, ProcessView::Own).unwrap(), "0.21.0");
        let argv = &r.ran()[0];
        assert_eq!(argv[0], "nix");
        assert!(argv.contains(&"--refresh".to_string()));
        assert_eq!(argv[argv.len() - 2], paths.stacked().display().to_string());
        assert_eq!(argv[argv.len() - 1], FLAKE);
    }

    #[test]
    fn a_failed_build_says_nixs_reason() {
        let (_dir, paths) = sandbox();
        let stderr =
            "building...\nerror: builder for '/nix/store/x.drv' failed\n       … while building\n";
        let r = Recording::default().answer(1, "", stderr);
        assert_eq!(
            update(&r, &paths, ProcessView::Own).unwrap_err(),
            CordialError::Stacked("error: builder for '/nix/store/x.drv' failed".into())
        );
        assert_eq!(engine_program(&paths), "cordial-run");
    }

    #[test]
    fn a_flatpak_does_not_try() {
        let (_dir, paths) = sandbox();
        let r = Recording::default();
        assert!(update(&r, &paths, ProcessView::Host).is_err());
        assert!(r.ran().is_empty());
    }

    #[test]
    fn the_version_comes_from_the_store_path() {
        assert_eq!(version("/nix/store/abc-stacked-0.21.0"), "0.21.0");
        assert_eq!(version("/nix/store/abc-other"), "abc-other");
    }
}
