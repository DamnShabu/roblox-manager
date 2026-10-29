//! A group's row across the table -- or, for the accounts in none,
//! Ungrouped: it folds, selects every account in it, says how many run and
//! what they take together, shows its game and launches or stops it; its
//! name opens its settings. Dropping an account on it moves the account in.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, Group};
use rbxmgr_core::types::UserId;

use super::format::{bytes, percent};
use super::row::{account_row, confirm_row, context_menu, dragged_id};
use super::{Col, table_row};
use crate::state::{Inspected, Tile};
use crate::ui::accounts::group_settings::GroupSettings;
use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name, toggle_class};
use crate::ui::window::Window;

/// The width of a group's Launch or Stop, in px.
const LAUNCH_WIDTH: i32 = 92;

/// The group's row, and under it (unless folded) the rows of `shown`, the
/// members the view shows. `members` are all of them, for its counts.
pub fn group_band(
    w: &Window,
    group: Option<&Group>,
    members: &[Account],
    shown: &[Account],
) -> gtk::Box {
    let gid = group.map(|g| g.id.clone());
    let cols = w.columns();
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
    let band = table_row("mn-row grp");

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
    band.append(&cols.cell(Col::Check, &check));

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
    let art = match (&game_name, group) {
        (Some(n), Some(g)) => {
            let icon = g.place_id.as_ref().and_then(|p| w.game_icon(p, g.universe_id.as_deref()));
            ds::art(n, icon.as_deref(), 22)
        }
        (Some(n), None) => ds::art(n, None, 22),
        (None, Some(_)) => ds::art_icon("gamepad", 22),
        (None, None) => ds::art_icon("users", 22),
    };
    let name_btn = gtk::Button::builder()
        .child(&lbl(&title, "mn-grp-n").ellipsize())
        .css_classes(["mn-quiet", "plain"])
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
    let running = lbl("", "mn-user");
    let who = gtk::Box::new(Orientation::Horizontal, 8);
    who.append(&fold);
    who.append(&art);
    who.append(&name_btn);
    who.append(&running);
    band.append(&cols.cell(Col::Name, &who));

    let game = gtk::Box::new(Orientation::Horizontal, 0);
    if let Some(gid) = &gid {
        let inner = gtk::Box::new(Orientation::Horizontal, 4);
        // As wide as its column and no wider: a long name is cut, never the
        // columns after it pushed along.
        inner.append(
            &lbl(game_name.as_deref().unwrap_or("Pick a game"), "").ellipsize().chars(1).hexpand(),
        );
        inner.append(&ds::icon("chev-down").css("mn-dim s16"));
        let chip = gtk::Button::builder()
            .child(&inner)
            .css_classes(["mn-quiet"])
            .valign(Align::Center)
            .tooltip_text("The game this group launches into")
            .build();
        toggle_class(&chip, "mn-dim", game_name.is_none());
        let (weak, gid) = (w.weak(), gid.clone());
        chip.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                GroupSettings::open(&w, &gid);
            }
        });
        game.append(&chip);
    }
    band.append(&cols.span(&[Col::Status, Col::Uptime], &game));
    let cpu = lbl("", "mn-num mn-dim");
    let memory = lbl("", "mn-num mn-dim");
    band.append(&cols.cell(Col::Cpu, &cpu));
    band.append(&cols.cell(Col::Memory, &memory));
    band.append(&cols.span(&[Col::Macro, Col::Perf], &gtk::Box::new(Orientation::Horizontal, 0)));

    let acts = gtk::Box::new(Orientation::Horizontal, 2);
    acts.set_halign(Align::End);
    let launch = gid.as_ref().map(|gid| {
        let has_game = game_name.is_some();
        let (gid, ids) = (gid.clone(), ids.clone());
        let b =
            ds::Button::with_icons("Launch", Variant::Secondary, true, Some("play-filled"), None)
                .on(w.act(move |w| {
                    if w.state().any_live(&ids) {
                        w.stop_group(&gid);
                    } else if has_game {
                        w.launch_group(&gid);
                    } else {
                        GroupSettings::open(w, &gid);
                    }
                }));
        b.button.set_valign(Align::Center);
        // One width whatever it says, so every group's button lines up.
        b.button.set_width_request(LAUNCH_WIDTH);
        acts.append(&b.button);
        b
    });
    let confirm = gid.as_ref().map(|gid| {
        let body = match members.len() {
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
            .css_classes(["ib", "sm", "mn-act"])
            .tooltip_text(format!("More for {title}"))
            .build();
        name(&more, &format!("More for {title}"));
        acts.append(&more);
        context_menu(&band, &model);
    }
    band.append(&cols.cell(Col::Actions, &acts));

    let section = gtk::Box::new(Orientation::Vertical, 0);
    section.append(&band);
    if let Some(c) = &confirm {
        section.append(c);
    }
    if opened {
        if members.is_empty() && group.is_some() {
            let note = gtk::Box::new(Orientation::Horizontal, 8).css("mn-note");
            note.append(&lbl("Drag accounts onto this group's row to add them to it.", "mn-dim"));
            section.append(&note);
        }
        for a in shown {
            section.append(&account_row(w, a));
        }
    }
    drop_into(w, &band, gid.clone());

    let launch_tip = game_name.as_ref().map_or_else(
        || "Pick the group's game to launch it".to_owned(),
        |n| format!("Launch every account here into {n}"),
    );
    let shown_band = band.clone();
    w.watch_accounts(Box::new(move |s| {
        if let Some(b) = &launch {
            let live = s.any_live(&ids);
            b.set_text(if live { "Stop" } else { "Launch" });
            b.set_icon(if live { "stop-filled" } else { "play-filled" });
            b.button.set_sensitive(live || !ids.is_empty());
            b.button.set_tooltip_text(Some(if live {
                "Close every account's client here"
            } else {
                &launch_tip
            }));
        }
        let up: Vec<&UserId> = ids.iter().filter(|id| s.running.contains(id)).collect();
        running.set_label(&format!("{} of {} running", up.len(), ids.len()));
        let usage: Vec<_> = up.iter().filter_map(|id| s.usage.get(id)).collect();
        let total_cpu: Option<f32> =
            usage.iter().map(|u| u.cpu).sum::<Option<f32>>().filter(|_| !usage.is_empty());
        cpu.set_label(&total_cpu.map_or_else(|| "—".to_owned(), percent));
        memory.set_label(&if usage.is_empty() {
            "—".to_owned()
        } else {
            bytes(usage.iter().map(|u| u.memory).sum())
        });
        let chosen =
            ids.iter().filter(|id| s.accounts.get(**id).is_some_and(|a| a.selected)).count();
        quiet.set(true);
        check.set_active(chosen > 0 && chosen == ids.len());
        check.set_inconsistent(chosen > 0 && chosen < ids.len());
        quiet.set(false);
        let current =
            matches!((&s.inspected, &gid), (Some(Inspected::Group(a)), Some(b)) if a == b);
        toggle_class(&shown_band, "current", current);
    }));
    section
}

/// Dropping an account on the group's row moves it into the group.
fn drop_into(w: &Window, band: &gtk::Box, gid: Option<String>) {
    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let weak = w.weak();
    drop.connect_drop(move |_, value, _, _| {
        if let (Some(id), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
            let target = gid.clone();
            glib::idle_add_local_once(move || w.set_group(id, target));
        }
        true
    });
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
    band.add_controller(drop);
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
