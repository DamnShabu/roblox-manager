//! Playing a timeline: its steps made into the presses, releases and
//! pointer movements they are, each due at its own moment from the
//! timeline's start, and sent then -- all against one clock, so a step
//! never waits on the one before it and nothing drifts.

use std::collections::BTreeMap;
use std::io;
use std::time::Duration;

use super::{GLIDE_TICK, Held, Input, StopFlag};
use crate::macros::grammar::{Step, TAP_PRESS, Timed, timeline::length};

/// One thing sent, and when: seconds from the timeline's start.
#[derive(Clone, Debug, PartialEq)]
struct Due {
    t: f64,
    what: Send,
}

#[derive(Clone, Debug, PartialEq)]
enum Send {
    /// A hold's or click's end. Let go first when due at the same moment
    /// as a press: a key held again straight after is let go of and
    /// pressed, not the other way.
    Up(u16),
    /// A hold or click starting.
    Down(u16),
    /// A `release` step: lets go of the key, whoever pressed it.
    Release(u16),
    /// A `press` step: the key stays down after the timeline.
    Press(u16),
    MoveTo(i32, i32),
    Motion(f64, f64),
    Scroll(bool, i32),
}

impl Send {
    fn order(&self) -> u8 {
        u8::from(!matches!(self, Send::Up(_) | Send::Release(_)))
    }
}

/// Play `items` from now on the macro's clock, lasting at least `secs`, or
/// until stopped. Whatever it presses is in `held`, for the round's end to
/// let go of if stop cuts it short.
pub(super) fn play(
    input: &mut dyn Input,
    items: &[Timed],
    secs: f64,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
    held: &mut Held,
) -> io::Result<()> {
    let start = held.on_time();
    let (dues, left_at) = schedule(items, held.at, pick);
    let end = dues.last().map_or(0.0, |d| d.t).max(secs);
    // How many of this timeline's holds have each key down. A hold's end
    // lets go only of what a hold here pressed, and only once the last
    // one holding it ends: a key a `press` before the timeline holds stays
    // down, as the same hold outside a timeline leaves it.
    let mut holding: BTreeMap<u16, u32> = BTreeMap::new();
    for due in dues {
        if held.idle_until(input, start + after(due.t), stop, pick)? {
            return Ok(());
        }
        match due.what {
            Send::Down(code) => match holding.get_mut(&code) {
                Some(n) => *n += 1,
                None if held.holds(code) => {}
                None => {
                    held.press(input, &[code])?;
                    holding.insert(code, 1);
                }
            },
            Send::Up(code) => {
                if let Some(n) = holding.get_mut(&code) {
                    *n -= 1;
                    if *n == 0 {
                        holding.remove(&code);
                        held.release(input, &[code])?;
                    }
                }
            }
            Send::Press(code) => {
                holding.remove(&code);
                held.press(input, &[code])?;
            }
            Send::Release(code) => {
                holding.remove(&code);
                held.release(input, &[code])?;
            }
            Send::MoveTo(x, y) => {
                input.move_to(x, y)?;
                held.at = Some((x, y));
            }
            Send::Motion(dx, dy) => input.motion(dx, dy)?,
            Send::Scroll(horizontal, notches) => input.scroll(horizontal, notches)?,
        }
    }
    held.at = left_at;
    held.idle_until(input, start + after(end), stop, pick).map(|_| ())
}

fn after(secs: f64) -> Duration {
    Duration::try_from_secs_f64(secs).unwrap_or_default()
}

/// Every send the steps make, ranges picked, in the order they are due --
/// and where they leave the pointer. Its moves are worked out in the order
/// they start, each from where the one before left it (`at`: where the
/// macro last put it, if anywhere).
fn schedule(
    items: &[Timed],
    mut at: Option<(i32, i32)>,
    pick: &dyn Fn(f64, f64) -> f64,
) -> (Vec<Due>, Option<(i32, i32)>) {
    let mut timed: Vec<(f64, &Step)> =
        items.iter().map(|i| (pick(i.at.0, i.at.1), &i.step)).collect();
    timed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut dues = Vec::new();
    let mut put = |t: f64, what: Send| dues.push(Due { t, what });
    for (t, step) in timed {
        match step {
            Step::Hold { keys, lo, hi } => {
                let end = t + pick(*lo, *hi);
                keys.iter().for_each(|&k| put(t, Send::Down(k)));
                keys.iter().rev().for_each(|&k| put(end, Send::Up(k)));
            }
            Step::Press(codes) => codes.iter().for_each(|&k| put(t, Send::Press(k))),
            Step::Release(codes) => codes.iter().for_each(|&k| put(t, Send::Release(k))),
            Step::Click { button, at: point } => {
                if let Some((x, y)) = *point {
                    put(t, Send::MoveTo(x, y));
                    at = Some((x, y));
                }
                put(t, Send::Down(*button));
                put(t + pick(TAP_PRESS.0, TAP_PRESS.1), Send::Up(*button));
            }
            Step::Move(dx, dy) => {
                put(t, Send::Motion(f64::from(*dx), f64::from(*dy)));
                at = at.map(|(x, y)| (x.saturating_add(*dx), y.saturating_add(*dy)));
            }
            Step::MoveTo { x, y, lo, hi } => {
                let secs = pick(*lo, *hi);
                let points = match at {
                    Some((fx, fy)) if secs > 0.0 => vec![(0.0, fx, fy), (secs, *x, *y)],
                    _ => vec![(0.0, *x, *y)],
                };
                glide(&points, t, &mut put);
                at = Some((*x, *y));
            }
            Step::Path(points) => {
                glide(points, t, &mut put);
                at = points.last().map(|p| (p.1, p.2)).or(at);
            }
            Step::Turn(points) => {
                let (dx, dy) = turn(points, t, &mut put);
                at = at.map(|(x, y)| (x.saturating_add(dx), y.saturating_add(dy)));
            }
            Step::Scroll { horizontal, notches } => put(t, Send::Scroll(*horizontal, *notches)),
            // Not timed: the grammar keeps them out of a timeline.
            Step::Type(_)
            | Step::Wait(..)
            | Step::Start(..)
            | Step::Repeat { .. }
            | Step::Timeline { .. }
            | Step::Exit => {}
        }
    }
    dues.sort_by(|a, b| a.t.total_cmp(&b.t).then(a.what.order().cmp(&b.what.order())));
    (dues, at)
}

/// The pointer along `points` from `t`: at the first at once, then a
/// small even step every tick towards each next, never the same pixel
/// twice running.
fn glide(points: &[(f64, i32, i32)], t: f64, put: &mut impl FnMut(f64, Send)) {
    let Some(&(t0, x0, y0)) = points.first() else { return };
    put(t + t0, Send::MoveTo(x0, y0));
    let mut last = (x0, y0);
    for pair in points.windows(2) {
        let ((ta, xa, ya), (tb, xb, yb)) = (pair[0], pair[1]);
        let ticks = ((tb - ta) / GLIDE_TICK).round().max(1.0) as u32;
        for tick in 1..=ticks {
            let f = f64::from(tick) / f64::from(ticks);
            let along = |a: i32, b: i32| (f64::from(a) + f64::from(b - a) * f).round() as i32;
            let p = (along(xa, xb), along(ya, yb));
            if p != last {
                put(t + ta + (tb - ta) * f, Send::MoveTo(p.0, p.1));
                last = p;
            }
        }
    }
}

/// Raw movement along `points` from `t`, a tick's worth at a time, in the
/// display's own 256ths of a unit: the amounts sent add up to exactly the
/// distance written, however it is cut. How far it went, in whole units.
fn turn(points: &[(f64, f64, f64)], t: f64, put: &mut impl FnMut(f64, Send)) -> (i32, i32) {
    let fixed = |v: f64| (v * 256.0).round();
    let mut sent = (0.0, 0.0);
    let mut prev = (0.0, 0.0, 0.0);
    for &(tb, xb, yb) in points {
        let (ta, xa, ya) = prev;
        let ticks = ((tb - ta) / GLIDE_TICK).round().max(1.0) as u32;
        for tick in 1..=ticks {
            let f = f64::from(tick) / f64::from(ticks);
            let to = (fixed(xa + (xb - xa) * f), fixed(ya + (yb - ya) * f));
            let by = (to.0 - sent.0, to.1 - sent.1);
            if by != (0.0, 0.0) {
                put(t + ta + (tb - ta) * f, Send::Motion(by.0 / 256.0, by.1 / 256.0));
                sent = to;
            }
        }
        prev = (tb, xb, yb);
    }
    ((sent.0 / 256.0).round() as i32, (sent.1 / 256.0).round() as i32)
}

/// A path or turn played on its own, in turn with the steps around it: a
/// timeline of just it.
pub(super) fn alone(
    input: &mut dyn Input,
    step: &Step,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
    held: &mut Held,
) -> io::Result<()> {
    let item = Timed { at: (0.0, 0.0), step: step.clone() };
    play(input, std::slice::from_ref(&item), length(step), stop, pick, held)
}

#[cfg(test)]
mod tests;
