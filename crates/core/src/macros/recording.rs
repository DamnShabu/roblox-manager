//! A recording: what a macro-ready window received between two presses of
//! the record key, made into the steps that do it again.
//!
//! An input with nothing else happening while it is down is one step: a
//! Key, a Hold, a Click at its point. Anything that overlaps something else
//! -- a key held while others are pressed, a button held while the mouse
//! moves -- is a Press and a Release where they happened. The pointer's path
//! is glides (`move to X Y SECONDS`) that keep the points it bends at, the
//! stops between movements waits. Every time is rounded to a hundredth of a
//! second, so the steps take as long as the recording did.

use std::collections::HashMap;

use super::MacroError;
use super::grammar::Row;
use super::keys::{self, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT};
use super::relay::event::{Event, Heard};
use super::relay::report::Report;

/// The key that starts and stops a recording, pressed in the window being
/// recorded: F8.
pub const RECORD_KEY: u16 = 66;

/// Hundredths of a second.
type Cs = i64;

/// A press shorter than this is a tap, whose length the player picks.
const TAP: Cs = 15;
/// The pointer moving no further than this while a press is down is a
/// hand's wobble, not a drag.
const WOBBLE: f64 = 2.0;
/// How far a glide may stray from the path it stands for, at any moment.
const STRAY: f64 = 2.0;
/// Pointer samples further apart than this are a stop between movements.
const REST: Cs = 5;
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

    /// The steps that do it again, after a note of where it was made.
    pub fn rows(&self, from: &str) -> Vec<Row> {
        let note = format!("recorded in {from} ({:.1} s)", self.secs);
        let mut events = self.events.clone();
        events.sort_by(|a, b| a.at.total_cmp(&b.at));
        let end = cs(self.secs).max(events.last().map_or(0, |e| cs(e.at)));
        let mut out = Writer { rows: vec![row("Note", note)], now: 0, at: None };
        let mut path = Vec::new();
        for item in timeline(&events, end) {
            match item {
                Item::Sample(point) => path.push(point),
                Item::Act(act) => {
                    out.glide(&path);
                    path.clear();
                    out.act(act);
                }
            }
        }
        out.glide(&path);
        out.wait_until(end);
        out.rows
    }
}

/// Where the pointer was seen, and when.
#[derive(Clone, Copy, Debug)]
struct Point {
    t: Cs,
    x: f64,
    y: f64,
}

#[derive(Debug)]
enum Act {
    /// A press with nothing else happening while it was down, and where the
    /// pointer was.
    Single {
        code: u16,
        from: Cs,
        to: Cs,
        at: Option<(f64, f64)>,
    },
    Press {
        code: u16,
        t: Cs,
    },
    Release {
        code: u16,
        t: Cs,
    },
    /// Wheel turns one after another, in 120ths of a notch.
    Scroll {
        horizontal: bool,
        value120: i32,
        from: Cs,
        to: Cs,
    },
}

#[derive(Debug)]
enum Item {
    Sample(Point),
    Act(Act),
}

/// Each press's partner: a press's release (None: held to the end), a
/// release's press.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Partner {
    None,
    Released(Option<usize>),
    Pressed,
}

/// The events as the samples and acts they make, in order.
fn timeline(events: &[Event], end: Cs) -> Vec<Item> {
    let partners = partners(events);
    let mut items = Vec::new();
    let mut pointer = None;
    let mut held_to_end = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        let t = cs(e.at);
        let mut next = i + 1;
        match (&e.heard, partners[i]) {
            (Heard::Motion { x, y }, _) => {
                pointer = Some((*x, *y));
                items.push(Item::Sample(Point { t, x: *x, y: *y }));
            }
            (Heard::Key { code, .. } | Heard::Button { code, .. }, Partner::Released(up)) => {
                if alone(events, i, up, pointer) {
                    let to = up.map_or(end, |u| cs(events[u].at));
                    items.push(Item::Act(Act::Single { code: *code, from: t, to, at: pointer }));
                    // The wobble while it was down, and its release, are part of it.
                    next = up.map_or(events.len(), |u| u + 1);
                } else {
                    items.push(Item::Act(Act::Press { code: *code, t }));
                    if up.is_none() {
                        held_to_end.push(*code);
                    }
                }
            }
            (Heard::Key { code, .. } | Heard::Button { code, .. }, Partner::Pressed) => {
                items.push(Item::Act(Act::Release { code: *code, t }));
            }
            (Heard::Scroll { horizontal, value120 }, _) => {
                let (mut sum, mut last) = (*value120, i);
                while let Some(Heard::Scroll { horizontal: h, value120: v }) =
                    events.get(last + 1).map(|e| &e.heard)
                {
                    let close = cs(events[last + 1].at) - cs(events[last].at) <= SPIN;
                    if *h != *horizontal || v.signum() != value120.signum() || !close {
                        break;
                    }
                    sum += v;
                    last += 1;
                }
                let to = cs(events[last].at);
                items.push(Item::Act(Act::Scroll {
                    horizontal: *horizontal,
                    value120: sum,
                    from: t,
                    to,
                }));
                next = last + 1;
            }
            // A release of nothing pressed, a press of what was down already.
            _ => {}
        }
        i = next;
    }
    items
        .extend(held_to_end.into_iter().rev().map(|code| Item::Act(Act::Release { code, t: end })));
    items
}

/// Pair each press with its release; a second press of what is down, and a
/// release of what never was, have none.
fn partners(events: &[Event]) -> Vec<Partner> {
    let mut partners = vec![Partner::None; events.len()];
    let mut down: HashMap<(bool, u16), usize> = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        let (id, pressed) = match e.heard {
            Heard::Key { code, down } => ((false, code), down),
            Heard::Button { code, down } => ((true, code), down),
            _ => continue,
        };
        if pressed {
            if let std::collections::hash_map::Entry::Vacant(slot) = down.entry(id) {
                slot.insert(i);
                partners[i] = Partner::Released(None);
            }
        } else if let Some(press) = down.remove(&id) {
            partners[press] = Partner::Released(Some(i));
            partners[i] = Partner::Pressed;
        }
    }
    partners
}

/// Whether nothing else happens while the press at `down` is held: at most
/// the pointer wobbling where it was.
fn alone(events: &[Event], down: usize, up: Option<usize>, at: Option<(f64, f64)>) -> bool {
    events[down + 1..up.unwrap_or(events.len())].iter().all(|e| match (&e.heard, at) {
        (Heard::Motion { x, y }, Some((ax, ay))) => (x - ax).hypot(y - ay) <= WOBBLE,
        _ => false,
    })
}

/// The rows so far, the time they have taken, and where they leave the
/// pointer.
struct Writer {
    rows: Vec<Row>,
    now: Cs,
    at: Option<(f64, f64)>,
}

impl Writer {
    fn wait_until(&mut self, t: Cs) {
        if t > self.now {
            self.rows.push(row("Wait", secs(t - self.now)));
            self.now = t;
        }
    }

    fn move_to(&mut self, (x, y): (f64, f64), over: Cs) {
        let (x, y) = (pixel(x), pixel(y));
        let value =
            if over > 0 { format!("to {x} {y} {}", secs(over)) } else { format!("to {x} {y}") };
        self.rows.push(row("Move", value));
    }

    /// The pointer's path between two acts: a wait for each stop, and glides
    /// along each movement, set off from where it rested the tick before it
    /// was first seen to move.
    fn glide(&mut self, path: &[Point]) {
        for stroke in strokes(path) {
            let mut points = Vec::with_capacity(stroke.len() + 1);
            if let Some((x, y)) = self.at {
                points.push(Point { t: (stroke[0].t - 1).max(self.now), x, y });
            }
            points.extend_from_slice(stroke);
            let kept = simplify(&points);
            let (first, last) = (kept[0], kept[kept.len() - 1]);
            self.wait_until(first.t);
            if self.at.is_none_or(|at| moved(at, (first.x, first.y))) {
                self.move_to((first.x, first.y), 0);
            }
            for pair in kept.windows(2) {
                self.move_to((pair[1].x, pair[1].y), pair[1].t - pair[0].t);
            }
            self.now = self.now.max(last.t);
            self.at = Some((last.x, last.y));
        }
    }

    fn act(&mut self, act: Act) {
        match act {
            Act::Single { code, from, to, at } => {
                self.wait_until(from);
                let held = to - from;
                match (click_word(code), held < TAP) {
                    (Some(word), true) => {
                        let point = at.map(|(x, y)| format!("{} {}", pixel(x), pixel(y)));
                        let words: Vec<String> = [Some(word.to_owned()), point]
                            .into_iter()
                            .flatten()
                            .filter(|w| !w.is_empty())
                            .collect();
                        self.rows.push(row("Click", words.join(" ")));
                    }
                    _ => {
                        if let Some(p) = at.filter(|_| keys::is_button(code)) {
                            if self.at.is_none_or(|a| moved(a, p)) {
                                self.move_to(p, 0);
                            }
                        }
                        let name = keys::key_name(code);
                        self.rows.push(if held < TAP {
                            row("Key", name)
                        } else {
                            row("Hold", format!("{name} {}", secs(held)))
                        });
                    }
                }
                if keys::is_button(code) {
                    self.at = at.or(self.at);
                }
                self.now = self.now.max(to);
            }
            Act::Press { code, t } => {
                self.wait_until(t);
                self.rows.push(row("Press", keys::key_name(code)));
            }
            Act::Release { code, t } => {
                self.wait_until(t);
                self.rows.push(row("Release", keys::key_name(code)));
            }
            Act::Scroll { horizontal, value120, from, to } => {
                self.wait_until(from);
                let notches = (f64::from(value120) / 120.0).round() as i32;
                if notches != 0 {
                    let way = match (horizontal, notches > 0) {
                        (false, true) => "down",
                        (false, false) => "up",
                        (true, true) => "right",
                        (true, false) => "left",
                    };
                    let n = notches.unsigned_abs().min(MOST_NOTCHES);
                    self.rows.push(row(
                        "Scroll",
                        if n == 1 { way.into() } else { format!("{way} {n}") },
                    ));
                }
                self.now = self.now.max(to);
            }
        }
    }
}

/// A path cut at each stop: runs of samples none further apart than REST.
fn strokes(path: &[Point]) -> Vec<&[Point]> {
    let mut strokes = Vec::new();
    let mut start = 0;
    for i in 1..path.len() {
        if path[i].t - path[i - 1].t > REST {
            strokes.push(&path[start..i]);
            start = i;
        }
    }
    if start < path.len() {
        strokes.push(&path[start..]);
    }
    strokes
}

/// The fewest points that glide within STRAY of every sample, at the time
/// each was seen (Douglas-Peucker on where a glide puts the pointer then).
/// Of points seen in the same hundredth, the last stands.
fn simplify(points: &[Point]) -> Vec<Point> {
    let last = points.len() - 1;
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[last] = true;
    let mut spans = vec![(0, last)];
    while let Some((a, b)) = spans.pop() {
        let furthest = (a + 1..b)
            .map(|i| (i, strays(points[a], points[b], points[i])))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, _)) = furthest.filter(|(_, by)| *by > STRAY) {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    let kept: Vec<Point> = points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect();
    kept.iter()
        .enumerate()
        .filter(|(i, p)| kept.get(i + 1).is_none_or(|next| next.t != p.t))
        .map(|(_, p)| *p)
        .collect()
}

/// How far `p` is from where a glide from `a` to `b` has the pointer at
/// `p`'s time.
fn strays(a: Point, b: Point, p: Point) -> f64 {
    let span = (b.t - a.t) as f64;
    let f = if span > 0.0 { (p.t - a.t) as f64 / span } else { 0.0 };
    let (x, y) = (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
    (p.x - x).hypot(p.y - y)
}

/// Whether two points are different pixels.
fn moved(a: (f64, f64), b: (f64, f64)) -> bool {
    (pixel(a.0), pixel(a.1)) != (pixel(b.0), pixel(b.1))
}

/// The word a Click step writes for a button, if it is one a click names.
fn click_word(code: u16) -> Option<&'static str> {
    match code {
        BUTTON_LEFT => Some(""),
        BUTTON_RIGHT => Some("right"),
        BUTTON_MIDDLE => Some("middle"),
        _ => None,
    }
}

fn pixel(v: f64) -> i32 {
    v.round().max(0.0) as i32
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
