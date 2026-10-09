//! The target popover's Friends tab: every friend of one account, in a
//! game, online or offline. A friend in a game with a visible server can be
//! made the target, or launched straight into.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation};
use rbxmgr_core::roblox::{Friend, FriendState, Roblox};
use rbxmgr_core::types::{Label, UserId};

use super::scroll;
use crate::state::FriendTarget;
use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, sentence};
use crate::ui::window::WeakWindow;
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

pub struct FriendsPage {
    pub root: gtk::Box,
    window: WeakWindow,
    /// Each account's friends, once loaded (or why they did not load).
    cache: RefCell<HashMap<UserId, Result<Vec<Friend>, String>>>,
    of: Cell<Option<UserId>>,
    accounts: RefCell<Vec<(UserId, Label)>>,
    picker: gtk::DropDown,
    counts: gtk::Label,
    query: RefCell<String>,
    list: gtk::Box,
}

impl FriendsPage {
    pub fn build(w: &WeakWindow) -> Rc<Self> {
        let picker = gtk::DropDown::from_strings(&[]);
        picker.add_css_class("cx-select");
        picker.set_tooltip_text(Some("Whose friends to list"));
        let counts = lbl("", "t-caption muted");
        let top = gtk::Box::new(Orientation::Horizontal, 8);
        top.append(&lbl("Friends of", "cx-lab").centered());
        top.append(&picker);
        top.append(&counts.clone().hexpand().xalign(1.0));
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search friends")
            .css_classes(["search-field"])
            .build();
        let head = gtk::Box::new(Orientation::Vertical, 8);
        head.set_margin_start(12);
        head.set_margin_end(12);
        head.set_margin_top(8);
        head.set_margin_bottom(8);
        head.append(&top);
        head.append(&search);
        let list = gtk::Box::new(Orientation::Vertical, 0).css("tp-list");
        let foot = gtk::Box::new(Orientation::Vertical, 4).css("tp-foot");
        foot.append(&lbl("Join makes their server the launch target.", ""));
        let keys = gtk::Box::new(Orientation::Horizontal, 6);
        keys.set_halign(Align::End);
        keys.append(&ds::keys(&["Ctrl", "Enter"]));
        keys.append(&lbl("launches the chain there", ""));
        foot.append(&keys);
        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&head);
        root.append(&scroll(&list));
        root.append(&foot);
        let me = Rc::new(FriendsPage {
            root,
            window: w.clone(),
            cache: RefCell::default(),
            of: Cell::new(None),
            accounts: RefCell::default(),
            picker,
            counts,
            query: RefCell::default(),
            list,
        });
        let weak = Rc::downgrade(&me);
        me.picker.connect_selected_notify(move |p| {
            if let Some(me) = weak.upgrade() {
                let id = me.accounts.borrow().get(p.selected() as usize).map(|(id, _)| *id);
                if let Some(id) = id {
                    me.load(id);
                }
            }
        });
        let weak = Rc::downgrade(&me);
        search.connect_search_changed(move |e| {
            if let Some(me) = weak.upgrade() {
                me.query.replace(e.text().trim().to_lowercase());
                me.show();
            }
        });
        me
    }

    /// The tab was opened: list the accounts, and the leader's friends.
    pub fn shown(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let (accounts, first) = {
            let s = w.state();
            let a = &s.accounts;
            let all: Vec<(UserId, Label)> =
                a.accounts().iter().map(|a| (a.user_id, a.name.clone())).collect();
            let first = a.leader().or_else(|| a.accounts().first()).map(|a| a.user_id);
            (all, first)
        };
        let names: Vec<&str> = accounts.iter().map(|(_, l)| l.as_str()).collect();
        self.picker.set_model(Some(&gtk::StringList::new(&names)));
        let of = self.of.get().filter(|o| accounts.iter().any(|(id, _)| id == o)).or(first);
        let at = of.and_then(|o| accounts.iter().position(|(id, _)| *id == o));
        self.accounts.replace(accounts);
        match (of, at) {
            (Some(of), Some(at)) => {
                self.picker.set_selected(at as u32);
                self.load(of);
            }
            _ => {
                clear(&self.list);
                self.list.append(&empty("No accounts yet", "Friends come from your accounts."));
            }
        }
    }

    /// The target changed elsewhere: redraw which friend is chosen.
    pub fn target_changed(self: &Rc<Self>) {
        if self.of.get().is_some() {
            self.show();
        }
    }

    fn load(self: &Rc<Self>, of: UserId) {
        self.of.set(Some(of));
        if self.cache.borrow().contains_key(&of) {
            return self.show();
        }
        clear(&self.list);
        let spinner = adw::Spinner::builder().width_request(24).height_request(24).build();
        spinner.set_margin_top(24);
        spinner.set_margin_bottom(24);
        self.list.append(&spinner);
        self.counts.set_label("Loading…");
        let Some(w) = self.window.upgrade() else { return };
        let Some(label) =
            self.accounts.borrow().iter().find(|(id, _)| *id == of).map(|(_, l)| l.clone())
        else {
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
                    if d.of.get() == Some(of) {
                        d.show();
                    }
                }
            },
        );
    }

    fn show(self: &Rc<Self>) {
        clear(&self.list);
        let Some(of) = self.of.get() else { return };
        let cache = self.cache.borrow();
        let Some(got) = cache.get(&of) else { return };
        let friends = match got {
            Ok(f) => f,
            Err(e) => {
                self.counts.set_label("");
                self.list.append(&empty("Could not load friends", &sentence(e)));
                return;
            }
        };
        let in_game = friends.iter().filter(|f| f.state == FriendState::Game).count();
        let online = friends.iter().filter(|f| f.state == FriendState::Online).count();
        self.counts.set_label(&format!("{in_game} in a game · {online} online"));
        let query = self.query.borrow().clone();
        let (chosen, chain) = match self.window.upgrade() {
            Some(w) => {
                let s = w.state();
                let chain = s.accounts.leader().map(|l| match s.accounts.followers().len() {
                    0 => format!("Launch {} there", l.name),
                    n => format!("Launch {} + {n} there", l.name),
                });
                (s.friend.as_ref().map(|f| f.user), chain)
            }
            None => (None, None),
        };
        let matches = |f: &&Friend| {
            query.is_empty()
                || f.name.to_lowercase().contains(&query)
                || f.display.to_lowercase().contains(&query)
        };
        let mut any = false;
        for (state, title) in [
            (FriendState::Game, "In a game"),
            (FriendState::Online, "Online"),
            (FriendState::Offline, "Offline"),
        ] {
            let these: Vec<&Friend> =
                friends.iter().filter(|f| f.state == state).filter(matches).collect();
            if these.is_empty() {
                continue;
            }
            any = true;
            let head = gtk::Box::new(Orientation::Horizontal, 8).css("tp-sub");
            head.append(&ds::overline(title));
            head.append(&lbl(&these.len().to_string(), "t-overline"));
            self.list.append(&head);
            for f in these {
                self.list.append(&self.friend_row(f, chosen == Some(f.id), chain.as_deref()));
            }
        }
        if !any {
            self.list.append(&if query.is_empty() {
                empty("No friends yet", "Friends of this account show up here.")
            } else {
                empty("No matches", "No friend's name has that in it.")
            });
        }
    }

    fn friend_row(self: &Rc<Self>, f: &Friend, chosen: bool, chain: Option<&str>) -> gtk::Box {
        let class = match f.state {
            FriendState::Game => "game",
            FriendState::Online => "online",
            FriendState::Offline => "offline",
        };
        let over = gtk::Overlay::new();
        over.set_child(Some(&ds::av(&f.display, None, 32)));
        let dot = gtk::Box::new(Orientation::Horizontal, 0).css(&format!("presence {class}"));
        dot.set_halign(Align::End);
        dot.set_valign(Align::End);
        over.add_overlay(&dot);
        let text = gtk::Box::new(Orientation::Vertical, 0);
        text.append(&lbl(&f.display, "tp-row-t").ellipsize());
        text.append(&lbl(&format!("@{} · {}", f.name, status_text(f)), "tp-row-s").ellipsize());
        let row = gtk::Box::new(Orientation::Horizontal, 12).css("tp-row");
        row.set_margin_start(12);
        row.set_margin_end(12);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.append(&over);
        row.append(&text.hexpand().centered());
        match FriendTarget::of(f) {
            Some(target) if chosen => {
                row.append(&ds::badge(ds::Tone::Accent, "Target", false, false));
                if let Some(chain) = chain {
                    row.append(&self.launch_there(chain, target));
                }
            }
            Some(target) => {
                let me = Rc::downgrade(self);
                let t = target.clone();
                let set = ds::Button::new("Set target", Variant::Secondary, true).on(move || {
                    if let Some(me) = me.upgrade()
                        && let Some(w) = me.window.upgrade()
                    {
                        w.join_friend(t.clone());
                    }
                });
                set.button.set_valign(Align::Center);
                row.append(&set.button);
                if let Some(chain) = chain {
                    row.append(&self.launch_there(chain, target));
                }
            }
            None if f.state == FriendState::Game => {
                let hidden = gtk::Box::new(Orientation::Horizontal, 4);
                hidden.append(&ds::icon("eye-off").css("muted s14"));
                hidden.append(&lbl("Their server is hidden", "t-caption muted"));
                row.append(&hidden.centered());
            }
            None => {}
        }
        row
    }

    fn launch_there(self: &Rc<Self>, label: &str, target: FriendTarget) -> gtk::Button {
        let me = Rc::downgrade(self);
        let b = ds::Button::new(label, Variant::Primary, true).on(move || {
            if let Some(me) = me.upgrade()
                && let Some(w) = me.window.upgrade()
            {
                w.join_friend(target.clone());
                w.launch_chain();
            }
        });
        b.button.set_valign(Align::Center);
        b.button
    }
}

fn empty(title: &str, text: &str) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Vertical, 8).css("cx-empty");
    b.append(&lbl(title, "cx-empty-t").xalign(0.5));
    b.append(&lbl(text, "").wrapped().xalign(0.5));
    b
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
