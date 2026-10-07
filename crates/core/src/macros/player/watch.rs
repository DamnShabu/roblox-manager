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
use crate::macros::grammar::when::Place;
use crate::macros::grammar::{Handler, Sight, Step, describe, when};
use crate::macros::sight::{Area, Eyes, Image, SLACK, color_matches, find, near};
use crate::stop::StopFlag;

/// How often the frame is looked at: twenty times a second. The copy waits
/// on cage's next frame, so it is never more often than the client draws.
const LOOK_EVERY: f64 = 0.05;
/// The whole window, as an area to copy: the display keeps a copy to the
/// part on it, so this is all of it at any size.
const WHOLE: Area = Area { x: 0, y: 0, w: 1 << 16, h: 1 << 16 };
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
    /// An image: near a place, or in an area -- and where it was last
    /// found there, tried first.
    Image {
        template: Image,
        place: Where,
        least: f64,
        last: Option<(i32, i32)>,
    },
    Color {
        at: (i32, i32),
        rgb: [u8; 3],
        within: u8,
    },
}

#[derive(Clone, Copy)]
enum Where {
    Near(i32, i32),
    Within(Area),
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
                Sight::Image { name, place, least } => {
                    let template = image(name)?;
                    let (place, covers) = match *place {
                        Place::Anywhere => (Where::Within(WHOLE), WHOLE),
                        Place::Near(x, y) => {
                            let covers = Area {
                                x: x - SLACK,
                                y: y - SLACK,
                                w: template.width + 2 * SLACK as u32,
                                h: template.height + 2 * SLACK as u32,
                            };
                            (Where::Near(x, y), covers)
                        }
                        Place::Within { x, y, w, h } => {
                            let area = Area { x, y, w, h };
                            (Where::Within(area), area)
                        }
                    };
                    (Seeing::Image { template, place, least: *least, last: None }, covers)
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
    fn seen(&mut self, frame: &Image) -> Vec<bool> {
        let shows_area = self.area;
        self.looks
            .iter_mut()
            .map(|look| {
                let shows = match &mut look.sight {
                    Seeing::Image { template, place, least, last } => {
                        let found = match *place {
                            Where::Near(x, y) => near(frame, shows_area, template, (x, y), *least),
                            Where::Within(area) => {
                                find(frame, shows_area, template, area, *least, *last)
                            }
                        };
                        *last = found.or(*last);
                        found.is_some()
                    }
                    Seeing::Color { at, rgb, within } => {
                        color_matches(frame, shows_area, *at, *rgb, *within)
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
/// looking, and the player with it; a frame slow to come, or one it could
/// not copy, only delays it.
pub fn watch(
    eyes: &mut dyn Eyes,
    looks: &mut Looks,
    seen: &Seen,
    stop: &StopFlag,
    done: &StopFlag,
) {
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
            // No frame this time -- late, or one the display failed to
            // copy -- is waited out; only a display that is gone ends it.
            Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::ResourceBusy) => {
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
    ///
    /// `outer` is what the steps it broke into hold: their keys stay down
    /// through it, and a Repeat caught mid-tap is let go of first, so a
    /// long `when` does not turn a tap into a hold.
    pub fn answer(
        &self,
        input: &mut dyn Input,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
        outer: &mut Held,
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
            outer.repeats.let_go_now(input)?;
            let mut held = Held { outer: outer.down.clone(), ..Held::default() };
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
