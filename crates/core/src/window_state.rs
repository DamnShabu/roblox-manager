//! How the window was left -- its size, whether it was maximised, whether
//! the macros pane was open -- so it opens the same way next time.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::json_file;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// The macros and activity pane.
    pub sidebar: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        WindowState { width: 1280, height: 860, maximized: false, sidebar: true }
    }
}

/// Smaller than this and the window is no use; larger is a bad file.
const SMALLEST: (i32, i32) = (360, 320);
const LARGEST: i32 = 16_384;

impl WindowState {
    /// What was saved, or the default; a size out of reason is put back in it.
    pub fn load(path: &Path) -> Self {
        let mut s: WindowState = json_file::read(path);
        s.width = s.width.clamp(SMALLEST.0, LARGEST);
        s.height = s.height.clamp(SMALLEST.1, LARGEST);
        s
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
        let s = WindowState { width: 900, height: 700, maximized: true, sidebar: false };
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
}
