//! The Flatpak Cordial the manager used before the fork: its profiles and
//! settings are carried over once.

use std::fs;
use std::io;
use std::path::Path;

use super::CordialError;
use super::profiles::{CordialProfiles, SECRET_KINDS, io, secret_attrs_at};
use crate::types::Profile;

impl CordialProfiles {
    /// Carry the manager's profiles and Cordial's settings over from the
    /// Flatpak Cordial: the profiles hold each account's Roblox storage and
    /// settings. Moved, not copied, and never over something already here. The
    /// sessions the Flatpak profiles were given are dropped from the keyring --
    /// they are keyed by the old path, and every profile gets a fresh one
    /// before each launch -- but never with a prompt, since nobody asked for
    /// this. A profile a client still has open waits for the next start.
    pub fn migrate_flatpak(&self, log: &dyn Fn(String)) -> Result<(), CordialError> {
        let flatpak = self.paths().flatpak_cordial();
        let old = flatpak.join("data/cordial/profiles");
        let names: Vec<String> = match fs::read_dir(&old) {
            Ok(entries) => entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| n.starts_with("rbxmgr-"))
                .collect(),
            Err(_) => Vec::new(),
        };
        if !names.is_empty() {
            let in_use = self.running()?;
            for name in names {
                let profile = Profile::named(name.as_str());
                let dest = self.path(&profile);
                if dest.exists() || in_use.contains(&profile) {
                    continue;
                }
                move_dir(&old.join(&name), &dest)
                    .map_err(|e| io(&format!("could not move Cordial profile {name}"), e))?;
                for kind in SECRET_KINDS {
                    self.keyring().forget(&secret_attrs_at(&old.join(&name), kind));
                }
                log(format!("Moved Cordial profile {name} out of the old Flatpak"));
            }
        }
        let old_cfg = flatpak.join("config/cordial/shell.json");
        let cfg = self.paths().cordial_shell_json();
        if old_cfg.exists() && !cfg.exists() {
            if let Some(dir) = cfg.parent() {
                fs::create_dir_all(dir).map_err(|e| io("could not create Cordial's config", e))?;
            }
            fs::copy(&old_cfg, &cfg)
                .map_err(|e| io("could not carry over Cordial's settings", e))?;
        }
        Ok(())
    }
}

/// A rename, or a copy then delete when the two sides are separate mounts
/// (under impermanence they are bind mounts, and rename() refuses to cross
/// those even on one disk).
fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    match fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_tree(from, to)?;
            fs::remove_dir_all(from)
        }
        other => other,
    }
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cordial::process::recording::Recording;
    use crate::keyring::{Keyring, MemorySecrets, Secrets};
    use crate::paths::Paths;
    use std::sync::Arc;

    #[test]
    fn the_managers_profiles_and_settings_move_out_of_the_flatpak() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path());
        let old = paths.flatpak_cordial().join("data/cordial/profiles");
        for name in ["rbxmgr-1", "rbxmgr-2", "rbxmgr-3", "someone-else"] {
            fs::create_dir_all(old.join(name).join("data")).unwrap();
        }
        fs::write(old.join("rbxmgr-1/data/storage"), "kept").unwrap();
        fs::create_dir_all(paths.cordial_profiles().join("rbxmgr-2")).unwrap();
        fs::create_dir_all(paths.flatpak_cordial().join("config/cordial")).unwrap();
        fs::write(
            paths.flatpak_cordial().join("config/cordial/shell.json"),
            r#"{"gamemode": false}"#,
        )
        .unwrap();

        let secrets = Arc::new(MemorySecrets::default());
        for kind in SECRET_KINDS {
            secrets.store(&secret_attrs_at(&old.join("rbxmgr-1"), kind), "old", "s").unwrap();
        }
        let keyring = Arc::new(Keyring::new(Box::new(Arc::clone(&secrets))));
        // rbxmgr-3 is still open in an old client.
        let runner =
            Arc::new(Recording::default().answer(0, "9 cordial-run --profile rbxmgr-3\n", ""));
        let profiles = CordialProfiles::new(keyring, &paths, runner, Arc::new(|_| {}));
        let logs = std::cell::RefCell::new(Vec::new());
        profiles.migrate_flatpak(&|l| logs.borrow_mut().push(l)).unwrap();

        let mut left: Vec<String> = fs::read_dir(&old)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["rbxmgr-2", "rbxmgr-3", "someone-else"]);
        assert_eq!(
            fs::read_to_string(profiles.path(&Profile::named("rbxmgr-1")).join("data/storage"))
                .unwrap(),
            "kept"
        );
        assert!(secrets.items().is_empty(), "the moved profile's old sessions are dropped");
        assert_eq!(secrets.unlock_count(), 0, "never prompts");
        assert_eq!(logs.borrow().len(), 1);
        assert_eq!(
            fs::read_to_string(paths.cordial_shell_json()).unwrap(),
            r#"{"gamemode": false}"#
        );
    }

    #[test]
    fn nothing_to_migrate_is_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let keyring = Arc::new(Keyring::new(Box::new(MemorySecrets::default())));
        let profiles = CordialProfiles::new(
            keyring,
            &Paths::under(dir.path()),
            Arc::new(Recording::default().answer(1, "", "")),
            Arc::new(|_| {}),
        );
        profiles.migrate_flatpak(&|_| {}).unwrap();
    }
}
