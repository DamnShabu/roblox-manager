//! A group's band across the table -- or, for the accounts in none,
//! Ungrouped: it folds, selects every account in it, shows its game and
//! launches or stops it, and its name opens it in the inspector. Dropping
//! an account on it moves the account in.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, Group};
use rbxmgr_core::types::UserId;

use super::row::{account_row, confirm_row, context_menu, dragged_id};
use crate::state::{Inspected, Tile};
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::ds::{self, Tone, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, name, plural, toggle_class};
use crate::ui::window::Window;

pub fn group_band(w: &Window, group: Option<&Group>, members: &[Account]) -> gtk::Box {
    let gid = group.map(|g| g.id.clone());
    let (opened, tile) = {
        let s = w.state();
        let opened = group.map_or(s.ungrouped_open, |g| g.open);
        let tile: Option<Tile> = group
            .and_then(|g| g.place_id.as_ref())
            .and_then(|p| s.game_list.iter().find(|t| &t.game.place_id == p).cloned());
        (opened, tile)
    };
    let ids: Vec<UserId> = members.iter().map(|a| a.user_id).collect();
    let game_name: Option<String> = group.and_then(|g| {
        let place = g.place_id.as_ref()?;
        Some(
            tile.as_ref()
                .map(|t| t.game.name.clone())
                .or_else(|| g.game.clone())
                .unwrap_or_else(|| format!("Place {place}")),
        )
    });
    let title = group.map_or_else(
        || "Ungrouped".to_owned(),
        |g| if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() },
    );
    let band = gtk::Box::new(Orientation::Horizontal, 8).css("mn-band");

    let fold = ds::ib("chev-down", if opened { "Fold" } else { "Unfold" }, true);
    fold.add_css_class("fold");
    toggle_class(&fold, "folded", !opened);
    fold.connect_clicked({
        let (w, gid) = (w.weak(), gid.clone());
        move |_| {
            if let Some(w) = w.upgrade() {
                w.toggle_group_open(gid.clone());
            }
        }
    });
    band.append(&fold);
    let quiet = Rc::new(Cell::new(false));
    let check = ds::check(false, &format!("Select every account in {title}"));
    check.set_sensitive(!members.is_empty());
    {
        let (weak, quiet, ids) = (w.weak(), quiet.clone(), ids.clone());
        check.connect_toggled(move |c| {
            if let (false, Some(w)) = (quiet.get(), weak.upgrade()) {
                w.select_accounts(&ids, c.is_active());
            }
        });
    }
    band.append(&check);
    let art = match (&game_name, group) {
        (Some(n), _) => ds::art(n, tile.as_ref().and_then(|t| t.icon.as_deref()), 24),
        (None, Some(_)) => ds::art_icon("gamepad", 24),
        (None, None) => ds::art_icon("users", 24),
    };
    band.append(&art);
    let name_btn = gtk::Button::builder()
        .child(&lbl(&title, "").ellipsize())
        .css_classes(["mn-band-name"])
        .tooltip_text(if group.is_some() { "Its name and game" } else { "Fold or unfold" })
        .build();
    name_btn.connect_clicked({
        let (w, gid) = (w.weak(), gid.clone());
        move |_| {
            if let Some(w) = w.upgrade() {
                match &gid {
                    Some(gid) => GroupSettings::open(&w, gid),
                    None => w.toggle_group_open(None),
                }
            }
        }
    });
    band.append(&name_btn);
    band.append(&lbl(&plural(members.len(), "account", "accounts"), "mn-band-meta"));
    let running = gtk::Box::new(Orientation::Horizontal, 0);
    band.append(&running);
    band.append(&gtk::Box::new(Orientation::Horizontal, 0).hexpand());

    let launch = gid.as_ref().map(|gid| {
        let chip_inner = gtk::Box::new(Orientation::Horizontal, 4);
        chip_inner.append(&lbl(game_name.as_deref().unwrap_or("Pick a game"), "").ellipsize());
        chip_inner.append(&ds::icon("chev-down").css("s14"));
        let chip = gtk::Button::builder()
            .child(&chip_inner)
            .css_classes(["cx-chip"])
            .valign(Align::Center)
            .tooltip_text("The game Launch on this band sends every account into")
            .build();
        toggle_class(&chip, "none", game_name.is_none());
        chip.connect_clicked({
            let (w, gid) = (w.weak(), gid.clone());
            move |_| {
                if let Some(w) = w.upgrade() {
                    GroupSettings::open(&w, &gid);
                }
            }
        });
        band.append(&chip);
        let has_game = game_name.is_some();
        let (gid, ids) = (gid.clone(), ids.clone());
        let b = ds::Button::new("Launch", Variant::Secondary, true).on(w.act(move |w| {
            if w.state().any_live(&ids) {
                w.stop_group(&gid);
            } else if has_game {
                w.launch_group(&gid);
            } else {
                GroupSettings::open(w, &gid);
            }
        }));
        b.button.set_valign(Align::Center);
        band.append(&b.button);
        b
    });
    let confirm = gid.as_ref().map(|gid| {
        let n = members.len();
        let body = match n {
            0 => "It has no accounts.".to_owned(),
            1 => "Its account moves to Ungrouped; it is not removed.".to_owned(),
            n => format!("Its {n} accounts move to Ungrouped; none is removed."),
        };
        let gid = gid.clone();
        confirm_row(
            &format!("Delete {title}?"),
            &body,
            "Delete",
            w.act(move |w| w.delete_group(&gid)),
        )
    });
    if let (Some(gid), Some(confirm)) = (&gid, &confirm) {
        w.register_group_confirm(gid, confirm);
        let model = group_menu(gid);
        let more = gtk::MenuButton::builder()
            .child(&ds::icon("more"))
            .menu_model(&model)
            .valign(Align::Center)
            .css_classes(["ib", "sm", "mn-more"])
            .tooltip_text(format!("More for {title}"))
            .build();
        name(&more, &format!("More for {title}"));
        band.append(&more);
        context_menu(&band, &model);
    }

    let section = gtk::Box::new(Orientation::Vertical, 0);
    section.append(&band);
    if let Some(c) = &confirm {
        section.append(c);
    }
    if opened {
        if members.is_empty() {
            let note = gtk::Box::new(Orientation::Horizontal, 8).css("mn-reason plain");
            note.append(&lbl(
                if group.is_some() {
                    "Drag accounts onto this band to add them to the group."
                } else {
                    "Every account is in a group."
                },
                "t-caption muted",
            ));
            section.append(&note);
        }
        for a in members {
            section.append(&account_row(w, a));
        }
    }

    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let (weak, target) = (w.weak(), gid.clone());
    drop.connect_drop(move |_, value, _, _| {
        if let (Some(id), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
            let target = target.clone();
            glib::idle_add_local_once(move || w.set_group(id, target));
        }
        true
    });
    {
        let b = band.downgrade();
        drop.connect_enter(move |_, _, _| {
            if let Some(b) = b.upgrade() {
                b.add_css_class("drop");
            }
            gdk::DragAction::MOVE
        });
        let b = band.downgrade();
        drop.connect_leave(move |_| {
            if let Some(b) = b.upgrade() {
                b.remove_css_class("drop");
            }
        });
    }
    band.add_controller(drop);

    let launch_tip = game_name.as_ref().map_or_else(
        || "Pick the group's game to launch it".to_owned(),
        |n| format!("Launch every account here into {n}"),
    );
    let has_game = game_name.is_some();
    let shown = band.clone();
    w.watch_accounts(Box::new(move |s| {
        if let Some(b) = &launch {
            let live = s.any_live(&ids);
            b.set_text(if live {
                "Stop"
            } else if has_game {
                "Launch"
            } else {
                "Launch…"
            });
            b.button.set_sensitive(live || !ids.is_empty());
            b.button.set_tooltip_text(Some(if live {
                "Close every account's client here"
            } else {
                &launch_tip
            }));
        }
        let n = ids.iter().filter(|id| s.running.contains(id)).count();
        clear(&running);
        if n > 0 {
            running.append(&ds::badge(Tone::Success, &format!("{n} running"), true, false));
        }
        let chosen =
            ids.iter().filter(|id| s.accounts.get(**id).is_some_and(|a| a.selected)).count();
        quiet.set(true);
        check.set_active(chosen > 0 && chosen == ids.len());
        check.set_inconsistent(chosen > 0 && chosen < ids.len());
        quiet.set(false);
        let current =
            matches!((&s.inspected, &gid), (Some(Inspected::Group(a)), Some(b)) if a == b);
        toggle_class(&shown, "current", current);
    }));
    section
}

/// A group's menu: window actions, aimed at this group.
fn group_menu(gid: &str) -> gio::Menu {
    let item = |label: &str, action: &str| {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some(action), Some(&gid.to_variant()));
        item
    };
    let model = gio::Menu::new();
    model.append_item(&item("Name and game", "win.group-settings"));
    model.append_item(&item("Select its accounts", "win.select-group"));
    let danger = gio::Menu::new();
    danger.append_item(&item("Delete group…", "win.delete-group"));
    model.append_section(None, &danger);
    model
}
