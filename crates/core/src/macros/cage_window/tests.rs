use std::io::Read;
use std::time::Duration;

use super::*;
use crate::macros::wire::{DISPLAY, message, wire_str, words};

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn a_window_relay_s_arguments_carry_the_display_and_the_whole_cage_command() {
    let argv = argv(
        Path::new("/app/bin/roblox-manager"),
        Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        &strings(&["cage", "--", "sh", "-c", "x"]),
    );
    assert_eq!(argv[..2], strings(&["/app/bin/roblox-manager", FLAG]));
    assert_eq!(
        parse(&argv[2..]),
        Some((
            PathBuf::from("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
            strings(&["cage", "--", "sh", "-c", "x"])
        ))
    );
}

#[test]
fn a_window_relay_with_nothing_to_run_is_refused() {
    for args in [vec![], strings(&["/d.wayland"]), strings(&["/d.wayland", "--"])] {
        assert_eq!(run(&args), 2, "{args:?}");
    }
}

#[test]
fn a_client_with_no_window_relay_has_no_window_state_to_give() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    assert_eq!(hidden(&display).unwrap(), None);
    assert_eq!(set_hidden(&display, true).unwrap(), None);
    // One its relay left behind, when it was killed.
    drop(UnixListener::bind(control_file(&display)).unwrap());
    assert_eq!(hidden(&display).unwrap(), None);
}

/// Exactly `len` bytes from `sock`.
fn read_exact(sock: &mut UnixStream, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    sock.read_exact(&mut buf).unwrap();
    buf
}

#[test]
fn hide_and_show_reach_cage_s_window_on_the_desktop() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let desktop_file = dir.path().join("wayland-0");
    let desktop = UnixListener::bind(&desktop_file).unwrap();
    let relay = Relay::open(&display, desktop_file).unwrap();
    assert_eq!(hidden(&display).unwrap(), Some(false));

    // Cage connects, through the relay, and makes its window: surface 4,
    // a toplevel through xdg_surface 5.
    let mut cage = UnixStream::connect(cage_file(&display)).unwrap();
    let (mut seen, _) = desktop.accept().unwrap();
    seen.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let start = [
        message(DISPLAY, 1, &words(&[2])),
        message(2, 0, &[words(&[1]), wire_str("xdg_wm_base"), words(&[1, 3])].concat()),
        message(3, 2, &words(&[5, 4])),
        message(5, 1, &words(&[6])),
        message(4, 1, &words(&[20, 0, 0])),
        message(4, 6, &[]),
    ]
    .concat();
    cage.write_all(&start).unwrap();
    assert_eq!(read_exact(&mut seen, start.len()), start, "passed through as it came");

    assert_eq!(set_hidden(&display, true).unwrap(), Some(true));
    let unmap = [message(4, 1, &words(&[0, 0, 0])), message(4, 6, &[])].concat();
    assert_eq!(read_exact(&mut seen, unmap.len()), unmap, "a null buffer, committed");
    assert_eq!(hidden(&display).unwrap(), Some(true));

    assert_eq!(set_hidden(&display, false).unwrap(), Some(false));
    let restart = message(4, 6, &[]);
    assert_eq!(read_exact(&mut seen, restart.len()), restart, "a commit with no buffer");

    drop(relay);
    assert!(!cage_file(&display).exists() && !control_file(&display).exists());
}

#[test]
fn a_line_that_is_no_command_is_turned_away() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let _relay = Relay::open(&display, dir.path().join("wayland-0")).unwrap();
    assert!(ask(&display, "explode").is_err());
    assert_eq!(hidden(&display).unwrap(), Some(false), "and nothing changed");
}
