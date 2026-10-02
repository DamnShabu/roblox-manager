//! The Wayland wire format, as both ends of a display connection put it on
//! the socket: messages of native-endian words, strings, fixed-point
//! numbers.

/// The display object every connection starts with.
pub const DISPLAY: u32 = 1;

/// One message: object, size and opcode, body.
pub fn message(obj: u32, op: u16, body: &[u8]) -> Vec<u8> {
    let mut msg = words(&[obj, ((8 + body.len() as u32) << 16) | u32::from(op)]);
    msg.extend_from_slice(body);
    msg
}

/// Wire words: native-endian u32s.
pub fn words(vals: &[u32]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_ne_bytes()).collect()
}

/// A wire string: length with its NUL, the bytes, NUL, padded to 4.
pub fn wire_str(s: &str) -> Vec<u8> {
    let mut out = words(&[s.len() as u32 + 1]);
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// The string at `off` in a message body.
pub fn read_str(body: &[u8], off: usize) -> Option<String> {
    let n = u32::from_ne_bytes(body.get(off..off + 4)?.try_into().ok()?) as usize;
    let bytes = body.get(off + 4..off + 3 + n)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// The word at `off`; 0 past the end.
pub fn word(body: &[u8], off: usize) -> u32 {
    body.get(off..off + 4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_ne_bytes)
}

/// Whole messages out of a byte stream, however it is split as it arrives.
#[derive(Debug, Default)]
pub struct Framer {
    buf: Vec<u8>,
}

/// A message shorter than its own header: the stream is not Wayland, or out
/// of step.
#[derive(Debug, PartialEq, Eq)]
pub struct Malformed;

impl Framer {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message, header and all, once all of it has arrived.
    pub fn next(&mut self) -> Result<Option<Vec<u8>>, Malformed> {
        if self.buf.len() < 8 {
            return Ok(None);
        }
        let size = (word(&self.buf, 4) >> 16) as usize;
        if size < 8 {
            return Err(Malformed);
        }
        if self.buf.len() < size {
            return Ok(None);
        }
        Ok(Some(self.buf.drain(..size).collect()))
    }

    /// Whatever has arrived and not been framed, for passing on unread once
    /// the stream cannot be framed.
    pub fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

/// A message's object and opcode.
pub fn header(msg: &[u8]) -> (u32, u16) {
    (word(msg, 0), (word(msg, 4) & 0xffff) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_cut_into_whole_messages_however_it_arrives() {
        let sent = [
            message(1, 0, &words(&[2])),
            message(9, 3, &[words(&[1, 2, 30, 1]), wire_str("wl_seat")].concat()),
            message(4, 5, &[]),
        ];
        let stream = sent.concat();
        let mut framer = Framer::default();
        let mut got = Vec::new();
        for piece in stream.chunks(5) {
            framer.push(piece);
            while let Some(msg) = framer.next().unwrap() {
                got.push(msg);
            }
        }
        assert_eq!(got, sent);
        assert_eq!(header(&got[1]), (9, 3));
    }

    #[test]
    fn a_message_shorter_than_its_header_cannot_be_framed() {
        let mut framer = Framer::default();
        framer.push(&words(&[7, (4 << 16) | 2]));
        assert_eq!(framer.next(), Err(Malformed));
    }

    #[test]
    fn wire_strings_are_nul_terminated_and_padded() {
        assert_eq!(wire_str("abc"), [words(&[4]), b"abc\0".to_vec()].concat());
        assert_eq!(wire_str("abcd").len(), 4 + 8);
        assert_eq!(read_str(&wire_str("wl_seat"), 0).as_deref(), Some("wl_seat"));
    }
}
