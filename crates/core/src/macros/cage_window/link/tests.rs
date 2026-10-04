use std::io::{Read, Write};
use std::time::Duration;

use super::*;
use crate::macros::wire::{DISPLAY, message, wire_str, words};

/// Cage and the desktop, linked through `windows`: (cage's end, desktop's).
fn linked(windows: &Arc<Windows>) -> (UnixStream, UnixStream) {
    let (cage, relay_cage) = UnixStream::pair().unwrap();
    let (relay_desktop, desktop) = UnixStream::pair().unwrap();
    windows.link(relay_cage, relay_desktop).unwrap();
    for end in [&cage, &desktop] {
        end.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    }
    (cage, desktop)
}

fn read_exact(mut sock: &UnixStream, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    sock.read_exact(&mut buf).unwrap();
    buf
}

/// Whether `sock` has been sent nothing, within a moment.
fn quiet(mut sock: &UnixStream) -> bool {
    sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let quiet = matches!(sock.read(&mut [0u8]), Err(e)
        if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut));
    sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    quiet
}

#[test]
fn a_hidden_window_s_frames_and_buffers_stay_between_cage_and_the_relay() {
    let windows = Arc::new(Windows::default());
    let (mut cage, mut desktop) = linked(&windows);
    let start = [
        message(DISPLAY, 1, &words(&[2])),
        message(2, 0, &[words(&[1]), wire_str("xdg_wm_base"), words(&[1, 3])].concat()),
        message(3, 2, &words(&[5, 4])),
        message(5, 1, &words(&[6])),
        message(4, 3, &words(&[30])),
        message(4, 6, &[]),
    ]
    .concat();
    cage.write_all(&start).unwrap();
    read_exact(&desktop, start.len());

    windows.set_hidden(true);
    assert!(windows.hidden());
    read_exact(&desktop, 28); // the null buffer and its commit
    desktop.write_all(&message(30, 0, &words(&[5]))).unwrap();
    // Two buffers while hidden: the first comes back when the second
    // replaces it, and neither reaches the desktop.
    cage.write_all(
        &[message(4, 1, &words(&[21, 0, 0])), message(4, 1, &words(&[22, 0, 0]))].concat(),
    )
    .unwrap();
    let release = message(21, 0, &[]);
    assert_eq!(read_exact(&cage, release.len()), release, "and no frame done before it");
    assert!(quiet(&desktop));
    assert!(quiet(&cage));
}

#[test]
fn when_either_end_closes_so_does_the_other() {
    let windows = Arc::new(Windows::default());
    let (cage, desktop) = linked(&windows);
    drop(cage);
    assert_eq!((&desktop).read(&mut [0u8; 8]).unwrap(), 0, "the desktop sees cage go");
    let (cage, desktop) = linked(&windows);
    drop(desktop);
    assert_eq!((&cage).read(&mut [0u8; 8]).unwrap(), 0, "cage sees the desktop go");
    windows.set_hidden(true);
}
