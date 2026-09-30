//! Join links from the browser. A link opens only its popup: the window
//! stays hidden unless it was already up, or something goes wrong that the
//! user has to see.

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::desktop::Handler;
use rbxmgr_core::roblox::JoinLink;

use super::Window;
use crate::ui::join_link::LinkPopup;
use crate::ui::widgets::sentence;

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

    /// The menu's Open Roblox Links Here: this app becomes the browser's
    /// handler for Roblox's links, whoever held them.
    pub fn open_links_here(&self) {
        let links = self.services().links.clone();
        self.run_task(
            move || {
                let before = links.current();
                links.claim().map(|()| before).map_err(|e| e.to_string())
            },
            |w, done| match done {
                Ok(Handler::Here) => w.toast("Roblox links already open here"),
                Ok(Handler::Elsewhere(other)) => {
                    let other = other.trim_end_matches(".desktop");
                    w.log(&format!("Roblox links now open here instead of {other}"));
                    w.toast("Roblox links from the browser now open here");
                }
                Ok(Handler::Unset) => {
                    w.log("Roblox links now open here");
                    w.toast("Roblox links from the browser now open here");
                }
                Err(e) => {
                    w.log(&e);
                    w.toast(&sentence(&e));
                }
            },
        );
    }

    /// On start: keep a desktop entry this app wrote pointing at this copy,
    /// and say in the log where Roblox's links go when it is not here.
    pub(super) fn check_link_handler(&self) {
        let links = self.services().links.clone();
        let log = self.logger();
        crate::worker::run(
            move || {
                match links.refresh() {
                    Ok(true) => log.line("Updated where Roblox links open this app"),
                    Ok(false) => {}
                    Err(e) => log.line(e.to_string()),
                }
                match links.current() {
                    Handler::Here => {}
                    Handler::Elsewhere(other) => log.line(format!(
                        "Roblox links open in {} -- Open Roblox Links Here in the menu changes that",
                        other.trim_end_matches(".desktop")
                    )),
                    Handler::Unset => log.line(
                        "To join games from the browser, pick Open Roblox Links Here in the menu",
                    ),
                }
            },
            |()| {},
        );
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
