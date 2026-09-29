//! The game bar: the games browser, Join a friend, and the accounts'
//! favourite games as tiles. The only places on offer are ones an account
//! favourited, so there is nothing to mistype.

use std::collections::HashMap;

use adw::prelude::*;
use gtk::Align;

use super::widgets::{Fluent, LabelFluent, clear_wrap, icon, lbl, symbol_thumb, thumb, wrap};
use super::window::WeakWindow;
use crate::state::AppState;
use rbxmgr_core::types::PlaceId;

pub struct GameBar {
    pub root: gtk::Box,
    tiles: adw::WrapBox,
    hint: gtk::Label,
    window: WeakWindow,
}

impl GameBar {
    pub fn new(window: WeakWindow) -> Self {
        let tiles = wrap(18, &[]);
        let hint =
            lbl("No favourites yet -- favourite a game on roblox.com, then refresh.", "phint2")
                .wrapped()
                .visible(false);
        let root = vbox!(10, "", tiles.clone(), hint.clone());
        GameBar { root, tiles, hint, window }
    }

    /// Redraw from the state. Main thread only.
    pub fn draw(&self, s: &AppState) {
        clear_wrap(&self.tiles);
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
            "Opens Roblox's list",
            Look::Symbol("travel_explore", "dashed"),
            friend.is_none() && s.place.is_none(),
            "Open Roblox's own games browser",
            self.window.act(|w| w.pick_game(None)),
        ));
        let (f_name, f_meta, f_css) = match friend {
            Some(f) => (f.display.as_str(), format!("In {}", f.game), "friend"),
            None => ("Join a friend", "Pick from a friends list".to_owned(), "dashed"),
        };
        self.tiles.append(&self.tile(
            f_name,
            &f_meta,
            Look::Symbol("person_search", f_css),
            friend.is_some(),
            "Send accounts into a friend's server",
            self.window.act(|w| w.on_friends()),
        ));
        for t in &s.game_list {
            let place = t.game.place_id.clone();
            let meta = uses.get(&place).map_or_else(|| "Favourite".to_owned(), |u| u.join(", "));
            self.tiles.append(&self.tile(
                &t.game.name,
                &meta,
                Look::Icon(t.icon.as_deref()),
                friend.is_none() && s.place.as_ref() == Some(&place),
                &t.game.name,
                self.window.act(move |w| w.pick_game(Some(place.clone()))),
            ));
        }
        self.hint.set_visible(s.game_list.is_empty());
    }

    fn tile(
        &self,
        name: &str,
        meta: &str,
        look: Look<'_>,
        selected: bool,
        tip: &str,
        on_click: impl Fn() + 'static,
    ) -> gtk::Button {
        let pic: gtk::Widget = match look {
            Look::Symbol(ic, css) => {
                let (over, b) = symbol_thumb(128, &format!("gthumb {css}"), ic, 36, "light");
                if selected {
                    b.add_css_class("sel");
                }
                over.upcast()
            }
            Look::Icon(path) => {
                let b = thumb(path, 128, "gthumb");
                if selected {
                    b.add_css_class("sel");
                }
                b.upcast()
            }
        };
        let over = gtk::Overlay::new();
        over.set_child(Some(&pic));
        if selected {
            // Packed top-right inside a box that fills the overlay: placed by
            // the overlay itself, the badge got the tile-wide slot to paint.
            let badge = icon("check", 16, "bold tilecheck");
            badge.set_size_request(24, 24);
            let corner = hbox!(0, "", badge).halign(Align::End);
            corner.set_margin_top(8);
            corner.set_margin_end(8);
            over.add_overlay(&vbox!(0, "", corner));
        }
        let b = gtk::Button::new();
        b.add_css_class("b");
        b.add_css_class("gtile");
        b.set_tooltip_text(Some(tip));
        b.set_width_request(128);
        b.set_cursor_from_name(Some("pointer"));
        b.set_child(Some(&vbox!(
            10,
            "",
            over,
            vbox!(
                1,
                "",
                lbl(name, if selected { "gname sel" } else { "gname" }).ellipsize().chars(1),
                lbl(meta, "gmeta").ellipsize().chars(1)
            )
        )));
        b.connect_clicked(move |_| on_click());
        b
    }
}

enum Look<'a> {
    /// A symbol on a (dashed or friend) tile.
    Symbol(&'a str, &'a str),
    /// A game's icon, or the striped placeholder.
    Icon(Option<&'a std::path::Path>),
}
