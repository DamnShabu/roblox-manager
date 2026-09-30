//! Join a friend: every friend of one account, with where they are. Join
//! makes a friend's server the launch target.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType, glib};
use rbxmgr_core::roblox::{Friend, FriendState, Roblox};
use rbxmgr_core::types::{Label, UserId};

use super::widgets::{Btn, Fluent, boxed_list, lbl};
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
    dialog: adw::Dialog,
    /// Each account's friends, once loaded (or why they did not load).
    cache: RefCell<HashMap<UserId, Result<Vec<Friend>, String>>>,
    of: Cell<UserId>,
    accounts: Vec<(UserId, Label)>,
    query: RefCell<String>,
    title: adw::WindowTitle,
    pages: gtk::Stack,
    list: gtk::ListBox,
    problem: adw::StatusPage,
}

impl FriendsDialog {
    pub fn open(w: &Window, of: UserId) {
        let accounts: Vec<(UserId, Label)> =
            w.state().accounts.accounts().iter().map(|a| (a.user_id, a.name.clone())).collect();
        let d = Rc::new(FriendsDialog {
            window: w.weak(),
            dialog: adw::Dialog::builder()
                .title("Join a Friend")
                .content_width(520)
                .content_height(620)
                .build(),
            cache: RefCell::default(),
            of: Cell::new(of),
            accounts,
            query: RefCell::default(),
            title: adw::WindowTitle::new("Join a Friend", ""),
            pages: gtk::Stack::new(),
            list: boxed_list(),
            problem: adw::StatusPage::builder().icon_name("network-offline-symbolic").build(),
        });
        d.assemble();
        d.load(of);
        let held = RefCell::new(Some(d.clone()));
        d.dialog.connect_closed(move |_| {
            held.take();
        });
        d.dialog.present(Some(w.gtk_window()));
    }

    fn assemble(self: &Rc<Self>) {
        let names: Vec<&str> = self.accounts.iter().map(|(_, l)| l.as_str()).collect();
        let picker = gtk::DropDown::from_strings(&names);
        picker.set_tooltip_text(Some("Whose friends to list"));
        picker.set_selected(
            self.accounts.iter().position(|(id, _)| *id == self.of.get()).unwrap_or(0) as u32,
        );
        let me = Rc::downgrade(self);
        picker.connect_selected_notify(move |p| {
            if let Some(d) = me.upgrade() {
                if let Some((id, _)) = d.accounts.get(p.selected() as usize) {
                    d.load(*id);
                }
            }
        });
        let search =
            gtk::SearchEntry::builder().placeholder_text("Search friends").hexpand(true).build();
        let me = Rc::downgrade(self);
        search.connect_search_changed(move |e| {
            if let Some(d) = me.upgrade() {
                d.query.replace(e.text().trim().to_lowercase());
                d.show();
            }
        });
        let bar = hbox!(8, "", lbl("Friends of", "dimmed").centered(), picker, search);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_bottom(8);

        let spinner = adw::Spinner::builder().width_request(32).height_request(32).build();
        spinner.set_halign(Align::Center);
        spinner.set_valign(Align::Center);
        self.pages.add_named(&spinner, Some("loading"));
        self.pages.add_named(&self.problem, Some("problem"));
        let page = vbox!(
            10,
            "",
            lbl("Join makes their server the launch target.", "caption dimmed"),
            self.list.clone()
        )
        .margins(12);
        self.pages.add_named(
            &gtk::ScrolledWindow::builder()
                .child(&adw::Clamp::builder().maximum_size(600).child(&page).build())
                .hscrollbar_policy(PolicyType::Never)
                .vexpand(true)
                .build(),
            Some("list"),
        );

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&self.title));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.add_top_bar(&bar);
        view.set_content(Some(&self.pages));
        self.dialog.set_child(Some(&view));
        self.dialog.set_focus(Some(&search));
    }

    fn load(self: &Rc<Self>, of: UserId) {
        self.of.set(of);
        if self.cache.borrow().contains_key(&of) {
            return self.show();
        }
        self.pages.set_visible_child_name("loading");
        self.title.set_subtitle("Loading…");
        let Some(w) = self.window.upgrade() else { return };
        let Some(label) = self.accounts.iter().find(|(id, _)| *id == of).map(|(_, l)| l.clone())
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
                    if d.of.get() == of {
                        d.show();
                    }
                }
            },
        );
    }

    fn show(self: &Rc<Self>) {
        self.list.remove_all();
        let cache = self.cache.borrow();
        let Some(got) = cache.get(&self.of.get()) else { return };
        let friends = match got {
            Ok(f) => f,
            Err(e) => {
                self.title.set_subtitle("");
                self.problem.set_title("Could Not Load Friends");
                self.problem.set_description(Some(&glib::markup_escape_text(e)));
                self.pages.set_visible_child_name("problem");
                return;
            }
        };
        let in_game = friends.iter().filter(|f| f.state == FriendState::Game).count();
        let online = friends.iter().filter(|f| f.state == FriendState::Online).count();
        self.title.set_subtitle(&format!("{in_game} in a game · {online} online"));
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
        if shown.is_empty() {
            self.problem.set_icon_name(Some(if query.is_empty() {
                "avatar-default-symbolic"
            } else {
                "system-search-symbolic"
            }));
            self.problem.set_title(if query.is_empty() { "No Friends Yet" } else { "No Matches" });
            self.problem.set_description(Some(if query.is_empty() {
                "Friends of this account show up here."
            } else {
                "No friend's name has that in it."
            }));
            self.pages.set_visible_child_name("problem");
            return;
        }
        self.problem.set_icon_name(Some("network-offline-symbolic"));
        for f in shown {
            let class = state_class(f.state);
            let picked = chosen == Some(f.id);
            let row = adw::ActionRow::builder()
                .title(&f.display)
                .subtitle(format!("@{} · {}", f.name, status_text(f)))
                .use_markup(false)
                .build();
            let dot =
                gtk::Box::new(gtk::Orientation::Horizontal, 0).css(&format!("presence {class}"));
            dot.set_valign(Align::Center);
            row.add_prefix(&dot);
            if let Some(target) = FriendTarget::of(f) {
                let me = Rc::downgrade(self);
                let join = Btn::new(if picked { "flat" } else { "suggested-action" })
                    .text(if picked { "Chosen" } else { "Join" })
                    .build(move || {
                        if let Some(d) = me.upgrade() {
                            d.dialog.close();
                            if let Some(w) = d.window.upgrade() {
                                w.join_friend(target.clone());
                            }
                        }
                    });
                join.button.set_valign(Align::Center);
                row.add_suffix(&join.button);
            }
            self.list.append(&row);
        }
        self.pages.set_visible_child_name("list");
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
