//! One display connection, passed through: the client's requests on to cage
//! as they come, cage's events back a whole message at a time so the hub can
//! keep the record key from the window. File descriptors (buffers, keymaps)
//! travel with the bytes they came with, never ahead of them.

use std::io::{self, IoSlice, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::net::Shutdown;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

use super::objects::{Input, Objects};
use super::report::{Hub, Verdict};
use crate::macros::wire::Framer;

/// The most file descriptors sent at once: libwayland's own limit, which
/// every display's receiving buffer is sized for.
const MOST_FDS: usize = 28;
/// How much is read at once.
const BUFFER: usize = 32 * 1024;

/// Pass `client`'s connection through to `cage` until either end closes, on
/// two threads of its own.
pub fn link(client: UnixStream, cage: UnixStream, hub: Arc<Hub>) -> io::Result<()> {
    let objects = Arc::new(Mutex::new(Objects::default()));
    let (client_too, cage_too, objects_too) =
        (client.try_clone()?, cage.try_clone()?, Arc::clone(&objects));
    thread::spawn(move || {
        let ended = requests(&client, &cage, &objects);
        close(&client, &cage, ended);
    });
    thread::spawn(move || {
        let ended = events(&cage_too, &client_too, &objects_too, &hub);
        close(&client_too, &cage_too, ended);
    });
    Ok(())
}

/// The client's requests, on to cage as they come, each noted first: by the
/// time cage answers one, the objects it made are known.
fn requests(client: &UnixStream, cage: &UnixStream, objects: &Mutex<Objects>) -> io::Result<()> {
    let mut buf = vec![0u8; BUFFER];
    let mut fds = Vec::new();
    let mut framer = Framer::default();
    let mut framed = true;
    loop {
        let n = recv(client, &mut buf, &mut fds)?;
        if n == 0 {
            return Ok(());
        }
        if framed {
            framer.push(&buf[..n]);
            let mut objects = lock(objects);
            loop {
                match framer.next() {
                    Ok(Some(msg)) => objects.request(&msg),
                    Ok(None) => break,
                    // Not followed any further; still passed on.
                    Err(_) => {
                        framed = false;
                        break;
                    }
                }
            }
        }
        send(cage, &buf[..n], &mut fds)?;
    }
}

/// Cage's events, back to the client a whole message at a time; each input
/// on one is the hub's to hear, and a record key it keeps is never sent.
fn events(
    cage: &UnixStream,
    client: &UnixStream,
    objects: &Mutex<Objects>,
    hub: &Hub,
) -> io::Result<()> {
    let mut buf = vec![0u8; BUFFER];
    let mut fds = Vec::new();
    let mut framer = Framer::default();
    let mut framed = true;
    // The hub's word on the last input, for its copies to the client's other
    // keyboards or pointers.
    let mut last = Verdict::Pass;
    loop {
        let n = recv(cage, &mut buf, &mut fds)?;
        if n == 0 {
            return Ok(());
        }
        let mut out = Vec::with_capacity(n);
        if framed {
            framer.push(&buf[..n]);
            loop {
                match framer.next() {
                    Ok(Some(msg)) => {
                        let input = lock(objects).event(&msg);
                        let verdict = match input {
                            None => Verdict::Pass,
                            Some(Input::New(heard, time)) => {
                                last = hub.heard(&heard, time, Instant::now());
                                last
                            }
                            Some(Input::Again) => last,
                        };
                        if verdict == Verdict::Pass {
                            out.extend_from_slice(&msg);
                        }
                    }
                    Ok(None) => break,
                    // From here on the stream goes as it comes, unread.
                    Err(_) => {
                        framed = false;
                        out.extend(framer.rest());
                        break;
                    }
                }
            }
        } else {
            out.extend_from_slice(&buf[..n]);
        }
        if !out.is_empty() {
            send(client, &out, &mut fds)?;
        }
    }
}

/// One end has gone: the other goes too, and anything but a plain close is
/// told in the client's log.
fn close(client: &UnixStream, cage: &UnixStream, ended: io::Result<()>) {
    if let Err(e) = ended {
        eprintln!("rbxmgr relay: a display connection ended: {e}");
    }
    for end in [client, cage] {
        // One already closed is closed all the same.
        let _ = end.shutdown(Shutdown::Both);
    }
}

/// Bytes into `buf`, and the file descriptors that came with them onto
/// `fds`; 0 is the other end closing.
fn recv(sock: &UnixStream, buf: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<usize> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MOST_FDS))];
    loop {
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let got =
            match recvmsg(sock, &mut [IoSliceMut::new(buf)], &mut control, RecvFlags::CMSG_CLOEXEC)
            {
                Ok(got) => got,
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            };
        for msg in control.drain() {
            if let RecvAncillaryMessage::ScmRights(received) = msg {
                fds.extend(received);
            }
        }
        if got.flags.contains(ReturnFlags::CTRUNC) {
            return Err(io::Error::other(
                "more file descriptors came than a display sends at once",
            ));
        }
        return Ok(got.bytes);
    }
}

/// `bytes`, with `fds` sent along the first of them -- never more at once
/// than a display reads. With more than that waiting, each batch takes one
/// byte; what finds no byte to go with stays in `fds` for the next send.
fn send(sock: &UnixStream, mut bytes: &[u8], fds: &mut Vec<OwnedFd>) -> io::Result<()> {
    while !fds.is_empty() && !bytes.is_empty() {
        let batch = fds.len().min(MOST_FDS);
        let carry = if batch < fds.len() { 1 } else { bytes.len() };
        let these: Vec<BorrowedFd<'_>> = fds[..batch].iter().map(AsFd::as_fd).collect();
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MOST_FDS))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        control.push(SendAncillaryMessage::ScmRights(&these));
        let sent = loop {
            match sendmsg(sock, &[IoSlice::new(&bytes[..carry])], &mut control, SendFlags::NOSIGNAL)
            {
                Err(rustix::io::Errno::INTR) => continue,
                other => break other?,
            }
        };
        drop(these);
        fds.drain(..batch);
        bytes = &bytes[sent..];
    }
    (&*sock).write_all(bytes)
}

fn lock(objects: &Mutex<Objects>) -> MutexGuard<'_, Objects> {
    objects.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{IoSlice, IoSliceMut, Read, Seek, Write};
    use std::mem::MaybeUninit;
    use std::os::fd::{AsFd, OwnedFd};
    use std::time::Duration;

    use rustix::net::{
        RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
        SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
    };

    use super::*;
    use crate::macros::wire::{DISPLAY, message, wire_str, words};

    const F8: u32 = 66;

    /// A client and a cage, linked through a relay: (client's end, cage's end).
    fn linked(hub: &Arc<Hub>) -> (UnixStream, UnixStream) {
        let (client, relay_client) = UnixStream::pair().unwrap();
        let (relay_cage, cage) = UnixStream::pair().unwrap();
        link(relay_client, relay_cage, Arc::clone(hub)).unwrap();
        for end in [&client, &cage] {
            end.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        }
        (client, cage)
    }

    /// A file holding `text`, as a display passes a keymap.
    fn file_of(text: &str) -> File {
        let fd = rustix::fs::memfd_create("relay-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        let mut file = File::from(fd);
        file.write_all(text.as_bytes()).unwrap();
        file
    }

    fn send_with(sock: &UnixStream, bytes: &[u8], file: &File) {
        let fds = [file.as_fd()];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        control.push(SendAncillaryMessage::ScmRights(&fds));
        sendmsg(sock, &[IoSlice::new(bytes)], &mut control, SendFlags::empty()).unwrap();
    }

    /// Exactly `len` bytes, and every file descriptor that came with them.
    fn recv_exact(sock: &UnixStream, len: usize) -> (Vec<u8>, Vec<OwnedFd>) {
        let (mut bytes, mut fds) = (Vec::new(), Vec::new());
        while bytes.len() < len {
            let mut buf = vec![0u8; len - bytes.len()];
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let got =
                recvmsg(sock, &mut [IoSliceMut::new(&mut buf)], &mut control, RecvFlags::empty())
                    .unwrap();
            assert!(got.bytes > 0, "closed after {} of {len} bytes", bytes.len());
            bytes.extend_from_slice(&buf[..got.bytes]);
            for msg in control.drain() {
                if let RecvAncillaryMessage::ScmRights(received) = msg {
                    fds.extend(received);
                }
            }
        }
        (bytes, fds)
    }

    fn text_of(fd: OwnedFd) -> String {
        let mut file = File::from(fd);
        file.rewind().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        text
    }

    /// Whether `sock` has been sent nothing, within a moment.
    fn quiet(sock: &UnixStream) -> bool {
        sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let mut byte = [0u8];
        let quiet = matches!((&*sock).read(&mut byte), Err(e)
            if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut));
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        quiet
    }

    #[test]
    fn requests_and_events_pass_through_byte_for_byte_with_their_file_descriptors() {
        let (client, cage) = linked(&Arc::new(Hub::default()));
        let request = message(DISPLAY, 1, &words(&[2]));
        send_with(&client, &request, &file_of("a buffer"));
        let (bytes, fds) = recv_exact(&cage, request.len());
        assert_eq!(bytes, request);
        assert_eq!(fds.into_iter().map(text_of).collect::<Vec<_>>(), ["a buffer"]);

        let event = message(7, 0, &words(&[1, 35544]));
        send_with(&cage, &event, &file_of("a keymap"));
        let (bytes, fds) = recv_exact(&client, event.len());
        assert_eq!(bytes, event);
        assert_eq!(fds.into_iter().map(text_of).collect::<Vec<_>>(), ["a keymap"]);
    }

    #[test]
    fn the_record_key_is_kept_from_the_client_and_all_else_reaches_it() {
        let hub = Arc::new(Hub::default());
        let (client, mut cage) = linked(&hub);
        let requests = [
            message(DISPLAY, 1, &words(&[2])),
            message(2, 0, &[words(&[5]), wire_str("wl_seat"), words(&[7, 3])].concat()),
            message(3, 1, &words(&[6])),
        ]
        .concat();
        (&client).write_all(&requests).unwrap();
        recv_exact(&cage, requests.len());
        let (recorder, armed) = UnixStream::pair().unwrap();
        hub.arm(armed, F8 as u16);

        let key = |code: u32, state: u32| message(6, 3, &words(&[1, 2, code, state]));
        cage.write_all(&[key(F8, 1), key(F8, 0), key(30, 1), key(F8, 1)].concat()).unwrap();
        assert_eq!(recv_exact(&client, key(30, 1).len()).0, key(30, 1));
        cage.write_all(&key(F8, 0)).unwrap();
        assert!(quiet(&client), "the record key's presses and releases never reach it");

        recorder.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let said: Vec<String> =
            io::BufRead::lines(io::BufReader::new(recorder)).map(Result::unwrap).collect();
        let kinds: Vec<&str> = said.iter().map(|l| l.split(' ').next().unwrap()).collect();
        assert_eq!(kinds, ["start", "key", "stop"]);
        assert!(said[1].ends_with(" 30 down"), "{said:?}");
    }

    #[test]
    fn a_client_with_two_keyboards_gets_one_recording_and_never_the_record_key() {
        let hub = Arc::new(Hub::default());
        let (client, mut cage) = linked(&hub);
        let requests = [
            message(DISPLAY, 1, &words(&[2])),
            message(2, 0, &[words(&[5]), wire_str("wl_seat"), words(&[7, 3])].concat()),
            message(3, 1, &words(&[6])),
            message(3, 1, &words(&[8])),
        ]
        .concat();
        (&client).write_all(&requests).unwrap();
        recv_exact(&cage, requests.len());
        let (recorder, armed) = UnixStream::pair().unwrap();
        hub.arm(armed, F8 as u16);

        // Cage sends each key to every keyboard the client has, one serial apiece.
        let key = |kbd: u32, serial: u32, code: u32, state: u32| {
            message(kbd, 3, &words(&[serial, 2, code, state]))
        };
        let both = |serial, code, state| [key(6, serial, code, state), key(8, serial, code, state)];
        let sent = [both(1, F8, 1), both(2, F8, 0), both(3, 30, 1), both(4, F8, 1), both(5, F8, 0)];
        cage.write_all(&sent.concat().concat()).unwrap();
        let a = [key(6, 3, 30, 1), key(8, 3, 30, 1)].concat();
        assert_eq!(recv_exact(&client, a.len()).0, a, "both copies of a, nothing of F8");
        assert!(quiet(&client));

        recorder.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let said: Vec<String> =
            io::BufRead::lines(io::BufReader::new(recorder)).map(Result::unwrap).collect();
        let kinds: Vec<&str> = said.iter().map(|l| l.split(' ').next().unwrap()).collect();
        assert_eq!(kinds, ["start", "key", "stop"], "{said:?}");
    }

    #[test]
    fn a_message_split_across_reads_reaches_the_client_whole() {
        let (client, mut cage) = linked(&Arc::new(Hub::default()));
        let event = message(9, 2, &words(&[1, 2, 3]));
        cage.write_all(&event[..5]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        cage.write_all(&event[5..]).unwrap();
        assert_eq!(recv_exact(&client, event.len()).0, event);
    }

    #[test]
    fn a_stream_that_cannot_be_framed_is_still_passed_on_as_it_came() {
        let (client, mut cage) = linked(&Arc::new(Hub::default()));
        let junk = [words(&[7, 4 << 16]), b"tail".to_vec()].concat();
        cage.write_all(&junk).unwrap();
        assert_eq!(recv_exact(&client, junk.len()).0, junk);
        let after = message(9, 0, &[]);
        cage.write_all(&after).unwrap();
        assert_eq!(recv_exact(&client, after.len()).0, after);
    }

    #[test]
    fn when_either_end_closes_so_does_the_other() {
        let hub = Arc::new(Hub::default());
        let (client, cage) = linked(&hub);
        drop(client);
        assert_eq!((&cage).read(&mut [0u8; 8]).unwrap(), 0, "cage sees the client go");
        let (client, cage) = linked(&hub);
        drop(cage);
        assert_eq!((&client).read(&mut [0u8; 8]).unwrap(), 0, "the client sees cage go");
    }
}
