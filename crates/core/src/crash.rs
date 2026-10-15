//! What the last run left behind when it did not end cleanly: a panic's
//! message and backtrace, and the run's activity up to it, kept as a crash
//! log the window shows on the next start.
//!
//! A panic inside a GTK callback aborts the process, so nothing after the
//! panic hook runs: the report has to be on disk before the abort, and is
//! only read on the next start. A run that is killed or segfaults writes no
//! panic at all, which is why a run marks itself running and unmarks itself
//! on a clean exit -- a mark that is still there next time is the crash.

use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The marker a run holds while it is up: its pid.
const RUNNING: &str = "running";
/// The run's activity, a line at a time.
const SESSION: &str = "session.log";
/// The previous run's activity, once the session log grew past its size.
const SESSION_OLD: &str = "session.log.1";
/// A panic's message, location and backtrace.
const PANIC: &str = "panic.txt";
/// Past this the session log starts over, keeping one older file.
const SESSION_LIMIT: u64 = 1024 * 1024;
/// How much of the activity a report keeps: the end is what matters.
const TAIL_LINES: usize = 300;
/// Crash logs kept; older ones go.
const KEPT: usize = 10;

/// A run that did not end cleanly, as found on the next start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    /// The whole report.
    pub log: String,
    /// Where it was saved.
    pub saved: PathBuf,
    /// Whether it panicked, rather than being killed or crashing in native code.
    pub panicked: bool,
}

/// This run's crash records, in `dir`.
#[derive(Debug)]
pub struct CrashLog {
    dir: PathBuf,
    session: Mutex<Option<File>>,
}

impl CrashLog {
    pub fn new(dir: &Path) -> Self {
        CrashLog { dir: dir.to_owned(), session: Mutex::new(None) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Start this run: the last one's crash, if it left one, saved as a
    /// crash log; then a fresh session log and this run marked running.
    /// `version` heads the report. A marker held by a process that is still
    /// alive is another copy, not a crash.
    pub fn begin(&self, pid: u32, version: &str) -> io::Result<Option<Crash>> {
        fs::create_dir_all(&self.dir)?;
        let crash = self.left_behind(pid, version)?;
        for stale in [PANIC, SESSION_OLD] {
            remove(&self.dir.join(stale))?;
        }
        let session = File::create(self.dir.join(SESSION))?;
        if let Ok(mut s) = self.session.lock() {
            *s = Some(session);
        }
        fs::write(self.dir.join(RUNNING), pid.to_string())?;
        Ok(crash)
    }

    /// One line of the run's activity, stamped. A line that cannot be
    /// written is dropped: the log is for after a crash, and is no reason
    /// to stop anything now.
    pub fn line(&self, stamp: &str, text: &str) {
        let Ok(mut guard) = self.session.lock() else { return };
        let Some(file) = guard.as_mut() else { return };
        if file.metadata().is_ok_and(|m| m.len() > SESSION_LIMIT) {
            let (path, old) = (self.dir.join(SESSION), self.dir.join(SESSION_OLD));
            match fs::rename(&path, &old).and_then(|()| File::create(&path)) {
                Ok(fresh) => *file = fresh,
                Err(_) => return,
            }
        }
        if writeln!(file, "{stamp} {text}").is_err() {
            *guard = None;
        }
    }

    /// Record a panic, before the process aborts. Added after any before it
    /// rather than over it: a panic inside a GTK callback is followed by a
    /// second, "panic in a function that cannot unwind", which would
    /// otherwise replace the one that says what went wrong.
    pub fn panicked(&self, report: &str) -> io::Result<()> {
        let mut file =
            fs::OpenOptions::new().append(true).create(true).open(self.dir.join(PANIC))?;
        // Whole in one write, so a report is never half a message.
        file.write_all(format!("{}\n\n", report.trim_end()).as_bytes())
    }

    /// A clean exit: nothing to report next time.
    pub fn end(&self) -> io::Result<()> {
        if let Ok(mut s) = self.session.lock() {
            s.take();
        }
        remove(&self.dir.join(RUNNING))?;
        remove(&self.dir.join(PANIC))
    }

    /// The report the last run left, saved; None after a clean exit.
    fn left_behind(&self, pid: u32, version: &str) -> io::Result<Option<Crash>> {
        let marker = self.dir.join(RUNNING);
        let held = match fs::read_to_string(&marker) {
            Ok(text) => text.trim().parse::<u32>().ok(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if held.is_some_and(|other| other != pid && alive_as_us(other)) {
            return Ok(None);
        }
        let panic = fs::read_to_string(self.dir.join(PANIC)).ok();
        let mut activity = fs::read_to_string(self.dir.join(SESSION_OLD)).unwrap_or_default();
        activity.push_str(&fs::read_to_string(self.dir.join(SESSION)).unwrap_or_default());
        let when = fs::metadata(self.dir.join(SESSION))
            .and_then(|m| m.modified())
            .ok()
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S"))
            .map_or_else(|| "unknown".to_owned(), |t| t.to_string());
        let log = report(version, &when, panic.as_deref(), &activity);
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let saved = self.dir.join(format!("crash-{stamp}.log"));
        fs::write(&saved, &log)?;
        self.prune()?;
        Ok(Some(Crash { log, saved, panicked: panic.is_some() }))
    }

    /// Keep the newest crash logs, by name (which is their time).
    fn prune(&self) -> io::Result<()> {
        let mut logs: Vec<PathBuf> = fs::read_dir(&self.dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".log"))
            })
            .collect();
        logs.sort();
        let extra = logs.len().saturating_sub(KEPT);
        for old in logs.into_iter().take(extra) {
            remove(&old)?;
        }
        Ok(())
    }
}

/// The text of a crash log: what crashed, how, and what it was doing.
pub fn report(version: &str, when: &str, panic: Option<&str>, activity: &str) -> String {
    let how = match panic {
        Some(p) => p.trim_end().to_owned(),
        None => "No panic was recorded: the app was killed, froze and was closed by force, \
                 or crashed in native code."
            .to_owned(),
    };
    let lines: Vec<&str> = activity.lines().collect();
    let tail = &lines[lines.len().saturating_sub(TAIL_LINES)..];
    let activity = if tail.is_empty() { "(none)".to_owned() } else { tail.join("\n") };
    format!(
        "Stacked {version} did not close cleanly.\nLast activity at: {when}\n\n\
         -- What happened --\n{how}\n\n-- Activity before it --\n{activity}\n"
    )
}

/// A panic as the report gives it: message, where, and the backtrace.
pub fn panic_report(message: &str, location: &str, thread: &str, backtrace: &str) -> String {
    format!("Panic in thread '{thread}' at {location}:\n{message}\n\nBacktrace:\n{backtrace}")
}

/// Whether `pid` is a copy of this program, not some other process that
/// took the pid since.
fn alive_as_us(pid: u32) -> bool {
    let exe = |p: &str| fs::read_link(format!("/proc/{p}/exe")).ok();
    match (exe(&pid.to_string()), exe("self")) {
        (Some(theirs), Some(ours)) => theirs.file_name() == ours.file_name(),
        _ => false,
    }
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Opened for appending, for tests that write a session by hand.
#[cfg(test)]
fn append(path: &Path) -> File {
    fs::OpenOptions::new().append(true).create(true).open(path).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_exit_leaves_nothing_to_report() {
        let dir = tempfile::tempdir().unwrap();
        let log = CrashLog::new(dir.path());
        assert_eq!(log.begin(1, "1.0").unwrap(), None);
        log.line("10:00", "Ready");
        log.end().unwrap();
        assert_eq!(CrashLog::new(dir.path()).begin(2, "1.0").unwrap(), None);
    }

    #[test]
    fn a_panic_is_reported_on_the_next_start_with_the_activity_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = CrashLog::new(dir.path());
        first.begin(u32::MAX - 1, "1.0").unwrap();
        first.line("10:00", "Launched alt");
        first.panicked(&panic_report("boom", "src/x.rs:1:2", "main", "frames")).unwrap();
        first.panicked(&panic_report("cannot unwind", "core", "main", "frames")).unwrap();
        // No end: the process aborted.
        let crash = CrashLog::new(dir.path()).begin(2, "1.0").unwrap().unwrap();
        assert!(crash.panicked);
        assert!(crash.log.contains("Stacked 1.0 did not close cleanly"));
        assert!(crash.log.contains("boom") && crash.log.contains("src/x.rs:1:2"));
        // The panic that says what went wrong is kept, not replaced.
        assert!(crash.log.find("boom") < crash.log.find("cannot unwind"));
        assert!(crash.log.contains("10:00 Launched alt"));
        assert_eq!(fs::read_to_string(&crash.saved).unwrap(), crash.log);
    }

    #[test]
    fn a_run_killed_without_a_panic_is_still_reported() {
        let dir = tempfile::tempdir().unwrap();
        CrashLog::new(dir.path()).begin(u32::MAX - 1, "1.0").unwrap();
        let crash = CrashLog::new(dir.path()).begin(2, "1.0").unwrap().unwrap();
        assert!(!crash.panicked);
        assert!(crash.log.contains("No panic was recorded"));
    }

    #[test]
    fn a_report_is_made_once() {
        let dir = tempfile::tempdir().unwrap();
        CrashLog::new(dir.path()).begin(u32::MAX - 1, "1.0").unwrap();
        let second = CrashLog::new(dir.path());
        assert!(second.begin(2, "1.0").unwrap().is_some());
        second.end().unwrap();
        assert_eq!(CrashLog::new(dir.path()).begin(3, "1.0").unwrap(), None);
    }

    #[test]
    fn only_the_newest_crash_logs_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..KEPT + 3 {
            fs::write(dir.path().join(format!("crash-2026{n:04}.log")), "x").unwrap();
        }
        CrashLog::new(dir.path()).prune().unwrap();
        let left = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(left, KEPT);
        assert!(!dir.path().join("crash-20260000.log").exists());
    }

    #[test]
    fn the_report_keeps_the_end_of_a_long_activity() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = append(&dir.path().join("a"));
        for n in 0..TAIL_LINES + 50 {
            writeln!(f, "line {n}").unwrap();
        }
        let activity = fs::read_to_string(dir.path().join("a")).unwrap();
        let r = report("1.0", "now", None, &activity);
        assert!(r.contains(&format!("line {}", TAIL_LINES + 49)));
        assert!(!r.contains("line 49\n"));
    }
}
