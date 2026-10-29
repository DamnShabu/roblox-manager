//! Work off the main loop, results back on it.

use std::thread;

use gtk::glib;

/// Run `work` on a thread of its own and hand its result to `done` on the
/// GTK main loop. The UI never waits on the keyring, Roblox or a process.
pub fn run<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded(1);
    thread::spawn(move || {
        // The receiver only goes away with the main loop; nothing to tell.
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(result) = rx.recv().await {
            done(result);
        }
    });
}

/// Work that panicked instead of returning, and what the panic said.
#[derive(Debug)]
pub struct Crashed(String);

impl std::fmt::Display for Crashed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "it crashed ({})", self.0)
    }
}

/// `work` with a panic caught and handed back as [`Crashed`]. For work
/// whose completion undoes state the UI set up for it: a panic otherwise
/// drops the completion unrun, and leaves a launch's rows on "Starting…"
/// or the running-clients poll switched off for good.
pub fn catching<T>(work: impl FnOnce() -> T + Send) -> impl FnOnce() -> Result<T, Crashed> + Send {
    move || {
        // Nothing the work shares with the main loop is left half-changed:
        // it hands its result back, and a crash hands back none.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|panic| {
            let said = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".to_owned());
            Crashed(said)
        })
    }
}

/// Activity lines from any thread, shown by the main loop in order.
#[derive(Clone)]
pub struct Logger(async_channel::Sender<String>);

impl Logger {
    /// A logger, and the main-loop task that hands each line to `show`.
    pub fn new(show: impl Fn(String) + 'static) -> Self {
        let (tx, rx) = async_channel::unbounded::<String>();
        glib::spawn_future_local(async move {
            while let Ok(line) = rx.recv().await {
                show(line);
            }
        });
        Logger(tx)
    }

    pub fn line(&self, text: impl Into<String>) {
        // Unbounded: sending only fails once the main loop has gone.
        let _ = self.0.try_send(text.into());
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use super::*;

    /// Spin the main context until `done` or a few seconds pass.
    fn until(done: impl Fn() -> bool) -> bool {
        let ctx = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() && Instant::now() < deadline {
            ctx.iteration(false);
            thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    #[test]
    fn a_result_or_a_crash_comes_back_on_the_main_loop() {
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().unwrap();
        let got = Rc::new(Cell::new(0));
        let seen = got.clone();
        run(|| 41 + 1, move |n| seen.set(n));
        assert!(until(|| got.get() == 42), "never delivered");
        // One test owns the default main context: a second would race it.
        let crashed = Rc::new(std::cell::RefCell::new(None));
        let seen = crashed.clone();
        run(catching(|| -> u32 { panic!("boom") }), move |r: Result<u32, Crashed>| {
            *seen.borrow_mut() = Some(r.map_err(|e| e.to_string()));
        });
        assert!(until(|| crashed.borrow().is_some()), "a crash never delivered");
        assert_eq!(*crashed.borrow(), Some(Err("it crashed (boom)".into())));
    }
}
