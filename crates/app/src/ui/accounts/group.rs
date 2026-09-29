//! A group card -- or, for the accounts in none, the Ungrouped card: a header
//! that selects, launches and edits it, then its rows. Dropping an account on
//! the card moves it into the group.

use adw::prelude::*;
use gtk::{Align, gdk, glib};
use rbxmgr_core::accounts::{Account, Group};
use rbxmgr_core::types::{PlaceId, UserId};

use super::row::account_row;
use crate::state::Tile;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, armed_btn, clear, dot, icon, lbl, setting, symbol_thumb, thumb, wrap,
};
use crate::ui::window::Window;

pub fn group_card(w: &Window, group: Option<&Group>, members: &[Account]) -> gtk::Box {
    let gid = group.map(|g| g.id.clone());
    let (opened, editing, game) = {
        let s = w.state();
        let opened = group.map_or(s.ungrouped_open, |g| g.open);
        let editing = gid.is_some() && s.edit_group == gid;
        let game: Option<Tile> = group
            .and_then(|g| g.place_id.as_ref())
            .and_then(|p| s.game_list.iter().find(|t| &t.game.place_id == p).cloned());
        (opened, editing, game)
    };
    let card = vbox!(0, "card");
    card.set_overflow(gtk::Overflow::Hidden);

    let selected = members.iter().filter(|a| a.selected).count();
    let every = !members.is_empty() && selected == members.len();
    let (pic, gline, game_name): (gtk::Widget, gtk::Box, Option<String>) = match group
        .and_then(|g| g.place_id.as_ref().map(|p| (g, p)))
    {
        Some((g, place)) => {
            let name = game
                .as_ref()
                .map(|t| t.game.name.clone())
                .or_else(|| g.game.clone())
                .unwrap_or_else(|| format!("Place {place}"));
            let icon_path = game.as_ref().and_then(|t| t.icon.clone());
            (
                thumb(icon_path.as_deref(), 40, "game").upcast(),
                hbox!(5, "ggame set", icon("videogame_asset", 14, ""), lbl(&name, "").ellipsize()),
                Some(name),
            )
        }
        None => {
            let (ic, text) = if group.is_some() {
                ("sports_esports", "No game assigned")
            } else {
                ("folder_open", "Not in a group")
            };
            (
                symbol_thumb(40, "none", ic, 20).0.upcast(),
                hbox!(
                    5,
                    "ggame",
                    icon(if group.is_some() { "link_off" } else { "folder_open" }, 14, ""),
                    lbl(text, "")
                ),
                None,
            )
        }
    };
    let title = group.map_or_else(
        || "Ungrouped".to_owned(),
        |g| if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() },
    );
    let runbox = hbox!(0, "").centered();
    let ids: Vec<UserId> = members.iter().map(|a| a.user_id).collect();
    let chev = {
        let gid = gid.clone();
        Btn::new("chev")
            .icon(if opened { "expand_more" } else { "chevron_right" })
            .size(20)
            .tip(if opened { "Collapse" } else { "Expand" })
            .build(w.act(move |w| w.toggle_group_open(gid.clone())))
    };
    let check = {
        let ids = ids.clone();
        let b = Btn::new(if selected > 0 { "cbox on" } else { "cbox" })
            .size(15)
            .tip("Select every account in this group")
            .build(w.act(move |w| w.select_accounts(&ids, !every)));
        if selected > 0 {
            b.button.set_child(Some(&icon(if every { "check" } else { "remove" }, 15, "")));
        }
        b
    };
    let count = format!("{} account{}", members.len(), if members.len() == 1 { "" } else { "s" });
    let head = hbox!(
        12,
        "ghead",
        chev.button.centered(),
        check.button.centered(),
        pic,
        vbox!(
            2,
            "",
            hbox!(8, "", lbl(&title, "gtitle").ellipsize(), lbl(&count, "gcount"), runbox.clone()),
            gline
        )
        .hexpand()
        .centered()
    );
    if let Some(gid) = &gid {
        let launch = {
            let gid = gid.clone();
            Btn::new("glaunch")
                .text("Launch")
                .icon("play_arrow")
                .fill()
                .gap(4)
                .tip(&game_name.as_ref().map_or_else(
                    || "Assign a game to launch this group".to_owned(),
                    |n| format!("Launch every account in this group into {n}"),
                ))
                .build(w.act(move |w| w.launch_group(&gid)))
        };
        launch.button.set_sensitive(game_name.is_some() && !members.is_empty());
        let gear = {
            let gid = gid.clone();
            Btn::new(if editing { "setb open" } else { "setb" })
                .icon("settings")
                .size(20)
                .tip("Group settings")
                .build(w.act(move |w| w.toggle_group_edit(&gid)))
        };
        head.append(&hbox!(4, "", launch.button.centered(), gear.button.centered()));
    }
    card.append(&head);
    if let (Some(g), true) = (group, editing) {
        card.append(&editor(w, g));
    }
    if opened {
        for a in members {
            card.append(&account_row(w, a, false));
        }
        if members.is_empty() {
            card.append(
                &hbox!(
                    8,
                    "gempty",
                    icon("drag_indicator", 16, ""),
                    lbl("Drag accounts here to add them to this group", "")
                )
                .halign(Align::Fill),
            );
        }
    }

    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let (weak, target) = (w.weak(), gid.clone());
    drop.connect_drop(move |_, value, _, _| {
        let id = value.get::<String>().ok().and_then(|v| v.parse().ok()).map(UserId);
        if let (Some(id), Some(w)) = (id, weak.upgrade()) {
            let target = target.clone();
            glib::idle_add_local_once(move || w.set_group(id, target));
        }
        true
    });
    card.add_controller(drop);

    w.add_chip(Box::new(move |s| {
        let n = ids.iter().filter(|id| s.running.contains(id)).count();
        clear(&runbox);
        if n > 0 {
            runbox.append(&hbox!(
                6,
                "rchip",
                dot("running", true, 7),
                lbl(&format!("{n} running"), "")
            ));
        }
    }));
    card
}

/// A group's settings: its name, its game, deleting it.
fn editor(w: &Window, g: &Group) -> gtk::Box {
    let panel = vbox!(16, "panel group");
    let gid = g.id.clone();
    let name = gtk::Entry::builder()
        .text(&g.name)
        .placeholder_text("Group name")
        .hexpand(true)
        .max_width_chars(40)
        .build();
    name.add_css_class("field");
    let weak = w.weak();
    let renamed = gid.clone();
    name.connect_changed(move |e| {
        if let Some(w) = weak.upgrade() {
            w.rename_group(&renamed, &e.text());
        }
    });
    let weak = w.weak();
    name.connect_activate(move |_| {
        if let Some(w) = weak.upgrade() {
            w.refresh_accounts();
        }
    });
    setting(&panel, "Group name", &hbox!(0, "", name).halign(Align::Start).width(420), 10);

    let games: Vec<Tile> = w.state().game_list.clone();
    let mut options: Vec<gtk::Widget> = vec![game_option(w, &gid, g.place_id.as_ref(), None)];
    options.extend(games.iter().map(|t| game_option(w, &gid, g.place_id.as_ref(), Some(t))));
    setting(
        &panel,
        "Game",
        &vbox!(
            8,
            "",
            wrap(6, &options),
            lbl("Marks which game these accounts play. Launch on the group header opens it directly.", "phint2").wrapped()
        ),
        9,
    );
    let delete = armed_btn(
        "Delete group",
        "Click again to delete",
        "delete",
        w.act(move |w| w.delete_group(&gid)),
    );
    setting(&panel, "", &hbox!(0, "prm", delete), 0);
    panel
}

fn game_option(
    w: &Window,
    gid: &str,
    current: Option<&PlaceId>,
    tile: Option<&Tile>,
) -> gtk::Widget {
    let on = tile.map(|t| &t.game.place_id) == current;
    let b = gtk::Button::new();
    for c in ["b", "opt", "gopt"] {
        b.add_css_class(c);
    }
    if on {
        b.add_css_class("on");
    }
    b.set_cursor_from_name(Some("pointer"));
    let (pic, label): (gtk::Widget, String) = match tile {
        Some(t) => (thumb(t.icon.as_deref(), 28, "game").upcast(), t.game.name.clone()),
        None => (symbol_thumb(28, "none", "block", 16).0.upcast(), "No game".to_owned()),
    };
    b.set_child(Some(&hbox!(8, "", pic, lbl(&label, "").ellipsize().chars(26))));
    let game = tile.map(|t| (t.game.place_id.clone(), t.game.name.clone()));
    let gid = gid.to_owned();
    let act = w.act(move |w| w.set_group_game(&gid, game.clone()));
    b.connect_clicked(move |_| act());
    b.upcast()
}
