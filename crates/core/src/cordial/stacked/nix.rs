//! Stacked built by Nix, for a host that cannot run a downloaded program
//! (NixOS has no /lib64 loader). The fork's own flake installs `cordial-run`
//! already wrapped with the libraries it loads.

use std::path::Path;
use std::time::Duration;

use crate::cordial::CordialError;
use crate::cordial::process::{Runner, last_line};

/// The fork's flake. Its engine builds from git submodules, which a flake
/// leaves out unless asked.
pub const FLAKE: &str = "github:DamnShabu/stacked?submodules=1";

/// A build from source: Rust, the bionic linker and the JNI layer.
const BUILD_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// Build the fork's newest commit, linked at `out_link` (which Nix keeps as
/// a GC root). Returns the version, as the store path names it.
pub fn build(runner: &dyn Runner, out_link: &Path) -> Result<String, CordialError> {
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
    .chain([out_link.display().to_string(), FLAKE.to_owned()])
    .collect();
    let out = runner.run(&argv, BUILD_TIMEOUT).map_err(|e| {
        CordialError::Stacked(format!(
            "this system runs only programs built for it, and Nix could not be run ({e})"
        ))
    })?;
    if !out.success() {
        return Err(CordialError::Stacked(nix_error(&out.stderr, out.status)));
    }
    Ok(version(&last_line(&out.stdout)))
}

/// `/nix/store/<hash>-stacked-0.21.0` is 0.21.0; anything else as it is.
pub(super) fn version(store_path: &str) -> String {
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

    #[test]
    fn the_newest_commit_is_built_into_the_link() {
        let r = Recording::default().answer(0, "/nix/store/abc-stacked-0.21.0\n", "");
        assert_eq!(build(&r, Path::new("/d/nix")).unwrap(), "0.21.0");
        let argv = &r.ran()[0];
        assert_eq!(argv[0], "nix");
        assert!(argv.contains(&"--refresh".to_string()));
        assert_eq!(argv[argv.len() - 2], "/d/nix");
        assert_eq!(argv[argv.len() - 1], FLAKE);
    }

    #[test]
    fn a_failed_build_says_nixs_reason() {
        let stderr =
            "building...\nerror: builder for '/nix/store/x.drv' failed\n       … while building\n";
        let r = Recording::default().answer(1, "", stderr);
        assert_eq!(
            build(&r, Path::new("/d/nix")).unwrap_err(),
            CordialError::Stacked("error: builder for '/nix/store/x.drv' failed".into())
        );
    }

    #[test]
    fn the_version_comes_from_the_store_path() {
        assert_eq!(version("/nix/store/abc-stacked-0.21.0"), "0.21.0");
        assert_eq!(version("/nix/store/abc-other"), "abc-other");
    }
}
