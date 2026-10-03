//! Repeats: keys tapped over and over for a while, behind the steps that
//! follow -- a Repeat starts its taps and the macro moves straight on. The
//! taps happen while the player waits (a Wait, a hold, a glide), and a
//! round ends once its repeats have.

use std::io;
use std::time::{Duration, Instant};

use super::{on_clock, send};
use crate::macros::grammar::{SHORTEST_EVERY, TAP_PRESS};

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
    /// Tap `keys` every `every` seconds for `secs`, the first tap at `at`
    /// -- where the macro is on its clock. A repeat of the same keys still
    /// going is let go of and replaced.
    pub(super) fn start(
        &mut self,
        input: &mut dyn super::Input,
        keys: &[u16],
        secs: f64,
        every: (f64, f64),
        at: Instant,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<()> {
        if let Some(i) = self.0.iter().position(|r| r.keys == keys) {
            let old = self.0.remove(i);
            if old.down {
                let_go(input, &old.keys)?;
            }
        }
        self.0.push(Repeat {
            keys: keys.to_vec(),
            ends: at + Duration::try_from_secs_f64(secs).unwrap_or_default(),
            every,
            down: false,
            next: at,
            again: at,
        });
        self.fire(input, at, pick)
    }

    /// Press or let go of whatever is due by `by`, the earliest first: the
    /// taps pick their lengths in the order they are due, however late the
    /// player woke, so clients picking from one seed pick alike. A repeat
    /// with no tap left to start is done with.
    pub(super) fn fire(
        &mut self,
        input: &mut dyn super::Input,
        by: Instant,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<()> {
        while let Some(i) = self.earliest().filter(|&i| self.0[i].next <= by) {
            let r = &mut self.0[i];
            let sent = if r.down { r.let_go(input) } else { r.tap(input, pick) };
            if !matches!(sent, Ok(true)) {
                self.0.remove(i);
            }
            sent?;
        }
        Ok(())
    }

    /// The repeat due soonest; the first started of those due together.
    fn earliest(&self) -> Option<usize> {
        self.0.iter().enumerate().min_by_key(|(_, r)| r.next).map(|(i, _)| i)
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

impl Repeat {
    /// Let go of its keys; whether it has a tap left to start.
    fn let_go(&mut self, input: &mut dyn super::Input) -> io::Result<bool> {
        self.down = false;
        self.next = self.again;
        let_go(input, &self.keys)?;
        Ok(self.again < self.ends)
    }

    /// Start the tap due now, timed from when it was due -- not from when
    /// the player got to it -- so the taps keep to the macro's clock and
    /// as many fit in. Whether it went down: false when its time is up.
    fn tap(
        &mut self,
        input: &mut dyn super::Input,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        if self.next >= self.ends {
            return Ok(false);
        }
        // Never none: every tap moves the repeat on along the clock.
        let every = pick(self.every.0, self.every.1).max(SHORTEST_EVERY);
        // A tap is let go of well before the next one presses.
        let tap = pick(TAP_PRESS.0, TAP_PRESS.1).min(every / 2.0);
        press(input, &self.keys)?;
        let from = on_clock(self.next);
        self.down = true;
        self.next = from + Duration::from_secs_f64(tap);
        self.again = from + Duration::from_secs_f64(every);
        Ok(true)
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
