use super::*;

struct World {
    _dir: tempfile::TempDir,
    paths: Paths,
    root: PathBuf,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    World { paths: Paths::under(&root), root, _dir: dir }
}

impl World {
    fn handler(&self, install: Install) -> LinkHandler {
        LinkHandler::new(&self.paths, install)
    }
    fn user_entry(&self) -> PathBuf {
        self.paths.user_applications().join(DESKTOP_ID)
    }
    fn system_entry(&self) -> PathBuf {
        self.root.join("system/applications").join(DESKTOP_ID)
    }
    fn mimeapps(&self) -> String {
        fs::read_to_string(self.paths.mimeapps()).unwrap()
    }
}

#[test]
fn an_appimage_gets_an_entry_that_runs_its_file_and_becomes_the_default() {
    let w = world();
    let h = w.handler(Install::AppImage("/home/u/Apps/roblox-manager.AppImage".into()));
    assert_eq!(h.current(), Handler::Unset);
    h.claim().unwrap();
    let written = fs::read_to_string(w.user_entry()).unwrap();
    assert!(written.contains("\nExec=/home/u/Apps/roblox-manager.AppImage %u\n"), "{written}");
    assert!(
        written.contains("\nMimeType=x-scheme-handler/roblox-player;x-scheme-handler/roblox;\n")
    );
    assert_eq!(h.current(), Handler::Here);
    assert!(w.mimeapps().contains("x-scheme-handler/roblox=io.github.mujo.RobloxManager.desktop;"));
}

#[test]
fn another_launchers_default_is_reported_then_taken_over() {
    let w = world();
    fs::create_dir_all(w.paths.mimeapps().parent().unwrap()).unwrap();
    fs::write(
        w.paths.mimeapps(),
        "[Default Applications]\nx-scheme-handler/roblox-player=org.vinegarhq.Sober.desktop\n",
    )
    .unwrap();
    let h = w.handler(Install::Flatpak);
    assert_eq!(h.current(), Handler::Elsewhere("org.vinegarhq.Sober.desktop".into()));
    h.claim().unwrap();
    assert_eq!(h.current(), Handler::Here);
}

#[test]
fn a_flatpak_writes_no_entry_of_its_own() {
    let w = world();
    w.handler(Install::Flatpak).claim().unwrap();
    assert!(!w.user_entry().exists());
}

#[test]
fn a_native_install_uses_its_installed_entry() {
    let w = world();
    fs::create_dir_all(w.system_entry().parent().unwrap()).unwrap();
    fs::write(w.system_entry(), "[Desktop Entry]\nExec=roblox-manager %u\n").unwrap();
    w.handler(Install::Native("/nix/store/x/bin/roblox-manager".into())).claim().unwrap();
    assert!(!w.user_entry().exists());
    assert_eq!(w.handler(Install::Flatpak).current(), Handler::Here);
}

#[test]
fn a_bare_binary_gets_an_entry_for_itself() {
    let w = world();
    w.handler(Install::Native("/opt/rbx/roblox-manager".into())).claim().unwrap();
    let written = fs::read_to_string(w.user_entry()).unwrap();
    assert!(written.contains("\nExec=/opt/rbx/roblox-manager %u\n"));
}

#[test]
fn an_entry_someone_else_wrote_is_left_alone() {
    let w = world();
    fs::create_dir_all(w.user_entry().parent().unwrap()).unwrap();
    fs::write(w.user_entry(), "[Desktop Entry]\nExec=my-wrapper %u\n").unwrap();
    let h = w.handler(Install::Native("/opt/rbx/roblox-manager".into()));
    h.claim().unwrap();
    assert_eq!(
        fs::read_to_string(w.user_entry()).unwrap(),
        "[Desktop Entry]\nExec=my-wrapper %u\n"
    );
    assert!(!h.refresh().unwrap());
}

#[test]
fn a_moved_appimage_has_its_entry_follow_it() {
    let w = world();
    w.handler(Install::AppImage("/a/old.AppImage".into())).claim().unwrap();
    let moved = w.handler(Install::AppImage("/b/new.AppImage".into()));
    assert!(moved.refresh().unwrap());
    assert!(fs::read_to_string(w.user_entry()).unwrap().contains("Exec=/b/new.AppImage %u"));
    assert!(!moved.refresh().unwrap(), "already up to date");
}

#[test]
fn refreshing_never_writes_an_entry_that_was_not_there() {
    let w = world();
    assert!(!w.handler(Install::AppImage("/a/x.AppImage".into())).refresh().unwrap());
    assert!(!w.user_entry().exists());
}

#[test]
fn a_generated_entry_gives_way_to_an_installed_one() {
    let w = world();
    w.handler(Install::AppImage("/a/x.AppImage".into())).claim().unwrap();
    fs::create_dir_all(w.system_entry().parent().unwrap()).unwrap();
    fs::write(w.system_entry(), "[Desktop Entry]\n").unwrap();
    assert!(w.handler(Install::Native("/usr/bin/roblox-manager".into())).refresh().unwrap());
    assert!(!w.user_entry().exists());
}

#[test]
fn exec_paths_are_quoted_and_escaped_as_the_spec_asks() {
    assert_eq!(exec_arg("/usr/bin/rm"), "/usr/bin/rm");
    assert_eq!(exec_arg("/home/u/My Apps/r.AppImage"), "\"/home/u/My Apps/r.AppImage\"");
    assert_eq!(exec_arg("/a/$x"), "\"/a/\\\\$x\"");
    assert_eq!(exec_arg("/a/100%"), "/a/100%%");
}
