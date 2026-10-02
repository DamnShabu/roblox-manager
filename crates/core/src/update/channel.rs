//! What each way of installing the app takes from a release, and how it is
//! started again on the new version. The release workflow
//! (`.github/workflows/release.yml`) names its files the way [`asset_name`]
//! expects; `tests/release_assets.rs` holds the two together.

use std::path::Path;

use crate::install::{Install, PackageFormat};

/// What a relaunched app is given, so it waits for this one to go before it
/// claims the app's name on the session bus.
pub const RESTARTED: &str = "--restarted";

/// The app id: its D-Bus name and its Flatpak.
pub const APP_ID: &str = "io.github.mujo.RobloxManager";

/// Where a package's launcher is.
const PACKAGE_LAUNCHER: &str = "/usr/bin/roblox-manager";

/// The release file this install updates from, for `version`. None for an
/// install that does not update itself (Nix, a hand-built binary), and on a
/// machine releases are not built for.
pub fn asset_name(install: &Install, version: &str) -> Option<String> {
    if std::env::consts::ARCH != "x86_64" {
        return None;
    }
    Some(match install {
        Install::AppImage(_) => format!("roblox-manager-{version}-x86_64.AppImage"),
        Install::Flatpak => format!("roblox-manager-{version}-x86_64.flatpak"),
        Install::Package(PackageFormat::Deb) => format!("roblox-manager_{version}_amd64.deb"),
        Install::Package(PackageFormat::Rpm) => format!("roblox-manager-{version}-1.x86_64.rpm"),
        Install::Package(PackageFormat::Arch) => {
            format!("roblox-manager-{version}-1-x86_64.pkg.tar.zst")
        }
        Install::Native(_) => return None,
    })
}

/// How someone updates an install the app cannot update itself.
pub fn by_hand(install: &Install) -> &'static str {
    match install {
        Install::Native(exe) if exe.starts_with("/nix/store") => {
            "it was installed with Nix: update the flake input and rebuild"
        }
        _ => "update it the way it was installed, or download it from the release page",
    }
}

/// Which Flatpak installation a copy runs from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatpakInstallation {
    /// `~/.local/share/flatpak`: the user installs into it freely.
    User,
    /// `/var/lib/flatpak`: installing asks polkit, which asks for a password.
    System,
}

impl FlatpakInstallation {
    /// The one `/.flatpak-info`'s `app-path` is in.
    pub fn of(flatpak_info: &str) -> Self {
        let user = flatpak_info
            .lines()
            .filter_map(|l| l.trim().strip_prefix("app-path="))
            .any(|p| p.contains("/.local/share/flatpak/"));
        if user { Self::User } else { Self::System }
    }
}

/// The program that installs a downloaded Flatpak bundle on the host, in
/// `installation`.
///
/// A system installation goes through pkexec, which has the desktop's polkit
/// agent ask for the password. Flatpak cannot be let to ask itself: with no
/// terminal it answers its own "Proceed?" with no, and `--assumeyes` (like
/// `--noninteractive`) tells its system helper that polkit must not ask --
/// so the install is refused without a prompt. Run as root, flatpak writes
/// the system installation itself and asks nobody.
pub fn flatpak_install(installation: FlatpakInstallation, bundle: &Path) -> Vec<String> {
    let (elevate, which): (&[&str], _) = match installation {
        FlatpakInstallation::User => (&[], "--user"),
        FlatpakInstallation::System => (&[PKEXEC], "--system"),
    };
    ["flatpak-spawn", "--host"]
        .iter()
        .chain(elevate)
        .chain(&["flatpak", "install", which, "--assumeyes", "--reinstall", "--bundle"])
        .map(|s| (*s).to_owned())
        .chain([bundle.display().to_string()])
        .collect()
}

pub const PKEXEC: &str = "pkexec";

/// pkexec's exit status when the password prompt was dismissed, or the
/// password refused.
pub const PKEXEC_NOT_AUTHORIZED: i32 = 126;

/// One way to start the app again: a program and the variables it adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relaunch {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// How to start the installed (new) version, in order of preference: the
/// first that starts is the one. Each passes [`RESTARTED`].
pub fn relaunch(install: &Install) -> Vec<Relaunch> {
    let plain = |argv: &[&str]| Relaunch {
        argv: argv.iter().map(|s| (*s).to_owned()).chain([RESTARTED.to_owned()]).collect(),
        env: Vec::new(),
    };
    match install {
        Install::Flatpak => vec![plain(&["flatpak-spawn", "--host", "flatpak", "run", APP_ID])],
        // From inside the old image's namespace a new AppImage cannot mount
        // itself (fusermount gains no root there): a user service starts it
        // outside, as the AppImage's own launcher does under no_new_privs.
        // Without systemd, it unpacks itself instead of mounting.
        Install::AppImage(image) => {
            let image = image.to_string_lossy();
            let mut extract = plain(&[&image]);
            extract.env.push(("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into()));
            vec![plain(&["systemd-run", "--user", "--quiet", "--collect", &image]), extract]
        }
        // Started straight away, so it stays in the login session: polkit
        // finds no session (and no password prompt) for a user service.
        Install::Package(_) => vec![plain(&[PACKAGE_LAUNCHER])],
        Install::Native(exe) => vec![plain(&[&exe.to_string_lossy()])],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_install_takes_its_own_file() {
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let name = |i: Install| asset_name(&i, "0.3.0");
        assert_eq!(name(Install::Flatpak).unwrap(), "roblox-manager-0.3.0-x86_64.flatpak");
        assert_eq!(
            name(Install::Package(PackageFormat::Deb)).unwrap(),
            "roblox-manager_0.3.0_amd64.deb"
        );
        assert_eq!(name(Install::Native("/usr/bin/roblox-manager".into())), None);
    }

    #[test]
    fn a_bundle_goes_where_this_copy_was_installed() {
        let user = "[Instance]\napp-path=/home/u/.local/share/flatpak/app/x/current/active/files\n";
        let system = "[Instance]\napp-path=/var/lib/flatpak/app/x/current/active/files\n";
        assert_eq!(FlatpakInstallation::of(user), FlatpakInstallation::User);
        assert_eq!(FlatpakInstallation::of(system), FlatpakInstallation::System);
        let bundle = Path::new("/c/b.flatpak");
        assert_eq!(
            flatpak_install(FlatpakInstallation::User, bundle),
            [
                "flatpak-spawn",
                "--host",
                "flatpak",
                "install",
                "--user",
                "--assumeyes",
                "--reinstall",
                "--bundle",
                "/c/b.flatpak"
            ]
        );
        let argv = flatpak_install(FlatpakInstallation::System, bundle);
        assert_eq!(
            argv[..6],
            ["flatpak-spawn", "--host", "pkexec", "flatpak", "install", "--system"]
        );
        assert_eq!(argv.last().unwrap(), "/c/b.flatpak");
    }

    #[test]
    fn an_appimage_restarts_outside_its_namespace_first() {
        let ways = relaunch(&Install::AppImage("/a/r.AppImage".into()));
        assert_eq!(
            ways[0].argv,
            ["systemd-run", "--user", "--quiet", "--collect", "/a/r.AppImage", RESTARTED]
        );
        assert_eq!(ways[1].argv, ["/a/r.AppImage", RESTARTED]);
        assert_eq!(ways[1].env, [("APPIMAGE_EXTRACT_AND_RUN".to_string(), "1".to_string())]);
    }
}
