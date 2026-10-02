//! Playing a macro into a macro-ready client's display.

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};

use super::MacroError;
use super::grammar::{Macro, Step, TAP_PRESS, TYPE_GAP, describe};
use super::keys::{self, SHIFT};
pub use crate::stop::StopFlag;

mod repeat;

/// How often a gliding pointer moves on: a hundred times a second.
const GLIDE_TICK: f64 = 0.01;
/// A wait this long or longer is reported; shorter ones are the rhythm of
/// the steps around them.
const REPORTED_WAIT: f64 = 1.0;

/// Where a macro's input goes. Adapters: [`super::wayland::VirtualInput`],
/// and a recorder in the self-check. Dropping it lets go of the display.
pub trait Input {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()>;
    fn motion(&mut self, dx: i32, dy: i32) -> io::Result<()>;
    fn button(&mut self, code: u16, down: bool) -> io::Result<()>;
    /// The pointer to (`x`, `y`), from the display's top-left corner.
    fn move_to(&mut self, x: i32, y: i32) -> io::Result<()>;
    /// The wheel turned `notches`: down, or right when `horizontal`, are
    /// positive.
    fn scroll(&mut self, horizontal: bool, notches: i32) -> io::Result<()>;
}

/// What a playing macro holds between its steps: the keys and buttons a
/// Press left down, in the order pressed, the point it last put the
/// pointer at, and the Repeats still tapping.
#[derive(Debug, Default)]
pub struct Held {
    down: Vec<u16>,
    at: Option<(i32, i32)>,
    repeats: repeat::Repeats,
}

impl Held {
    /// Wait `secs`, or until stopped (then true), tapping whatever Repeats
    /// are due meanwhile.
    pub fn idle(
        &mut self,
        input: &mut dyn Input,
        secs: f64,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        let until = Instant::now() + Duration::try_from_secs_f64(secs).unwrap_or_default();
        loop {
            self.repeats.fire(input, pick)?;
            let now = Instant::now();
            if now >= until {
                return Ok(stop.is_set());
            }
            let wake = self.repeats.next_due().map_or(until, |due| due.min(until));
            if stop.wait(wake.saturating_duration_since(now).as_secs_f64()) {
                return Ok(true);
            }
        }
    }

    /// Wait until every Repeat has tapped its last, or until stopped (then
    /// true).
    pub fn finish_repeats(
        &mut self,
        input: &mut dyn Input,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        while let Some(due) = self.repeats.next_due() {
            let secs = due.saturating_duration_since(Instant::now()).as_secs_f64();
            if self.idle(input, secs, stop, pick)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Let go of everything still down, the last pressed first, and stop
    /// every Repeat. Every one is let go of, whatever fails; the first
    /// failure is the one returned.
    pub fn release_all(&mut self, input: &mut dyn Input) -> io::Result<()> {
        let mut result = self.repeats.release_all(input);
        while let Some(code) = self.down.pop() {
            let released = send(input, code, false);
            if result.is_ok() {
                result = released;
            }
        }
        result
    }

    fn press(&mut self, input: &mut dyn Input, codes: &[u16]) -> io::Result<()> {
        for &code in codes {
            if !self.down.contains(&code) {
                send(input, code, true)?;
                self.down.push(code);
            }
        }
        Ok(())
    }

    fn release(&mut self, input: &mut dyn Input, codes: &[u16]) -> io::Result<()> {
        for code in codes {
            if let Some(i) = self.down.iter().position(|c| c == code) {
                self.down.remove(i);
                send(input, *code, false)?;
            }
        }
        Ok(())
    }
}

/// A random moment in [lo, hi]: humans never press a key for the same few
/// milliseconds twice.
pub fn random_pick(lo: f64, hi: f64) -> f64 {
    if lo < hi { rand::random_range(lo..=hi) } else { lo }
}

/// Everything a playing macro reaches outside itself.
pub struct Player<'a> {
    /// The client's display link (`nested::display_file`).
    pub display: &'a Path,
    /// Whether the client is still up.
    pub running: &'a dyn Fn() -> bool,
    pub connect: &'a dyn Fn(&Path) -> io::Result<Box<dyn Input>>,
    /// Hears what the macro does as it starts something new -- and a long
    /// wait with the clock time it ends at, since a macro that opens on
    /// minutes of waiting otherwise looks exactly like one that does nothing.
    pub report: &'a dyn Fn(String),
    pub pick: &'a dyn Fn(f64, f64) -> f64,
    pub now: &'a dyn Fn() -> DateTime<Local>,
}

impl Player<'_> {
    /// Play `m` until `stop` is set or its rounds run out. Before every round
    /// the client is checked to still be up. Whatever ends it, nothing it
    /// pressed is left down.
    pub fn play(&self, m: &Macro, stop: &StopFlag) -> Result<(), MacroError> {
        if !(self.running)() {
            return Err(MacroError::NotRunning);
        }
        let mut input = (self.connect)(self.display).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => MacroError::NotNested,
            _ => went_away(e),
        })?;
        let mut held = Held::default();
        let played = self.rounds(m, stop, input.as_mut(), &mut held);
        let let_go = held.release_all(input.as_mut()).map_err(went_away);
        played.and(let_go)
    }

    fn rounds(
        &self,
        m: &Macro,
        stop: &StopFlag,
        input: &mut dyn Input,
        held: &mut Held,
    ) -> Result<(), MacroError> {
        let mut last = String::new();
        let mut say = |line: String| {
            if line != last {
                (self.report)(line.clone());
                last = line;
            }
        };
        let mut done = 0;
        while !stop.is_set() && (m.loops == 0 || done < m.loops) {
            if done > 0 && !(self.running)() {
                return Err(MacroError::NotRunning);
            }
            let round = done + 1;
            for step in &m.steps {
                if stop.is_set() {
                    return Ok(());
                }
                match step {
                    Step::Start(..) if done > 0 => {}
                    Step::Wait(lo, hi) | Step::Start(lo, hi) => {
                        let secs = (self.pick)(*lo, *hi);
                        if secs >= REPORTED_WAIT {
                            let until = (self.now)() + Duration::from_secs_f64(secs);
                            say(format!(
                                "round {round}: waiting {secs:.0}s, until {}",
                                until.format("%H:%M:%S")
                            ));
                        }
                        held.idle(input, secs, stop, self.pick).map_err(went_away)?;
                    }
                    _ => {
                        say(format!("round {round}: {}", describe(step)));
                        play_step(input, step, stop, self.pick, held).map_err(went_away)?;
                    }
                }
            }
            if held.finish_repeats(input, stop, self.pick).map_err(went_away)? {
                return Ok(());
            }
            held.release_all(input).map_err(went_away)?;
            done += 1;
        }
        Ok(())
    }
}

fn went_away(e: io::Error) -> MacroError {
    MacroError::WentAway(e.to_string())
}

/// One step that does something. A hold, a tap or a click lets go of what
/// it pressed even when stop cuts it short or the display fails; a Press
/// leaves its keys in `held`, and a Repeat its taps.
pub fn play_step(
    input: &mut dyn Input,
    step: &Step,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
    held: &mut Held,
) -> io::Result<()> {
    let tap = || pick(TAP_PRESS.0, TAP_PRESS.1);
    match step {
        Step::Hold { keys, lo, hi } => press(input, keys, pick(*lo, *hi), stop, pick, held),
        Step::Type(chars) => {
            for &(code, shifted) in chars {
                let keys: &[u16] = if shifted { &[SHIFT, code] } else { &[code] };
                press(input, keys, tap(), stop, pick, held)?;
                if held.idle(input, pick(TYPE_GAP.0, TYPE_GAP.1), stop, pick)? {
                    break;
                }
            }
            Ok(())
        }
        Step::Click { button, at } => {
            if let Some((x, y)) = at {
                input.move_to(*x, *y)?;
                held.at = Some((*x, *y));
            }
            press(input, &[*button], tap(), stop, pick, held)
        }
        Step::Move(dx, dy) => {
            input.motion(*dx, *dy)?;
            held.at = held.at.map(|(x, y)| (x.saturating_add(*dx), y.saturating_add(*dy)));
            Ok(())
        }
        Step::MoveTo { x, y, lo, hi } => glide(input, (*x, *y), pick(*lo, *hi), stop, pick, held),
        Step::Press(codes) => held.press(input, codes),
        Step::Release(codes) => held.release(input, codes),
        Step::Repeat { keys, lo, hi, every } => {
            held.repeats.start(input, keys, pick(*lo, *hi), *every, pick)
        }
        Step::Scroll { horizontal, notches } => input.scroll(*horizontal, *notches),
        Step::Wait(..) | Step::Start(..) => Ok(()),
    }
}

/// The pointer to `to`, over `secs`: from where the macro last put it, a
/// small even step every tick, each due on time from the start so the
/// glide never drifts. With no point to start from, or no time, it goes
/// straight there.
fn glide(
    input: &mut dyn Input,
    to: (i32, i32),
    secs: f64,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
    held: &mut Held,
) -> io::Result<()> {
    let from = match held.at {
        Some(from) if secs > 0.0 => from,
        _ => {
            input.move_to(to.0, to.1)?;
            held.at = Some(to);
            held.idle(input, secs, stop, pick)?;
            return Ok(());
        }
    };
    let ticks = (secs / GLIDE_TICK).round().max(1.0) as u32;
    let start = Instant::now();
    let along = |a: i32, b: i32, t: f64| (f64::from(a) + f64::from(b - a) * t).round() as i32;
    for tick in 1..=ticks {
        let t = f64::from(tick) / f64::from(ticks);
        let due = start + Duration::from_secs_f64(secs * t);
        let secs = due.saturating_duration_since(Instant::now()).as_secs_f64();
        if held.idle(input, secs, stop, pick)? {
            break;
        }
        let at = (along(from.0, to.0, t), along(from.1, to.1, t));
        if held.at != Some(at) {
            input.move_to(at.0, at.1)?;
            held.at = Some(at);
        }
    }
    Ok(())
}

/// A key to the keyboard, a mouse button to the pointer.
fn send(input: &mut dyn Input, code: u16, down: bool) -> io::Result<()> {
    if keys::is_button(code) { input.button(code, down) } else { input.key(code, down) }
}

/// Press `codes` in order, hold for `secs` (or until stopped), and release
/// in reverse -- every one that went down, whatever failed.
fn press(
    input: &mut dyn Input,
    codes: &[u16],
    secs: f64,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
    held: &mut Held,
) -> io::Result<()> {
    let mut down = Vec::new();
    let mut result = Ok(());
    for &code in codes {
        match send(input, code, true) {
            Ok(()) => down.push(code),
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    if result.is_ok() {
        result = held.idle(input, secs, stop, pick).map(|_| ());
    }
    for &code in down.iter().rev() {
        let released = send(input, code, false);
        if result.is_ok() {
            result = released;
        }
    }
    result
}

#[cfg(test)]
mod tests;
