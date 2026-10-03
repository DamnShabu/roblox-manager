//! A timeline: steps that each start at a time of their own, from the
//! timeline's start, and play over one another -- `timeline 3.2`, then a
//! line `at 0.95 tap space 0.08` for each. And the two steps a recording
//! moves the mouse with: a `path` it follows on the window, and a `turn`,
//! raw mouse movement, which is what a game turns its camera by.

use super::{FURTHEST, LONGEST_SECS, Step, seconds};

/// One step of a timeline, and when it starts: seconds from the timeline's
/// start, a range picked afresh every round.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub at: (f64, f64),
    pub step: Step,
}

/// The steps that may start at a time of their own: those that take a set
/// time, or none. Typing, waiting, repeating and timelines play in turn.
pub(super) const TIMED: [&str; 9] =
    ["tap", "hold", "press", "release", "click", "move", "scroll", "path", "turn"];

/// The furthest a turn may reach, in the mouse's own units.
const FURTHEST_TURN: f64 = 1.0e7;

/// `T X Y, T X Y, ...`: the points a path passes, each at its seconds from
/// the step's start; the first is where it starts from.
pub(super) fn path(rest: &str) -> Result<Step, String> {
    let points = points(rest, "path")?;
    points
        .into_iter()
        .map(|(t, x, y)| Ok((t, pixel(x)?, pixel(y)?)))
        .collect::<Result<_, String>>()
        .map(Step::Path)
}

/// `T DX DY, T DX DY, ...`: how far the mouse has moved, all told, at each
/// time from the step's start -- from nothing at its start.
pub(super) fn turn(rest: &str) -> Result<Step, String> {
    let points = points(rest, "turn")?;
    if points.iter().any(|(_, dx, dy)| dx.abs().max(dy.abs()) > FURTHEST_TURN) {
        return Err(format!("a turn reaches at most {FURTHEST_TURN} either way"));
    }
    Ok(Step::Turn(points))
}

fn points(rest: &str, what: &str) -> Result<Vec<(f64, f64, f64)>, String> {
    let usage = || format!("expected {what} SECONDS X Y, SECONDS X Y, ...");
    let mut last = 0.0;
    let mut out = Vec::new();
    for point in rest.split(',') {
        let words: Vec<&str> = point.split_whitespace().collect();
        let [t, x, y] = words.as_slice() else { return Err(usage()) };
        let (t, x, y) = (number(t)?, number(x)?, number(y)?);
        if t < last || t > LONGEST_SECS {
            return Err(format!("a {what}'s times go forward, from 0, within a day"));
        }
        last = t;
        out.push((t, x, y));
    }
    Ok(out)
}

fn number(word: &str) -> Result<f64, String> {
    word.parse()
        .ok()
        .filter(|n: &f64| n.is_finite())
        .ok_or_else(|| format!("not a number: '{word}'"))
}

fn pixel(v: f64) -> Result<i32, String> {
    let n = v.round();
    if n.abs() > f64::from(FURTHEST) {
        return Err(format!("'{v}' is too far -- at most {FURTHEST} pixels"));
    }
    Ok(n as i32)
}

/// `at SECONDS COMMAND ...`: when, and the rest of the line.
pub(super) fn at(rest: &str) -> Result<((f64, f64), &str), String> {
    let (when, step) = rest.split_once(' ').ok_or("expected at SECONDS STEP")?;
    Ok((seconds(when)?, step.trim()))
}

/// How long a step of a timeline lasts once started, at the top of its
/// ranges: what the timeline is at least as long as.
pub fn length(step: &Step) -> f64 {
    match step {
        Step::Hold { hi, .. } | Step::MoveTo { hi, .. } => *hi,
        Step::Click { .. } => super::TAP_PRESS.1,
        Step::Path(points) => points.last().map_or(0.0, |p| p.0),
        Step::Turn(points) => points.last().map_or(0.0, |p| p.0),
        Step::Timeline { secs, items } => {
            items.iter().map(|i| i.at.1 + length(&i.step)).fold(secs.1, f64::max)
        }
        _ => 0.0,
    }
}
