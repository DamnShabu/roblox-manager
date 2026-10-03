//! A relay's report: the one recorder armed on it, the record key that
//! starts and stops it, and the lines that carry what the window received
//! -- both ends, so the two can never disagree.
//!
//! The relay says `rbxmgr-relay 1`; the recorder answers `arm CODE`; the
//! relay says `armed`. From the record key's next press: `start`, an event
//! line for each input (seconds from that press), and at its press after
//! that `stop SECONDS`, and the report closes.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::event::{Event, Heard};
use crate::macros::MacroError;

/// What a relay of this version says first.
const HELLO: &str = "rbxmgr-relay 1";
/// How long either end waits on the other to agree to a report.
const HANDSHAKE: Duration = Duration::from_secs(5);

/// What every connection of one relay hears, and the recorder armed on it.
#[derive(Debug, Default)]
pub struct Hub(Mutex<State>);

#[derive(Debug, Default)]
struct State {
    armed: Option<Armed>,
    /// A record key whose press was kept from the window: its release is
    /// kept from it too.
    holding: Option<u16>,
    /// Where the pointer is and what is held down: how a recording opens.
    pointer: Option<(f64, f64)>,
    down: Vec<Heard>,
    /// When the last input with a time of its own happened, and that time.
    clock: Option<(Instant, u32)>,
}

#[derive(Debug)]
struct Armed {
    /// Never waited on: a recorder that cannot keep up is dropped rather
    /// than ever holding up the window.
    out: UnixStream,
    key: u16,
    /// When the record key started the report.
    since: Option<Instant>,
}

/// Whether an input goes on to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// Kept from it: the record key, starting or ending a recording.
    Hold,
}

impl Hub {
    /// Report to `out` from the next press of `key`. A newer recorder takes
    /// over from any before it, whose report closes unsaid.
    pub fn arm(&self, out: UnixStream, key: u16) {
        self.lock().armed = Some(Armed { out, key, since: None });
    }

    /// What the window just received, at `now`, with the display's time on
    /// it in milliseconds if it has one: whether it goes on to it.
    pub fn heard(&self, heard: &Heard, time: Option<u32>, now: Instant) -> Verdict {
        let mut s = self.lock();
        let now = s.when(time, now);
        if let Heard::Key { code, down } = *heard {
            if !down && s.holding == Some(code) {
                s.holding = None;
                return Verdict::Hold;
            }
            if down && s.armed.as_ref().is_some_and(|a| a.key == code) && s.toggle(now) {
                s.holding = Some(code);
                return Verdict::Hold;
            }
        }
        s.track(heard);
        s.report(heard, now);
        Verdict::Pass
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    /// When an input arriving at `now` happened. A display that is busy
    /// hands inputs over a frame's worth at a time, so arriving says little;
    /// its own `time` on each says how far on from the input before it was
    /// -- though never later than now. One with no time happened as it came.
    fn when(&mut self, time: Option<u32>, now: Instant) -> Instant {
        let Some(ms) = time else { return now };
        let when = match self.clock {
            // The difference in a u32 that wraps round every 49 days.
            Some((at, last)) => match ms.wrapping_sub(last) as i32 {
                gap @ 0.. => at + Duration::from_millis(gap.unsigned_abs().into()),
                gap => {
                    at.checked_sub(Duration::from_millis(gap.unsigned_abs().into())).unwrap_or(at)
                }
            },
            None => now,
        }
        .min(now);
        self.clock = Some((when, ms));
        when
    }

    /// The record key, pressed: the report starts, or it ends and closes.
    /// False when the recorder has gone, which disarms.
    fn toggle(&mut self, now: Instant) -> bool {
        let opening: Vec<String> = self
            .pointer
            .map(|(x, y)| Heard::Motion { x, y })
            .into_iter()
            .chain(self.down.iter().cloned())
            .map(|heard| Event { at: 0.0, heard }.line())
            .collect();
        let Some(armed) = self.armed.as_mut() else { return false };
        let said = match armed.since {
            None => {
                armed.since = Some(now);
                let lines: Vec<&str> =
                    std::iter::once("start").chain(opening.iter().map(String::as_str)).collect();
                say(&mut armed.out, &lines.join("\n"))
            }
            Some(since) => {
                let said = say(&mut armed.out, &format!("stop {:.3}", seconds(since, now)));
                self.armed = None;
                return said.is_ok();
            }
        };
        if said.is_err() {
            self.armed = None;
        }
        said.is_ok()
    }

    /// Keep where the pointer is and what is down, for the next opening.
    fn track(&mut self, heard: &Heard) {
        match heard {
            Heard::Motion { x, y } => self.pointer = Some((*x, *y)),
            Heard::Key { code, down } | Heard::Button { code, down } => {
                let same = |h: &Heard| match (h, heard) {
                    (Heard::Key { code: c, .. }, Heard::Key { .. })
                    | (Heard::Button { code: c, .. }, Heard::Button { .. }) => c == code,
                    _ => false,
                };
                self.down.retain(|h| !same(h));
                if *down {
                    self.down.push(heard.clone());
                }
            }
            Heard::Scroll { .. } | Heard::Turn { .. } => {}
        }
    }

    /// Tell a started report: a recorder that is gone or cannot keep up is
    /// dropped.
    fn report(&mut self, heard: &Heard, now: Instant) {
        let Some(armed) = self.armed.as_mut() else { return };
        let Some(since) = armed.since else { return };
        let line = Event { at: seconds(since, now), heard: heard.clone() }.line();
        if say(&mut armed.out, &line).is_err() {
            self.armed = None;
        }
    }
}

/// Seconds from `since` to `now`.
fn seconds(since: Instant, now: Instant) -> f64 {
    now.saturating_duration_since(since).as_secs_f64()
}

/// One or more lines, whole, or an error.
fn say(out: &mut UnixStream, lines: &str) -> io::Result<()> {
    out.write_all(format!("{lines}\n").as_bytes())
}

/// Arm `hub` for the recorder on `stream`: say which relay this is, take its
/// record key, and from then on give it the report.
pub fn serve(stream: UnixStream, hub: &Hub) -> io::Result<()> {
    stream.set_read_timeout(Some(HANDSHAKE))?;
    let mut out = &stream;
    writeln!(out, "{HELLO}")?;
    let mut asked = String::new();
    BufReader::new(&stream).read_line(&mut asked)?;
    let key = asked.trim().strip_prefix("arm ").and_then(|k| k.parse().ok());
    let key = key.ok_or_else(|| io::Error::other(format!("not a record key: {asked:?}")))?;
    writeln!(out, "armed")?;
    stream.set_nonblocking(true)?;
    hub.arm(stream, key);
    Ok(())
}

/// What a relay says next, to the recorder armed on it.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// The record key was pressed: inputs follow.
    Started,
    Heard(Event),
    /// The record key again, this many seconds after it started.
    Stopped(f64),
}

/// The manager's end of a report.
#[derive(Debug)]
pub struct Recorder {
    lines: BufReader<UnixStream>,
    stream: UnixStream,
}

/// Ends a recorder's wait from any thread.
#[derive(Debug)]
pub struct Closer(UnixStream);

impl Recorder {
    /// Arm the relay whose report socket is `record_file`: from the next
    /// press of `key` in its window, it reports.
    pub fn arm(record_file: &Path, key: u16) -> Result<Self, MacroError> {
        let stream = UnixStream::connect(record_file).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => MacroError::NoRelay,
            _ => went_away(&e),
        })?;
        stream.set_read_timeout(Some(HANDSHAKE)).map_err(|e| went_away(&e))?;
        let mut lines = BufReader::new(stream.try_clone().map_err(|e| went_away(&e))?);
        if read_line(&mut lines)? != HELLO {
            return Err(MacroError::NoRelay);
        }
        writeln!(&stream, "arm {key}").map_err(|e| went_away(&e))?;
        if read_line(&mut lines)? != "armed" {
            return Err(MacroError::NoRelay);
        }
        // From here, a wait lasts as long as the recording does.
        stream.set_read_timeout(None).map_err(|e| went_away(&e))?;
        Ok(Recorder { lines, stream })
    }

    /// Wait for what the relay says next. A line this version does not know
    /// is passed over.
    pub fn hear(&mut self) -> Result<Report, MacroError> {
        loop {
            let line = read_line(&mut self.lines)?;
            if line == "start" {
                return Ok(Report::Started);
            }
            if let Some(secs) = line.strip_prefix("stop ").and_then(|s| s.parse().ok()) {
                return Ok(Report::Stopped(secs));
            }
            if let Some(event) = Event::parse(&line) {
                return Ok(Report::Heard(event));
            }
        }
    }

    /// What ends a wait from any thread; the relay disarms.
    pub fn closer(&self) -> io::Result<Closer> {
        Ok(Closer(self.stream.try_clone()?))
    }
}

impl Closer {
    pub fn close(&self) {
        // Already closed from the other end is closed all the same.
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

/// The next line, without its end; the report closing is the window going.
fn read_line(lines: &mut BufReader<UnixStream>) -> Result<String, MacroError> {
    let mut line = String::new();
    match lines.read_line(&mut line) {
        Ok(0) => Err(MacroError::WentAway("its report ended".into())),
        Ok(_) => Ok(line.trim_end().to_owned()),
        Err(e) => Err(went_away(&e)),
    }
}

fn went_away(e: &io::Error) -> MacroError {
    MacroError::WentAway(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::thread;

    use super::*;

    const F8: u16 = 66;

    fn key(code: u16, down: bool) -> Heard {
        Heard::Key { code, down }
    }

    /// Every line `stream` is sent until its end is closed.
    fn said(stream: UnixStream) -> Vec<String> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        BufReader::new(stream).lines().map(Result::unwrap).collect()
    }

    /// Whether `stream` has been sent nothing so far.
    fn quiet(stream: &UnixStream) -> bool {
        stream.set_nonblocking(true).unwrap();
        let mut byte = [0u8];
        let quiet = matches!(
            io::Read::read(&mut &*stream, &mut byte),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock
        );
        stream.set_nonblocking(false).unwrap();
        quiet
    }

    /// A moment `secs` after `t0`.
    fn after(t0: Instant, secs: f64) -> Instant {
        t0 + Duration::from_secs_f64(secs)
    }

    #[test]
    fn nothing_is_reported_or_kept_from_the_window_before_the_record_key() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        assert_eq!(hub.heard(&key(30, true), None, Instant::now()), Verdict::Pass);
        assert!(quiet(&ours));
    }

    #[test]
    fn the_record_key_starts_and_stops_a_report_and_never_reaches_the_window() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        let t0 = Instant::now();
        assert_eq!(hub.heard(&key(F8, true), None, t0), Verdict::Hold);
        assert_eq!(hub.heard(&key(F8, false), None, after(t0, 0.1)), Verdict::Hold);
        assert_eq!(hub.heard(&key(30, true), None, after(t0, 0.5)), Verdict::Pass);
        let motion = Heard::Motion { x: 1.5, y: 2.0 };
        assert_eq!(hub.heard(&motion, None, after(t0, 0.75)), Verdict::Pass);
        assert_eq!(hub.heard(&key(F8, true), None, after(t0, 1.25)), Verdict::Hold);
        assert_eq!(hub.heard(&key(F8, false), None, after(t0, 1.3)), Verdict::Hold);
        assert_eq!(
            hub.heard(&key(F8, true), None, after(t0, 2.0)),
            Verdict::Pass,
            "the report is over: the key is the window's again"
        );
        assert_eq!(said(ours), ["start", "key 0.500 30 down", "motion 0.750 1.5 2", "stop 1.250"]);
    }

    #[test]
    fn inputs_are_timed_by_the_display_not_by_when_a_busy_one_hands_them_over() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        let t0 = Instant::now();
        hub.heard(&key(F8, true), Some(10_000), t0);
        hub.heard(&key(F8, false), Some(10_050), after(t0, 0.05));
        // A lagging display: a second of input, handed over all at once.
        let late = after(t0, 1.5);
        hub.heard(&key(30, true), Some(10_200), late);
        hub.heard(&key(30, false), Some(10_700), late);
        hub.heard(&Heard::Motion { x: 3.0, y: 4.0 }, None, late);
        hub.heard(&key(F8, true), Some(11_200), late);
        assert_eq!(
            said(ours),
            ["start", "key 0.200 30 down", "key 0.700 30 up", "motion 1.500 3 4", "stop 1.200"],
            "an input with no time of its own happened as it came"
        );
    }

    #[test]
    fn the_display_s_clock_running_ahead_never_puts_an_input_in_the_future() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        let t0 = Instant::now();
        hub.heard(&key(F8, true), Some(u32::MAX - 100), t0);
        hub.heard(&key(30, true), Some(400), after(t0, 0.5)); // wrapped round
        hub.heard(&key(30, false), Some(9_000), after(t0, 0.75));
        hub.heard(&key(F8, true), Some(9_100), after(t0, 1.0));
        assert_eq!(said(ours), ["start", "key 0.500 30 down", "key 0.750 30 up", "stop 0.850"]);
    }

    #[test]
    fn a_recording_opens_with_where_the_pointer_is_and_what_is_held_down() {
        let hub = Hub::default();
        let t0 = Instant::now();
        hub.heard(&Heard::Motion { x: 10.0, y: 20.0 }, None, t0);
        hub.heard(&key(17, true), None, t0);
        hub.heard(&Heard::Button { code: 0x110, down: true }, None, t0);
        hub.heard(&key(18, true), None, t0);
        hub.heard(&key(18, false), None, t0);
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        hub.heard(&key(F8, true), None, after(t0, 3.0));
        hub.heard(&key(F8, true), None, after(t0, 4.0));
        assert_eq!(
            said(ours),
            [
                "start",
                "motion 0.000 10 20",
                "key 0.000 17 down",
                "button 0.000 272 down",
                "stop 1.000"
            ]
        );
    }

    #[test]
    fn a_recorder_that_went_away_is_dropped_and_the_key_reaches_the_window() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        drop(ours);
        assert_eq!(hub.heard(&key(F8, true), None, Instant::now()), Verdict::Pass);
        assert_eq!(hub.heard(&key(F8, false), None, Instant::now()), Verdict::Pass);
    }

    #[test]
    fn a_recorder_that_goes_away_mid_recording_leaves_the_window_its_keys() {
        let hub = Hub::default();
        let (ours, theirs) = UnixStream::pair().unwrap();
        hub.arm(theirs, F8);
        let t0 = Instant::now();
        assert_eq!(hub.heard(&key(F8, true), None, t0), Verdict::Hold);
        drop(ours);
        assert_eq!(hub.heard(&key(30, true), None, t0), Verdict::Pass);
        assert_eq!(hub.heard(&key(F8, true), None, t0), Verdict::Pass, "no longer armed");
    }

    #[test]
    fn a_newer_recorder_takes_over_from_the_one_before() {
        let hub = Hub::default();
        let (old, old_theirs) = UnixStream::pair().unwrap();
        let (new, new_theirs) = UnixStream::pair().unwrap();
        hub.arm(old_theirs, F8);
        hub.arm(new_theirs, F8);
        assert!(said(old).is_empty(), "the old one is closed, unsaid");
        hub.heard(&key(F8, true), None, Instant::now());
        hub.heard(&key(F8, true), None, Instant::now());
        assert_eq!(said(new).first().map(String::as_str), Some("start"));
    }

    /// A relay's report socket at `path`, serving one recorder with `hub`.
    fn relay_at(path: &Path, hub: &Arc<Hub>) -> thread::JoinHandle<()> {
        let listener = UnixListener::bind(path).unwrap();
        let hub = Arc::clone(hub);
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve(stream, &hub).unwrap();
        })
    }

    #[test]
    fn a_recorder_hears_its_relay_from_the_first_record_key_to_the_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rbxmgr-7.record");
        let hub = Arc::new(Hub::default());
        let relay = relay_at(&path, &hub);
        let mut rec = Recorder::arm(&path, F8).unwrap();
        relay.join().unwrap();
        let t0 = Instant::now();
        assert_eq!(
            hub.heard(&key(F8, true), None, t0),
            Verdict::Hold,
            "armed with the key asked for"
        );
        hub.heard(&key(30, true), None, after(t0, 0.5));
        hub.heard(&key(F8, true), None, after(t0, 1.0));
        assert_eq!(rec.hear(), Ok(Report::Started));
        assert_eq!(rec.hear(), Ok(Report::Heard(Event { at: 0.5, heard: key(30, true) })));
        assert_eq!(rec.hear(), Ok(Report::Stopped(1.0)));
    }

    #[test]
    fn no_relay_to_arm_is_named_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let err = Recorder::arm(&dir.path().join("rbxmgr-7.record"), F8).unwrap_err();
        assert_eq!(err, MacroError::NoRelay);
    }

    #[test]
    fn a_relay_of_another_version_is_no_relay_to_arm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rbxmgr-7.record");
        let listener = UnixListener::bind(&path).unwrap();
        let other = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            writeln!(stream, "rbxmgr-relay 99").unwrap();
        });
        assert_eq!(Recorder::arm(&path, F8).unwrap_err(), MacroError::NoRelay);
        other.join().unwrap();
    }

    #[test]
    fn a_recorder_whose_report_ends_unfinished_is_told_its_window_went_away() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rbxmgr-7.record");
        let hub = Arc::new(Hub::default());
        let relay = relay_at(&path, &hub);
        let mut rec = Recorder::arm(&path, F8).unwrap();
        relay.join().unwrap();
        // Another recorder takes over: this one's report ends unfinished.
        hub.arm(UnixStream::pair().unwrap().0, F8);
        assert!(matches!(rec.hear(), Err(MacroError::WentAway(_))));
    }

    #[test]
    fn a_recorder_closed_from_another_thread_stops_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rbxmgr-7.record");
        let hub = Arc::new(Hub::default());
        let relay = relay_at(&path, &hub);
        let mut rec = Recorder::arm(&path, F8).unwrap();
        relay.join().unwrap();
        let closer = rec.closer().unwrap();
        let closing = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            closer.close();
        });
        assert!(rec.hear().is_err());
        closing.join().unwrap();
    }
}
