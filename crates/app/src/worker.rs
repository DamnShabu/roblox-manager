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
    fn a_result_comes_back_on_the_main_loop() {
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().unwrap();
        let got = Rc::new(Cell::new(0));
        let seen = got.clone();
        run(|| 41 + 1, move |n| seen.set(n));
        assert!(until(|| got.get() == 42), "never delivered");
    }
}
