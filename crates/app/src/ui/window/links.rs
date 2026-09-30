//! Join links from the browser. A link opens only its popup: the window
//! stays hidden unless it was already up, or something goes wrong that the
//! user has to see.

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::roblox::JoinLink;

use super::Window;
use crate::ui::join_link::LinkPopup;

impl Window {
    /// A link the app was opened with: its popup, or why it cannot be joined.
    pub fn open_link(&self, uri: &str) {
        let link = match JoinLink::parse(uri) {
            Ok(link) => link,
            Err(e) => {
                self.log(&format!("Could not open a link: {e}"));
                self.surface();
                return self.toast(&format!("Could not open the link: {e}"));
            }
        };
        if self.state().accounts.accounts().is_empty() {
            self.surface();
            return self.toast("Add an account first, then open the link again");
        }
        let popup = LinkPopup::open(self, link);
        let old = self.0.link_popup.replace(Some(popup.downgrade()));
        // After the new one is in, so closing the old one does not leave.
        if let Some(old) = old.and_then(|o| o.upgrade()) {
            old.close();
        }
    }

    /// Where a link's popup is shown from: over the window when that is up.
    pub fn popup_parent(&self) -> Option<&adw::ApplicationWindow> {
        self.0.win.is_visible().then_some(&self.0.win)
    }

    /// A link's popup closed.
    pub fn link_popup_closed(&self) {
        // Once the close is through, and a launch it asked for has begun.
        let weak = self.weak();
        glib::idle_add_local_once(move || {
            if let Some(w) = weak.upgrade() {
                w.leave_if_unseen();
            }
        });
    }

    /// Show the window if a link alone opened it: for a problem the user
    /// has to read.
    pub fn surface(&self) {
        if !self.seen() {
            self.present();
        }
    }

    /// A window made only for a link goes once nothing needs it: no popup
    /// open and no task under way. The clients it started keep running.
    pub(super) fn leave_if_unseen(&self) {
        let popup = self.0.link_popup.borrow().as_ref().and_then(glib::WeakRef::upgrade);
        if self.seen() || popup.is_some_and(|p| p.is_visible()) || self.state().busy > 0 {
            return;
        }
        self.close_now();
        self.0.win.destroy();
    }
}
