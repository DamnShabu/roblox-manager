//! Join a friend: every friend of one account, with where they are. Join
//! makes a friend's server the launch target.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType};
use rbxmgr_core::roblox::{Friend, FriendState, Roblox};
use rbxmgr_core::types::UserId;

use super::modal::Modal;
use super::widgets::{Btn, Fluent, LabelFluent, clear, clear_wrap, icon, lbl, wrap};
use super::window::{WeakWindow, Window};
use crate::state::FriendTarget;
use crate::worker;

/// What a friend's row says under their name.
pub fn status_text(f: &Friend) -> String {
    match f.state {
        FriendState::Game => {
            let game = f.game.as_deref().unwrap_or("a game");
            format!("In {game}{}", if f.server.is_some() { "" } else { " · server hidden" })
        }
        FriendState::Online => "Online".to_owned(),
        FriendState::Offline => "Offline".to_owned(),
    }
}

fn state_class(s: FriendState) -> &'static str {
    match s {
        FriendState::Game => "game",
        FriendState::Online => "online",
        FriendState::Offline => "offline",
    }
}

pub struct FriendsDialog {
    window: WeakWindow,
    modal: Modal,
    /// Each account's friends, once loaded (or why they did not load).
    cache: RefCell<HashMap<UserId, Result<Vec<Friend>, String>>>,
    of: Cell<UserId>,
    query: RefCell<String>,
    chips: adw::WrapBox,
    list: gtk::Box,
    summary: gtk::Label,
}

impl FriendsDialog {
    pub fn open(w: &Window, of: UserId) {
        let modal = Modal::new(
            "person_search",
            "Join a friend",
            "Selected accounts launch into their server",
            520,
        );
        let d = Rc::new(FriendsDialog {
            window: w.weak(),
            modal,
            cache: RefCell::default(),
            of: Cell::new(of),
            query: RefCell::default(),
            chips: wrap(6, &[]),
            list: vbox!(2, "flist"),
            summary: lbl("", "fsum").hexpand(),
        });
        let search = gtk::Entry::builder().placeholder_text("Search friends").hexpand(true).build();
        let me = Rc::downgrade(&d);
        search.connect_changed(move |e| {
            if let Some(d) = me.upgrade() {
                d.query.replace(e.text().trim().to_lowercase());
                d.show();
            }
        });
        // A floor, not only a ceiling: the dialog is sized while the list is
        // still a spinner and does not grow when it fills.
        let scroller = gtk::ScrolledWindow::builder()
            .child(&d.list)
            .min_content_height(280)
            .max_content_height(340)
            .propagate_natural_height(true)
            .hscrollbar_policy(PolicyType::Never)
            .build();
        let close = {
            let dialog = d.modal.dialog.clone();
            Btn::new("cancel").text("Close").build(move || {
                dialog.close();
            })
        };
        d.modal.build(
            &vbox!(
                0,
                "",
                vbox!(
                    12,
                    "fbody",
                    d.chips.clone(),
                    hbox!(8, "search", icon("search", 18, ""), search)
                ),
                scroller
            ),
            &hbox!(10, "mfoot", d.summary.clone(), close.button),
        );
        d.load(of);
        d.modal.present(w.gtk_window());
    }

    fn load(self: &Rc<Self>, of: UserId) {
        self.of.set(of);
        let Some(w) = self.window.upgrade() else { return };
        clear_wrap(&self.chips);
        self.chips.append(&lbl("Friends of", "fof").centered());
        let accounts: Vec<(UserId, rbxmgr_core::types::Label)> =
            w.state().accounts.accounts().iter().map(|a| (a.user_id, a.name.clone())).collect();
        for (id, name) in &accounts {
            let me = Rc::downgrade(self);
            let id = *id;
            let b = Btn::new(if id == of { "fchip on" } else { "fchip" })
                .text(name.as_str())
                .build(move || {
                    if let Some(d) = me.upgrade() {
                        d.load(id);
                    }
                });
            self.chips.append(&b.button);
        }
        if self.cache.borrow().contains_key(&of) {
            return self.show();
        }
        clear(&self.list);
        let spinner = gtk::Spinner::new();
        spinner.start();
        spinner.set_halign(Align::Center);
        spinner.set_margin_top(24);
        spinner.set_margin_bottom(24);
        self.list.append(&spinner);
        self.summary.set_label("Loading…");
        let Some(label) = accounts.iter().find(|(id, _)| *id == of).map(|(_, l)| l.clone()) else {
            return;
        };
        let (keyring, roblox) = (w.services().keyring.clone(), w.services().roblox.clone());
        let me = Rc::downgrade(self);
        worker::run(
            move || {
                let cookie = keyring.cookie(&label).map_err(|e| e.to_string())?;
                roblox.friends(&cookie, of).map_err(|e| e.to_string())
            },
            move |got| {
                if let Some(d) = me.upgrade() {
                    d.cache.borrow_mut().insert(of, got);
                    if d.of.get() == of {
                        d.show();
                    }
                }
            },
        );
    }

    fn show(self: &Rc<Self>) {
        clear(&self.list);
        let cache = self.cache.borrow();
        let Some(got) = cache.get(&self.of.get()) else { return };
        let friends = match got {
            Ok(f) => f,
            Err(e) => {
                self.summary.set_label("");
                self.list.append(
                    &lbl(&format!("Could not load friends: {e}"), "nofriends")
                        .wrapped()
                        .xalign(0.5),
                );
                return;
            }
        };
        let in_game = friends.iter().filter(|f| f.state == FriendState::Game).count();
        let online = friends.iter().filter(|f| f.state == FriendState::Online).count();
        self.summary.set_label(&format!("{in_game} in game · {online} online"));
        let query = self.query.borrow().clone();
        let chosen = self.window.upgrade().and_then(|w| w.state().friend.as_ref().map(|f| f.user));
        let shown: Vec<&Friend> = friends
            .iter()
            .filter(|f| {
                query.is_empty()
                    || f.name.to_lowercase().contains(&query)
                    || f.display.to_lowercase().contains(&query)
            })
            .collect();
        for f in &shown {
            let class = state_class(f.state);
            let picked = chosen == Some(f.id);
            let row = hbox!(
                12,
                if picked { "friend sel" } else { "friend" },
                gtk::Box::new(gtk::Orientation::Horizontal, 0)
                    .css(&format!("fdot {class}"))
                    .centered(),
                vbox!(
                    2,
                    "",
                    lbl(&f.display, &format!("frname {class}")).ellipsize(),
                    lbl(&status_text(f), &format!("frstatus {class}")).ellipsize()
                )
                .hexpand()
                .centered()
            )
            .tip(&format!("@{}", f.name));
            if let Some(target) = FriendTarget::of(f) {
                let me = Rc::downgrade(self);
                let join = Btn::new(if picked { "join chosen" } else { "join" })
                    .text(if picked { "Selected" } else { "Join" })
                    .icon(if picked { "check" } else { "login" })
                    .size(17)
                    .gap(5)
                    .build(move || {
                        if let Some(d) = me.upgrade() {
                            d.modal.dialog.close();
                            if let Some(w) = d.window.upgrade() {
                                w.join_friend(target.clone());
                            }
                        }
                    });
                row.append(&join.button.centered());
            }
            self.list.append(&row);
        }
        if shown.is_empty() {
            let text =
                if query.is_empty() { "No friends yet" } else { "No friends match that search" };
            self.list.append(&lbl(text, "nofriends").xalign(0.5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbxmgr_core::types::ServerId;

    fn friend(state: FriendState, game: Option<&str>, server: Option<&str>) -> Friend {
        Friend {
            id: UserId(1),
            name: "pal".into(),
            display: "Pal".into(),
            state,
            place: None,
            server: server.map(|s| ServerId::parse(s).unwrap()),
            game: game.map(str::to_owned),
        }
    }

    #[test]
    fn a_friends_status_says_where_they_are() {
        assert_eq!(status_text(&friend(FriendState::Game, Some("Obby"), Some("s-1"))), "In Obby");
        assert_eq!(
            status_text(&friend(FriendState::Game, Some("Obby"), None)),
            "In Obby · server hidden"
        );
        assert_eq!(status_text(&friend(FriendState::Online, None, None)), "Online");
        assert_eq!(status_text(&friend(FriendState::Offline, None, None)), "Offline");
    }
}
