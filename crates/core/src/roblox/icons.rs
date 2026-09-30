//! Game icons and account headshots: where Roblox serves them, and a cache
//! on disk so each is downloaded once. Both are public; no cookie is sent
//! for them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::RobloxError;
use super::http::{self, Request, Transport};
use crate::types::UserId;

/// {universe id: icon url} for the icons Roblox has finished rendering.
pub(super) fn urls(
    t: &dyn Transport,
    universes: &[String],
) -> Result<HashMap<String, String>, RobloxError> {
    rendered(t, "https://thumbnails.roblox.com/v1/games/icons?universeIds=", universes)
}

/// {user id: headshot url} for the headshots Roblox has finished rendering.
pub(super) fn headshot_urls(
    t: &dyn Transport,
    users: &[UserId],
) -> Result<HashMap<UserId, String>, RobloxError> {
    let ids: Vec<String> = users.iter().map(ToString::to_string).collect();
    let found =
        rendered(t, "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds=", &ids)?;
    Ok(found.into_iter().filter_map(|(id, url)| Some((UserId(id.parse().ok()?), url))).collect())
}

/// {id: image url} from one of the thumbnails endpoints, which all answer
/// the same shape. Every account's favourites together repeat games, and the
/// endpoints take a bounded number of ids: each is asked for once, in
/// batches.
fn rendered(
    t: &dyn Transport,
    endpoint: &str,
    targets: &[String],
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
    let mut ids: Vec<&str> = targets.iter().map(String::as_str).filter(|u| !u.is_empty()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut found = HashMap::new();
    for batch in ids.chunks(BATCH) {
        let url = format!("{endpoint}{}&size=150x150&format=Png&isCircular=false", batch.join(","));
        let page: Page = http::ok(http::send(t, Request::get(url))?, false)?.json()?;
        found.extend(
            page.data
                .into_iter()
                .filter(|d| d.state.as_deref() == Some("Completed"))
                .filter_map(|d| Some((d.target_id?.to_string(), d.image_url?))),
        );
    }
    Ok(found)
}

/// The ids per request the thumbnails endpoints take.
const BATCH: usize = 100;

/// Images on disk, one PNG per universe (game icons) or per user
/// (headshots). Regenerable, so it lives in the cache.
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

    /// The icon, downloading it from `url` first when it is not on disk. A
    /// failure is for the log: the tile shows its placeholder either way.
    pub fn fetch(
        &self,
        t: &dyn Transport,
        universe: &str,
        url: &str,
    ) -> Result<PathBuf, IconError> {
        if let Some(path) = self.cached(universe) {
            return Ok(path);
        }
        self.refresh(t, universe, url)
    }

    /// Download the image from `url` whether or not one is on disk: for a
    /// headshot, which changes with the avatar. The old one stays until the
    /// new one is whole.
    pub fn refresh(&self, t: &dyn Transport, key: &str, url: &str) -> Result<PathBuf, IconError> {
        let path = self.path(key).ok_or_else(|| IconError::NotAUniverse(key.to_owned()))?;
        let resp = http::ok(http::send(t, Request::get(url))?, false)?;
        write_atomically(&path, &resp.body).map_err(|e| IconError::Write(e.to_string()))?;
        Ok(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IconError {
    #[error("could not download the icon: {0}")]
    Fetch(#[from] RobloxError),
    #[error("could not save the icon: {0}")]
    Write(String),
    #[error("{0:?} is not a universe or user id")]
    NotAUniverse(String),
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
    fn each_universe_is_asked_for_once_in_batches_of_100() {
        let mut universes: Vec<String> = (1..=150).map(|n| n.to_string()).collect();
        universes.extend(["1".to_string(), "2".to_string()]);
        let t = Canned::new().answer(200, r#"{"data": []}"#).answer(200, r#"{"data": []}"#);
        urls(&t, &universes).unwrap();
        let asked: Vec<usize> = t
            .asked()
            .iter()
            .map(|r| r.url.split("universeIds=").nth(1).unwrap().split('&').next().unwrap())
            .map(|ids| ids.split(',').count())
            .collect();
        assert_eq!(asked, [100, 50]);
    }

    #[test]
    fn headshots_are_asked_for_by_user_id_and_come_back_keyed_by_it() {
        let t = Canned::new().answer(
            200,
            r#"{"data": [
                {"targetId": 7, "state": "Completed", "imageUrl": "https://t/7.png"},
                {"targetId": 8, "state": "Blocked", "imageUrl": "https://t/blocked.png"}
            ]}"#,
        );
        let got = headshot_urls(&t, &[UserId(8), UserId(7)]).unwrap();
        assert_eq!(got, HashMap::from([(UserId(7), "https://t/7.png".to_string())]));
        assert_eq!(
            t.asked()[0].url,
            "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds=7,8&size=150x150&format=Png&isCircular=false"
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
        assert_eq!(cache.fetch(&t, "7", "https://t/7.png").unwrap(), path.clone());
        assert_eq!(t.asked().len(), 1);
        assert_eq!(cache.cached("7"), Some(path));
    }

    #[test]
    fn a_refresh_downloads_again_over_the_old_image() {
        let dir = tempfile::tempdir().unwrap();
        let cache = IconCache::new(dir.path());
        let t = Canned::new().answer(200, "OLD").answer(200, "NEW");
        cache.fetch(&t, "7", "https://t/7.png").unwrap();
        let path = cache.refresh(&t, "7", "https://t/7.png").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"NEW");
        assert_eq!(t.asked().len(), 2);
    }

    #[test]
    fn a_failed_download_says_why_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = IconCache::new(dir.path());
        let t = Canned::new().answer(404, "");
        assert!(matches!(
            cache.fetch(&t, "7", "https://t/7.png"),
            Err(IconError::Fetch(RobloxError::Http { status: 404, .. }))
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_cache_that_cannot_be_written_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-dir");
        fs::write(&blocker, "").unwrap();
        let cache = IconCache::new(&blocker);
        let t = Canned::new().answer(200, "PNG");
        assert!(matches!(cache.fetch(&t, "7", "https://t/7.png"), Err(IconError::Write(_))));
    }

    #[test]
    fn a_universe_that_is_not_digits_is_never_a_path() {
        let cache = IconCache::new("/tmp/x");
        assert_eq!(cache.cached("../../etc/passwd"), None);
    }
}
