//! Game icons: where Roblox serves them, and a cache on disk so each is
//! downloaded once. Icons are public; no cookie is sent for them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::RobloxError;
use super::http::{self, Request, Transport};

/// {universe id: icon url}, in one request, for the icons Roblox has finished
/// rendering.
pub(super) fn urls(
    t: &dyn Transport,
    universes: &[String],
) -> Result<HashMap<String, String>, RobloxError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Thumb {
        target_id: Option<u64>,
        state: Option<String>,
        image_url: Option<String>,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        data: Vec<Thumb>,
    }
    let ids: Vec<&str> = universes.iter().map(String::as_str).filter(|u| !u.is_empty()).collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let url = format!(
        "https://thumbnails.roblox.com/v1/games/icons?universeIds={}&size=150x150&format=Png&isCircular=false",
        ids.join(",")
    );
    let page: Page = http::ok(http::send(t, Request::get(url))?, false)?.json()?;
    Ok(page
        .data
        .into_iter()
        .filter(|d| d.state.as_deref() == Some("Completed"))
        .filter_map(|d| Some((d.target_id?.to_string(), d.image_url?)))
        .collect())
}

/// Icons on disk, one PNG per universe. Regenerable, so it lives in the cache.
pub struct IconCache {
    dir: PathBuf,
}

impl IconCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        IconCache { dir: dir.into() }
    }

    fn path(&self, universe: &str) -> Option<PathBuf> {
        // A universe id is digits; anything else must not become a path.
        (!universe.is_empty() && universe.bytes().all(|b| b.is_ascii_digit()))
            .then(|| self.dir.join(format!("{universe}.png")))
    }

    /// The icon if it is already on disk.
    pub fn cached(&self, universe: &str) -> Option<PathBuf> {
        self.path(universe).filter(|p| p.exists())
    }

    /// The icon, downloading it from `url` first when it is not on disk. None
    /// when there is none to be had: a missing icon is a placeholder tile,
    /// not a failure.
    pub fn fetch(&self, t: &dyn Transport, universe: &str, url: &str) -> Option<PathBuf> {
        if let Some(path) = self.cached(universe) {
            return Some(path);
        }
        let path = self.path(universe)?;
        let resp = http::ok(http::send(t, Request::get(url)).ok()?, false).ok()?;
        write_atomically(&path, &resp.body).ok()?;
        Some(path)
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("png.tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::http::canned::Canned;

    #[test]
    fn only_finished_icons_are_used() {
        let t = Canned::new().answer(
            200,
            r#"{"data": [
                {"targetId": 1, "state": "Completed", "imageUrl": "https://t/1.png"},
                {"targetId": 2, "state": "Pending", "imageUrl": null}
            ]}"#,
        );
        let got = urls(&t, &["1".into(), "2".into()]).unwrap();
        assert_eq!(got, HashMap::from([("1".to_string(), "https://t/1.png".to_string())]));
        assert_eq!(
            t.asked()[0].url,
            "https://thumbnails.roblox.com/v1/games/icons?universeIds=1,2&size=150x150&format=Png&isCircular=false"
        );
        assert!(t.asked()[0].cookie.is_none());
    }

    #[test]
    fn no_universes_asks_nothing() {
        let t = Canned::new();
        assert!(urls(&t, &[]).unwrap().is_empty());
    }

    #[test]
    fn an_icon_is_downloaded_once_and_then_read_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let cache = IconCache::new(dir.path());
        let t = Canned::new().answer(200, "PNGDATA");
        let path = cache.fetch(&t, "7", "https://t/7.png").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"PNGDATA");
        assert_eq!(cache.fetch(&t, "7", "https://t/7.png"), Some(path.clone()));
        assert_eq!(t.asked().len(), 1);
        assert_eq!(cache.cached("7"), Some(path));
    }

    #[test]
    fn a_failed_download_is_no_icon_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = IconCache::new(dir.path());
        let t = Canned::new().answer(404, "");
        assert_eq!(cache.fetch(&t, "7", "https://t/7.png"), None);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_universe_that_is_not_digits_is_never_a_path() {
        let cache = IconCache::new("/tmp/x");
        assert_eq!(cache.cached("../../etc/passwd"), None);
    }
}
