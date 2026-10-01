use std::cell::RefCell;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::cordial::SystemRunner;
use crate::cordial::process::recording::Recording;

fn sandbox() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path());
    (dir, paths)
}

/// A release whose AppImage holds an engine that prints how it was started.
#[derive(Default)]
struct FakeReleases {
    version: Mutex<String>,
    downloads: Mutex<u32>,
}

const FAKE_ENGINE: &[u8] = b"#!/bin/sh\necho \"$0 $*\"\necho \"$LD_LIBRARY_PATH\"\n";

impl FakeReleases {
    fn at(version: &str) -> Self {
        let r = FakeReleases::default();
        *r.version.lock().unwrap() = version.into();
        r
    }
}

impl Releases for FakeReleases {
    fn latest(&self) -> Result<Release, CordialError> {
        let version = self.version.lock().unwrap().clone();
        Ok(Release { url: format!("https://x/{version}"), version })
    }
    fn download(&self, _url: &str, to: &Path) -> Result<(), CordialError> {
        *self.downloads.lock().unwrap() += 1;
        let image = appimage::fake::appimage(
            &[("usr/bin/cordial-run", FAKE_ENGINE, 0o755), ("usr/lib/libx.so", b"", 0o644)],
            &[],
        );
        fs::write(to, image).map_err(|e| CordialError::Io(e.to_string()))
    }
}

fn update_portable(paths: &Paths, releases: &FakeReleases) -> Updated {
    update(&SystemRunner, releases, paths, Host::Portable, &|_| {}).unwrap()
}

fn names(paths: &Paths) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(paths.stacked())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn launches_use_the_bundled_engine_until_an_update() {
    let (_dir, paths) = sandbox();
    assert_eq!(engine_program(&paths), "cordial-run");
}

#[test]
fn the_newest_release_is_unpacked_and_launches_run_it_with_its_libraries() {
    let (_dir, paths) = sandbox();
    let releases = FakeReleases::at("0.21.0");
    let got = update_portable(&paths, &releases);
    assert_eq!(got, Updated { version: "0.21.0".into(), fresh: true, left_behind: None });
    assert_eq!(names(&paths), ["0.21.0", "current"]);

    let program = engine_program(&paths);
    assert!(program.ends_with("stacked/current/bin/cordial-run"), "{program}");
    let out = SystemRunner
        .run(&[program, "--profile".into(), "p".into()], Duration::from_secs(10))
        .unwrap();
    let out = String::from_utf8(out.stdout).unwrap();
    let mut lines = out.lines();
    let engine = paths.stacked().join("0.21.0/usr/bin/cordial-run");
    assert_eq!(lines.next().unwrap(), format!("{} --profile p", engine.display()));
    let libs = paths.stacked().join("0.21.0/usr/lib");
    assert!(lines.next().unwrap().starts_with(&libs.display().to_string()));
}

#[test]
fn the_same_release_again_downloads_nothing() {
    let (_dir, paths) = sandbox();
    let releases = FakeReleases::at("0.21.0");
    update_portable(&paths, &releases);
    let again = update_portable(&paths, &releases);
    assert!(!again.fresh);
    assert_eq!(*releases.downloads.lock().unwrap(), 1);
}

#[test]
fn a_newer_release_replaces_it_and_only_the_one_before_is_kept() {
    let (_dir, paths) = sandbox();
    let releases = FakeReleases::at("0.21.0");
    update_portable(&paths, &releases);
    *releases.version.lock().unwrap() = "0.22.0".into();
    assert!(update_portable(&paths, &releases).fresh);
    *releases.version.lock().unwrap() = "0.23.0".into();
    update_portable(&paths, &releases);
    assert_eq!(names(&paths), ["0.22.0", "0.23.0", "current"]);
    assert!(engine_program(&paths).ends_with("current/bin/cordial-run"));
    assert_eq!(current_target(&paths.stacked()).as_deref(), Some("0.23.0"));
}

#[test]
fn an_appimage_that_does_not_unpack_leaves_launches_alone() {
    struct Broken;
    impl Releases for Broken {
        fn latest(&self) -> Result<Release, CordialError> {
            Ok(Release { version: "0.21.0".into(), url: "https://x".into() })
        }
        fn download(&self, _url: &str, to: &Path) -> Result<(), CordialError> {
            fs::write(to, "#!/bin/sh\necho 'not an AppImage' >&2\nexit 1\n")
                .map_err(|e| CordialError::Io(e.to_string()))
        }
    }
    let (_dir, paths) = sandbox();
    let err = update(&SystemRunner, &Broken, &paths, Host::Portable, &|_| {}).unwrap_err();
    assert_eq!(
        err,
        CordialError::Stacked("could not unpack the AppImage (not an AppImage)".into())
    );
    assert_eq!(engine_program(&paths), "cordial-run");
}

#[test]
fn a_host_with_the_loader_or_nixos_stand_in_for_it_is_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("ld-linux-x86-64.so.2");
    fs::write(&real, b"\x7fELF...ld.so").unwrap();
    assert_eq!(Host::with_loader(&real), Host::Portable);
    let stub = dir.path().join("stub-ld");
    fs::write(&stub, b"\x7fELF...see:\nhttps://nix.dev/permalink/stub-ld\n").unwrap();
    assert_eq!(Host::with_loader(&stub), Host::Nix);
    assert_eq!(Host::with_loader(&dir.path().join("none")), Host::Nix);
}

#[test]
fn a_host_without_the_loader_builds_with_nix_and_links_that() {
    let (_dir, paths) = sandbox();
    // What `nix build --out-link` leaves: the link, with the engine in it.
    let store = paths.stacked().parent().unwrap().join("store-stacked-0.21.0");
    fs::create_dir_all(store.join("bin")).unwrap();
    fs::write(store.join("bin/cordial-run"), "").unwrap();
    fs::create_dir_all(paths.stacked()).unwrap();
    symlink(&store, paths.stacked().join(NIX_LINK)).unwrap();

    let r = Recording::default().answer(0, "/nix/store/abc-stacked-0.21.0\n", "");
    let logs = RefCell::new(Vec::new());
    let got =
        update(&r, &FakeReleases::at("unused"), &paths, Host::Nix, &|l| logs.borrow_mut().push(l))
            .unwrap();
    assert_eq!(got.version, "0.21.0");
    assert!(got.fresh);
    assert_eq!(r.ran()[0][0], "nix");
    assert_eq!(current_target(&paths.stacked()).as_deref(), Some(NIX_LINK));
    assert!(engine_program(&paths).ends_with("current/bin/cordial-run"));
    assert_eq!(logs.borrow().len(), 1);
}

#[test]
fn a_newer_release_is_offered_until_it_is_installed() {
    let (_dir, paths) = sandbox();
    // Before any update, launches run the pinned version.
    let pinned = installed_version(&paths).unwrap();
    assert_eq!(newer(&FakeReleases::at(&pinned), &paths).unwrap(), None);
    assert_eq!(newer(&FakeReleases::at("99.0.0"), &paths).unwrap(), Some("99.0.0".into()));

    let releases = FakeReleases::at("99.0.0");
    update_portable(&paths, &releases);
    assert_eq!(installed_version(&paths).as_deref(), Some("99.0.0"));
    assert_eq!(newer(&releases, &paths).unwrap(), None);
}
