//! The app's side of the crash log: the panic hook that writes one, the
//! window's activity copied into it as it happens, and the message that
//! shows the last run's on the next start.

use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

use adw::prelude::*;
use gtk::{gio, glib};
use rbxmgr_core::crash::{self, Crash, CrashLog};

use crate::ui::window::Window;

static LOG: OnceLock<Arc<CrashLog>> = OnceLock::new();

thread_local! {
    /// The last run's crash, until the window is up to show it.
    static PENDING: RefCell<Option<Crash>> = const { RefCell::new(None) };
}

/// Start this run's crash log and hook panics into it. The last run's
/// crash, if it left one, waits for [`show_pending`].
pub fn install() {
    let log = Arc::new(CrashLog::new(&rbxmgr_core::Paths::from_env().crashes()));
    match log.begin(std::process::id(), env!("CARGO_PKG_VERSION")) {
        Ok(found) => PENDING.with_borrow_mut(|p| *p = found),
        Err(e) => {
            eprintln!("roblox-manager: no crash log this run ({}): {e}", log.dir().display());
            return;
        }
    }
    let hooked = log.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let location =
            info.location().map_or_else(|| "an unknown place".to_owned(), |l| l.to_string());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let report = crash::panic_report(&message, &location, &thread, &backtrace);
        if let Err(e) = hooked.panicked(&report) {
            eprintln!("roblox-manager: could not write the crash log: {e}");
        }
        previous(info);
    }));
    // A run ended by the session or Ctrl+C is a clean one, not a crash.
    for signal in [libc_signal::SIGTERM, libc_signal::SIGINT, libc_signal::SIGHUP] {
        glib_unix::unix_signal_add_local(signal, || {
            if let Some(app) = gio::Application::default() {
                app.quit();
            }
            glib::ControlFlow::Break
        });
    }
    // Set once, here, and never before: nothing else sets it.
    let _ = LOG.set(log);
}

/// A clean exit: nothing for the next start to report.
pub fn end() {
    if let Some(log) = LOG.get() {
        if let Err(e) = log.end() {
            eprintln!("roblox-manager: could not mark a clean exit: {e}");
        }
    }
}

/// One line of the window's activity, into the crash log.
pub fn line(stamp: &str, text: &str) {
    if let Some(log) = LOG.get() {
        log.line(stamp, text);
    }
}

/// The last run's crash, over the window, once.
pub fn show_pending(w: &Window) {
    if let Some(found) = PENDING.with_borrow_mut(Option::take) {
        show(w, found);
    }
}

fn show(w: &Window, found: Crash) {
    let why = if found.panicked {
        "It hit an error it could not recover from."
    } else {
        "It was closed by force, froze, or crashed outside its own code."
    };
    let dialog = adw::AlertDialog::new(
        Some("Stacked Closed Unexpectedly"),
        Some(&format!(
            "{why} The crash log below is saved as {}; include it when you report this.",
            found.saved.display()
        )),
    );
    let text = gtk::TextView::builder()
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(10)
        .right_margin(10)
        .build();
    text.buffer().set_text(&found.log);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&text)
        .min_content_height(260)
        .min_content_width(520)
        .build();
    scroller.add_css_class("card");
    dialog.set_extra_child(Some(&scroller));
    dialog.add_response("folder", "_Open Folder");
    dialog.add_response("copy", "_Copy Log");
    dialog.add_response("close", "_Close");
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    let (weak, log, saved) = (w.weak(), found.log.clone(), found.saved.clone());
    dialog.connect_response(None, move |_, response| {
        let Some(w) = weak.upgrade() else { return };
        match response {
            "copy" => {
                w.gtk_window().clipboard().set_text(&log);
                w.toast("Crash log copied");
            }
            "folder" => {
                let dir = saved.parent().map(gio::File::for_path);
                if let Some(dir) = dir {
                    let opened = gio::AppInfo::launch_default_for_uri(
                        &dir.uri(),
                        None::<&gio::AppLaunchContext>,
                    );
                    if let Err(e) = opened {
                        w.toast(&format!("Could not open {}: {e}", saved.display()));
                    }
                }
            }
            _ => {}
        }
    });
    dialog.present(Some(w.gtk_window()));
}

/// The signal numbers, which glib takes as plain integers.
mod libc_signal {
    pub const SIGHUP: i32 = 1;
    pub const SIGINT: i32 = 2;
    pub const SIGTERM: i32 = 15;
}
