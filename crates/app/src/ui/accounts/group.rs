//! A group -- or, for the accounts in none, Ungrouped: a header that folds,
//! selects, launches and edits it, then its rows. Dropping an account on it
//! moves the account into the group.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, Group};
use rbxmgr_core::types::UserId;

use super::leader::placeholder;
use super::row::{account_row, context_menu, dragged_id};
use crate::state::Tile;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, boxed_list, clear, icon, lbl, plural, status, thumb, toggle_class,
};
use crate::ui::window::Window;

pub fn group_section(w: &Window, group: Option<&Group>, members: &[Account]) -> gtk::Box {
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
    let mut sub = plural(members.len(), "account", "accounts");
    match (&game_name, group.is_some()) {
        (Some(game), _) => sub.push_str(&format!(" · {game}")),
        (None, true) => sub.push_str(" · no game"),
        (None, false) => {}
    }

    let fold = {
        let gid = gid.clone();
        Btn::new("flat circular")
            .icon(if opened { "pan-down-symbolic" } else { "pan-end-symbolic" })
            .tip(if opened { "Fold" } else { "Unfold" })
            .build(w.act(move |w| w.toggle_group_open(gid.clone())))
    };
    let quiet = Rc::new(Cell::new(false));
    let check = gtk::CheckButton::builder()
        .valign(Align::Center)
        .tooltip_text("Select every account here")
        .sensitive(!members.is_empty())
        .build();
    {
        let (weak, quiet, ids) = (w.weak(), quiet.clone(), ids.clone());
        check.connect_toggled(move |c| {
            if let (false, Some(w)) = (quiet.get(), weak.upgrade()) {
                w.select_accounts(&ids, c.is_active());
            }
        });
    }
    let pic: gtk::Widget = match (&game_name, group) {
        (Some(_), _) => thumb(tile.as_ref().and_then(|t| t.icon.as_deref()), 36, "thumb"),
        (None, g) => {
            let frame = gtk::CenterBox::new();
            frame.set_size_request(36, 36);
            frame.set_valign(Align::Center);
            frame.add_css_class("thumb");
            frame.add_css_class("placeholder-art");
            frame.set_center_widget(Some(
                &icon(if g.is_some() { "applications-games-symbolic" } else { "folder-symbolic" })
                    .css("dimmed"),
            ));
            frame.upcast()
        }
    };
    let heading =
        vbox!(1, "", lbl(&title, "title").ellipsize(), lbl(&sub, "caption dimmed").ellipsize())
            .hexpand()
            .centered();
    // The name folds and unfolds, like the arrow beside it.
    let click = gtk::GestureClick::new();
    let act = {
        let gid = gid.clone();
        w.act(move |w| w.toggle_group_open(gid.clone()))
    };
    click.connect_released(move |_, _, _, _| act());
    heading.add_controller(click);
    heading.set_cursor_from_name(Some("pointer"));
    let running = hbox!(0, "").centered();
    let head = hbox!(
        8,
        "section-header",
        fold.button.centered(),
        check.clone(),
        pic,
        heading,
        running.clone()
    );

    // Launch while the group is idle, Stop while any member is up or
    // starting: the redraw below flips it, the click asks which.
    let launch = gid.as_ref().map(|gid| {
        let (gid, ids) = (gid.clone(), ids.clone());
        let b = Btn::new("").text("Launch").icon("media-playback-start-symbolic").build(w.act(
            move |w| {
                if w.state().any_live(&ids) { w.stop_group(&gid) } else { w.launch_group(&gid) }
            },
        ));
        b.button.set_valign(Align::Center);
        head.append(&b.button);
        b
    });
    if let Some(gid) = &gid {
        let model = group_menu(gid);
        head.append(
            &gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .menu_model(&model)
                .valign(Align::Center)
                .tooltip_text("More")
                .css_classes(["flat", "circular"])
                .build(),
        );
        context_menu(&head, &model);
    }

    let section = vbox!(8, "drop-zone", head);
    if opened {
        let list = boxed_list();
        for a in members {
            list.append(&account_row(w, a));
        }
        if members.is_empty() {
            list.append(&placeholder(
                "list-drag-handle-symbolic",
                if group.is_some() {
                    "Drag accounts here to add them to this group."
                } else {
                    "Every account is in a group."
                },
            ));
        }
        section.append(&list);
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
    section.add_controller(drop);

    let can_launch = game_name.is_some() && !members.is_empty();
    let launch_tip = game_name.map_or_else(
        || "Choose the group's game in its settings to launch it".to_owned(),
        |n| format!("Launch every account here into {n}"),
    );
    w.watch_accounts(Box::new(move |s| {
        if let Some(b) = &launch {
            let live = s.any_live(&ids);
            b.set_text(if live { "Stop" } else { "Launch" });
            b.set_icon(if live {
                "media-playback-stop-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            toggle_class(&b.button, "destructive-action", live);
            b.button.set_sensitive(live || can_launch);
            b.button.set_tooltip_text(Some(if live {
                "Close every account's client here"
            } else {
                &launch_tip
            }));
        }
        let n = ids.iter().filter(|id| s.running.contains(id)).count();
        clear(&running);
        if n > 0 {
            running.append(&status("running", &format!("{n} running"), true));
        }
        let chosen =
            ids.iter().filter(|id| s.accounts.get(**id).is_some_and(|a| a.selected)).count();
        quiet.set(true);
        check.set_active(chosen > 0 && chosen == ids.len());
        check.set_inconsistent(chosen > 0 && chosen < ids.len());
        quiet.set(false);
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
    model.append_item(&item("Group Settings…", "win.group-settings"));
    let danger = gio::Menu::new();
    danger.append_item(&item("Delete Group…", "win.delete-group"));
    model.append_section(None, &danger);
    model
}
