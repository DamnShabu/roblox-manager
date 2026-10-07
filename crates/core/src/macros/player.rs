//! Playing a macro into a macro-ready client's display.

use std::cell::RefCell;
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::MacroError;
use super::grammar::{Macro, Step, TAP_PRESS, TYPE_GAP, describe};
use super::keys::{self, SHIFT};
use super::sight::{Eyes, Image};
pub use crate::stop::StopFlag;

mod repeat;
mod timeline;
mod watch;

use watch::{Looks, Seen, When};

/// How often a gliding pointer moves on: a hundred times a second.
const GLIDE_TICK: f64 = 0.01;
/// A wait this long or longer is reported; shorter ones are the rhythm of
/// the steps around them.
const REPORTED_WAIT: f64 = 1.0;
/// Seconds a macro may fall behind its timeline and still catch up, by
/// shortening the waits after. Further behind -- the machine stalled -- it
/// carries on from where it is rather than rushing through steps.
const MOST_BEHIND: f64 = 0.25;

/// Where a macro's input goes. Adapters: [`super::wayland::VirtualInput`],
/// and a recorder in the self-check. Dropping it lets go of the display.
pub trait Input {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()>;
    /// The pointer moved by (`dx`, `dy`): raw movement, as a mouse sends it,
    /// to 256ths of a unit.
    fn motion(&mut self, dx: f64, dy: f64) -> io::Result<()>;
    fn button(&mut self, code: u16, down: bool) -> io::Result<()>;
    /// The pointer to (`x`, `y`), from the display's top-left corner.
    fn move_to(&mut self, x: i32, y: i32) -> io::Result<()>;
    /// The wheel turned `notches`: down, or right when `horizontal`, are
    /// positive.
    fn scroll(&mut self, horizontal: bool, notches: i32) -> io::Result<()>;
}

/// What a playing macro holds between its steps: the keys and buttons a
/// Press left down, in the order pressed, the point it last put the
/// pointer at, the Repeats still tapping, where it is on its timeline, and
/// the `when`s it answers while it waits.
#[derive(Default)]
pub struct Held<'a> {
    down: Vec<u16>,
    at: Option<(i32, i32)>,
    repeats: repeat::Repeats,
    /// When the last wait was due to end. The next counts on from there,
    /// not from whenever it began: a wait woken late, or a step slow to
    /// send while the display lags, is made up by the waits after it, so
    /// a macro keeps to the clock however busy the machine is.
    due: Option<Instant>,
    when: Option<When<'a>>,
    /// An Exit was played -- by the macro's own steps or by a `when` -- and
    /// the round is ending: every wait from here on ends at once, as a stop
    /// ends it, until the round has.
    exiting: bool,
    /// Keys the steps a `when` broke into are holding. A `when` neither
    /// presses nor lets go of them: they stay down through it, as its
    /// help says, rather than being let go of by its first tap of one.
    outer: Vec<u16>,
}

impl Held<'_> {
    /// Wait `secs` on from where the last wait was due to end, or until
    /// stopped (then true), tapping whatever Repeats are due meanwhile.
    pub fn idle(
        &mut self,
        input: &mut dyn Input,
        secs: f64,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        let until = self.on_time() + Duration::try_from_secs_f64(secs).unwrap_or_default();
        self.idle_until(input, until, stop, pick)
    }

    /// Where the macro is on its timeline: when the last wait was due to
    /// end -- or now, when it is further behind than can be made up.
    fn on_time(&self) -> Instant {
        self.due.map_or_else(Instant::now, on_clock)
    }

    /// Wait until `until`, or until stopped or the round exited (then
    /// true), tapping whatever Repeats are due meanwhile.
    fn idle_until(
        &mut self,
        input: &mut dyn Input,
        until: Instant,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        if self.exiting {
            return Ok(true);
        }
        self.due = Some(until);
        loop {
            // Read before the `when`s are: one that sees after it rings past it.
            let rings = stop.rings();
            if let Some(when) = self.when {
                if when.answer(input, stop, pick, self)? {
                    self.exiting = true;
                    return Ok(true);
                }
            }
            let now = Instant::now();
            self.repeats.fire(input, now.min(until), pick)?;
            if now >= until {
                return Ok(stop.is_set());
            }
            let wake = self.repeats.next_due().map_or(until, |due| due.min(until));
            if stop.wait_rung(wake.saturating_duration_since(now).as_secs_f64(), rings) {
                return Ok(true);
            }
        }
    }

    /// Wait until every Repeat has tapped its last, or until stopped or the
    /// round exited (then true).
    pub fn finish_repeats(
        &mut self,
        input: &mut dyn Input,
        stop: &StopFlag,
        pick: &dyn Fn(f64, f64) -> f64,
    ) -> io::Result<bool> {
        while let Some(due) = self.repeats.next_due() {
            if self.idle_until(input, due, stop, pick)? {
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

    /// The round's end, however it came: everything let go of, and an exit
    /// done with. An exit leaves the timeline where its cut-short wait was
    /// due to end, so the next round counts from now instead.
    fn end_exit(&mut self, input: &mut dyn Input) -> io::Result<()> {
        if std::mem::take(&mut self.exiting) {
            self.due = None;
        }
        self.release_all(input)
    }

    /// Whether `code` is already down, by a Press or by the steps a `when`
    /// broke into.
    fn holds(&self, code: u16) -> bool {
        self.down.contains(&code) || self.outer.contains(&code)
    }

    fn press(&mut self, input: &mut dyn Input, codes: &[u16]) -> io::Result<()> {
        for &code in codes {
            if !self.holds(code) {
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

/// `due`, or now when that is further behind than can be made up.
fn on_clock(due: Instant) -> Instant {
    let now = Instant::now();
    if now.saturating_duration_since(due).as_secs_f64() <= MOST_BEHIND { due } else { now }
}

/// What a run's picks from a written range (`wait 60-240`) are drawn from.
/// Nothing the user did not write as a range is picked. The clients a
/// macro is started on together share one, so they pick the same moments in
/// the same order and play alike -- a pick each of their own would set them
/// apart a little more with every step. The next run draws a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed(u64);

impl Seed {
    pub fn fresh() -> Self {
        Seed(rand::random())
    }

    /// A random moment in [lo, hi] each call: the same ones, in the same
    /// order, for every picker from this seed.
    pub fn picker(self) -> impl Fn(f64, f64) -> f64 {
        let rng = RefCell::new(StdRng::seed_from_u64(self.0));
        move |lo, hi| if lo < hi { rng.borrow_mut().random_range(lo..=hi) } else { lo }
    }
}

/// How a macro with `when`s sees its client.
pub struct Sight<'a> {
    /// Eyes on the display at the path, which a thread of their own looks
    /// through.
    pub open: &'a dyn Fn(&Path) -> io::Result<Box<dyn Eyes + Send>>,
    /// An image a `when` names, from wherever picked images are kept.
    pub image: &'a dyn Fn(&str) -> Result<Image, String>,
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
    /// When its first step is due. Clients a macro is started on together
    /// share one, a little ahead: each gets ready to play in its own time,
    /// so counting from when each was ready would start them apart.
    pub start: Instant,
    pub sight: &'a Sight<'a>,
}

impl Player<'_> {
    /// Play `m` until `stop` is set or its rounds run out. Before every round
    /// the client is checked to still be up. Whatever ends it, nothing it
    /// pressed is left down.
    pub fn play(&self, m: &Macro, stop: &StopFlag) -> Result<(), MacroError> {
        if !(self.running)() {
            return Err(MacroError::NotRunning);
        }
        let unreachable = |e: io::Error| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => MacroError::NotNested,
            _ => went_away(e),
        };
        let mut input = (self.connect)(self.display).map_err(unreachable)?;
        // Ready to look before the first step, so a `when` that cannot see
        // says so now and not mid-macro.
        let watching = if m.handlers.is_empty() {
            None
        } else {
            let looks = Looks::new(&m.handlers, self.sight.image).map_err(MacroError::Sight)?;
            let eyes = (self.sight.open)(self.display).map_err(unreachable)?;
            Some((looks, eyes))
        };
        let seen = Seen::default();
        let done = StopFlag::default();
        thread::scope(|s| {
            if let Some((mut looks, mut eyes)) = watching {
                let (seen, done) = (&seen, &done);
                s.spawn(move || watch::watch(eyes.as_mut(), &mut looks, seen, stop, done));
            }
            let when = When { handlers: &m.handlers, seen: &seen, report: self.report };
            let played = self.play_with(m, stop, input.as_mut(), when);
            done.set();
            played
        })
    }

    /// [`Player::play`], once connected: its `when`s answered from the
    /// first step.
    fn play_with(
        &self,
        m: &Macro,
        stop: &StopFlag,
        input: &mut dyn Input,
        when: When<'_>,
    ) -> Result<(), MacroError> {
        let mut held = Held::default();
        let ahead = self.start.saturating_duration_since(Instant::now());
        let late = Instant::now().saturating_duration_since(self.start).as_secs_f64();
        if ahead.as_secs_f64() >= REPORTED_WAIT {
            let until = ((self.now)() + ahead).format("%H:%M:%S");
            let secs = ahead.as_secs_f64().round();
            (self.report)(format!("waiting {secs:.0}s for its turn, until {until}"));
        } else if late > MOST_BEHIND {
            (self.report)(format!("starting {late:.1}s late: its client was slow to reach"));
        }
        let played =
            held.idle_until(input, self.start, stop, self.pick).map_err(went_away).and_then(|_| {
                held.when = Some(when);
                self.rounds(m, stop, input, &mut held)
            });
        let let_go = held.release_all(input).map_err(went_away);
        played.and(let_go)
    }

    fn rounds(
        &self,
        m: &Macro,
        stop: &StopFlag,
        input: &mut dyn Input,
        held: &mut Held<'_>,
    ) -> Result<(), MacroError> {
        if m.steps.is_empty() {
            // Only `when`s: they play as they see, until stopped -- its
            // client checked on every second meanwhile.
            // An exit there has no round to end, and only lets go.
            while !held.idle(input, 1.0, stop, self.pick).map_err(went_away)? || held.exiting {
                if held.exiting {
                    held.end_exit(input).map_err(went_away)?;
                }
                if !(self.running)() {
                    return Err(MacroError::NotRunning);
                }
            }
            return Ok(());
        }
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
            let total = m.steps.len();
            for (i, step) in m.steps.iter().enumerate() {
                // Where this client's run is: each its own, from step 1.
                let at = format!("round {round}, step {}/{total}", i + 1);
                if stop.is_set() {
                    return Ok(());
                }
                if held.exiting {
                    break;
                }
                match step {
                    Step::Start(..) if done > 0 => {}
                    Step::Wait(lo, hi) | Step::Start(lo, hi) => {
                        let secs = (self.pick)(*lo, *hi);
                        if secs >= REPORTED_WAIT {
                            let until = (self.now)() + Duration::from_secs_f64(secs);
                            say(format!(
                                "{at}: waiting {secs:.0}s, until {}",
                                until.format("%H:%M:%S")
                            ));
                        }
                        held.idle(input, secs, stop, self.pick).map_err(went_away)?;
                    }
                    _ => {
                        say(format!("{at}: {}", describe(step)));
                        play_step(input, step, stop, self.pick, held).map_err(went_away)?;
                    }
                }
            }
            if held.finish_repeats(input, stop, self.pick).map_err(went_away)? && !held.exiting {
                return Ok(());
            }
            if stop.is_set() {
                return Ok(());
            }
            held.end_exit(input).map_err(went_away)?;
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
            input.motion(f64::from(*dx), f64::from(*dy))?;
            held.at = held.at.map(|(x, y)| (x.saturating_add(*dx), y.saturating_add(*dy)));
            Ok(())
        }
        Step::MoveTo { x, y, lo, hi } => glide(input, (*x, *y), pick(*lo, *hi), stop, pick, held),
        Step::Press(codes) => held.press(input, codes),
        Step::Release(codes) => held.release(input, codes),
        Step::Repeat { keys, lo, hi, every } => {
            let secs = pick(*lo, *hi);
            let at = held.on_time();
            held.repeats.start(input, keys, secs, *every, at, pick)
        }
        Step::Scroll { horizontal, notches } => input.scroll(*horizontal, *notches),
        Step::Path(_) | Step::Turn(_) => timeline::alone(input, step, stop, pick, held),
        Step::Timeline { secs, items } => {
            timeline::play(input, items, pick(secs.0, secs.1), stop, pick, held)
        }
        Step::Exit => {
            held.exiting = true;
            Ok(())
        }
        Step::Wait(..) | Step::Start(..) => Ok(()),
    }
}

/// The pointer to `to`, over `secs`: from where the macro last put it, a
/// small even step every tick, each due on the macro's timeline so the
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
    let start = held.on_time();
    let along = |a: i32, b: i32, t: f64| (f64::from(a) + f64::from(b - a) * t).round() as i32;
    for tick in 1..=ticks {
        let t = f64::from(tick) / f64::from(ticks);
        let due = start + Duration::from_secs_f64(secs * t);
        if held.idle_until(input, due, stop, pick)? {
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
/// in reverse -- every one that went down, whatever failed. A key a Press
/// is already holding is neither pressed again nor let go of: `press
/// shift` then `type A` keeps shift down for what follows.
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
    for &code in codes.iter().filter(|&&c| !held.holds(c)) {
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
