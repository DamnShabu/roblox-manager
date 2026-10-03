//! A macro's text: one command per line. One tokenizer serves the editor
//! (text as rows, and back) and the player (text as steps).

use std::fmt;
use std::time::{Duration, Instant};

use super::keys::{self, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT};

pub mod timeline;
pub use timeline::Timed;

/// A tap is a short hold: its press length is random as well.
pub const TAP_PRESS: (f64, f64) = (0.04, 0.12);
/// Typed characters are this far apart.
pub const TYPE_GAP: (f64, f64) = (0.05, 0.16);
/// A Repeat with no EVERY taps this far apart, press to press.
pub const REPEAT_EVERY: (f64, f64) = (0.1, 0.2);
/// The closest together a Repeat's taps may be.
pub(crate) const SHORTEST_EVERY: f64 = 0.02;
/// The longest any one duration may be: a day.
const LONGEST_SECS: f64 = 86_400.0;
/// The furthest a move or click may reach from where it starts.
const FURTHEST: i32 = 65_535;
/// The most wheel notches one scroll turns.
const MOST_NOTCHES: i32 = 1000;

/// The editor's step types and the command each is in a macro's text.
const STEP_TYPES: [(&str, &str); 17] = [
    ("Key", "tap"),
    ("Hold", "hold"),
    ("Press", "press"),
    ("Release", "release"),
    ("Repeat", "repeat"),
    ("Type", "type"),
    ("Click", "click"),
    ("Move", "move"),
    ("Scroll", "scroll"),
    ("Wait", "wait"),
    ("Start", "start"),
    ("Stagger", "stagger"),
    ("Note", "#"),
    ("Timeline", "timeline"),
    ("At", "at"),
    ("Path", "path"),
    ("Turn", "turn"),
];

/// One line as the editor shows it: a step type and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: String,
    pub value: String,
}

/// One thing the player does. Durations are ranges in seconds, picked
/// afresh every time.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Keys pressed together, held, released in reverse.
    Hold {
        keys: Vec<u16>,
        lo: f64,
        hi: f64,
    },
    /// Characters as (key, shifted).
    Type(Vec<(u16, bool)>),
    /// A button, optionally at a point (from the display's corner).
    Click {
        button: u16,
        at: Option<(i32, i32)>,
    },
    Move(i32, i32),
    Wait(f64, f64),
    /// A wait before the first round only.
    Start(f64, f64),
    /// Keys or buttons pressed and left down, until a Release.
    Press(Vec<u16>),
    /// Keys or buttons let go of, in the order written.
    Release(Vec<u16>),
    /// Keys tapped over and over for a time in [lo, hi], a tap every
    /// `every` seconds (a range, press to press), while the steps after it
    /// play. The round ends once it has.
    Repeat {
        keys: Vec<u16>,
        lo: f64,
        hi: f64,
        every: (f64, f64),
    },
    /// The pointer to a point (from the display's corner): at once, or
    /// gliding there over a time in [lo, hi].
    MoveTo {
        x: i32,
        y: i32,
        lo: f64,
        hi: f64,
    },
    /// The wheel turned some notches: down or right are positive.
    Scroll {
        horizontal: bool,
        notches: i32,
    },
    /// The pointer along a path: (seconds from the step's start, x, y),
    /// gliding from point to point; the first is where it starts.
    Path(Vec<(f64, i32, i32)>),
    /// Raw mouse movement -- what a game turns its camera by: (seconds from
    /// the step's start, how far it has moved all told), from nothing.
    Turn(Vec<(f64, f64, f64)>),
    /// Steps that each start at a time of their own and play over one
    /// another; it lasts `secs`, or until its last step has played.
    Timeline {
        secs: (f64, f64),
        items: Vec<Timed>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    /// 0 is until stopped.
    pub loops: u32,
    /// Seconds between the accounts it is run on together, each starting
    /// this long after the one before; 0 starts them all at once.
    pub stagger: f64,
    pub steps: Vec<Step>,
}

impl Macro {
    /// When the `nth` (from 0) of the accounts it is run on together
    /// starts, the first starting at `first`.
    pub fn start_of(&self, first: Instant, nth: usize) -> Instant {
        let nth = u32::try_from(nth).unwrap_or(u32::MAX);
        first + Duration::try_from_secs_f64(self.stagger * f64::from(nth)).unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The line at fault, from 1; None for the macro as a whole.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "line {n}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// One line that says anything.
struct Line<'a> {
    /// From 1.
    number: usize,
    /// Lower-cased; "#" for a note.
    command: String,
    rest: &'a str,
    /// The whole line, trimmed.
    text: &'a str,
}

fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.lines().enumerate().filter_map(|(i, raw)| {
        let line = raw.trim();
        if line.is_empty() {
            return None;
        }
        let (command, rest) = match line.strip_prefix('#') {
            Some(note) => ("#".to_owned(), note.trim()),
            None => {
                let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
                (alias(&cmd.to_lowercase()).to_owned(), rest.trim())
            }
        };
        Some(Line { number: i + 1, command, rest, text: line })
    })
}

/// A command by the name the editor shows its step type under ("key" for
/// "tap", "note" for "#"), as the help writes them: text written either way
/// reads the same.
fn alias(command: &str) -> &str {
    STEP_TYPES
        .iter()
        .find(|(kind, _)| kind.eq_ignore_ascii_case(command))
        .map_or(command, |(_, cmd)| cmd)
}

/// A macro as the editor's rows, and its loop count (0 until stopped). A
/// command the editor has no type for keeps its own name, so saving writes
/// it back unchanged.
pub fn rows(text: &str) -> (Vec<Row>, u32) {
    let mut rows = Vec::new();
    let mut loops = 0;
    for line in lines(text) {
        if line.command == "loop" {
            if let Ok(n) = line.rest.parse() {
                loops = n;
                continue;
            }
        }
        let kind = STEP_TYPES
            .iter()
            .find(|(_, cmd)| *cmd == line.command)
            .map_or_else(|| capitalize(&line.command), |(kind, _)| (*kind).to_owned());
        rows.push(Row { kind, value: line.rest.to_owned() });
    }
    (rows, loops)
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// [`rows`] backwards.
pub fn to_text(rows: &[Row], loops: u32) -> String {
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let cmd = STEP_TYPES
                .iter()
                .find(|(kind, _)| *kind == row.kind)
                .map_or_else(|| row.kind.to_lowercase(), |(_, cmd)| (*cmd).to_owned());
            format!("{cmd} {}", row.value).trim().to_owned()
        })
        .collect();
    if loops > 0 {
        lines.push(format!("loop {loops}"));
    }
    lines.join("\n") + "\n"
}

/// "Until stopped", "Once", "3 rounds".
pub fn loop_label(loops: u32) -> String {
    match loops {
        0 => "Until stopped".to_owned(),
        1 => "Once".to_owned(),
        n => format!("{n} rounds"),
    }
}

/// The steps a macro's text stands for. The error names the line at fault.
pub fn parse(text: &str) -> Result<Macro, ParseError> {
    let mut m = Macro { loops: 0, stagger: 0.0, steps: Vec::new() };
    // Whether the last step is a timeline its `at` lines still go under.
    let mut open = false;
    for line in lines(text) {
        if line.command == "#" {
            continue;
        }
        let at_line = |message: String| ParseError { line: Some(line.number), message };
        if line.command == "at" {
            let item = timed(&line).map_err(at_line)?;
            match m.steps.last_mut() {
                Some(Step::Timeline { items, .. }) if open => items.push(item),
                _ => return Err(at_line("an 'at' step goes under a 'timeline' line".into())),
            }
            continue;
        }
        match step(&line).map_err(at_line)? {
            Parsed::Step(s) => {
                open = matches!(s, Step::Timeline { .. });
                m.steps.push(s);
            }
            Parsed::Loops(n) => m.loops = n,
            Parsed::Stagger(secs) => m.stagger = secs,
        }
    }
    if m.steps.is_empty() {
        return Err(ParseError { line: None, message: "the macro has no steps".into() });
    }
    Ok(m)
}

enum Parsed {
    Step(Step),
    Loops(u32),
    Stagger(f64),
}

fn step(line: &Line<'_>) -> Result<Parsed, String> {
    let args: Vec<&str> = line.rest.split_whitespace().collect();
    let step = match (line.command.as_str(), args.as_slice()) {
        ("type", _) if !line.rest.is_empty() => Step::Type(typed(line.rest)?),
        ("tap", [k]) => hold(k, TAP_PRESS)?,
        ("tap", [k, secs]) | ("hold", [k, secs]) => hold(k, seconds(secs)?)?,
        ("wait", [secs]) => {
            let (lo, hi) = seconds(secs)?;
            Step::Wait(lo, hi)
        }
        ("start", [secs]) => {
            let (lo, hi) = seconds(secs)?;
            Step::Start(lo, hi)
        }
        ("press", [k]) => Step::Press(codes(k)?),
        ("release", [k]) => Step::Release(codes(k)?),
        ("repeat", [k, secs]) => repeat(k, secs, REPEAT_EVERY)?,
        ("repeat", [k, secs, every]) => {
            let every = seconds(every)?;
            if every.0 < SHORTEST_EVERY {
                return Err(format!("a repeat taps at most every {SHORTEST_EVERY} seconds"));
            }
            repeat(k, secs, every)?
        }
        ("click", args) if args.len() <= 3 => click(args)?,
        ("move", [dx, dy]) if *dx != "to" => Step::Move(int(dx)?, int(dy)?),
        ("move", ["to", x, y]) => Step::MoveTo { x: int(x)?, y: int(y)?, lo: 0.0, hi: 0.0 },
        ("move", ["to", x, y, secs]) => {
            let (lo, hi) = seconds(secs)?;
            Step::MoveTo { x: int(x)?, y: int(y)?, lo, hi }
        }
        ("scroll", args) => scroll(args)?,
        ("path", _) => timeline::path(line.rest)?,
        ("turn", _) => timeline::turn(line.rest)?,
        ("timeline", []) => Step::Timeline { secs: (0.0, 0.0), items: Vec::new() },
        ("timeline", [secs]) => Step::Timeline { secs: seconds(secs)?, items: Vec::new() },
        ("stagger", [secs]) => {
            return match seconds(secs)? {
                (lo, hi) if lo == hi => Ok(Parsed::Stagger(lo)),
                _ => Err("a stagger is one number of seconds, not a range".into()),
            };
        }
        ("loop", [n]) if n.bytes().all(|b| b.is_ascii_digit()) => {
            return Ok(Parsed::Loops(n.parse().map_err(|_| format!("not a count: '{n}'"))?));
        }
        _ => return Err(format!("don't understand '{}'", line.text)),
    };
    Ok(Parsed::Step(step))
}

/// An `at` line: one of the steps that may start at a time of its own.
fn timed(line: &Line<'_>) -> Result<Timed, String> {
    let (at, rest) = timeline::at(line.rest)?;
    let (command, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let command = alias(&command.to_lowercase()).to_owned();
    if !timeline::TIMED.contains(&command.as_str()) {
        let allowed = timeline::TIMED.join(", ");
        return Err(format!("a timeline cannot '{command}' -- only {allowed}"));
    }
    let inner = Line { command, rest: rest.trim(), ..*line };
    match step(&inner)? {
        Parsed::Step(step) => Ok(Timed { at, step }),
        Parsed::Loops(_) | Parsed::Stagger(_) => Err(format!("don't understand '{}'", line.text)),
    }
}

fn hold(token: &str, (lo, hi): (f64, f64)) -> Result<Step, String> {
    Ok(Step::Hold { keys: codes(token)?, lo, hi })
}

fn repeat(token: &str, secs: &str, every: (f64, f64)) -> Result<Step, String> {
    let (lo, hi) = seconds(secs)?;
    Ok(Step::Repeat { keys: codes(token)?, lo, hi, every })
}

/// The keys or buttons of `shift+w`, in the order written.
fn codes(token: &str) -> Result<Vec<u16>, String> {
    token
        .split('+')
        .map(|k| keys::key_code(k).ok_or_else(|| format!("unknown key '{token}'")))
        .collect()
}

fn scroll(args: &[&str]) -> Result<Step, String> {
    let usage = || "expected scroll up|down|left|right [NOTCHES]".to_owned();
    let (direction, notches) = match args {
        [d] => (d.to_lowercase(), 1),
        [d, n] => {
            let n = n.parse().ok().filter(|n| (1..=MOST_NOTCHES).contains(n));
            let n = n.ok_or_else(|| format!("a scroll turns 1 to {MOST_NOTCHES} notches"))?;
            (d.to_lowercase(), n)
        }
        _ => return Err(usage()),
    };
    let (horizontal, sign) = match direction.as_str() {
        "up" => (false, -1),
        "down" => (false, 1),
        "left" => (true, -1),
        "right" => (true, 1),
        _ => return Err(usage()),
    };
    Ok(Step::Scroll { horizontal, notches: sign * notches })
}

fn typed(text: &str) -> Result<Vec<(u16, bool)>, String> {
    text.chars()
        .map(|c| {
            keys::char_code(c)
                .ok_or_else(|| format!("cannot type '{c}' -- US keyboard characters only"))
        })
        .collect()
}

fn seconds(token: &str) -> Result<(f64, f64), String> {
    let bad = || format!("not a duration: '{token}'");
    let (lo, hi) = token.split_once('-').unwrap_or((token, token));
    let (lo, hi): (f64, f64) = (lo.parse().map_err(|_| bad())?, hi.parse().map_err(|_| bad())?);
    if !(lo.is_finite() && hi.is_finite() && 0.0 <= lo && lo <= hi) {
        return Err(bad());
    }
    if hi > LONGEST_SECS {
        return Err(format!("'{token}' is longer than a day"));
    }
    Ok((lo, hi))
}

/// A distance in pixels, within reach of any screen.
fn int(token: &str) -> Result<i32, String> {
    let n: i32 = token.parse().map_err(|_| format!("not a number: '{token}'"))?;
    if n.unsigned_abs() > FURTHEST.unsigned_abs() {
        return Err(format!("'{token}' is too far -- at most {FURTHEST} pixels"));
    }
    Ok(n)
}

fn click(args: &[&str]) -> Result<Step, String> {
    let usage = || "expected click [left|right|middle] [X Y]".to_owned();
    let (button, point) = match args.split_first() {
        Some((first, rest)) if first.chars().all(char::is_alphabetic) => (*first, rest),
        _ => ("left", args),
    };
    let button = match button {
        "left" => BUTTON_LEFT,
        "right" => BUTTON_RIGHT,
        "middle" => BUTTON_MIDDLE,
        _ => return Err(usage()),
    };
    let at = match point {
        [] => None,
        [x, y] => Some((int(x)?, int(y)?)),
        _ => return Err(usage()),
    };
    Ok(Step::Click { button, at })
}

/// A click step's value aimed at (`x`, `y`): its button word stays, any
/// point it had is replaced.
pub fn click_at(value: &str, x: i32, y: i32) -> String {
    match value.split_whitespace().next() {
        Some(button) if button.chars().all(char::is_alphabetic) => format!("{button} {x} {y}"),
        _ => format!("{x} {y}"),
    }
}

/// What a step does, for the status line.
pub fn describe(step: &Step) -> String {
    let names =
        |keys: &[u16]| keys.iter().map(|k| keys::key_name(*k)).collect::<Vec<_>>().join("+");
    match step {
        Step::Hold { keys, .. } => format!("pressing {}", names(keys)),
        Step::Press(keys) => format!("holding down {}", names(keys)),
        Step::Release(keys) => format!("letting go of {}", names(keys)),
        Step::Repeat { keys, .. } => format!("repeating {}", names(keys)),
        Step::Type(_) => "typing".into(),
        Step::Click { .. } => "clicking".into(),
        Step::Move(..) | Step::MoveTo { .. } => "moving the mouse".into(),
        Step::Scroll { .. } => "scrolling".into(),
        Step::Path(_) => "moving the mouse".into(),
        Step::Turn(_) => "turning the camera".into(),
        Step::Timeline { .. } => "playing a timeline".into(),
        Step::Wait(..) | Step::Start(..) => "waiting".into(),
    }
}

#[cfg(test)]
mod tests;
