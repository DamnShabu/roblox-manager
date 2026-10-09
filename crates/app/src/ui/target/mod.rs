//! Where launches go, picked in the popover under the top bar's target:
//! Roblox's games browser, a favourite game, or (on the Friends tab) a
//! friend's server. The only places on offer are ones an account
//! favourited, so there is nothing to mistype.

mod friends;

use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};
use rbxmgr_core::types::PlaceId;

use crate::state::AppState;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, name, toggle_class};
use crate::ui::window::{WeakWindow, Window};

pub struct TargetPopover {
    pub popover: gtk::Popover,
    games_tab: gtk::ToggleButton,
    friends_tab: gtk::ToggleButton,
    games: gtk::Box,
    filter: gtk::SearchEntry,
    friends: Rc<friends::FriendsPage>,
    window: WeakWindow,
}

impl TargetPopover {
    pub fn build(w: &WeakWindow) -> Rc<Self> {
        let games_tab = tab("gamepad", "Games");
        games_tab.set_active(true);
        let friends_tab = tab("users", "Friends");
        friends_tab.set_group(Some(&games_tab));
        let reload = ds::ib("refresh", "Reload favourites and friends", true);
        reload.set_action_name(Some("win.reload-games"));
        let tabs = gtk::Box::new(Orientation::Horizontal, 4).css("cx-tabs");
        tabs.append(&games_tab);
        tabs.append(&friends_tab);
        tabs.append(&gtk::Box::new(Orientation::Horizontal, 0).hexpand());
        tabs.append(&reload);

        let filter = gtk::SearchEntry::builder()
            .placeholder_text("Filter favourites")
            .css_classes(["search-field"])
            .build();
        let games = gtk::Box::new(Orientation::Vertical, 0).css("tp-list");
        let games_page = gtk::Box::new(Orientation::Vertical, 0);
        let bar = gtk::Box::new(Orientation::Horizontal, 8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_top(8);
        bar.set_margin_bottom(8);
        bar.append(&filter.clone().hexpand());
        games_page.append(&bar);
        games_page.append(&scroll(&games));
        let friends = friends::FriendsPage::build(w);
        let stack = gtk::Stack::builder().vhomogeneous(false).build();
        stack.add_named(&games_page, Some("games"));
        stack.add_named(&friends.root, Some("friends"));
        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.set_size_request(400, -1);
        root.append(&tabs);
        root.append(&stack);
        let popover = gtk::Popover::builder().child(&root).has_arrow(false).build();
        popover.add_css_class("rm-pop");
        popover.set_halign(Align::Start);
        let me = Rc::new(TargetPopover {
            popover,
            games_tab,
            friends_tab,
            games,
            filter,
            friends,
            window: w.clone(),
        });
        {
            let (stack, me2) = (stack.clone(), Rc::downgrade(&me));
            me.games_tab.connect_toggled(move |t| {
                if t.is_active() {
                    stack.set_visible_child_name("games");
                    if let Some(me) = me2.upgrade() {
                        me.filter.grab_focus();
                    }
                }
            });
        }
        {
            let (stack, me2) = (stack, Rc::downgrade(&me));
            me.friends_tab.connect_toggled(move |t| {
                if t.is_active() {
                    stack.set_visible_child_name("friends");
                    if let Some(me) = me2.upgrade() {
                        me.friends.shown();
                    }
                }
            });
        }
        {
            let me2 = Rc::downgrade(&me);
            me.filter.connect_search_changed(move |_| {
                if let Some(me) = me2.upgrade()
                    && let Some(w) = me.window.upgrade()
                {
                    me.draw(&w.state());
                }
            });
        }
        me
    }

    /// Open on the Friends tab.
    pub fn show_friends(&self) {
        self.friends_tab.set_active(true);
        self.popover.popup();
    }

    /// Redraw the games list from the state.
    pub fn draw(&self, s: &AppState) {
        clear(&self.games);
        let query = self.filter.text().trim().to_lowercase();
        let mut uses: HashMap<&PlaceId, Vec<&str>> = HashMap::new();
        for g in s.accounts.groups() {
            if let Some(p) = &g.place_id {
                uses.entry(p).or_default().push(if g.name.is_empty() {
                    "Untitled group"
                } else {
                    &g.name
                });
            }
        }
        if query.is_empty() {
            let browser = self.row(
                &ds::art_icon("grid", 28),
                "Roblox games browser",
                "Open Roblox's own games browser, and pick there",
                s.friend.is_none() && s.place.is_none(),
                {
                    let w = self.window.clone();
                    move || {
                        if let Some(w) = w.upgrade() {
                            w.pick_game(None);
                        }
                    }
                },
            );
            self.games.append(&browser);
        }
        let shown: Vec<_> = s
            .game_list
            .iter()
            .filter(|t| query.is_empty() || t.game.name.to_lowercase().contains(&query))
            .collect();
        let head = gtk::Box::new(Orientation::Horizontal, 8).css("tp-sub");
        head.append(&ds::overline("Favourites"));
        head.append(&lbl(&shown.len().to_string(), "t-overline"));
        self.games.append(&head);
        for t in shown {
            let place = t.game.place_id.clone();
            let sub = uses
                .get(&place)
                .map_or_else(|| "Favourite".to_owned(), |u| format!("Used by {}", u.join(", ")));
            let row = self.row(
                &ds::art(&t.game.name, t.icon.as_deref(), 28),
                &t.game.name,
                &sub,
                s.friend.is_none() && s.place.as_ref() == Some(&place),
                {
                    let w = self.window.clone();
                    move || {
                        if let Some(w) = w.upgrade() {
                            w.pick_game(Some(place.clone()));
                        }
                    }
                },
            );
            self.games.append(&row);
        }
        if s.game_list.is_empty() {
            let empty = gtk::Box::new(Orientation::Vertical, 8).css("cx-empty");
            empty.append(&lbl("No favourites yet", "cx-empty-t").xalign(0.5));
            empty.append(
                &lbl("Favourite a game on roblox.com, then reload.", "").wrapped().xalign(0.5),
            );
            self.games.append(&empty);
        }
        self.friends.target_changed();
    }

    fn row(
        &self,
        art: &gtk::Widget,
        title: &str,
        sub: &str,
        on: bool,
        pick: impl Fn() + 'static,
    ) -> gtk::Button {
        let text = gtk::Box::new(Orientation::Vertical, 0);
        text.append(&lbl(title, "tp-row-t").ellipsize());
        text.append(&lbl(sub, "tp-row-s").ellipsize());
        let inner = gtk::Box::new(Orientation::Horizontal, 12);
        inner.append(art);
        inner.append(&text.hexpand().centered());
        let check = ds::icon("check").css("check s16");
        check.set_visible(on);
        inner.append(&check);
        let b = gtk::Button::builder().child(&inner).css_classes(["tp-row"]).build();
        toggle_class(&b, "on", on);
        name(&b, title);
        let pop = self.popover.downgrade();
        b.connect_clicked(move |_| {
            pick();
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
        });
        b
    }
}

fn tab(icon: &str, text: &str) -> gtk::ToggleButton {
    let inner = gtk::Box::new(Orientation::Horizontal, 8);
    inner.append(&ds::icon(icon).css("s16"));
    inner.append(&lbl(text, ""));
    let t = gtk::ToggleButton::builder().child(&inner).build();
    t.add_css_class("cx-tab");
    t
}

pub(crate) fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(440)
        .build()
}

/// The top bar's target and recent targets, from the state.
pub fn draw_bar(w: &Window, art: &gtk::Box, name_label: &gtk::Label, recents: &gtk::Box) {
    let s = w.state();
    clear(art);
    let (pic, text) = match (&s.friend, &s.place) {
        (Some(f), _) => (ds::art_icon("user", 28), format!("{} · {}", f.display, f.game)),
        (None, Some(p)) => match s.game_list.iter().find(|t| &t.game.place_id == p) {
            Some(t) => (ds::art(&t.game.name, t.icon.as_deref(), 28), t.game.name.clone()),
            None => (ds::art_icon("gamepad", 28), format!("Place {p}")),
        },
        (None, None) => (ds::art_icon("grid", 28), "Games browser".to_owned()),
    };
    art.append(&pic);
    name_label.set_label(&text);
    clear(recents);
    for t in s.recents() {
        let place = t.game.place_id.clone();
        let on = s.friend.is_none() && s.place.as_ref() == Some(&place);
        let b = gtk::Button::builder()
            .child(&ds::art(&t.game.name, t.icon.as_deref(), 28))
            .css_classes(["mn-recent-b"])
            .tooltip_text(format!("Launch into {}", t.game.name))
            .valign(Align::Center)
            .build();
        name(&b, &t.game.name);
        toggle_class(&b, "on", on);
        let weak = w.weak();
        b.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                w.pick_game(Some(place.clone()));
            }
        });
        recents.append(&b);
    }
}
