//! How this copy of the app was installed: what decides where its desktop
//! entry comes from and how it updates itself.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Set by the launcher a distribution package installs
/// (`packaging/linux/build.sh`): `deb`, `rpm` or `arch`.
pub const PACKAGE_VAR: &str = "RBXMGR_PACKAGE";

/// How this copy of the app was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Its entry is exported by Flatpak.
    Flatpak,
    /// Runs from this AppImage file, which may move.
    AppImage(PathBuf),
    /// A release's distribution package: the AppImage's contents under
    /// `/opt/roblox-manager`, with an entry of its own.
    Package(PackageFormat),
    /// Anything else (Nix, a hand-built binary): this binary, with or
    /// without an installed entry.
    Native(PathBuf),
}

/// The distribution packages a release ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageFormat {
    /// Debian, Ubuntu, Mint, Pop!_OS.
    Deb,
    /// Fedora, openSUSE.
    Rpm,
    /// Arch, Manjaro, EndeavourOS.
    Arch,
}

impl PackageFormat {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "deb" => Some(PackageFormat::Deb),
            "rpm" => Some(PackageFormat::Rpm),
            "arch" => Some(PackageFormat::Arch),
            _ => None,
        }
    }
}

impl Install {
    pub fn detect() -> Self {
        Self::from_env(
            Path::new("/.flatpak-info").exists(),
            |k| std::env::var_os(k),
            std::env::current_exe().unwrap_or_else(|_| "roblox-manager".into()),
        )
    }

    /// From whether this runs in a Flatpak, a variable lookup, and the binary.
    pub fn from_env(flatpak: bool, var: impl Fn(&str) -> Option<OsString>, exe: PathBuf) -> Self {
        if flatpak {
            return Install::Flatpak;
        }
        let var = |k: &str| var(k).filter(|v| !v.is_empty());
        // The AppImage runtime names the file it runs from.
        if let Some(image) = var("APPIMAGE") {
            return Install::AppImage(image.into());
        }
        if let Some(format) =
            var(PACKAGE_VAR).and_then(|v| PackageFormat::parse(&v.to_string_lossy()))
        {
            return Install::Package(format);
        }
        Install::Native(exe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(flatpak: bool, vars: &[(&str, &str)]) -> Install {
        let vars: Vec<(String, OsString)> =
            vars.iter().map(|(k, v)| ((*k).to_owned(), OsString::from(v))).collect();
        Install::from_env(
            flatpak,
            |k| vars.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone()),
            "/bin/rm".into(),
        )
    }

    #[test]
    fn each_install_is_told_apart() {
        assert_eq!(detect(true, &[("APPIMAGE", "/a.AppImage")]), Install::Flatpak);
        assert_eq!(
            detect(false, &[("APPIMAGE", "/a.AppImage")]),
            Install::AppImage("/a.AppImage".into())
        );
        assert_eq!(detect(false, &[(PACKAGE_VAR, "rpm")]), Install::Package(PackageFormat::Rpm));
        assert_eq!(detect(false, &[(PACKAGE_VAR, "snap")]), Install::Native("/bin/rm".into()));
        assert_eq!(detect(false, &[("APPIMAGE", "")]), Install::Native("/bin/rm".into()));
    }
}
