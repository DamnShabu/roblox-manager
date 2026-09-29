//! One account: select, launch or stop, and a gear that opens everything else
//! about it. Every account but the leader can be dragged -- onto another row
//! to reorder or regroup, onto a group, or onto the leader to auto-join it.

use adw::prelude::*;
use gtk::{Align, gdk, glib};
use rbxmgr_core::accounts::{Account, relative_time};
use rbxmgr_core::types::UserId;

use super::settings;
use crate::state::Chip;
use crate::ui::widgets::{Btn, Fluent, LabelFluent, clear, dot, icon, icon_fill, lbl};
use crate::ui::window::Window;

pub fn account_row(w: &Window, acct: &Account, first: bool) -> gtk::Box {
    let id = acct.user_id;
    let (opened, macro_shown) = {
        let s = w.state();
        let m = acct.macro_name.clone().filter(|m| s.macros.contains(m));
        (s.open_accounts.contains(&id), m)
    };
    let on = acct.selected;

    let lead_or_handle = if acct.leader {
        icon_fill("star", 18, "amber").tip("Leader")
    } else {
        let h = icon("drag_indicator", 18, "handle")
            .tip("Drag to reorder, move to a group, or drop on the leader");
        h.set_cursor_from_name(Some("grab"));
        h
    };
    let check = Btn::new(if on { "cbox on" } else { "cbox" })
        .size(15)
        .tip("Include in Launch selected")
        .build(w.act(move |w| w.toggle_selected(id)));
    check.button.set_valign(Align::Center);
    if on {
        check.button.set_child(Some(&icon("check", 15, "")));
    }

    let title =
        hbox!(8, "", lbl(acct.name.as_str(), if on { "aname on" } else { "aname" }).ellipsize());
    if let Some(user) = acct.username.as_deref().filter(|u| *u != acct.name.as_str()) {
        title.set_tooltip_text(Some(&format!("@{user}")));
    }
    if let Some(n) = acct.follow.filter(|_| !acct.leader) {
        title.append(
            &hbox!(4, "ajbadge", icon("link", 13, ""), lbl(&format!("Auto-join #{n}"), ""))
                .centered(),
        );
    }
    let note = acct.note.trim();
    if !note.is_empty() {
        let n = vbox!(0, "noteic", icon("sticky_note_2", 16, "")).centered();
        n.set_tooltip_markup(Some(&format!(
            "<span size='small' weight='bold' foreground='#e9bd6a'>NOTE</span>\n{}",
            glib::markup_escape_text(note)
        )));
        n.set_cursor_from_name(Some("help"));
        title.append(&n);
    }
    if acct.low_power {
        title.append(&icon("eco", 15, "eco").tip("Low-power client"));
    }
    let sub = hbox!(
        6,
        "sub",
        icon("schedule", 14, ""),
        lbl(&relative_time(acct.last_launch.as_deref(), chrono::Utc::now()), "")
    );
    if let Some(m) = &macro_shown {
        sub.append(&gtk::Box::new(gtk::Orientation::Horizontal, 0).css("bullet").centered());
        sub.append(&icon("keyboard", 14, ""));
        sub.append(&lbl(m, "").ellipsize());
    }
    let info = vbox!(3, "", title, sub).hexpand().centered();

    let chipbox = hbox!(0, "").centered();
    let play = Btn::new("playb")
        .icon("play_arrow")
        .size(20)
        .fill()
        .build(w.act(move |w| w.play_or_stop(id)));
    play.button.set_valign(Align::Center);
    let gear = Btn::new(if opened { "setb open" } else { "setb" })
        .icon("settings")
        .size(20)
        .tip("Account settings")
        .build(w.act(move |w| w.toggle_account(id)));
    gear.button.set_valign(Align::Center);
    let css = format!(
        "arow{}{}{}",
        if first { " first" } else { "" },
        if on { " sel" } else { "" },
        if opened { " open" } else { "" }
    );
    let line = hbox!(
        12,
        &css,
        lead_or_handle,
        check.button,
        info,
        chipbox.clone(),
        play.button.clone(),
        gear.button
    );
    let row = vbox!(0, "", line.clone());
    if !acct.leader {
        drag_and_drop(w, &line, id);
    }
    if opened {
        row.append(&settings::panel(w, acct));
    }

    let leader = acct.leader;
    w.add_chip(Box::new(move |s| {
        let chip = s.chip(id);
        clear(&chipbox);
        chipbox.append(&chip_widget(chip, false));
        let live = chip == Chip::Running;
        play.set_icon(if live { "stop" } else { "play_arrow" });
        if live {
            play.button.add_css_class("stop");
        } else {
            play.button.remove_css_class("stop");
        }
        play.button.set_sensitive(!matches!(chip, Chip::Starting | Chip::Joining));
        play.button.set_tooltip_text(Some(if live {
            "Close this account's client"
        } else if leader {
            "Launch the leader, then auto-join the linked accounts"
        } else {
            "Launch"
        }));
    }));
    row
}

/// A status chip.
pub fn chip_widget(chip: Chip, small: bool) -> gtk::Box {
    let (css, text, pulse) = chip.look();
    hbox!(
        6,
        &format!("chip {css}{}", if small { " small" } else { "" }),
        dot(css, pulse, 7),
        lbl(text, "")
    )
    .centered()
}

/// The row as a drag source (its user id), and a drop target that marks
/// where a dragged account would land: after this row when it comes from
/// above, before it when from below.
fn drag_and_drop(w: &Window, line: &gtk::Box, id: UserId) {
    let src = gtk::DragSource::new();
    src.set_actions(gdk::DragAction::MOVE);
    src.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&id.0.to_string().to_value()))
    });
    let dragged = line.clone();
    src.connect_drag_begin(move |s, _| {
        s.set_icon(Some(&gtk::WidgetPaintable::new(Some(&dragged))), 20, 20);
        dragged.add_css_class("dragging");
    });
    let dragged = line.clone();
    src.connect_drag_end(move |_, _, _| dragged.remove_css_class("dragging"));
    line.add_controller(src);

    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    drop.set_preload(true);
    let (marked, weak) = (line.clone(), w.weak());
    drop.connect_motion(move |t, _, _| {
        mark(&marked, None);
        let from =
            t.value().and_then(|v| v.get::<String>().ok()).and_then(|v| v.parse().ok()).map(UserId);
        if let (Some(from), Some(w)) = (from.filter(|f| *f != id), weak.upgrade()) {
            let order: Vec<UserId> =
                w.state().accounts.visual_order().iter().map(|a| a.user_id).collect();
            if let (Some(a), Some(b)) =
                (order.iter().position(|x| *x == from), order.iter().position(|x| *x == id))
            {
                mark(&marked, Some(if a < b { "below" } else { "above" }));
            }
        }
        gdk::DragAction::MOVE
    });
    let marked = line.clone();
    drop.connect_leave(move |_| mark(&marked, None));
    let (marked, weak) = (line.clone(), w.weak());
    drop.connect_drop(move |_, value, _, _| {
        mark(&marked, None);
        let from = value.get::<String>().ok().and_then(|v| v.parse().ok()).map(UserId);
        if let (Some(from), Some(w)) = (from, weak.upgrade()) {
            // Deferred: the row that took the drop is rebuilt by it.
            glib::idle_add_local_once(move || w.drop_on_row(from, id));
        }
        true
    });
    line.add_controller(drop);
}

fn mark(line: &gtk::Box, place: Option<&str>) {
    for c in ["above", "below"] {
        if Some(c) == place {
            line.add_css_class(c);
        } else {
            line.remove_css_class(c);
        }
    }
}
