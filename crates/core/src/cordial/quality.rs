//! Roblox's own graphics-quality slider, set for a client's performance
//! level and given back to the player's own afterwards.
//!
//! The frame cap and throttle bound how often a client draws; this bounds
//! what each frame costs and what the game keeps loaded: render distance,
//! level of detail, shadows and texture detail all follow the slider. On
//! "Automatic" the engine sees a desktop GPU and picks near the top, which
//! is how a client along for the ride ends up holding 1.6-2 GB. The slider
//! lives in the engine's own preferences file inside the profile, which it
//! reads at start and writes back on exit -- the same file and the same
//! value as moving the slider in the game's menu, so nothing about the
//! client itself is touched.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::CordialError;

/// The slider ("SavedQualityLevel", 0 is Automatic, 1-10 its steps) and the
/// level the engine last ran at. Both are set, so the first frame is already
/// at the level whichever one this build reads first.
const KEYS: [&str; 2] = ["SavedQualityLevel", "GraphicsQualityLevel"];

/// The key in the saved file recording the level the manager set, so a
/// level moved since in the game's menu is told apart and kept. Not a
/// property name. A file without it was written when only the lowest step
/// was ever set.
const SET: &str = "set";

/// What a file without [`SET`] was set to.
const LOWEST: &str = "1";

/// The engine's preferences file in a profile directory.
pub fn settings_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join("data/files/appData/GlobalBasicSettings_13.xml")
}

/// Where the values the slider had before the manager set it are kept.
fn saved_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join("rbxmgr-quality.json")
}

/// Turn the slider to `level`, remembering what it was the first time.
/// A profile the engine has not run in yet has no file; its first run is at
/// the engine's own choice and the next one at `level`.
pub fn set(profile_dir: &Path, level: &str) -> Result<(), CordialError> {
    let path = settings_path(profile_dir);
    let Some(xml) = read(&path)? else { return Ok(()) };
    let saved = saved_path(profile_dir);
    let mut record = read_saved(&saved)?.unwrap_or_else(|| {
        KEYS.iter().filter_map(|k| Some(((*k).to_owned(), value(&xml, k)?.to_owned()))).collect()
    });
    record.insert(SET.to_owned(), level.to_owned());
    crate::json_file::write(&saved, &record)
        .map_err(|e| io(&format!("could not write {}", saved.display()), e))?;
    let changed = KEYS.iter().fold(xml.clone(), |xml, k| with_value(&xml, k, level));
    write_if_changed(&path, &xml, &changed)
}

/// Put back what [`set`] found, for a client that leaves the slider to the
/// player. A value changed since from the game's menu is the player's, and
/// stays.
pub fn restore(profile_dir: &Path) -> Result<(), CordialError> {
    let saved = saved_path(profile_dir);
    let Some(mut before) = read_saved(&saved)? else { return Ok(()) };
    let ours = before.remove(SET).unwrap_or_else(|| LOWEST.to_owned());
    let path = settings_path(profile_dir);
    if let Some(xml) = read(&path)? {
        let restored = before.iter().fold(xml.clone(), |xml, (k, v)| match value(&xml, k) {
            Some(now) if now == ours => with_value(&xml, k, v),
            _ => xml,
        });
        write_if_changed(&path, &xml, &restored)?;
    }
    fs::remove_file(&saved).map_err(|e| io(&format!("could not remove {}", saved.display()), e))
}

/// The player's own levels, as first found. The only record of them: one
/// that does not read is moved aside rather than taken as empty and deleted,
/// and one that cannot be read at all is an error, never a reason to remove
/// it.
fn read_saved(saved: &Path) -> Result<Option<BTreeMap<String, String>>, CordialError> {
    crate::json_file::read_owned::<BTreeMap<String, String>>(saved)
        .map(|owned| owned.value)
        .map_err(|e| CordialError::Io(format!("could not read the saved quality: {e}")))
}

/// The file's text, or None when there is none yet.
fn read(path: &Path) -> Result<Option<String>, CordialError> {
    match fs::read_to_string(path) {
        Ok(xml) => Ok(Some(xml)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(&format!("could not read {}", path.display()), e)),
    }
}

/// Leave the file alone, timestamp included, when nothing changes. It holds
/// every in-game setting (volume, keybinds, sensitivity), so it is replaced
/// whole: a crash mid-write must not leave the engine half a file to reset.
fn write_if_changed(path: &Path, old: &str, new: &str) -> Result<(), CordialError> {
    if old == new {
        return Ok(());
    }
    crate::json_file::write_bytes(path, new.as_bytes())
        .map_err(|e| io(&format!("could not write {}", path.display()), e))
}

/// The value of the property named `key`: `<int name="key">10</int>`.
fn value<'a>(xml: &'a str, key: &str) -> Option<&'a str> {
    let (start, end) = span(xml, key)?;
    Some(&xml[start..end])
}

/// `xml` with `key`'s value replaced. A property the file does not have is
/// not added: the engine writes every one it reads, so a missing one is one
/// this build does not use.
fn with_value(xml: &str, key: &str, new: &str) -> String {
    match span(xml, key) {
        Some((start, end)) => format!("{}{new}{}", &xml[..start], &xml[end..]),
        None => xml.to_owned(),
    }
}

/// Where `key`'s value sits in `xml`, whatever type tag carries it.
fn span(xml: &str, key: &str) -> Option<(usize, usize)> {
    let marker = format!(" name=\"{key}\">");
    let start = xml.find(&marker)? + marker.len();
    let end = start + xml[start..].find('<')?;
    Some((start, end))
}

fn io(what: &str, e: std::io::Error) -> CordialError {
    CordialError::Io(format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<roblox version="4">
	<Item class="UserGameSettings" referent="RBX0">
		<Properties>
			<int name="GraphicsQualityLevel">8</int>
			<float name="MasterVolume">0.5</float>
			<token name="SavedQualityLevel">0</token>
		</Properties>
	</Item>
</roblox>"#;

    fn profile(xml: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if let Some(xml) = xml {
            let path = settings_path(dir.path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, xml).unwrap();
        }
        dir
    }

    fn settings(dir: &tempfile::TempDir) -> String {
        fs::read_to_string(settings_path(dir.path())).unwrap()
    }

    #[test]
    fn low_power_turns_the_slider_down_and_back() {
        let dir = profile(Some(XML));
        set(dir.path(), LOWEST).unwrap();
        let low = settings(&dir);
        assert_eq!(value(&low, "SavedQualityLevel"), Some("1"));
        assert_eq!(value(&low, "GraphicsQualityLevel"), Some("1"));
        assert_eq!(value(&low, "MasterVolume"), Some("0.5"), "nothing else touched");
        restore(dir.path()).unwrap();
        assert_eq!(settings(&dir), XML);
        assert!(!saved_path(dir.path()).exists());
    }

    #[test]
    fn launching_low_twice_still_remembers_the_players_own_level() {
        let dir = profile(Some(XML));
        set(dir.path(), LOWEST).unwrap();
        set(dir.path(), LOWEST).unwrap();
        restore(dir.path()).unwrap();
        assert_eq!(settings(&dir), XML);
    }

    #[test]
    fn a_level_chosen_in_the_game_meanwhile_is_kept() {
        let dir = profile(Some(XML));
        set(dir.path(), LOWEST).unwrap();
        let moved = with_value(&settings(&dir), "SavedQualityLevel", "5");
        fs::write(settings_path(dir.path()), moved).unwrap();
        restore(dir.path()).unwrap();
        let after = settings(&dir);
        assert_eq!(value(&after, "SavedQualityLevel"), Some("5"));
        assert_eq!(value(&after, "GraphicsQualityLevel"), Some("8"));
    }

    #[test]
    fn a_profile_the_engine_never_ran_in_is_left_to_it() {
        let dir = profile(None);
        set(dir.path(), LOWEST).unwrap();
        restore(dir.path()).unwrap();
        assert!(!settings_path(dir.path()).exists());
        assert!(!saved_path(dir.path()).exists());
    }

    #[test]
    fn a_client_never_low_power_is_not_touched() {
        let dir = profile(Some(XML));
        restore(dir.path()).unwrap();
        assert_eq!(settings(&dir), XML);
    }

    #[test]
    fn a_saved_level_that_does_not_read_is_kept_not_deleted() {
        let dir = profile(Some(XML));
        set(dir.path(), LOWEST).unwrap();
        let saved = saved_path(dir.path());
        fs::write(&saved, "{not json").unwrap();
        restore(dir.path()).unwrap();
        let kept: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("rbxmgr-quality.json.bad-"))
            .collect();
        assert_eq!(kept.len(), 1, "set aside for the player to recover");
    }

    #[test]
    fn a_property_the_file_lacks_is_not_invented() {
        let only = XML.replace("\t\t\t<int name=\"GraphicsQualityLevel\">8</int>\n", "");
        let dir = profile(Some(&only));
        set(dir.path(), LOWEST).unwrap();
        assert_eq!(value(&settings(&dir), "GraphicsQualityLevel"), None);
        restore(dir.path()).unwrap();
        assert_eq!(settings(&dir), only);
    }
}
