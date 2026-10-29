//! The server a client is joining, read from its own log as it starts.
//!
//! Stacked reads the engine's log and prints the server it is joining the
//! moment the engine names it:
//!
//! ```text
//! [cordial] game: joining server 6baeb082-1c5f-4e0b-9c4a-0123456789ab of place 17625359962
//! ```
//!
//! That line goes to the log the manager already gives every client. Roblox's
//! presence API reports the same server only once the client has finished
//! joining and Roblox has caught up, so a group waiting on presence alone
//! starts its followers seconds after the leader's server was already known
//! here. Older Stacked builds never print the line; presence still answers
//! for them.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::types::{PlaceId, ServerId};

const MARKER: &str = "[cordial] game: joining server ";

/// Follows one client's log from its start, a read of what is new each time.
pub struct JoiningWatch {
    path: PathBuf,
    offset: u64,
    /// The end of the last read when it stopped mid-line.
    partial: String,
}

impl JoiningWatch {
    pub fn new(path: PathBuf) -> Self {
        JoiningWatch { path, offset: 0, partial: String::new() }
    }

    /// The newest server the log has named since the last call, if any. A
    /// log not there yet, or not readable, has named nothing: presence is
    /// still asked, so this is never the only way to find the server.
    pub fn server(&mut self) -> Option<(ServerId, PlaceId)> {
        let mut file = File::open(&self.path).ok()?;
        let len = file.metadata().ok()?.len();
        if len < self.offset {
            // Rotated or replaced under the same name: a new client's log.
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return None;
        }
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut bytes = Vec::new();
        let read = file.take(len - self.offset).read_to_end(&mut bytes).ok()?;
        self.offset += read as u64;
        let text = std::mem::take(&mut self.partial) + &String::from_utf8_lossy(&bytes);
        let (whole, rest) = match text.rfind('\n') {
            Some(end) => text.split_at(end + 1),
            None => ("", text.as_str()),
        };
        let found = whole.lines().rev().find_map(parse);
        self.partial = rest.to_owned();
        found
    }
}

fn parse(line: &str) -> Option<(ServerId, PlaceId)> {
    let after = &line[line.find(MARKER)? + MARKER.len()..];
    let (server, place) = after.trim_end().split_once(" of place ")?;
    Some((ServerId::parse(server).ok()?, PlaceId::parse(place).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const JOB: &str = "6baeb082-1c5f-4e0b-9c4a-0123456789ab";

    fn line(job: &str, place: &str) -> String {
        format!("[cordial] game: joining server {job} of place {place}\n")
    }

    #[test]
    fn the_joining_line_names_the_server_and_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.log");
        std::fs::write(&path, format!("  pumping the looper\n{}", line(JOB, "88"))).unwrap();
        let (server, place) = JoiningWatch::new(path).server().unwrap();
        assert_eq!((server.as_str(), place.as_str()), (JOB, "88"));
    }

    #[test]
    fn a_log_not_there_yet_or_silent_names_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.log");
        let mut watch = JoiningWatch::new(path.clone());
        assert_eq!(watch.server(), None);
        std::fs::write(&path, "[cordial] game: server 1.2.3.4:5\n").unwrap();
        assert_eq!(watch.server(), None);
    }

    #[test]
    fn a_line_written_in_two_halves_is_read_once_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.log");
        let full = line(JOB, "88");
        let (a, b) = full.split_at(30);
        std::fs::write(&path, a).unwrap();
        let mut watch = JoiningWatch::new(path.clone());
        assert_eq!(watch.server(), None);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b.as_bytes())
            .unwrap();
        assert_eq!(watch.server().map(|(s, _)| s.as_str().to_owned()), Some(JOB.into()));
        assert_eq!(watch.server(), None, "only what is new is read");
    }

    #[test]
    fn after_a_teleport_the_newest_server_is_the_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.log");
        let other = "aaaaaaaa-1c5f-4e0b-9c4a-0123456789ab";
        std::fs::write(&path, line(JOB, "88") + &line(other, "99")).unwrap();
        let (server, place) = JoiningWatch::new(path).server().unwrap();
        assert_eq!((server.as_str(), place.as_str()), (other, "99"));
    }

    #[test]
    fn a_malformed_line_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.log");
        std::fs::write(&path, "[cordial] game: joining server x y of place z\n").unwrap();
        assert_eq!(JoiningWatch::new(path).server(), None);
    }
}
