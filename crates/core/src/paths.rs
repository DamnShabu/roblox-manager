//! Where everything lives on disk. One rule resolves every XDG directory: an
//! unset or empty variable falls back to its default.

use std::path::{Path, PathBuf};

/// The directories the manager reads and writes, resolved once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    home: PathBuf,
    data_home: PathBuf,
    config_home: PathBuf,
    cache_home: PathBuf,
    state_home: PathBuf,
    runtime_dir: PathBuf,
    /// XDG_DATA_DIRS: where installed applications' desktop entries are.
    data_dirs: Vec<PathBuf>,
}

impl Paths {
    /// From this process's environment.
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok(), rustix::process::getuid().as_raw())
    }

    /// From any variable lookup, for the user `uid` (whose runtime directory
    /// is `/run/user/<uid>` when `XDG_RUNTIME_DIR` is not set).
    pub fn from_vars(get: impl Fn(&str) -> Option<String>, uid: u32) -> Self {
        let var = |k: &str| get(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").unwrap_or_else(|| PathBuf::from("/"));
        Paths {
            data_home: var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
            config_home: var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            cache_home: var("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
            state_home: var("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")),
            runtime_dir: var("XDG_RUNTIME_DIR")
                .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}"))),
            data_dirs: get("XDG_DATA_DIRS")
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned())
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .collect(),
            home,
        }
    }

    /// Everything inside `root`: the self-check's sandbox.
    pub fn under(root: &Path) -> Self {
        Paths {
            home: root.join("home"),
            data_home: root.join("data"),
            config_home: root.join("config"),
            cache_home: root.join("cache"),
            state_home: root.join("state"),
            runtime_dir: root.join("run"),
            data_dirs: vec![root.join("system")],
        }
    }

    /// The manager's own state: accounts, groups, macros.
    pub fn state(&self) -> PathBuf {
        self.data_home.join("rbxmgr")
    }

    pub fn accounts(&self) -> PathBuf {
        self.state().join("accounts.json")
    }

    pub fn groups(&self) -> PathBuf {
        self.state().join("groups.json")
    }

    pub fn macros(&self) -> PathBuf {
        self.state().join("macros.json")
    }

    /// The newest Stacked, once updated to: a link to its Nix build.
    pub fn stacked(&self) -> PathBuf {
        self.state().join("stacked")
    }

    /// Regenerable: game icons and client logs.
    pub fn cache(&self) -> PathBuf {
        self.cache_home.join("rbxmgr")
    }

    /// Leading `_` so no account label can claim it.
    pub fn icons(&self) -> PathBuf {
        self.cache().join("_icons")
    }

    /// Accounts' headshots, one PNG per user id.
    pub fn avatars(&self) -> PathBuf {
        self.cache().join("_avatars")
    }

    /// How the window was left: its size, and which panes were open.
    /// Neither data nor a setting, so it is XDG state.
    pub fn window_state(&self) -> PathBuf {
        self.state_home.join("rbxmgr/window.json")
    }

    pub fn logs(&self) -> PathBuf {
        self.cache().join("logs")
    }

    pub fn cordial_profiles(&self) -> PathBuf {
        self.data_home.join("cordial/profiles")
    }

    /// Cordial's settings, in the format upstream's window saved them.
    pub fn cordial_shell_json(&self) -> PathBuf {
        self.config_home.join("cordial/shell.json")
    }

    /// Where the Flatpak Cordial the manager used before the fork kept its data.
    pub fn flatpak_cordial(&self) -> PathBuf {
        self.home.join(".var/app/io.github.luohoa97.Cordial")
    }

    /// Where this user's own desktop entries go.
    pub fn user_applications(&self) -> PathBuf {
        self.data_home.join("applications")
    }

    /// Where installed desktop entries are, in the desktop's order.
    pub fn system_applications(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.data_dirs.iter().map(|d| d.join("applications"))
    }

    /// Which application opens which type or link scheme, for this user.
    pub fn mimeapps(&self) -> PathBuf {
        self.config_home.join("mimeapps.list")
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn paths(vars: &[(&str, &str)]) -> Paths {
        let vars: HashMap<String, String> =
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Paths::from_vars(|k| vars.get(k).cloned(), 1000)
    }

    #[test]
    fn unset_directories_fall_back_to_their_defaults() {
        let p = paths(&[("HOME", "/home/u")]);
        assert_eq!(p.accounts(), PathBuf::from("/home/u/.local/share/rbxmgr/accounts.json"));
        assert_eq!(p.icons(), PathBuf::from("/home/u/.cache/rbxmgr/_icons"));
        assert_eq!(p.avatars(), PathBuf::from("/home/u/.cache/rbxmgr/_avatars"));
        assert_eq!(p.window_state(), PathBuf::from("/home/u/.local/state/rbxmgr/window.json"));
        assert_eq!(p.cordial_shell_json(), PathBuf::from("/home/u/.config/cordial/shell.json"));
        assert_eq!(p.runtime_dir(), Path::new("/run/user/1000"));
        assert_eq!(p.mimeapps(), PathBuf::from("/home/u/.config/mimeapps.list"));
        assert_eq!(
            p.system_applications().collect::<Vec<_>>(),
            [PathBuf::from("/usr/local/share/applications"), "/usr/share/applications".into()]
        );
    }

    #[test]
    fn an_empty_variable_counts_as_unset() {
        let p = paths(&[("HOME", "/home/u"), ("XDG_DATA_HOME", ""), ("XDG_RUNTIME_DIR", "")]);
        assert_eq!(p.cordial_profiles(), PathBuf::from("/home/u/.local/share/cordial/profiles"));
        assert_eq!(p.runtime_dir(), Path::new("/run/user/1000"));
    }

    #[test]
    fn set_variables_are_used() {
        let p = paths(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CACHE_HOME", "/c"),
            ("XDG_STATE_HOME", "/s"),
            ("XDG_RUNTIME_DIR", "/r"),
            ("XDG_DATA_DIRS", "/a:/b/"),
        ]);
        assert_eq!(p.user_applications(), PathBuf::from("/d/applications"));
        assert_eq!(
            p.system_applications().collect::<Vec<_>>(),
            [PathBuf::from("/a/applications"), "/b/applications".into()]
        );
        assert_eq!(p.window_state(), PathBuf::from("/s/rbxmgr/window.json"));
        assert_eq!(p.macros(), PathBuf::from("/d/rbxmgr/macros.json"));
        assert_eq!(p.logs(), PathBuf::from("/c/rbxmgr/logs"));
        assert_eq!(p.runtime_dir(), Path::new("/r"));
        assert_eq!(
            p.flatpak_cordial(),
            PathBuf::from("/home/u/.var/app/io.github.luohoa97.Cordial")
        );
    }
}
