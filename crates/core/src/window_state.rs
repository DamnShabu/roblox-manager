//! How the window was left -- its size, whether it was maximised, whether
//! the macros pane was open, the style it was in -- so it opens the same way
//! next time.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::json_file;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// The macros and activity pane.
    pub sidebar: bool,
    pub style: Style,
}

/// Light or dark: the desktop's choice, or the user's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    System,
    Light,
    Dark,
}

impl Style {
    /// Its name, as the style menu's action carries it.
    pub fn name(self) -> &'static str {
        match self {
            Style::System => "system",
            Style::Light => "light",
            Style::Dark => "dark",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Style::System, Style::Light, Style::Dark].into_iter().find(|s| s.name() == name)
    }
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            width: 1280,
            height: 860,
            maximized: false,
            sidebar: true,
            style: Style::System,
        }
    }
}

/// Smaller than this and the window is no use; larger is a bad file.
const SMALLEST: (i32, i32) = (360, 320);
const LARGEST: i32 = 16_384;

impl WindowState {
    /// What was saved, or the default; a size out of reason is put back in it.
    pub fn load(path: &Path) -> Self {
        // Field by field: one of the wrong type (a style this version does
        // not know) loses only itself, not the rest of the file.
        let raw: Map<String, Value> = json_file::read(path);
        let field = |k: &str| raw.get(k).and_then(|v| i32::deserialize(v).ok());
        let flag = |k: &str| raw.get(k).and_then(Value::as_bool);
        let d = WindowState::default();
        WindowState {
            width: field("width").unwrap_or(d.width).clamp(SMALLEST.0, LARGEST),
            height: field("height").unwrap_or(d.height).clamp(SMALLEST.1, LARGEST),
            maximized: flag("maximized").unwrap_or(d.maximized),
            sidebar: flag("sidebar").unwrap_or(d.sidebar),
            style: raw.get("style").and_then(|v| Style::deserialize(v).ok()).unwrap_or(d.style),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        json_file::write(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_state_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rbxmgr/window.json");
        let s = WindowState {
            width: 900,
            height: 700,
            maximized: true,
            sidebar: false,
            style: Style::Dark,
        };
        s.save(&path).unwrap();
        assert_eq!(WindowState::load(&path), s);
    }

    #[test]
    fn nothing_saved_is_the_default_and_a_bad_size_is_put_back_in_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.json");
        assert_eq!(WindowState::load(&path), WindowState::default());
        std::fs::write(&path, r#"{"width": -5, "height": 99999, "sidebar": false}"#).unwrap();
        let s = WindowState::load(&path);
        assert_eq!((s.width, s.height, s.maximized, s.sidebar), (360, 16_384, false, false));
    }

    #[test]
    fn a_style_this_version_does_not_know_loses_only_the_style() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.json");
        std::fs::write(&path, r#"{"width": 1000, "style": "sepia"}"#).unwrap();
        let s = WindowState::load(&path);
        assert_eq!((s.width, s.style), (1000, Style::System));
        assert_eq!(Style::parse("dark"), Some(Style::Dark));
        assert_eq!(Style::parse(Style::Light.name()), Some(Style::Light));
        assert_eq!(Style::parse("sepia"), None);
    }
}
