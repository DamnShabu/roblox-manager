//! The leader card: the leader's row, and the accounts that auto-join its
//! server, in order. Dropping an account on the card makes it auto-join.

use adw::prelude::*;
use gtk::{Align, gdk, glib};
use rbxmgr_core::accounts::Account;
use rbxmgr_core::types::UserId;

use super::row::{account_row, chip_widget};
use crate::ui::widgets::{Btn, Fluent, LabelFluent, clear, icon, icon_fill, lbl};
use crate::ui::window::Window;

pub fn leader_card(
    w: &Window,
    leader: Option<&Account>,
    followers: &[(Account, String)],
) -> gtk::Box {
    let card = vbox!(0, "card lead");
    card.set_overflow(gtk::Overflow::Hidden);
    let strip = lbl("", "").ellipsize().chars(34);
    card.append(&hbox!(
        8,
        "lstrip",
        icon_fill("star", 16, "amber"),
        lbl("LEADER", "ltitle"),
        lbl("Launches first. Linked accounts auto-join its server.", "ldesc").hexpand().ellipsize(),
        hbox!(5, "ltarget", icon("arrow_forward", 14, ""), strip.clone())
    ));
    w.add_chip(Box::new(move |s| strip.set_label(&s.target_label())));
    match leader {
        Some(ld) => card.append(&account_row(w, ld, true)),
        None => card.append(
            &hbox!(
                8,
                "noleader",
                icon("star", 16, ""),
                lbl("No leader yet. Make an account the leader from its settings.", "")
            )
            .halign(Align::Center),
        ),
    }
    let count = if followers.is_empty() {
        String::new()
    } else {
        format!("· {} account{}", followers.len(), if followers.len() == 1 { "" } else { "s" })
    };
    let foot = vbox!(
        6,
        "ffoot",
        hbox!(
            6,
            "fhead",
            icon("link", 15, ""),
            lbl("Auto-join after leader", ""),
            lbl(&count, "fcount")
        )
    );
    for (i, (a, group)) in followers.iter().enumerate() {
        foot.append(&follower_row(w, a, group, i + 1, followers.len()));
    }
    if followers.is_empty() {
        foot.append(
            &hbox!(
                8,
                "dashedbox",
                icon("add_link", 16, ""),
                lbl("Drag accounts here, or turn on Auto-join in an account's settings", "")
                    .wrapped()
            )
            .halign(Align::Fill),
        );
    }
    card.append(&foot);
    if leader.is_some() {
        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let weak = w.weak();
        drop.connect_drop(move |_, value, _, _| {
            let id = value.get::<String>().ok().and_then(|v| v.parse().ok()).map(UserId);
            if let (Some(id), Some(w)) = (id, weak.upgrade()) {
                glib::idle_add_local_once(move || w.set_follow(id, true));
            }
            true
        });
        card.add_controller(drop);
    }
    card
}

/// One account in the auto-join list, `n` of `count`.
fn follower_row(w: &Window, a: &Account, group: &str, n: usize, count: usize) -> gtk::Box {
    let id = a.user_id;
    let chipbox = hbox!(0, "").centered();
    let up = Btn::new("mini")
        .icon("arrow_upward")
        .tip("Join earlier")
        .build(w.act(move |w| w.move_follower(id, -1)));
    up.button.set_sensitive(n > 1);
    let down = Btn::new("mini")
        .icon("arrow_downward")
        .tip("Join later")
        .build(w.act(move |w| w.move_follower(id, 1)));
    down.button.set_sensitive(n < count);
    let unlink = Btn::new("mini unlink")
        .icon("link_off")
        .tip("Stop auto-joining")
        .build(w.act(move |w| w.set_follow(id, false)));
    let row = hbox!(
        10,
        "frow",
        lbl(&n.to_string(), "fnum mono").xalign(0.5).centered(),
        hbox!(8, "", lbl(a.name.as_str(), "fname").ellipsize(), lbl(group, "fgroup"))
            .hexpand()
            .centered(),
        chipbox.clone(),
        hbox!(2, "", up.button, down.button, unlink.button)
    );
    w.add_chip(Box::new(move |s| {
        clear(&chipbox);
        chipbox.append(&chip_widget(s.chip(id), true));
    }));
    row
}
