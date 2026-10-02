use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

use super::report::{Recorder, Report};
use super::*;
use crate::macros::wire::{DISPLAY, message, wire_str, words};

const F8: u16 = 66;

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

/// What `run` makes of a relay's argv, minus the program and the flag.
fn relayed(argv: &[String]) -> Vec<String> {
    assert_eq!(argv[1], FLAG);
    argv[2..].to_vec()
}

#[test]
fn a_relay_s_arguments_carry_the_display_and_the_whole_client_command() {
    let argv = argv(
        Path::new("/app/libexec/roblox-manager"),
        Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        &strings(&["cordial-run", "--profile", "rbxmgr-7", "--", "x"]),
    );
    assert_eq!(argv[0], "/app/libexec/roblox-manager");
    assert_eq!(
        parse(&relayed(&argv)),
        Some((
            PathBuf::from("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
            strings(&["cordial-run", "--profile", "rbxmgr-7", "--", "x"])
        ))
    );
}

#[test]
fn a_relay_with_nothing_to_run_is_refused() {
    for args in [vec![], strings(&["/d.wayland"]), strings(&["/d.wayland", "--"])] {
        assert_eq!(run(&args), 2, "{args:?}");
    }
}

#[test]
fn the_client_runs_on_the_relay_s_display_and_its_status_is_the_relay_s() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let check =
        "[ -S \"$WAYLAND_DISPLAY\" ] && [ -S \"${WAYLAND_DISPLAY%.client}.record\" ] && exit 3";
    let code =
        run(&[display.display().to_string(), "--".into(), "sh".into(), "-c".into(), check.into()]);
    assert_eq!(code, 3, "it ran with both sockets up");
    assert!(!display.with_extension("client").exists() && !record_file(&display).exists());
}

#[test]
fn with_no_relay_possible_the_client_still_runs_on_cage_s_own_display() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("gone/rbxmgr-7.wayland");
    let check = "case \"$WAYLAND_DISPLAY\" in *.client) exit 1;; esac; exit 4";
    let code =
        run(&[display.display().to_string(), "--".into(), "sh".into(), "-c".into(), check.into()]);
    assert_eq!(code, 4);
}

#[test]
fn a_live_relay_s_sockets_are_never_taken_over_but_stale_ones_are() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let first = Relay::open(&display).unwrap();
    assert!(Relay::open(&display).is_err(), "the first is still up");
    drop(first);
    // A crashed relay's: the file is left, nothing listens on it.
    drop(UnixListener::bind(display.with_extension("client")).unwrap());
    assert!(Relay::open(&display).is_ok());
}

/// The requests that give the client keyboard 6.
fn keyboard_requests() -> Vec<u8> {
    [
        message(DISPLAY, 1, &words(&[2])),
        message(2, 0, &[words(&[5]), wire_str("wl_seat"), words(&[7, 3])].concat()),
        message(3, 1, &words(&[6])),
    ]
    .concat()
}

#[test]
fn a_client_reaches_cage_through_the_relay_and_its_window_can_be_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let cage = UnixListener::bind(&display).unwrap();
    let relay = Relay::open(&display).unwrap();
    relay.serve().unwrap();

    let client = UnixStream::connect(display.with_extension("client")).unwrap();
    client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    (&client).write_all(&keyboard_requests()).unwrap();
    let (mut from_client, _) = cage.accept().unwrap();
    let mut requests = vec![0u8; keyboard_requests().len()];
    from_client.read_exact(&mut requests).unwrap();
    assert_eq!(requests, keyboard_requests());

    let mut recorder = Recorder::arm(&record_file(&display), F8).unwrap();
    let key = |code: u16, state: u32| message(6, 3, &words(&[1, 2, u32::from(code), state]));
    from_client.write_all(&[key(F8, 1), key(F8, 0), key(30, 1), key(F8, 1)].concat()).unwrap();
    let mut got = vec![0u8; key(30, 1).len()];
    (&client).read_exact(&mut got).unwrap();
    assert_eq!(got, key(30, 1));

    assert_eq!(recorder.hear(), Ok(Report::Started));
    assert!(matches!(recorder.hear(), Ok(Report::Heard(e)) if e.line().ends_with(" 30 down")));
    assert!(matches!(recorder.hear(), Ok(Report::Stopped(_))));
}

#[test]
fn a_recorder_waits_while_the_relay_hears_its_handshake_from_another() {
    // A recorder that connects and never speaks holds up no other.
    let dir = tempfile::tempdir().unwrap();
    let display = dir.path().join("rbxmgr-7.wayland");
    let relay = Relay::open(&display).unwrap();
    relay.serve().unwrap();
    let silent = UnixStream::connect(record_file(&display)).unwrap();
    let mut hello = String::new();
    BufReader::new(&silent).read_line(&mut hello).unwrap();
    let armed = thread::spawn(move || Recorder::arm(&record_file(&display), F8).is_ok());
    assert!(armed.join().unwrap());
}
