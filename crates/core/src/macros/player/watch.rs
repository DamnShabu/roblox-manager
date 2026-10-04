//! A macro's `when`s: a thread of their own that looks at the client's
//! frame many times a second, and the moment a `when` sees what it waits
//! for, rings the player -- which plays its steps straight away, wherever
//! it is in its own. The macro's own steps hold still while they play, and
//! go on after, keys they hold still down; a `when` never waits its turn.
//!
//! A `when` plays once each time what it waits for appears -- not over and
//! over while it stays -- and again once it has gone and comes back. One
//! that sees it while another plays is next.

use std::collections::VecDeque;
use std::io;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use super::{Held, Input, play_step};
use crate::macros::grammar::{Handler, Sight, Step, describe, when};
use crate::macros::sight::{Area, Eyes, Image, SLACK, color_matches, image_score};
use crate::stop::StopFlag;

/// How often the frame is looked at: twenty times a second. The copy waits
/// on cage's next frame, so it is never more often than the client draws.
const LOOK_EVERY: f64 = 0.05;
/// Seconds without a frame to look at before it is said: a client loading
/// draws nothing for a while, and a when is no less ready for it.
const BLIND_FOR: f64 = 3.0;

/// What each `when` looks for, and the one area of the display that holds
/// them all, copied once a look.
pub struct Looks {
    looks: Vec<Look>,
    area: Area,
}

struct Look {
    sight: Seeing,
    not: bool,
}

enum Seeing {
    Image { template: Image, at: (i32, i32), least: f64 },
    Color { at: (i32, i32), rgb: [u8; 3], within: u8 },
}

impl Looks {
    /// The looks `handlers` make, with each image they name from `image`.
    pub fn new(
        handlers: &[Handler],
        image: &dyn Fn(&str) -> Result<Image, String>,
    ) -> Result<Self, String> {
        let mut looks = Vec::new();
        let mut area: Option<Area> = None;
        for h in handlers {
            let (sight, covers) = match &h.when.sight {
                Sight::Image { name, x, y, least } => {
                    let template = image(name)?;
                    let covers = Area {
                        x: x - SLACK,
                        y: y - SLACK,
                        w: template.width + 2 * SLACK as u32,
                        h: template.height + 2 * SLACK as u32,
                    };
                    (Seeing::Image { template, at: (*x, *y), least: *least }, covers)
                }
                Sight::Color { x, y, rgb, within } => {
                    let covers = Area { x: *x, y: *y, w: 1, h: 1 };
                    (Seeing::Color { at: (*x, *y), rgb: *rgb, within: *within }, covers)
                }
            };
            area = Some(area.map_or(covers, |a| a.union(covers)));
            looks.push(Look { sight, not: h.when.not });
        }
        let area = area.ok_or("the macro has no 'when' to look for")?.on_display();
        Ok(Looks { looks, area })
    }

    /// Whether each `when` sees what it waits for, in a copy of the area.
    fn seen(&self, frame: &Image) -> Vec<bool> {
        self.looks
            .iter()
            .map(|look| {
                let shows = match &look.sight {
                    Seeing::Image { template, at, least } => {
                        image_score(frame, self.area, template, *at, *least).is_some()
                    }
                    Seeing::Color { at, rgb, within } => {
                        color_matches(frame, self.area, *at, *rgb, *within)
                    }
                };
                shows != look.not
            })
            .collect()
    }
}

/// What the looking has found, for the player: the `when`s due to play, in
/// the order they saw, and why the looking stopped, if it has.
#[derive(Default)]
pub struct Seen(Mutex<Found>);

#[derive(Default)]
struct Found {
    due: VecDeque<usize>,
    failed: Option<String>,
    said: Option<String>,
}

impl Seen {
    fn found(&self) -> MutexGuard<'_, Found> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fire(&self, i: usize) {
        let mut f = self.found();
        if !f.due.contains(&i) {
            f.due.push_back(i);
        }
    }

    /// The next `when` due to play; the error that stopped the looking.
    fn next(&self) -> io::Result<Option<usize>> {
        let mut f = self.found();
        match &f.failed {
            Some(why) => Err(io::Error::other(why.clone())),
            None => Ok(f.due.pop_front()),
        }
    }

    /// Something to tell the player's listener, once.
    fn said(&self) -> Option<String> {
        self.found().said.take()
    }
}

/// Look until `done` or `stop` is set, ringing `stop` for the player each
/// time a `when` sees what it waits for. A display that fails ends the
/// looking, and the player with it; a frame slow to come only delays it.
pub fn watch(eyes: &mut dyn Eyes, looks: &Looks, seen: &Seen, stop: &StopFlag, done: &StopFlag) {
    // Whether each saw what it waits for last look. A `when not` starts out
    // seeing it: it waits for what it waits on to go, so has to see it first.
    let mut was: Vec<bool> = looks.looks.iter().map(|l| l.not).collect();
    let mut seen_last = Instant::now();
    let mut blind = false;
    while !done.is_set() && !stop.is_set() {
        let started = Instant::now();
        match eyes.look(looks.area) {
            Ok(frame) => {
                seen_last = Instant::now();
                if blind {
                    blind = false;
                    seen.found().said = Some("can see its window again".into());
                    stop.ring();
                }
                let now = looks.seen(&frame);
                let mut rang = false;
                for (i, (&is, was)) in now.iter().zip(was.iter_mut()).enumerate() {
                    if is && !*was {
                        seen.fire(i);
                        rang = true;
                    }
                    *was = is;
                }
                if rang {
                    stop.ring();
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                if !blind && seen_last.elapsed().as_secs_f64() >= BLIND_FOR {
                    blind = true;
                    seen.found().said = Some(format!("cannot see its window: {e}"));
                    stop.ring();
                }
            }
            Err(e) => {
                seen.found().failed = Some(format!("could not look at its window: {e}"));
                stop.ring();
                return;
            }
        }
        let left = LOOK_EVERY - started.elapsed().as_secs_f64();
        if left > 0.0 && done.wait(left) {
            return;
        }
    }
}

/// The `when`s of a playing macro, as the player answers them.
#[derive(Clone, Copy)]
pub struct When<'a> {
    pub handlers: &'a [Handler],
    pub seen: &'a Seen,
    pub report: &'a dyn Fn(String),
}

impl When<'_> {
    /// Play every `when` that is due, each to its end or until stopped --
    /// any it presses let go of after, whatever fails. True when one played
    /// an Exit: the round it broke into is to end, and any `when` still due
    /// plays at the next wait, in the round after.
    pub fn answer(
        &self,
        input: &mut dyn Input,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        if let Some(said) = self.seen.said() {
            (self.report)(said);
        }
        while let Some(i) = self.seen.next()? {
            let Some(h) = self.handlers.get(i) else { continue };
            if stop.is_set() {
                return Ok(false);
            }
            let when = when::describe(&h.when);
            let mut held = Held::default();
            let mut result = Ok(());
            for step in &h.steps {
                if stop.is_set() || result.is_err() || held.exiting {
                    break;
                }
                (self.report)(format!("when {when}: {}", describe(step)));
                result = match step {
                    Step::Wait(lo, hi) => held.idle(input, pick(*lo, *hi), stop, pick).map(drop),
                    _ => play_step(input, step, stop, pick, &mut held),
                };
            }
            let let_go = held.release_all(input);
            result.and(let_go)?;
            if held.exiting {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
