//! Hiding running clients' windows and showing them again. The game goes on
//! behind a hidden window: Stacked unmaps it on a signal, and a macro-ready
//! client's window relay unmaps its cage's, with macros still playing in it
//! (see `CordialProfiles::set_hidden`).

use std::collections::HashSet;

use rbxmgr_core::cordial::Window as ClientWindow;
use rbxmgr_core::types::{Profile, UserId};

use super::Window;
use crate::state::AppState;

impl Window {
    /// Hide or show one account's window.
    pub fn set_window_hidden(&self, id: UserId, hide: bool) {
        self.signal_windows(vec![id], hide);
    }

    pub fn on_hide_all(&self) {
        let ids = hideable(&self.state());
        self.signal_windows(ids, true);
    }

    pub fn on_show_all(&self) {
        let ids = hidden(&self.state());
        self.signal_windows(ids, false);
    }

    fn signal_windows(&self, ids: Vec<UserId>, hide: bool) {
        if ids.is_empty() {
            return;
        }
        let which: HashSet<Profile> = ids.iter().map(|id| Profile::of(*id)).collect();
        let profiles = self.services().profiles.clone();
        self.run_task(
            move || profiles.set_hidden(&which, hide),
            move |w, done| {
                let verb = if hide { "hide" } else { "show" };
                match done {
                    Ok(n) => {
                        let done = if hide { "Hid" } else { "Showed" };
                        w.log(&format!("{done} {n} window(s)"));
                        // The engine acts within a pump tick; read it back now
                        // rather than at the next poll.
                        w.poll_running();
                    }
                    Err(e) => {
                        w.toast(&format!("Could not {verb} the window"));
                        w.log(&format!("Could not {verb} the window: {e}"));
                    }
                }
            },
        );
    }

    /// Whether Hide All and Show All have anything to do.
    pub(super) fn refresh_window_actions(&self, s: &AppState) {
        self.set_action_enabled("hide-all", !hideable(s).is_empty());
        self.set_action_enabled("show-all", !hidden(s).is_empty());
    }
}

/// Whether the account's window can be hidden: it is up, and something can
/// hide it -- its engine, or a macro-ready client's window relay. A
/// macro-ready client launched before window relays reports no window at
/// all, since its engine could only hide its window inside cage, where a
/// macro's input needs it.
pub fn can_hide(s: &AppState, id: UserId) -> bool {
    s.windows.get(&id) == Some(&ClientWindow::Shown)
}

fn hideable(s: &AppState) -> Vec<UserId> {
    s.windows.keys().copied().filter(|id| can_hide(s, *id)).collect()
}

fn hidden(s: &AppState) -> Vec<UserId> {
    s.windows.iter().filter(|(_, w)| **w == ClientWindow::Hidden).map(|(id, _)| *id).collect()
}
