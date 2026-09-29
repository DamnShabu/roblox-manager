//! Playing a macro into a macro-ready client's display.

use std::io;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Local};

use super::MacroError;
use super::grammar::{Macro, Step, TAP_PRESS, TYPE_GAP, describe};
use super::keys::SHIFT;

/// Where a macro's input goes. Adapters: [`super::wayland::VirtualInput`],
/// and a recorder in the self-check. Dropping it lets go of the display.
pub trait Input {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()>;
    fn motion(&mut self, dx: i32, dy: i32) -> io::Result<()>;
    fn button(&mut self, code: u16, down: bool) -> io::Result<()>;
}

/// Stops a playing macro, from any thread; waits end early when it is set.
#[derive(Clone, Default)]
pub struct StopFlag(Arc<(Mutex<bool>, Condvar)>);

impl StopFlag {
    pub fn set(&self) {
        let (lock, wake) = &*self.0;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    pub fn is_set(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait up to `secs`; true when stopped meanwhile.
    pub fn wait(&self, secs: f64) -> bool {
        let (lock, wake) = &*self.0;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let timeout = Duration::try_from_secs_f64(secs).unwrap_or_default();
        let (stopped, _) = wake
            .wait_timeout_while(guard, timeout, |stopped| !*stopped)
            .unwrap_or_else(PoisonError::into_inner);
        *stopped
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
    /// Hears each step as it starts -- a wait with the clock time it ends
    /// at, since a macro that opens on minutes of waiting otherwise looks
    /// exactly like one that does nothing.
    pub report: &'a dyn Fn(String),
    pub pick: &'a dyn Fn(f64, f64) -> f64,
    pub now: &'a dyn Fn() -> DateTime<Local>,
}

impl Player<'_> {
    /// Play `m` until `stop` is set or its rounds run out. Before every round
    /// the client is checked to still be up.
    pub fn play(&self, m: &Macro, stop: &StopFlag) -> Result<(), MacroError> {
        if !(self.running)() {
            return Err(MacroError::NotRunning);
        }
        let mut input = (self.connect)(self.display).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => MacroError::NotNested,
            _ => MacroError::WentAway(e.to_string()),
        })?;
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
                        let until = (self.now)() + Duration::from_secs_f64(secs);
                        (self.report)(format!(
                            "round {round}: waiting {secs:.0}s, until {}",
                            until.format("%H:%M:%S")
                        ));
                        stop.wait(secs);
                    }
                    _ => {
                        (self.report)(format!("round {round}: {}", describe(step)));
                        play_step(input.as_mut(), step, stop, self.pick)
                            .map_err(|e| MacroError::WentAway(e.to_string()))?;
                    }
                }
            }
            done += 1;
        }
        Ok(())
    }
}

/// One key, text, click or move step. Every key or button pressed is let go
/// again, even when stop cuts a hold short or the display fails.
pub fn play_step(
    input: &mut dyn Input,
    step: &Step,
    stop: &StopFlag,
    pick: &dyn Fn(f64, f64) -> f64,
) -> io::Result<()> {
    let tap = || pick(TAP_PRESS.0, TAP_PRESS.1);
    match step {
        Step::Hold { keys, lo, hi } => press(input, keys, pick(*lo, *hi), stop, Press::Key),
        Step::Type(chars) => {
            for &(code, shifted) in chars {
                let keys: &[u16] = if shifted { &[SHIFT, code] } else { &[code] };
                press(input, keys, tap(), stop, Press::Key)?;
                if stop.wait(pick(TYPE_GAP.0, TYPE_GAP.1)) {
                    break;
                }
            }
            Ok(())
        }
        Step::Click { button, at } => {
            if let Some((x, y)) = at {
                // Relative motion only; the far corner is the origin, since
                // the compositor clamps the pointer to its output.
                input.motion(-100_000, -100_000)?;
                input.motion(*x, *y)?;
            }
            press(input, &[*button], tap(), stop, Press::Button)
        }
        Step::Move(dx, dy) => input.motion(*dx, *dy),
        Step::Wait(..) | Step::Start(..) => Ok(()),
    }
}

#[derive(Clone, Copy)]
enum Press {
    Key,
    Button,
}

/// Press `codes` in order, hold for `secs` (or until stopped), and release
/// in reverse -- every one that went down, whatever failed.
fn press(
    input: &mut dyn Input,
    codes: &[u16],
    secs: f64,
    stop: &StopFlag,
    how: Press,
) -> io::Result<()> {
    let send = |input: &mut dyn Input, code, down| match how {
        Press::Key => input.key(code, down),
        Press::Button => input.button(code, down),
    };
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
        stop.wait(secs);
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
