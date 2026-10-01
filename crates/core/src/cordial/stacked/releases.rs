//! Stacked's published releases on GitHub: which is the newest, and its
//! AppImage for this machine.

use std::path::Path;

use crate::cordial::CordialError;
use crate::github::{self, GithubClient, GithubError};

/// The fork's newest release: its version and the AppImage to download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// Where releases come from. Adapters: [`GithubReleases`], and a local
/// stand-in in the tests.
pub trait Releases: Send + Sync {
    fn latest(&self) -> Result<Release, CordialError>;
    /// The file at `url`, written to `to`.
    fn download(&self, url: &str, to: &Path) -> Result<(), CordialError>;
}

const REPO: &str = "DamnShabu/stacked";

/// The fork's releases on GitHub.
#[derive(Default)]
pub struct GithubReleases {
    github: GithubClient,
}

impl Releases for GithubReleases {
    fn latest(&self) -> Result<Release, CordialError> {
        let release = self.github.latest(REPO).map_err(stacked)?;
        appimage_for(release, std::env::consts::ARCH)
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), CordialError> {
        self.github.download(url, to).map_err(stacked)
    }
}

fn stacked(e: GithubError) -> CordialError {
    match e {
        GithubError::Io(why) => CordialError::Io(why),
        other => CordialError::Stacked(other.to_string()),
    }
}

/// The release GitHub describes, with its AppImage for `arch`
/// (`Stacked-<version>-<arch>.AppImage`).
pub fn parse_latest(json: &[u8], arch: &str) -> Result<Release, CordialError> {
    appimage_for(github::parse_release(json).map_err(stacked)?, arch)
}

fn appimage_for(release: github::Release, arch: &str) -> Result<Release, CordialError> {
    let suffix = format!("-{arch}.AppImage");
    let version = release.version;
    let asset =
        release.assets.into_iter().find(|a| a.name.ends_with(&suffix)).ok_or_else(|| {
            CordialError::Stacked(format!("release {version} has no AppImage for {arch}"))
        })?;
    Ok(Release { version, url: asset.url })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST_JSON: &str = r#"{
        "tag_name": "v0.21.0",
        "assets": [
            {"name": "stacked_0.21.0_amd64.deb", "browser_download_url": "https://x/deb"},
            {"name": "Stacked-0.21.0-aarch64.AppImage", "browser_download_url": "https://x/arm"},
            {"name": "Stacked-0.21.0-x86_64.AppImage", "browser_download_url": "https://x/amd"}
        ]
    }"#;

    #[test]
    fn the_appimage_for_this_machine_is_picked() {
        let got = parse_latest(LATEST_JSON.as_bytes(), "x86_64").unwrap();
        assert_eq!(got, Release { version: "0.21.0".into(), url: "https://x/amd".into() });
        assert_eq!(parse_latest(LATEST_JSON.as_bytes(), "aarch64").unwrap().url, "https://x/arm");
    }

    #[test]
    fn a_release_without_one_says_so() {
        let err = parse_latest(LATEST_JSON.as_bytes(), "riscv64").unwrap_err();
        assert_eq!(err, CordialError::Stacked("release 0.21.0 has no AppImage for riscv64".into()));
    }

    #[test]
    fn a_tag_that_is_no_directory_name_is_refused() {
        let json = r#"{"tag_name": "../../x", "assets": []}"#;
        assert!(parse_latest(json.as_bytes(), "x86_64").is_err());
    }
}
