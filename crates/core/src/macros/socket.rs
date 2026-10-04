//! A display connection's socket, as a relay reads and writes it: bytes with
//! the file descriptors (buffers, keymaps) that travel alongside them.

use std::io::{self, IoSlice, IoSliceMut, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

/// The most file descriptors sent at once: libwayland's own limit, which
/// every display's receiving buffer is sized for.
const MOST_FDS: usize = 28;
/// How much is read at once.
pub const BUFFER: usize = 32 * 1024;

/// Bytes into `buf`, and the file descriptors that came with them onto
/// `fds`; 0 is the other end closing.
pub fn recv(sock: &UnixStream, buf: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<usize> {
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
pub fn send(sock: &UnixStream, mut bytes: &[u8], fds: &mut Vec<OwnedFd>) -> io::Result<()> {
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
