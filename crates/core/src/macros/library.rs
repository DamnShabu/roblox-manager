//! Every macro by name: its text, whether it is switched on, and its hotkey.
//! macros.json, read once and written whole on every change.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::MacroError;
use super::grammar;
use crate::json_file;

/// A plain macro is stored as a bare string; one switched off or with a
/// hotkey as `{"text", "enabled", "hotkey"}`; any other object is the earlier
/// macro manager's entry, carried over by [`migrate_legacy`]. Main thread only.
#[derive(Debug)]
pub struct MacroLibrary {
    path: PathBuf,
    text: BTreeMap<String, String>,
    off: BTreeSet<String>,
    hotkeys: BTreeMap<String, String>,
    set_aside: Option<PathBuf>,
}

impl MacroLibrary {
    pub fn load(path: &Path) -> Self {
        let mut lib = MacroLibrary {
            path: path.to_owned(),
            text: BTreeMap::new(),
            off: BTreeSet::new(),
            hotkeys: BTreeMap::new(),
            set_aside: None,
        };
        let owned = json_file::read_owned::<Map<String, Value>>(path);
        lib.set_aside = owned.set_aside;
        let stored = owned.value.unwrap_or_default();
        for (name, entry) in stored {
            let text = match &entry {
                Value::String(text) => text.clone(),
                Value::Object(e) => match e.get("text") {
                    Some(Value::String(text)) => {
                        if e.get("enabled") == Some(&Value::Bool(false)) {
                            lib.off.insert(name.clone());
                        }
                        if let Some(Value::String(key)) = e.get("hotkey").filter(|k| k != &"") {
                            lib.hotkeys.insert(name.clone(), key.clone());
                        }
                        text.clone()
                    }
                    Some(_) => continue,
                    None => migrate_legacy(e),
                },
                _ => continue,
            };
            lib.text.insert(name, text);
        }
        lib
    }

    /// Where a macros.json that did not parse was moved on load.
    pub fn set_aside(&self) -> &[PathBuf] {
        self.set_aside.as_slice()
    }

    /// The names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.text.keys().map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.text.contains_key(name)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.text.get(name).map(String::as_str)
    }

    /// A switched-off macro cannot run.
    pub fn enabled(&self, name: &str) -> bool {
        !self.off.contains(name)
    }

    /// Its GTK accelerator, if it has one.
    pub fn hotkey(&self, name: &str) -> Option<&str> {
        self.hotkeys.get(name).map(String::as_str)
    }

    pub fn hotkeys(&self) -> impl Iterator<Item = (&str, &str)> {
        self.hotkeys.iter().map(|(n, k)| (n.as_str(), k.as_str()))
    }

    /// Create (`old` None) or replace `old`, renaming it to `new`: its switch
    /// and hotkey go with it. The text must parse.
    pub fn save(
        &mut self,
        old: Option<&str>,
        new: &str,
        text: &str,
        hotkey: Option<&str>,
    ) -> Result<(), MacroError> {
        let new = new.trim();
        if new.is_empty() {
            return Err(MacroError::Name("A macro needs a name".into()));
        }
        if old != Some(new) && self.contains(new) {
            return Err(MacroError::Name(format!("A macro called {new} already exists")));
        }
        let hotkey = hotkey.filter(|k| !k.is_empty());
        if let Some(key) = hotkey {
            if let Some(by) =
                self.hotkeys.iter().find(|(n, k)| *k == key && Some(n.as_str()) != old)
            {
                return Err(MacroError::HotkeyTaken { hotkey: key.to_owned(), by: by.0.clone() });
            }
        }
        grammar::parse(text)?;
        if let Some(old) = old.filter(|o| *o != new) {
            self.text.remove(old);
            self.hotkeys.remove(old);
            if self.off.remove(old) {
                self.off.insert(new.to_owned());
            }
        }
        self.text.insert(new.to_owned(), text.to_owned());
        match hotkey {
            Some(key) => self.hotkeys.insert(new.to_owned(), key.to_owned()),
            None => self.hotkeys.remove(new),
        };
        self.write()
    }

    pub fn set_enabled(&mut self, name: &str, on: bool) -> Result<(), MacroError> {
        if on {
            self.off.remove(name);
        } else if self.contains(name) {
            self.off.insert(name.to_owned());
        }
        self.write()
    }

    pub fn delete(&mut self, name: &str) -> Result<(), MacroError> {
        self.text.remove(name);
        self.off.remove(name);
        self.hotkeys.remove(name);
        self.write()
    }

    fn write(&self) -> Result<(), MacroError> {
        let entries: Map<String, Value> = self
            .text
            .iter()
            .map(|(name, text)| {
                let hotkey = self.hotkeys.get(name);
                let entry = if self.enabled(name) && hotkey.is_none() {
                    Value::String(text.clone())
                } else {
                    let mut e = json!({"text": text, "enabled": self.enabled(name)});
                    if let (Some(key), Value::Object(e)) = (hotkey, &mut e) {
                        e.insert("hotkey".into(), Value::String(key.clone()));
                    }
                    e
                };
                (name.clone(), entry)
            })
            .collect();
        json_file::write(&self.path, &entries).map_err(|e| MacroError::Io(e.to_string()))
    }
}

/// The earlier macro manager's entry -- `{"script", "start_delay", ...}`,
/// with `key K`, `wait range(A,B)` and a closing bare `loop` -- as this one's
/// text. Its other fields (hidden, place) have no counterpart.
pub fn migrate_legacy(old: &Map<String, Value>) -> String {
    let mut lines = Vec::new();
    if let Some(delay) = old.get("start_delay").filter(|d| d.as_f64().is_some_and(|d| d > 0.0)) {
        lines.push(format!("start {delay}"));
    }
    let script = old.get("script").and_then(Value::as_str).unwrap_or_default();
    for line in script.lines().map(str::trim) {
        // Repeating until stopped is the default here.
        if line.is_empty() || line == "loop" {
            continue;
        }
        let line = match line.strip_prefix("key") {
            Some(rest) if rest.starts_with(char::is_whitespace) => {
                format!("tap {}", rest.trim_start())
            }
            _ => line.to_owned(),
        };
        lines.push(legacy_ranges(&line));
    }
    lines.join("\n").trim().to_owned() + "\n"
}

/// `range(A,B)` as `A-B`.
fn legacy_ranges(line: &str) -> String {
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("range(") {
        let after = &rest[at + "range(".len()..];
        let pair = after.find(')').and_then(|end| {
            let (a, b) = after[..end].split_once(',')?;
            let (a, b) = (a.trim(), b.trim());
            (numeric(a) && numeric(b)).then(|| (format!("{a}-{b}"), end))
        });
        match pair {
            Some((range, end)) => {
                out.push_str(&rest[..at]);
                out.push_str(&range);
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[..at + "range(".len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn legacy() -> Value {
        json!({"script": "\nwait range(60,70)\nkey j\nwait range(340,341)\nkey j\nloop",
               "start_delay": 45, "hidden": false, "place_id": "", "place_name": ""})
    }

    fn library(contents: Value) -> (tempfile::TempDir, MacroLibrary) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("macros.json");
        json_file::write(&path, &contents).unwrap();
        let lib = MacroLibrary::load(&path);
        (dir, lib)
    }

    fn on_disk(dir: &tempfile::TempDir) -> Value {
        serde_json::from_str(&fs::read_to_string(dir.path().join("macros.json")).unwrap()).unwrap()
    }

    #[test]
    fn the_earlier_managers_macros_carry_over_with_the_same_timing() {
        let text = migrate_legacy(legacy().as_object().unwrap());
        assert_eq!(text, "start 45\nwait 60-70\ntap j\nwait 340-341\ntap j\n");
        let m = grammar::parse(&text).unwrap();
        assert_eq!(
            &m.steps[..2],
            [grammar::Step::Start(45.0, 45.0), grammar::Step::Wait(60.0, 70.0)]
        );
    }

    #[test]
    fn loading_converts_old_entries_and_drops_what_cannot_be_text() {
        let (_d, lib) = library(json!({"haki": legacy(), "new": "tap e\n", "junk": 5}));
        assert_eq!(lib.names().collect::<Vec<_>>(), ["haki", "new"]);
        assert_eq!(lib.text("haki"), Some("start 45\nwait 60-70\ntap j\nwait 340-341\ntap j\n"));
    }

    #[test]
    fn a_switch_and_a_hotkey_are_kept_with_their_macro() {
        let (dir, mut lib) = library(json!({}));
        lib.save(None, "a", "tap e\n", Some("F6")).unwrap();
        lib.save(None, "b", "tap f\n", Some("<Control>q")).unwrap();
        lib.save(None, "c", "tap g\n", None).unwrap();
        lib.set_enabled("b", false).unwrap();
        assert_eq!(
            on_disk(&dir),
            json!({
                "a": {"text": "tap e\n", "enabled": true, "hotkey": "F6"},
                "b": {"text": "tap f\n", "enabled": false, "hotkey": "<Control>q"},
                "c": "tap g\n"
            })
        );
        let again = MacroLibrary::load(&dir.path().join("macros.json"));
        assert!(!again.enabled("b") && again.enabled("a"));
        assert_eq!(again.hotkeys().collect::<Vec<_>>(), [("a", "F6"), ("b", "<Control>q")]);
    }

    #[test]
    fn a_rename_carries_the_switch_and_the_hotkey() {
        let (_d, mut lib) =
            library(json!({"old ✓": {"text": "tap e\n", "enabled": false, "hotkey": "F6"}}));
        lib.save(Some("old ✓"), "new name", "tap f\n", Some("F6")).unwrap();
        assert!(!lib.contains("old ✓"));
        assert_eq!(
            (lib.text("new name"), lib.enabled("new name"), lib.hotkey("new name")),
            (Some("tap f\n"), false, Some("F6"))
        );
    }

    #[test]
    fn a_save_is_refused_for_a_missing_or_taken_name_a_taken_hotkey_or_bad_text() {
        let (_d, mut lib) = library(
            json!({"a": {"text": "tap e\n", "hotkey": "F6", "enabled": true}, "b": "tap f\n"}),
        );
        assert_eq!(
            lib.save(None, " ", "tap e", None),
            Err(MacroError::Name("A macro needs a name".into()))
        );
        assert_eq!(
            lib.save(Some("b"), "a", "tap e", None),
            Err(MacroError::Name("A macro called a already exists".into()))
        );
        assert_eq!(
            lib.save(Some("b"), "b", "tap e", Some("F6")),
            Err(MacroError::HotkeyTaken { hotkey: "F6".into(), by: "a".into() })
        );
        assert!(matches!(lib.save(None, "c", "jump", None), Err(MacroError::Parse(_))));
        assert!(
            lib.save(Some("a"), "a", "tap g", Some("F6")).is_ok(),
            "its own hotkey is no clash"
        );
    }

    #[test]
    fn a_hand_broken_macros_file_is_set_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("macros.json");
        fs::write(&path, "{\"a\": \"tap e\",}").unwrap();
        let mut lib = MacroLibrary::load(&path);
        assert_eq!(lib.set_aside().len(), 1);
        lib.save(None, "b", "tap f", None).unwrap();
        assert_eq!(fs::read_to_string(&lib.set_aside()[0]).unwrap(), "{\"a\": \"tap e\",}");
    }

    #[test]
    fn deleting_forgets_the_macro_its_switch_and_its_hotkey() {
        let (dir, mut lib) =
            library(json!({"a": {"text": "tap e\n", "hotkey": "F6", "enabled": false}}));
        lib.delete("a").unwrap();
        assert_eq!(lib.hotkeys().count(), 0);
        assert_eq!(on_disk(&dir), json!({}));
    }
}
