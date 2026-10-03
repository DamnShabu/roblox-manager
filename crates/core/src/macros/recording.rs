//! A recording: what a macro-ready window received between two presses of
//! the record key, made into one timeline that does it again.
//!
//! Every input is a step at the moment it happened: each key or button
//! held for exactly as long as it was, each run of wheel turns a scroll.
//! The mouse is a step for each movement between its rests: raw movement
//! (a turn) when the window heard any -- that is what a game turns its
//! camera by, and the pointer follows it -- or the pointer's path on the
//! window when it heard none. A movement keeps only the points it bends
//! at. Every time is rounded to a hundredth of a second.

use std::collections::HashMap;

use super::MacroError;
use super::grammar::Row;
use super::keys;
use super::relay::event::{Event, Heard};
use super::relay::report::Report;

mod simplify;

use simplify::{Point, simplify};

/// The key that starts and stops a recording, pressed in the window being
/// recorded: F8.
pub const RECORD_KEY: u16 = 66;

/// Hundredths of a second.
type Cs = i64;

/// A press shorter than this is written as a tap.
const TAP: Cs = 15;
/// How far a path may stray from the pointer's, in pixels, at any moment.
const STRAY: f64 = 2.0;
/// How far a turn may stray from the mouse's movement, in its own units.
const STRAY_TURN: f64 = 1.0;
/// The mouse still for longer than this has stopped: the next movement is
/// a step of its own.
const REST: Cs = 25;
/// Wheel turns this close together, the same way, are one scroll.
const SPIN: Cs = 25;
/// The most notches one Scroll step turns.
const MOST_NOTCHES: u32 = 1000;

/// What the window received, from the record key's first press to its
/// second.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub events: Vec<Event>,
    /// How long it ran.
    pub secs: f64,
}

/// Wait for the record key, then gather what the window receives until it
/// is pressed again; `started` hears the first press.
pub fn record(
    next: &mut dyn FnMut() -> Result<Report, MacroError>,
    started: &dyn Fn(),
) -> Result<Recording, MacroError> {
    while next()? != Report::Started {}
    started();
    let mut events = Vec::new();
    loop {
        match next()? {
            Report::Heard(event) => events.push(event),
            Report::Stopped(secs) => return Ok(Recording { events, secs }),
            Report::Started => {}
        }
    }
}

impl Recording {
    /// Nothing pressed, turned or moved: no more than where the pointer was
    /// when it started.
    pub fn is_empty(&self) -> bool {
        self.events.iter().all(|e| e.at == 0.0 && matches!(e.heard, Heard::Motion { .. }))
    }

    /// The timeline that does it again, after a note of where it was made.
    pub fn rows(&self, from: &str) -> Vec<Row> {
        let note = format!("recorded in {from} ({:.1} s)", self.secs);
        let mut events = self.events.clone();
        events.sort_by(|a, b| a.at.total_cmp(&b.at));
        let end = cs(self.secs).max(events.last().map_or(0, |e| cs(e.at)));
        let mut steps = presses(&events, end);
        steps.extend(scrolls(&events));
        steps.extend(mouse(&events));
        // In time order; at the same moment, as made.
        steps.sort_by_key(|(t, _)| *t);
        let mut rows = vec![row("Note", note), row("Timeline", secs(end))];
        rows.extend(steps.into_iter().map(|(t, step)| row("At", format!("{} {step}", secs(t)))));
        rows
    }
}

/// Each key and button press, held as long as it was: one still down at
/// the end is let go of then.
fn presses(events: &[Event], end: Cs) -> Vec<(Cs, String)> {
    let mut down: HashMap<(bool, u16), Cs> = HashMap::new();
    let mut out = Vec::new();
    let mut write = |code: u16, from: Cs, to: Cs| {
        let held = (to - from).max(1);
        let kind = if held < TAP { "tap" } else { "hold" };
        out.push((from, format!("{kind} {} {}", keys::key_name(code), secs(held))));
    };
    for e in events {
        let (id, pressed) = match e.heard {
            Heard::Key { code, down } => ((false, code), down),
            Heard::Button { code, down } => ((true, code), down),
            _ => continue,
        };
        // A second press of what is down, and a release of what never was,
        // are passed over.
        if pressed {
            down.entry(id).or_insert(cs(e.at));
        } else if let Some(from) = down.remove(&id) {
            write(id.1, from, cs(e.at));
        }
    }
    let mut left: Vec<((bool, u16), Cs)> = down.into_iter().collect();
    left.sort_by_key(|(id, from)| (*from, *id));
    for ((_, code), from) in left {
        write(code, from, end);
    }
    out
}

/// Wheel turns one after another, the same way, as one scroll each.
fn scrolls(events: &[Event]) -> Vec<(Cs, String)> {
    let mut out = Vec::new();
    // The run so far: its axis, its start and last turn, and its total.
    let mut run: Option<(bool, Cs, Cs, i32)> = None;
    let mut flush = |run: Option<(bool, Cs, Cs, i32)>| {
        let Some((horizontal, from, _, value120)) = run else { return };
        let notches = (f64::from(value120) / 120.0).round() as i32;
        if notches == 0 {
            return;
        }
        let way = match (horizontal, notches > 0) {
            (false, true) => "down",
            (false, false) => "up",
            (true, true) => "right",
            (true, false) => "left",
        };
        let n = notches.unsigned_abs().min(MOST_NOTCHES);
        out.push((
            from,
            if n == 1 { format!("scroll {way}") } else { format!("scroll {way} {n}") },
        ));
    };
    for e in events {
        let Heard::Scroll { horizontal, value120 } = e.heard else { continue };
        let t = cs(e.at);
        run = match run {
            Some((h, from, last, sum))
                if h == horizontal && sum.signum() == value120.signum() && t - last <= SPIN =>
            {
                Some((h, from, t, sum + value120))
            }
            other => {
                flush(other);
                Some((horizontal, t, t, value120))
            }
        };
    }
    flush(run);
    out
}

/// The mouse: its turns if the window heard any, else the pointer's path.
fn mouse(events: &[Event]) -> Vec<(Cs, String)> {
    let turned = events.iter().any(|e| matches!(e.heard, Heard::Turn { .. }));
    if turned { turns(events) } else { paths(events) }
}

/// Each movement of the pointer on the window, set off from where it
/// rested the hundredth before it was first seen to move.
fn paths(events: &[Event]) -> Vec<(Cs, String)> {
    let samples: Vec<Point> = events
        .iter()
        .filter_map(|e| match e.heard {
            Heard::Motion { x, y } => Some(Point { t: cs(e.at), x, y }),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    let mut rested: Option<Point> = None;
    for stroke in strokes(&samples) {
        let mut points = Vec::with_capacity(stroke.len() + 1);
        if let Some(at) = rested {
            points.push(Point { t: (stroke[0].t - 1).max(at.t), ..at });
        }
        points.extend_from_slice(stroke);
        let kept = simplify(&points, STRAY);
        let start = kept[0].t;
        let list: Vec<String> = kept
            .iter()
            .map(|p| format!("{} {} {}", secs(p.t - start), pixel(p.x), pixel(p.y)))
            .collect();
        out.push((start, format!("path {}", list.join(", "))));
        rested = kept.last().copied();
    }
    out
}

/// Each run of raw movement, as how far the mouse had moved all told at
/// each hundredth, from where it set off.
fn turns(events: &[Event]) -> Vec<(Cs, String)> {
    // The movement in each hundredth, summed.
    let mut moved: Vec<Point> = Vec::new();
    for e in events {
        let Heard::Turn { dx, dy } = e.heard else { continue };
        let t = cs(e.at);
        match moved.last_mut() {
            Some(last) if last.t == t => (last.x, last.y) = (last.x + dx, last.y + dy),
            _ => moved.push(Point { t, x: dx, y: dy }),
        }
    }
    let mut out = Vec::new();
    for stroke in strokes(&moved) {
        let start = stroke[0].t - 1;
        let mut total = Point { t: start, x: 0.0, y: 0.0 };
        let mut points = vec![total];
        for p in stroke {
            total = Point { t: p.t, x: total.x + p.x, y: total.y + p.y };
            points.push(total);
        }
        let list: Vec<String> = simplify(&points, STRAY_TURN)[1..]
            .iter()
            .map(|p| format!("{} {} {}", secs(p.t - start), amount(p.x), amount(p.y)))
            .collect();
        out.push((start.max(0), format!("turn {}", list.join(", "))));
    }
    out
}

/// Samples cut at each rest: runs none further apart than REST.
fn strokes(samples: &[Point]) -> Vec<&[Point]> {
    let mut strokes = Vec::new();
    let mut start = 0;
    for i in 1..samples.len() {
        if samples[i].t - samples[i - 1].t > REST {
            strokes.push(&samples[start..i]);
            start = i;
        }
    }
    if start < samples.len() {
        strokes.push(&samples[start..]);
    }
    strokes
}

fn pixel(v: f64) -> i32 {
    v.round().max(0.0) as i32
}

/// A turn's amount to a hundredth of a unit: `12.5`, `-3`, never `-0`.
fn amount(v: f64) -> String {
    format!("{}", (v * 100.0).round() / 100.0 + 0.0)
}

fn cs(secs: f64) -> Cs {
    (secs * 100.0).round() as Cs
}

/// Hundredths as a step's seconds: `0.42`, `1`, `1.5`.
fn secs(cs: Cs) -> String {
    format!("{}", cs as f64 / 100.0)
}

fn row(kind: &str, value: String) -> Row {
    Row { kind: kind.to_owned(), value }
}

#[cfg(test)]
mod tests;
