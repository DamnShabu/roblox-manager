//! GitHub's published releases: a repository's newest one, its files, and
//! downloading them. Stacked's updates and the app's own both come from here.

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

/// A published release: its version (the tag, without a leading `v`) and
/// its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub assets: Vec<Asset>,
}

/// One file of a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

impl Release {
    /// The file named `name`.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GithubError {
    #[error("GitHub could not be reached: {0}")]
    Offline(String),
    #[error("GitHub answered HTTP {0} when asked for the newest release")]
    Status(u16),
    #[error("GitHub's answer was not a release ({0})")]
    NotARelease(String),
    #[error("the newest release has an unusable tag, {0:?}")]
    BadTag(String),
    #[error("the download answered HTTP {0}")]
    DownloadStatus(u16),
    #[error("the download broke off: {0}")]
    BrokenOff(String),
    #[error("{0}")]
    Io(String),
}

/// A release's file can be a few hundred megabytes, on whatever line there is.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3600);
/// The newest release's description is a few kilobytes: a check that has
/// not had it in this long is stuck (a captive portal, a proxy that holds
/// the connection open), and would otherwise hold Update for the hour above.
const ASK_TIMEOUT: Duration = Duration::from_secs(30);
/// No host worth waiting for takes this long to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Far past any file a release here ships (the largest is the Flatpak
/// bundle, well under a gigabyte): a download growing past it is not one,
/// and stops before it fills the disk.
const LARGEST_DOWNLOAD: u64 = 2 << 30;

/// HTTPS to GitHub through ureq.
pub struct GithubClient {
    agent: ureq::Agent,
}

impl Default for GithubClient {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            // GitHub's API turns away a request without one.
            .user_agent(concat!("roblox-manager/", env!("CARGO_PKG_VERSION")))
            .build();
        GithubClient { agent: ureq::Agent::new_with_config(config) }
    }
}

impl GithubClient {
    /// The newest release of `repo` (`owner/name`). Drafts and pre-releases
    /// are not "latest" to GitHub, so they are never offered.
    pub fn latest(&self, repo: &str) -> Result<Release, GithubError> {
        let url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let mut resp = self
            .agent
            .get(&url)
            .config()
            .timeout_global(Some(ASK_TIMEOUT))
            .build()
            .call()
            .map_err(offline)?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(GithubError::Status(status));
        }
        let body = resp.body_mut().read_to_vec().map_err(offline)?;
        parse_release(&body)
    }

    /// The file at `url`, written to `to`.
    pub fn download(&self, url: &str, to: &Path) -> Result<(), GithubError> {
        let resp = self.agent.get(url).call().map_err(offline)?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(GithubError::DownloadStatus(status));
        }
        let mut file = File::create(to)
            .map_err(|e| GithubError::Io(format!("could not create {}: {e}", to.display())))?;
        let (_, body) = resp.into_parts();
        io::copy(&mut body.into_with_config().limit(LARGEST_DOWNLOAD).reader(), &mut file)
            .map_err(|e| GithubError::BrokenOff(e.to_string()))?;
        file.sync_all()
            .map_err(|e| GithubError::Io(format!("could not write {}: {e}", to.display())))
    }
}

fn offline(e: ureq::Error) -> GithubError {
    GithubError::Offline(e.to_string())
}

/// The release GitHub describes. Its version names directories and files,
/// so a tag that could leave one is refused.
pub fn parse_release(json: &[u8]) -> Result<Release, GithubError> {
    #[derive(Deserialize)]
    struct Latest {
        tag_name: String,
        #[serde(default)]
        assets: Vec<RawAsset>,
    }
    #[derive(Deserialize)]
    struct RawAsset {
        name: String,
        browser_download_url: String,
    }
    let latest: Latest =
        serde_json::from_slice(json).map_err(|e| GithubError::NotARelease(e.to_string()))?;
    let version = latest.tag_name.trim_start_matches('v').to_owned();
    let usable = version.starts_with(|c: char| c.is_ascii_alphanumeric())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c));
    if !usable {
        return Err(GithubError::BadTag(latest.tag_name));
    }
    let assets = latest
        .assets
        .into_iter()
        .map(|a| Asset { name: a.name, url: a.browser_download_url })
        .collect();
    Ok(Release { version, assets })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_reads_with_its_files() {
        let json = r#"{"tag_name": "v0.3.0", "assets": [
            {"name": "SHA256SUMS", "browser_download_url": "https://x/sums"}
        ]}"#;
        let got = parse_release(json.as_bytes()).unwrap();
        assert_eq!(got.version, "0.3.0");
        assert_eq!(got.asset("SHA256SUMS").unwrap().url, "https://x/sums");
        assert_eq!(got.asset("other"), None);
    }

    #[test]
    fn a_tag_that_is_no_file_name_is_refused() {
        let json = r#"{"tag_name": "../../x", "assets": []}"#;
        assert_eq!(parse_release(json.as_bytes()), Err(GithubError::BadTag("../../x".into())));
        assert!(matches!(parse_release(b"{}"), Err(GithubError::NotARelease(_))));
    }
}
