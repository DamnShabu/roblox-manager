//! Repeats: keys tapped over and over for a while, behind the steps that
//! follow -- a Repeat starts its taps and the macro moves straight on. The
//! taps happen while the player waits (a Wait, a hold, a glide), and a
//! round ends once its repeats have.

use std::io;
use std::time::{Duration, Instant};

use super::send;
use crate::macros::grammar::TAP_PRESS;

#[derive(Debug, Default)]
pub(super) struct Repeats(Vec<Repeat>);

#[derive(Debug)]
struct Repeat {
    keys: Vec<u16>,
    /// No tap starts at or after this.
    ends: Instant,
    /// Seconds from one tap's press to the next's, a range.
    every: (f64, f64),
    /// Whether its keys are down now.
    down: bool,
    /// When it next lets go (down) or presses (up).
    next: Instant,
    /// When the tap after this one presses.
    again: Instant,
}

impl Repeats {
    /// Tap `keys` every `every` seconds for `secs`, the first tap now. A
    /// repeat of the same keys still going is let go of and replaced.
    pub(super) fn start(
        &mut self,
        input: &mut dyn super::Input,
        keys: &[u16],
        secs: f64,
        every: (f64, f64),
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<()> {
        if let Some(i) = self.0.iter().position(|r| r.keys == keys) {
            let old = self.0.remove(i);
            if old.down {
                let_go(input, &old.keys)?;
            }
        }
        let now = Instant::now();
        self.0.push(Repeat {
            keys: keys.to_vec(),
            ends: now + Duration::try_from_secs_f64(secs).unwrap_or_default(),
            every,
            down: false,
            next: now,
            again: now,
        });
        self.fire(input, pick)
    }

    /// Press or let go of whatever is due by now; a repeat with no tap left
    /// to start is done with.
    pub(super) fn fire(
        &mut self,
        input: &mut dyn super::Input,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<()> {
        let now = Instant::now();
        let mut result = Ok(());
        self.0.retain_mut(|r| {
            if result.is_err() || now < r.next {
                return true;
            }
            if r.down {
                r.down = false;
                r.next = r.again;
                result = let_go(input, &r.keys);
                return result.is_ok() && r.again < r.ends;
            }
            if now >= r.ends {
                return false;
            }
            let every = pick(r.every.0, r.every.1);
            // A tap is let go of well before the next one presses.
            let tap = pick(TAP_PRESS.0, TAP_PRESS.1).min(every / 2.0);
            result = press(input, &r.keys);
            r.down = result.is_ok();
            r.next = now + Duration::from_secs_f64(tap);
            r.again = now + Duration::from_secs_f64(every);
            r.down
        });
        result
    }

    /// When the next press or let-go is due; None with nothing repeating.
    pub(super) fn next_due(&self) -> Option<Instant> {
        self.0.iter().map(|r| r.next).min()
    }

    /// Stop every repeat, letting go of the keys of any mid-tap. Every one
    /// is let go of, whatever fails; the first failure is the one returned.
    pub(super) fn release_all(&mut self, input: &mut dyn super::Input) -> io::Result<()> {
        let mut result = Ok(());
        for r in self.0.drain(..).filter(|r| r.down) {
            let released = let_go(input, &r.keys);
            if result.is_ok() {
                result = released;
            }
        }
        result
    }
}

/// `keys` down in order; if one fails, those already down are let go of and
/// the press's failure is the one returned.
fn press(input: &mut dyn super::Input, keys: &[u16]) -> io::Result<()> {
    for (i, &code) in keys.iter().enumerate() {
        if let Err(e) = send(input, code, true) {
            return Err::<(), _>(e).and(let_go(input, &keys[..i]));
        }
    }
    Ok(())
}

/// `keys` up in reverse -- every one, whatever fails; the first failure is
/// the one returned.
fn let_go(input: &mut dyn super::Input, keys: &[u16]) -> io::Result<()> {
    let mut result = Ok(());
    for &code in keys.iter().rev() {
        let released = send(input, code, false);
        if result.is_ok() {
            result = released;
        }
    }
    result
}
