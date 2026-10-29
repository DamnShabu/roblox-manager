//! Clients that ended on their own, badly: watched from the moment they are
//! started until they exit, so the window can say so at once with the log
//! that explains it, rather than the row quietly going back to stopped.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::process::Child;
use crate::types::Profile;

/// How often a watched client is looked at.
const EVERY: Duration = Duration::from_secs(1);
/// How much of the log a report keeps: the end is what explains it.
const TAIL_LINES: usize = 300;

/// A client that ended badly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCrash {
    pub profile: Profile,
    /// Its exit status; 128 + the signal when a signal ended it.
    pub status: i32,
    /// Its log, which stays where it was written.
    pub log: PathBuf,
    /// The end of that log.
    pub tail: String,
}

impl ClientCrash {
    /// What ended it, in words.
    pub fn how(&self) -> String {
        match self.status - 128 {
            6 => "It aborted (SIGABRT).".to_owned(),
            9 => "It was killed (SIGKILL), often for running out of memory.".to_owned(),
            11 => "It crashed (segmentation fault).".to_owned(),
            n if n > 0 && n < 64 => format!("It was ended by signal {n}."),
            _ => format!("It exited with status {}.", self.status),
        }
    }
}

/// Whether a client's exit status is a crash rather than an ordinary end:
/// a clean exit, or the SIGTERM the manager's Stop sends, or a hangup or
/// Ctrl+C, the ways a session normally ends one.
pub fn is_crash(status: i32) -> bool {
    !matches!(status, 0 | 129 | 130 | 143)
}

/// Clients' crashes as they happen, for the window to take.
#[derive(Clone, Default)]
pub struct Crashes(Arc<Mutex<Vec<ClientCrash>>>);

impl Crashes {
    /// Watch `child` on a thread of its own until it exits; a crash is kept
    /// for [`Crashes::take`]. A client whose state cannot be read is left
    /// unwatched: nothing it does after that could be told apart.
    pub fn watch(&self, profile: Profile, mut child: Box<dyn Child>, log: PathBuf) {
        let found = self.0.clone();
        thread::spawn(move || {
            let status = loop {
                match child.exited() {
                    Ok(Some(status)) => break status,
                    Ok(None) => thread::sleep(EVERY),
                    Err(_) => return,
                }
            };
            if is_crash(status) {
                let tail = tail(&log);
                if let Ok(mut f) = found.lock() {
                    f.push(ClientCrash { profile, status, log, tail });
                }
            }
        });
    }

    /// The crashes since the last look.
    pub fn take(&self) -> Vec<ClientCrash> {
        self.0.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
    }
}

/// The last lines of a log; empty when it cannot be read.
fn tail(path: &Path) -> String {
    let text = fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cordial::CordialError;
    use crate::types::UserId;

    struct Ends(Vec<Option<i32>>);

    impl Child for Ends {
        fn exited(&mut self) -> Result<Option<i32>, CordialError> {
            Ok(if self.0.len() > 1 { self.0.remove(0) } else { self.0[0] })
        }
    }

    fn wait_for(crashes: &Crashes) -> Vec<ClientCrash> {
        for _ in 0..50 {
            let found = crashes.take();
            if !found.is_empty() {
                return found;
            }
            thread::sleep(Duration::from_millis(100));
        }
        Vec::new()
    }

    #[test]
    fn ordinary_ends_are_not_crashes() {
        for status in [0, 129, 130, 143] {
            assert!(!is_crash(status), "{status}");
        }
        for status in [1, 134, 137, 139] {
            assert!(is_crash(status), "{status}");
        }
    }

    #[test]
    fn a_crash_is_kept_once_with_the_end_of_its_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("1.log");
        let lines: Vec<String> = (0..TAIL_LINES + 20).map(|n| format!("line {n}")).collect();
        fs::write(&log, lines.join("\n")).unwrap();
        let crashes = Crashes::default();
        let profile = Profile::of(UserId(1));
        crashes.watch(profile.clone(), Box::new(Ends(vec![None, Some(139)])), log.clone());
        let found = wait_for(&crashes);
        assert_eq!(found.len(), 1);
        assert_eq!((&found[0].profile, found[0].status, &found[0].log), (&profile, 139, &log));
        assert!(found[0].tail.ends_with(&format!("line {}", TAIL_LINES + 19)));
        assert!(!found[0].tail.contains("line 19\n"));
        assert!(found[0].how().contains("segmentation fault"));
        assert!(crashes.take().is_empty());
    }

    #[test]
    fn a_clean_exit_is_not_kept() {
        let crashes = Crashes::default();
        crashes.watch(
            Profile::of(UserId(1)),
            Box::new(Ends(vec![Some(0)])),
            PathBuf::from("/nonexistent"),
        );
        thread::sleep(Duration::from_millis(200));
        assert!(crashes.take().is_empty());
    }
}
