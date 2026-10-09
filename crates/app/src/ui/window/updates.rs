//! One Update for everything: the Roblox build, Stacked and Roblox Manager
//! itself. A check at start and every few hours shows the header's Update
//! button when the app or Stacked has a newer release; the main menu's
//! Update All is always there (Roblox's own build can only be checked by
//! fetching it).

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::cordial::stacked::Host;
use rbxmgr_core::update::{self, AppOutcome, Report, Sources};

use super::Window;
use crate::ui::activity;
use crate::ui::widgets::sentence;
use crate::worker;

/// The first check waits for the window to settle.
const FIRST_CHECK_SECS: u32 = 10;
/// How often to ask again while the app stays open.
const CHECK_EVERY_SECS: u32 = 6 * 3600;

impl Window {
    /// Check now-ish, and then every few hours while the window lives.
    pub(super) fn schedule_update_checks(&self) {
        let weak = self.weak();
        glib::timeout_add_seconds_local_once(FIRST_CHECK_SECS, move || {
            if let Some(w) = weak.upgrade() {
                w.check_updates();
            }
        });
        let weak = self.weak();
        glib::timeout_add_seconds_local(CHECK_EVERY_SECS, move || match weak.upgrade() {
            Some(w) => {
                w.check_updates();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
    }

    /// Ask GitHub what is out, quietly: no spinner, and the button is all
    /// that changes.
    fn check_updates(&self) {
        let (app, stacked, paths) = (
            self.services().app_releases.clone(),
            self.services().releases.clone(),
            self.services().paths.clone(),
        );
        let weak = self.weak();
        worker::run(
            move || update::check(&*app, &*stacked, &paths),
            move |checked| {
                let Some(w) = weak.upgrade() else { return };
                for problem in &checked.problems {
                    w.log(problem);
                }
                let fresh = checked.available != w.state().available;
                if fresh && checked.available.any() {
                    w.log(&format!("{} out: Update installs it", out(&checked.available)));
                }
                w.state_mut().available = checked.available;
                w.show_available();
            },
        );
    }

    fn show_available(&self) {
        let s = self.state();
        let button = &self.0.ui.top.update.button;
        // An app version already installed waits only for Restart.
        let app_left = s.available.app.is_some() && s.available.app != s.restart_to;
        button.set_visible(app_left || s.available.stacked.is_some());
        button.set_tooltip_text(Some(&format!(
            "{} out — Update installs it, and the newest Roblox build",
            out(&s.available)
        )));
    }

    // -- updating ---------------------------------------------------------------
    /// Bring the Roblox build, Stacked and the app up to date. Clients
    /// already running keep what they started with.
    pub fn on_update(&self) {
        if self.state().updating {
            return;
        }
        self.state_mut().updating = true;
        self.set_action_enabled("update", false);
        let banner = &self.0.ui.banner;
        banner
            .set_title("Updating Roblox, Stacked and Roblox Manager — this can take a few minutes");
        banner.set_button_label(None);
        banner.set_revealed(true);
        let s = self.services();
        let (runner, stacked, app, packages, paths, install, log) = (
            s.runner.clone(),
            s.releases.clone(),
            s.app_releases.clone(),
            s.packages.clone(),
            s.paths.clone(),
            s.install.clone(),
            self.logger(),
        );
        self.run_task(
            move || {
                let sources = Sources {
                    runner: &*runner,
                    stacked: &*stacked,
                    app: &*app,
                    packages: &*packages,
                    paths: &paths,
                    install: &install,
                    host: Host::detect(),
                };
                update::update_all(&sources, &|l| log.line(l))
            },
            |w, report| w.finish_update(report),
        );
    }

    fn finish_update(&self, report: Report) {
        self.state_mut().updating = false;
        self.set_action_enabled("update", true);
        self.0.ui.banner.set_revealed(false);
        let mut failed = Vec::new();
        match report.roblox {
            Ok(()) => self.log("Roblox is up to date"),
            Err(e) => {
                self.log(&format!("Could not update Roblox: {e}"));
                failed.push(("Roblox", e));
            }
        }
        match report.stacked {
            Ok(done) => {
                if let Some(why) = &done.left_behind {
                    self.log(&format!("Older Stacked versions were not all deleted: {why}"));
                }
                self.log(&if done.fresh {
                    format!(
                        "Stacked {} is installed; clients launched from now on run it",
                        done.version
                    )
                } else {
                    format!("Stacked {} is up to date", done.version)
                });
            }
            Err(e) => {
                self.log(&format!("Could not update Stacked: {e}"));
                failed.push(("Stacked", e));
            }
        }
        match report.app {
            AppOutcome::Current => {
                self.log(&format!("Roblox Manager {} is up to date", update::RUNNING));
            }
            AppOutcome::Installed(version) => {
                self.log(&format!("Roblox Manager {version} is installed; restart to use it"));
                self.state_mut().restart_to = Some(version);
            }
            AppOutcome::Failed(e) => {
                self.log(&format!("Could not update Roblox Manager: {e}"));
                failed.push(("Roblox Manager", e));
            }
        }
        let restart_to = self.state().restart_to.clone();
        if let Some(version) = &restart_to {
            let banner = &self.0.ui.banner;
            banner.set_title(&format!("Roblox Manager {version} is installed"));
            banner.set_button_label(Some("_Restart"));
            banner.set_action_name(Some("win.restart"));
            banner.set_revealed(true);
        }
        match failed.as_slice() {
            [] => {
                let done = "Everything is up to date";
                self.toast(done);
                if restart_to.is_none() {
                    self.notify(
                        done,
                        "Clients launched from now on run the newest Roblox and Stacked.",
                    );
                } else {
                    self.notify(done, "Restart Roblox Manager to run its new version.");
                }
            }
            [(what, why)] => {
                let title = format!("Could not update {what}");
                self.toast_with(&title, "Details", activity::open_log);
                self.notify(&title, &sentence(why));
            }
            _ => {
                let names: Vec<&str> = failed.iter().map(|(what, _)| *what).collect();
                let title = format!("Could not update {}", names.join(" or "));
                self.toast_with(&title, "Details", activity::open_log);
                self.notify(&title, "The activity log says why.");
            }
        }
        self.check_updates();
    }

    /// Start the version an Update installed, and close this one: the new
    /// one waits for it to go.
    pub fn on_restart(&self) {
        if !self.state().macro_runs.is_empty() {
            return self.toast("Stop the playing macros first: restarting would stop them");
        }
        let s = self.services();
        match update::start_installed(&*s.runner, &s.install, &s.paths) {
            Ok(()) => self.gtk_window().close(),
            Err(e) => {
                self.log(&format!("Could not restart Roblox Manager: {e}"));
                self.toast_with(
                    "Could not restart; open Roblox Manager again",
                    "Details",
                    activity::open_log,
                );
            }
        }
    }
}

/// "Roblox Manager 0.3.0 is", "Roblox Manager 0.3.0 and Stacked 0.21.0 are".
fn out(available: &update::Available) -> String {
    let verb = if available.app.is_some() && available.stacked.is_some() { "are" } else { "is" };
    format!("{} {verb}", available.describe())
}
