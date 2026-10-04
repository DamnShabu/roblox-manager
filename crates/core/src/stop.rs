//! Stopping work that runs on another thread: a playing macro, a launch
//! part-way through its accounts.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Stops work from any thread; waits on it end early when it is set.
/// Clones are the same flag. It can also be rung, which ends a
/// [`StopFlag::wait_rung`] without stopping anything: a playing macro's
/// `when` seeing what it waits for.
#[derive(Clone, Default)]
pub struct StopFlag(Arc<(Mutex<Flag>, Condvar)>);

#[derive(Default)]
struct Flag {
    stopped: bool,
    rings: u64,
}

impl StopFlag {
    pub fn set(&self) {
        let (lock, wake) = &*self.0;
        lock.lock().unwrap_or_else(PoisonError::into_inner).stopped = true;
        wake.notify_all();
    }

    pub fn is_set(&self) -> bool {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner).stopped
    }

    /// Wake whatever waits in [`StopFlag::wait_rung`].
    pub fn ring(&self) {
        let (lock, wake) = &*self.0;
        lock.lock().unwrap_or_else(PoisonError::into_inner).rings += 1;
        wake.notify_all();
    }

    /// How often it has been rung: what [`StopFlag::wait_rung`] waits past.
    pub fn rings(&self) -> u64 {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner).rings
    }

    /// Wait up to `secs`, or until it is rung more than `seen` times all
    /// told; true when stopped meanwhile.
    pub fn wait_rung(&self, secs: f64, seen: u64) -> bool {
        let (lock, wake) = &*self.0;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let timeout = Duration::try_from_secs_f64(secs).unwrap_or_default();
        let (flag, _) = wake
            .wait_timeout_while(guard, timeout, |f| !f.stopped && f.rings <= seen)
            .unwrap_or_else(PoisonError::into_inner);
        flag.stopped
    }

    /// Whether `other` is this flag (or a clone of it), not merely another.
    pub fn same_as(&self, other: &StopFlag) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Wait up to `secs`; true when stopped meanwhile.
    pub fn wait(&self, secs: f64) -> bool {
        let (lock, wake) = &*self.0;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let timeout = Duration::try_from_secs_f64(secs).unwrap_or_default();
        let (flag, _) = wake
            .wait_timeout_while(guard, timeout, |f| !f.stopped)
            .unwrap_or_else(PoisonError::into_inner);
        flag.stopped
    }
}

impl std::fmt::Debug for StopFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StopFlag({})", if self.is_set() { "set" } else { "clear" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_is_the_same_flag_and_a_wait_ends_when_it_is_set() {
        let flag = StopFlag::default();
        let other = flag.clone();
        assert!(flag.same_as(&other) && !flag.same_as(&StopFlag::default()));
        assert!(!flag.wait(0.0), "not set: the wait runs out");
        let setter = std::thread::spawn(move || other.set());
        assert!(flag.wait(10.0), "set from another thread: the wait ends early");
        setter.join().unwrap();
        assert!(flag.is_set());
    }

    #[test]
    fn a_ring_ends_a_wait_for_one_without_stopping_anything() {
        let flag = StopFlag::default();
        let seen = flag.rings();
        let other = flag.clone();
        let ringer = std::thread::spawn(move || other.ring());
        assert!(!flag.wait_rung(10.0, seen), "rung, not stopped");
        ringer.join().unwrap();
        assert!(flag.rings() > seen && !flag.is_set());
        assert!(!flag.wait(0.0), "a plain wait is not ended by a ring it never sees");
    }
}
