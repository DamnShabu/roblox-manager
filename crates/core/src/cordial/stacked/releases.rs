//! Stacked's published releases on GitHub: which is the newest, and its
//! AppImage for this machine.

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::cordial::CordialError;

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

const LATEST: &str = "https://api.github.com/repos/DamnShabu/stacked/releases/latest";

/// An AppImage is about a hundred megabytes, on whatever line there is.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3600);

/// HTTPS to GitHub through ureq.
pub struct GithubReleases {
    agent: ureq::Agent,
}

impl Default for GithubReleases {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            // GitHub's API turns away a request without one.
            .user_agent(concat!("roblox-manager/", env!("CARGO_PKG_VERSION")))
            .build();
        GithubReleases { agent: ureq::Agent::new_with_config(config) }
    }
}

impl Releases for GithubReleases {
    fn latest(&self) -> Result<Release, CordialError> {
        let mut resp = self.agent.get(LATEST).call().map_err(offline)?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(CordialError::Stacked(format!(
                "GitHub answered HTTP {status} when asked for the newest release"
            )));
        }
        let body = resp.body_mut().read_to_vec().map_err(offline)?;
        parse_latest(&body, std::env::consts::ARCH)
    }

    fn download(&self, url: &str, to: &Path) -> Result<(), CordialError> {
        let resp = self.agent.get(url).call().map_err(offline)?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(CordialError::Stacked(format!("the download answered HTTP {status}")));
        }
        let mut file = File::create(to)
            .map_err(|e| CordialError::Io(format!("could not create {}: {e}", to.display())))?;
        let (_, body) = resp.into_parts();
        io::copy(&mut body.into_reader(), &mut file)
            .map_err(|e| CordialError::Stacked(format!("the download broke off: {e}")))?;
        file.sync_all()
            .map_err(|e| CordialError::Io(format!("could not write {}: {e}", to.display())))
    }
}

fn offline(e: ureq::Error) -> CordialError {
    CordialError::Stacked(format!("GitHub could not be reached: {e}"))
}

/// The release GitHub describes, with its AppImage for `arch`
/// (`Stacked-<version>-<arch>.AppImage`).
pub fn parse_latest(json: &[u8], arch: &str) -> Result<Release, CordialError> {
    #[derive(Deserialize)]
    struct Latest {
        tag_name: String,
        #[serde(default)]
        assets: Vec<Asset>,
    }
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
    }
    let latest: Latest = serde_json::from_slice(json)
        .map_err(|e| CordialError::Stacked(format!("GitHub's answer was not a release ({e})")))?;
    let version = latest.tag_name.trim_start_matches('v').to_owned();
    // It names a directory, so nothing that could leave it.
    let usable = version.starts_with(|c: char| c.is_ascii_alphanumeric())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c));
    if !usable {
        return Err(CordialError::Stacked(format!(
            "the newest release has an unusable tag, {:?}",
            latest.tag_name
        )));
    }
    let suffix = format!("-{arch}.AppImage");
    let asset = latest.assets.into_iter().find(|a| a.name.ends_with(&suffix)).ok_or_else(|| {
        CordialError::Stacked(format!("release {version} has no AppImage for {arch}"))
    })?;
    Ok(Release { version, url: asset.browser_download_url })
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
