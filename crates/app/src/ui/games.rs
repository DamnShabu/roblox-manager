//! The game strip: Roblox's games browser, Join a friend, and the accounts'
//! favourite games, in one row that scrolls sideways. The only places on
//! offer are ones an account favourited, so there is nothing to mistype.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::{Align, PolicyType};

use super::widgets::{Fluent, LabelFluent, clear, icon, lbl, thumb};
use super::window::WeakWindow;
use crate::state::AppState;
use rbxmgr_core::types::PlaceId;

/// A tile's picture, in px.
const ART: i32 = 84;

pub struct GameBar {
    pub root: gtk::Box,
    tiles: gtk::Box,
    scroller: gtk::ScrolledWindow,
    hint: gtk::Label,
    window: WeakWindow,
}

impl GameBar {
    pub fn new(window: WeakWindow) -> Self {
        let tiles = hbox!(4, "game-strip");
        let scroller = gtk::ScrolledWindow::builder()
            .child(&tiles)
            .hscrollbar_policy(PolicyType::Automatic)
            .vscrollbar_policy(PolicyType::Never)
            .build();
        // Arrows at either end while there is more that way: a mouse wheel
        // scrolls the page, not the strip.
        let back = arrow("go-previous-symbolic", "Scroll Back", Align::Start);
        let on = arrow("go-next-symbolic", "Scroll On", Align::End);
        let over = gtk::Overlay::new();
        over.set_child(Some(&scroller));
        over.add_overlay(&back);
        over.add_overlay(&on);
        let adj = scroller.hadjustment();
        let step = |dir: f64| {
            let adj = adj.clone();
            move |_: &gtk::Button| {
                let to = adj.value() + dir * (adj.page_size() * 0.8).max(ART as f64);
                adj.set_value(to.clamp(adj.lower(), adj.upper() - adj.page_size()));
            }
        };
        back.connect_clicked(step(-1.0));
        on.connect_clicked(step(1.0));
        let update = {
            let (back, on) = (back.clone(), on.clone());
            move |adj: &gtk::Adjustment| {
                back.set_visible(adj.value() > adj.lower() + 1.0);
                on.set_visible(adj.value() + adj.page_size() < adj.upper() - 1.0);
            }
        };
        update(&adj);
        adj.connect_changed(update.clone());
        adj.connect_value_changed(update);

        let hint = lbl(
            "No favourites yet: favourite a game on roblox.com, then reload.",
            "caption dimmed",
        )
        .wrapped()
        .visible(false);
        hint.set_margin_start(6);
        let root = vbox!(4, "", over, hint.clone());
        GameBar { root, tiles, scroller, hint, window }
    }

    /// Redraw from the state, keeping the scroll position. Main thread only.
    pub fn draw(&self, s: &AppState) {
        let kept = self.scroller.hadjustment().value();
        clear(&self.tiles);
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
        let friend = s.friend.as_ref();
        self.tiles.append(&self.tile(
            "Games browser",
            "Roblox's own list",
            Art::Symbol("view-app-grid-symbolic"),
            friend.is_none() && s.place.is_none(),
            "Open Roblox's own games browser, and pick there",
            self.window.act(|w| w.pick_game(None)),
        ));
        let (f_name, f_meta, f_tip) = match friend {
            Some(f) => (
                f.display.as_str(),
                format!("in {}", f.game),
                "Launches join this friend's server. Click to pick another friend.",
            ),
            None => {
                ("Join a friend", "Their server".to_owned(), "Send accounts into a friend's server")
            }
        };
        self.tiles.append(&self.tile(
            f_name,
            &f_meta,
            Art::Symbol("avatar-default-symbolic"),
            friend.is_some(),
            f_tip,
            self.window.act(|w| w.on_friends()),
        ));
        for t in &s.game_list {
            let place = t.game.place_id.clone();
            let meta = uses.get(&place).map_or_else(|| "Favourite".to_owned(), |u| u.join(", "));
            self.tiles.append(&self.tile(
                &t.game.name,
                &meta,
                Art::Icon(t.icon.as_deref()),
                friend.is_none() && s.place.as_ref() == Some(&place),
                &t.game.name,
                self.window.act(move |w| w.pick_game(Some(place.clone()))),
            ));
        }
        self.hint.set_visible(s.game_list.is_empty());
        let adj = self.scroller.hadjustment();
        glib_idle(move || adj.set_value(kept));
    }

    fn tile(
        &self,
        name: &str,
        meta: &str,
        art: Art<'_>,
        selected: bool,
        tip: &str,
        on_click: impl Fn() + 'static,
    ) -> gtk::Button {
        let pic: gtk::Widget = match art {
            Art::Symbol(ic) => {
                let frame = gtk::CenterBox::new();
                frame.add_css_class("tile-art");
                frame.add_css_class("symbol");
                frame.set_size_request(ART, ART);
                let glyph = icon(ic);
                glyph.set_pixel_size(32);
                frame.set_center_widget(Some(&glyph));
                frame.upcast()
            }
            Art::Icon(path) => thumb(path, ART, "tile-art"),
        };
        pic.set_halign(Align::Center);
        let over = gtk::Overlay::new();
        over.set_child(Some(&pic));
        if selected {
            let check = icon("object-select-symbolic").css("tile-check");
            check.set_halign(Align::End);
            check.set_valign(Align::Start);
            over.add_overlay(&check);
        }
        let b = gtk::Button::new();
        b.add_css_class("flat");
        b.add_css_class("game-tile");
        if selected {
            b.add_css_class("selected");
        }
        b.set_tooltip_text(Some(tip));
        b.set_child(Some(&vbox!(
            6,
            "",
            over,
            vbox!(
                0,
                "",
                lbl(name, "tile-name").ellipsize().chars(1).xalign(0.5),
                lbl(meta, "tile-meta dimmed").ellipsize().chars(1).xalign(0.5)
            )
        )));
        b.set_width_request(ART + 20);
        b.connect_clicked(move |_| on_click());
        b
    }
}

enum Art<'a> {
    /// A symbolic icon on the accent's tint.
    Symbol(&'a str),
    /// A game's icon, or the striped placeholder.
    Icon(Option<&'a std::path::Path>),
}

fn arrow(icon_name: &str, tip: &str, side: Align) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tip)
        .halign(side)
        .valign(Align::Start)
        .css_classes(["circular", "osd"])
        .visible(false)
        .build();
    // Level with the middle of the pictures, clear of the names under them.
    b.set_margin_top(ART / 2 - 10);
    b.set_margin_start(4);
    b.set_margin_end(4);
    b
}

/// Once the redraw has been measured, so the adjustment can hold the value.
fn glib_idle(f: impl FnOnce() + 'static) {
    gtk::glib::idle_add_local_once(f);
}
