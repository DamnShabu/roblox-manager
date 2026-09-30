//! Stopping work that runs on another thread: a playing macro, a launch
//! part-way through its accounts.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// Stops work from any thread; waits on it end early when it is set.
/// Clones are the same flag.
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

    /// Whether `other` is this flag (or a clone of it), not merely another.
    pub fn same_as(&self, other: &StopFlag) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
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
}
