use std::path::PathBuf;
use std::sync::Mutex;

use super::*;
use crate::cordial::process::recording::Recording;
use crate::github::Asset;
use crate::install::PackageFormat;

const NEW: &str = "999.0.0";
const SUM: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

/// A release whose every file is `b"new"` and whose sums list `listed`.
struct FakeReleases {
    listed: String,
}

impl FakeReleases {
    fn listing(names: &[&str]) -> Self {
        let listed = names.iter().map(|n| format!("{SUM}  {n}\n")).collect();
        FakeReleases { listed }
    }
}

impl AppReleases for FakeReleases {
    fn latest(&self) -> Result<Release, GithubError> {
        Ok(release())
    }
    fn download(&self, url: &str, to: &Path) -> Result<(), GithubError> {
        let body = if url.ends_with(SUMS) { self.listed.as_bytes() } else { b"new" };
        fs::write(to, body).map_err(|e| GithubError::Io(e.to_string()))
    }
}

fn release() -> Release {
    let names = [
        format!("roblox-manager-{NEW}-x86_64.AppImage"),
        format!("roblox-manager-{NEW}-x86_64.flatpak"),
        format!("roblox-manager_{NEW}_amd64.deb"),
        SUMS.to_owned(),
    ];
    Release {
        version: NEW.into(),
        assets: names
            .into_iter()
            .map(|n| Asset { url: format!("https://x/{n}"), name: n })
            .collect(),
    }
}

#[derive(Default)]
struct FakePackages {
    installed: Mutex<Vec<PathBuf>>,
    refuse: bool,
}

impl PackageInstaller for FakePackages {
    fn install_file(&self, file: &Path) -> Result<(), UpdateError> {
        assert_eq!(fs::read(file).unwrap(), b"new");
        self.installed.lock().unwrap().push(file.to_path_buf());
        if self.refuse { Err(UpdateError::Install("PackageKit: no".into())) } else { Ok(()) }
    }
}

struct Sandbox {
    dir: tempfile::TempDir,
    paths: Paths,
    runner: Recording,
    packages: FakePackages,
}

fn sandbox(sha256sum_says: &str) -> Sandbox {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    let runner = Recording::default().answer(0, &format!("{sha256sum_says}  file\n"), "");
    Sandbox { dir, paths, runner, packages: FakePackages::default() }
}

impl Sandbox {
    fn install(&self, install: &Install, releases: &FakeReleases) -> Result<(), UpdateError> {
        self.install_from(install, releases, "/home/u/.local/share/flatpak/app/x")
    }

    /// [`Sandbox::install`] as a Flatpak whose app is at `app_path`.
    fn install_from(
        &self,
        install: &Install,
        releases: &FakeReleases,
        app_path: &str,
    ) -> Result<(), UpdateError> {
        let info = self.dir.path().join("flatpak-info");
        fs::write(&info, format!("[Instance]\napp-path={app_path}\n")).unwrap();
        let update = SelfUpdate {
            releases,
            runner: &self.runner,
            packages: &self.packages,
            paths: &self.paths,
            install,
            flatpak_info: &info,
        };
        update.install(&release(), &|_| {})
    }

    fn appimage(&self) -> PathBuf {
        let image = self.dir.path().join("Apps/roblox-manager.AppImage");
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        fs::write(&image, b"old").unwrap();
        image
    }
}

fn x86_64() -> bool {
    std::env::consts::ARCH == "x86_64"
}

#[test]
fn only_a_later_release_is_offered() {
    assert_eq!(newer(&FakeReleases::listing(&[])).unwrap().unwrap().version, NEW);
    struct Same;
    impl AppReleases for Same {
        fn latest(&self) -> Result<Release, GithubError> {
            Ok(Release { version: RUNNING.into(), assets: Vec::new() })
        }
        fn download(&self, _: &str, _: &Path) -> Result<(), GithubError> {
            unreachable!()
        }
    }
    assert_eq!(newer(&Same).unwrap(), None);
}

#[test]
fn an_appimage_is_replaced_in_place_once_its_checksum_matches() {
    if !x86_64() {
        return;
    }
    let s = sandbox(SUM);
    let image = s.appimage();
    let name = format!("roblox-manager-{NEW}-x86_64.AppImage");
    s.install(&Install::AppImage(image.clone()), &FakeReleases::listing(&[&name])).unwrap();
    assert_eq!(fs::read(&image).unwrap(), b"new");
    assert_eq!(fs::metadata(&image).unwrap().permissions().mode() & 0o777, 0o755);
    let left: Vec<_> = fs::read_dir(image.parent().unwrap()).unwrap().collect();
    assert_eq!(left.len(), 1, "only the AppImage stays");
    assert_eq!(s.runner.ran()[0][0], "sha256sum");
}

#[test]
fn a_download_that_does_not_match_replaces_nothing() {
    if !x86_64() {
        return;
    }
    let s = sandbox("0000");
    let image = s.appimage();
    let name = format!("roblox-manager-{NEW}-x86_64.AppImage");
    let got = s.install(&Install::AppImage(image.clone()), &FakeReleases::listing(&[&name]));
    assert_eq!(got, Err(UpdateError::Checksum));
    assert_eq!(fs::read(&image).unwrap(), b"old");
    assert_eq!(fs::read_dir(image.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn a_file_the_sums_do_not_list_is_refused() {
    if !x86_64() {
        return;
    }
    let s = sandbox(SUM);
    let got = s.install(&Install::AppImage(s.appimage()), &FakeReleases::listing(&["other"]));
    assert!(matches!(got, Err(UpdateError::Install(why)) if why.contains("does not list")));
}

#[test]
fn a_package_goes_to_packagekit_and_is_kept_when_it_fails() {
    if !x86_64() {
        return;
    }
    let deb = Install::Package(PackageFormat::Deb);
    let name = format!("roblox-manager_{NEW}_amd64.deb");
    let s = sandbox(SUM);
    s.install(&deb, &FakeReleases::listing(&[&name])).unwrap();
    let kept = s.paths.updates().join(&name);
    assert_eq!(*s.packages.installed.lock().unwrap(), std::slice::from_ref(&kept));
    assert!(!kept.exists());

    let mut s = sandbox(SUM);
    s.packages.refuse = true;
    let err = s.install(&deb, &FakeReleases::listing(&[&name])).unwrap_err();
    let kept = s.paths.updates().join(&name);
    assert!(err.to_string().contains(&kept.display().to_string()), "{err}");
    assert!(kept.exists());
}

#[test]
fn a_flatpak_installs_its_bundle_on_the_host() {
    if !x86_64() {
        return;
    }
    let s = sandbox(SUM);
    let name = format!("roblox-manager-{NEW}-x86_64.flatpak");
    s.install(&Install::Flatpak, &FakeReleases::listing(&[&name])).unwrap();
    let flatpak = &s.runner.ran()[1];
    assert_eq!(flatpak[..5], ["flatpak-spawn", "--host", "flatpak", "install", "--user"]);
}

#[test]
fn a_system_flatpak_asks_for_the_password_through_pkexec() {
    if !x86_64() {
        return;
    }
    let name = format!("roblox-manager-{NEW}-x86_64.flatpak");
    let system = "/var/lib/flatpak/app/x";
    let s = sandbox(SUM);
    s.install_from(&Install::Flatpak, &FakeReleases::listing(&[&name]), system).unwrap();
    assert_eq!(s.runner.ran()[1][..4], ["flatpak-spawn", "--host", "pkexec", "flatpak"]);

    let mut dismissed = sandbox(SUM);
    dismissed.runner = Recording::default().answer(0, &format!("{SUM}  file\n"), "").answer(
        126,
        "",
        "Error executing command as another user: Request dismissed",
    );
    let err = dismissed
        .install_from(&Install::Flatpak, &FakeReleases::listing(&[&name]), system)
        .unwrap_err();
    assert!(err.to_string().contains("password prompt was closed"), "{err}");
}

#[test]
fn a_nix_install_says_how_to_update_it() {
    let s = sandbox(SUM);
    let nix = Install::Native("/nix/store/x/bin/roblox-manager".into());
    let err = s.install(&nix, &FakeReleases::listing(&[])).unwrap_err();
    assert!(matches!(&err, UpdateError::Unsupported(why) if why.contains("Nix")), "{err}");
}

#[test]
fn the_sums_are_read_as_sha256sum_writes_them() {
    let sums = "aa  one.deb\nbb *two.rpm\n";
    assert_eq!(checksum_for(sums, "two.rpm").as_deref(), Some("bb"));
    assert_eq!(checksum_for(sums, "one.deb").as_deref(), Some("aa"));
    assert_eq!(checksum_for(sums, "three"), None);
}

#[test]
fn no_release_yet_is_nothing_newer() {
    struct NoneYet;
    impl AppReleases for NoneYet {
        fn latest(&self) -> Result<Release, GithubError> {
            Err(GithubError::Status(404))
        }
        fn download(&self, _: &str, _: &Path) -> Result<(), GithubError> {
            unreachable!()
        }
    }
    assert_eq!(newer(&NoneYet).unwrap(), None);
}
