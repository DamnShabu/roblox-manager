//! VirtualInput against a stand-in compositor: the real wire bytes, over a
//! real socket, with the keymap passed as a file descriptor.

use std::fs::File;
use std::io::Seek;
use std::mem::MaybeUninit;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use std::thread;

use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};

use super::*;

const GLOBALS: [&str; 3] =
    ["wl_seat", "zwp_virtual_keyboard_manager_v1", "zwlr_virtual_pointer_manager_v1"];

/// A display that also says how big its output is.
const SIZED: [&str; 5] = [
    "wl_seat",
    "zwp_virtual_keyboard_manager_v1",
    "zwlr_virtual_pointer_manager_v1",
    "wl_output",
    "zxdg_output_manager_v1",
];

/// What the compositor saw: (object, opcode, body), and the keymap it was sent.
#[derive(Default)]
struct Seen {
    requests: Vec<(u32, u16, Vec<u8>)>,
    keymap: Option<String>,
}

fn send(conn: &mut UnixStream, obj: u32, op: u16, body: &[u8]) {
    let mut msg = words(&[obj, ((8 + body.len() as u32) << 16) | u32::from(op)]);
    msg.extend_from_slice(body);
    conn.write_all(&msg).unwrap();
}

/// Serve one client: offer `offer` as globals, answer every sync, and keep
/// every other request (and the keymap's contents) in `seen`.
fn compositor(
    path: &Path,
    offer: &'static [&'static str],
    seen: Arc<Mutex<Seen>>,
) -> thread::JoinHandle<()> {
    sized_compositor(path, offer, &[], seen)
}

/// [`compositor`], whose output tells its xdg-output each of `sizes` in turn,
/// as a display that changes size does.
fn sized_compositor(
    path: &Path,
    offer: &'static [&'static str],
    sizes: &'static [(u32, u32)],
    seen: Arc<Mutex<Seen>>,
) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut xdg_outputs = 0;
        loop {
            let mut data = [0u8; 4096];
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let got = recvmsg(
                &conn,
                &mut [io::IoSliceMut::new(&mut data)],
                &mut control,
                RecvFlags::empty(),
            )
            .unwrap();
            for msg in control.drain() {
                if let RecvAncillaryMessage::ScmRights(fds) = msg {
                    for fd in fds {
                        // A compositor maps the keymap from its start; the
                        // sender's write left the shared offset at the end.
                        let mut file = File::from(fd);
                        file.seek(io::SeekFrom::Start(0)).unwrap();
                        let mut text = String::new();
                        file.read_to_string(&mut text).unwrap();
                        seen.lock().unwrap().keymap = Some(text);
                    }
                }
            }
            if got.bytes == 0 {
                return;
            }
            buf.extend_from_slice(&data[..got.bytes]);
            while buf.len() >= 8 {
                let obj = word(&buf, 0);
                let size_op = word(&buf, 4);
                let size = (size_op >> 16) as usize;
                if buf.len() < size {
                    break;
                }
                let body = buf[8..size].to_vec();
                buf.drain(..size);
                let op = (size_op & 0xffff) as u16;
                match (obj, op) {
                    (DISPLAY, 1) => {
                        let registry = word(&body, 0);
                        for (name, iface) in offer.iter().enumerate() {
                            let mut ev = words(&[name as u32 + 1]);
                            ev.extend(wire_str(iface));
                            ev.extend(words(&[1]));
                            send(&mut conn, registry, 0, &ev);
                        }
                    }
                    (DISPLAY, 0) => send(&mut conn, word(&body, 0), 0, &words(&[0])),
                    _ if op == 0
                        && read_str(&body, 4).as_deref() == Some("zxdg_output_manager_v1") =>
                    {
                        // The bind's new id follows the name, the string and the version.
                        let at = 4 + wire_str("zxdg_output_manager_v1").len() + 4;
                        xdg_outputs = word(&body, at);
                    }
                    (o, 1) if o == xdg_outputs => {
                        for &(w, h) in sizes {
                            send(&mut conn, word(&body, 0), 1, &words(&[w, h])); // logical_size
                        }
                    }
                    _ => seen.lock().unwrap().requests.push((obj, op, body)),
                }
            }
        }
    })
}

#[test]
fn keys_arrive_with_a_real_keymap_and_explicit_modifiers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = compositor(&path, &GLOBALS, Arc::clone(&seen));
    let mut input = VirtualInput::connect(&path).unwrap();
    let kbd = input.keyboard;
    input.key(keys::SHIFT, true).unwrap();
    input.key(35, true).unwrap();
    input.key(35, false).unwrap();
    input.key(keys::SHIFT, false).unwrap();
    input.motion(3, -4).unwrap();
    input.button(keys::BUTTON_LEFT, true).unwrap();
    drop(input);
    server.join().unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen.keymap.as_deref().map(|k| k.trim_end_matches('\0')), Some(KEYMAP));
    let on_kbd: Vec<(u16, Vec<u32>)> = seen
        .requests
        .iter()
        .filter(|(o, _, _)| *o == kbd)
        .map(|(_, op, body)| (*op, body.chunks(4).map(|c| word(c, 0)).collect()))
        .collect();
    let keys_and_mods: Vec<(u16, Vec<u32>)> = on_kbd
        .iter()
        .filter(|(op, _)| *op == 1 || *op == 2)
        .map(|(op, w)| (*op, if *op == 1 { w[1..].to_vec() } else { w.clone() }))
        .collect();
    assert_eq!(
        keys_and_mods,
        vec![
            (1, vec![42, 1]),
            (2, vec![1, 0, 0, 0]),
            (1, vec![35, 1]),
            (1, vec![35, 0]),
            (1, vec![42, 0]),
            (2, vec![0, 0, 0, 0]),
        ]
    );
    assert!(on_kbd.iter().any(|(op, _)| *op == 3), "the keyboard is destroyed on drop");
    let motion = seen.requests.iter().find(|(o, op, _)| *o == kbd + 1 && *op == 0).unwrap();
    assert_eq!((word(&motion.2, 4) as i32, word(&motion.2, 8) as i32), (3 * 256, -4 * 256));
}

#[test]
fn a_display_without_virtual_input_is_refused_at_connect() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let _server = compositor(&path, &["wl_seat"], Arc::new(Mutex::new(Seen::default())));
    let err = VirtualInput::connect(&path).err().unwrap();
    assert!(err.to_string().contains("zwp_virtual_keyboard_manager_v1"), "{err}");
    assert_eq!(err.kind(), io::ErrorKind::Other, "not mistaken for a missing display");
}

#[test]
fn no_display_is_not_found() {
    let err = VirtualInput::connect(Path::new("/nonexistent/wl")).err().unwrap();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
}

/// The requests that reached the virtual pointer, as (opcode, words): the
/// object made just after the keyboard.
fn on_pointer(seen: &Seen, pointer: u32) -> Vec<(u16, Vec<u32>)> {
    seen.requests
        .iter()
        .filter(|(o, _, _)| *o == pointer)
        .map(|(_, op, body)| (*op, body.chunks(4).map(|c| word(c, 0)).collect()))
        .collect()
}

#[test]
fn a_point_is_aimed_at_absolutely_on_a_display_that_says_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = sized_compositor(&path, &SIZED, &[(1280, 720)], Arc::clone(&seen));
    let mut input = VirtualInput::connect(&path).unwrap();
    let pointer = input.pointer;
    input.move_to(640, 360).unwrap();
    input.move_to(-5, 9999).unwrap();
    drop(input);
    server.join().unwrap();

    let moves: Vec<(u16, Vec<u32>)> = on_pointer(&seen.lock().unwrap(), pointer)
        .into_iter()
        .filter(|(op, _)| *op == 1)
        .map(|(op, w)| (op, w[1..].to_vec()))
        .collect();
    assert_eq!(
        moves,
        [(1, vec![640, 360, 1280, 720]), (1, vec![0, 720, 1280, 720])],
        "motion_absolute, held to the display"
    );
}

#[test]
fn the_display_s_latest_size_is_the_one_aimed_by() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = sized_compositor(&path, &SIZED, &[(1280, 720), (1920, 1080)], Arc::clone(&seen));
    let mut input = VirtualInput::connect(&path).unwrap();
    let pointer = input.pointer;
    input.move_to(10, 20).unwrap();
    drop(input);
    server.join().unwrap();

    let aimed = on_pointer(&seen.lock().unwrap(), pointer)
        .into_iter()
        .find(|(op, _)| *op == 1)
        .map(|(_, w)| w[1..].to_vec());
    assert_eq!(aimed, Some(vec![10, 20, 1920, 1080]));
}

#[test]
fn with_no_size_to_aim_by_a_point_is_reached_from_the_far_corner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = compositor(&path, &GLOBALS, Arc::clone(&seen));
    let mut input = VirtualInput::connect(&path).unwrap();
    let pointer = input.pointer;
    input.move_to(10, 20).unwrap();
    drop(input);
    server.join().unwrap();

    let motions: Vec<(i32, i32)> = on_pointer(&seen.lock().unwrap(), pointer)
        .into_iter()
        .filter(|(op, _)| *op == 0)
        .map(|(_, w)| (w[1] as i32 / 256, w[2] as i32 / 256))
        .collect();
    assert_eq!(motions, [(-100_000, -100_000), (10, 20)]);
}

#[test]
fn a_scroll_is_whole_wheel_notches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wl");
    let seen = Arc::new(Mutex::new(Seen::default()));
    let server = compositor(&path, &GLOBALS, Arc::clone(&seen));
    let mut input = VirtualInput::connect(&path).unwrap();
    let pointer = input.pointer;
    input.scroll(false, -3).unwrap();
    input.scroll(true, 2).unwrap();
    drop(input);
    server.join().unwrap();

    let sent: Vec<(u16, Vec<u32>)> = on_pointer(&seen.lock().unwrap(), pointer)
        .into_iter()
        .filter(|(op, _)| [4, 5, 7].contains(op))
        // Times vary; the axis_discrete's is dropped.
        .map(|(op, w)| (op, if op == 7 { w[1..].to_vec() } else { w }))
        .collect();
    let fixed = |notches: i32| (notches * 15 * 256) as u32;
    assert_eq!(
        sent,
        [
            (5, vec![0]),
            (7, vec![0, fixed(-3), -3i32 as u32]),
            (4, vec![]),
            (5, vec![0]),
            (7, vec![1, fixed(2), 2]),
            (4, vec![]),
        ],
        "a wheel's source, its notches (15 apiece), and a frame"
    );
}
