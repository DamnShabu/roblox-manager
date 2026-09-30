//! Being the desktop's handler for Roblox's links, however the app was
//! installed. A browser hands `roblox-player:` and `roblox:` links to the
//! application `mimeapps.list` names, found by its desktop entry: the
//! Flatpak and the NixOS module ship one; an AppImage or a hand-built binary
//! has none, so one is written for it. Setting the default is always an
//! explicit choice: another Roblox launcher (Sober, Vinegar) may hold it.

mod mimeapps;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths::Paths;

/// The app's desktop entry, under the app id (the window's app_id).
pub const DESKTOP_ID: &str = "io.github.mujo.RobloxManager.desktop";

/// The link schemes, as the types desktops file them under.
pub const SCHEMES: [&str; 2] = ["x-scheme-handler/roblox-player", "x-scheme-handler/roblox"];

/// Marks an entry this app wrote, which it may rewrite or remove.
const GENERATED: &str = "X-RobloxManager-Generated=true";

#[derive(Debug, thiserror::Error)]
#[error("could not make this app open Roblox links: {0}")]
pub struct DesktopError(#[from] io::Error);

/// How this copy of the app was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Its entry is exported by Flatpak.
    Flatpak,
    /// Runs from this AppImage file, which may move.
    AppImage(PathBuf),
    /// Anything else: this binary, with or without an installed entry.
    Native(PathBuf),
}

impl Install {
    pub fn detect() -> Self {
        if Path::new("/.flatpak-info").exists() {
            return Install::Flatpak;
        }
        // The AppImage runtime names the file it runs from.
        if let Some(image) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
            return Install::AppImage(image.into());
        }
        Install::Native(std::env::current_exe().unwrap_or_else(|_| "roblox-manager".into()))
    }
}

/// Who opens Roblox's links now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Handler {
    /// This app, for every scheme.
    Here,
    /// Another application, by its desktop id.
    Elsewhere(String),
    /// No default set: the desktop picks any entry that claims the scheme.
    Unset,
}

pub struct LinkHandler {
    paths: Paths,
    install: Install,
}

impl LinkHandler {
    pub fn new(paths: &Paths, install: Install) -> Self {
        LinkHandler { paths: paths.clone(), install }
    }

    pub fn current(&self) -> Handler {
        let text = fs::read_to_string(self.paths.mimeapps()).unwrap_or_default();
        let ids: Vec<Option<String>> =
            SCHEMES.iter().map(|s| mimeapps::default_for(&text, s)).collect();
        if ids.iter().all(|id| id.as_deref() == Some(DESKTOP_ID)) {
            return Handler::Here;
        }
        match ids.into_iter().flatten().find(|id| id != DESKTOP_ID) {
            Some(other) => Handler::Elsewhere(other),
            None => Handler::Unset,
        }
    }

    /// Make this app the one Roblox's links open: its desktop entry where
    /// the install has none, then the default for both schemes.
    pub fn claim(&self) -> Result<(), DesktopError> {
        self.write_entry()?;
        let path = self.paths.mimeapps();
        let mut text = fs::read_to_string(&path).or_else(|e| match e.kind() {
            io::ErrorKind::NotFound => Ok(String::new()),
            _ => Err(e),
        })?;
        for scheme in SCHEMES {
            text = mimeapps::set_default(&text, scheme, DESKTOP_ID);
        }
        write_atomically(&path, &text)?;
        Ok(())
    }

    /// On start: keep an entry this app wrote pointing at this copy (an
    /// AppImage that moved), without claiming anything. True if rewritten.
    pub fn refresh(&self) -> Result<bool, DesktopError> {
        let Ok(text) = fs::read_to_string(self.user_entry()) else { return Ok(false) };
        if !is_generated(&text) {
            return Ok(false);
        }
        let wanted = self.exec_to_write().map(|exec| entry(&exec));
        if wanted.as_deref() == Some(text.as_str()) {
            return Ok(false);
        }
        self.write_entry()?;
        Ok(true)
    }

    /// The program a written entry runs: an AppImage's file, or a binary no
    /// installed entry covers. None when the install brings its own.
    fn exec_to_write(&self) -> Option<PathBuf> {
        match &self.install {
            Install::Flatpak => None,
            Install::AppImage(image) => Some(image.clone()),
            Install::Native(exe) => (!self.installed_entry()).then(|| exe.clone()),
        }
    }

    fn write_entry(&self) -> Result<(), DesktopError> {
        let ours = self.user_entry();
        match self.exec_to_write() {
            Some(exec) => write_atomically(&ours, &entry(&exec))?,
            // An entry written by an earlier copy would shadow the installed one.
            None if fs::read_to_string(&ours).is_ok_and(|t| is_generated(&t)) => {
                fs::remove_file(&ours)?;
            }
            None => {}
        }
        Ok(())
    }

    fn user_entry(&self) -> PathBuf {
        self.paths.user_applications().join(DESKTOP_ID)
    }

    /// Whether a package installed the app's entry: in the system's
    /// directories, or the user's own when this app did not write it.
    fn installed_entry(&self) -> bool {
        let user = fs::read_to_string(self.user_entry()).is_ok_and(|t| !is_generated(&t));
        user || self.paths.system_applications().any(|d| d.join(DESKTOP_ID).is_file())
    }
}

fn is_generated(text: &str) -> bool {
    text.lines().any(|l| l.trim() == GENERATED)
}

/// The desktop entry for `exec`.
fn entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Roblox Manager\n\
         GenericName=Roblox Account Manager\n\
         Comment=Launch several Roblox accounts into the same server\n\
         Icon=io.github.mujo.RobloxManager\n\
         Exec={} %u\n\
         Terminal=false\n\
         StartupWMClass=io.github.mujo.RobloxManager\n\
         Categories=Game;\n\
         MimeType={};\n\
         {GENERATED}\n",
        exec_arg(&exec.to_string_lossy()),
        SCHEMES.join(";"),
    )
}

/// A path as one argument of an Exec line: quoted when it has anything the
/// spec reserves, with `"`, `` ` ``, `$` and `\` escaped inside the quotes;
/// then, as for any value in the file, `\` doubled and `%` as `%%`.
fn exec_arg(path: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = if path.contains(RESERVED) {
        let mut q = String::from("\"");
        for c in path.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    } else {
        path.to_owned()
    };
    arg.replace('\\', "\\\\").replace('%', "%%")
}

fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests;
