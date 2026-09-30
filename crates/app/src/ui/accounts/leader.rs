//! The leader's card: the leader's row, then the accounts that auto-join its
//! server, in order. Dropping an account on the card makes it auto-join.

use adw::prelude::*;
use gtk::{Align, gdk, glib};
use rbxmgr_core::accounts::Account;

use super::row::{account_row, dragged_id};
use crate::state::Chip;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, avatar, boxed_list, clear, icon, lbl, plural, section_header, status,
};
use crate::ui::window::Window;

pub fn leader_section(
    w: &Window,
    leader: Option<&Account>,
    followers: &[(Account, String)],
) -> gtk::Box {
    let list = boxed_list();
    match leader {
        Some(ld) => list.append(&account_row(w, ld)),
        None => list.append(&placeholder(
            "starred-symbolic",
            "No leader yet: choose Make Leader in an account's menu.",
        )),
    }
    let name = leader.map_or_else(|| "the leader".to_owned(), |l| l.name.to_string());
    let head = gtk::ListBoxRow::builder().activatable(false).selectable(false).build();
    head.add_css_class("subheader");
    head.set_child(Some(&hbox!(
        8,
        "",
        icon("insert-link-symbolic").css("dimmed"),
        lbl(&format!("Auto-join after {name}"), "caption-heading").ellipsize(),
        lbl(&plural(followers.len(), "account", "accounts"), "caption dimmed")
    )));
    list.append(&head);
    for (i, (a, group)) in followers.iter().enumerate() {
        list.append(&follower_row(w, a, group, i + 1, followers.len()));
    }
    if followers.is_empty() {
        list.append(&placeholder(
            "list-drag-handle-symbolic",
            "Drag accounts here, or choose Auto-join the Leader in an account's menu.",
        ));
    }
    if leader.is_some() {
        let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let weak = w.weak();
        drop.connect_drop(move |_, value, _, _| {
            if let (Some(id), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
                glib::idle_add_local_once(move || w.set_follow(id, true));
            }
            true
        });
        list.add_css_class("drop-zone");
        list.add_controller(drop);
    }
    vbox!(
        8,
        "",
        section_header(
            "Leader",
            Some("Launch as Group starts it first; its auto-join accounts follow into its server"),
            &[]
        ),
        list
    )
}

/// One account in the auto-join list, `n` of `count`.
fn follower_row(w: &Window, a: &Account, group: &str, n: usize, count: usize) -> gtk::ListBoxRow {
    let id = a.user_id;
    let status_box = hbox!(0, "").centered();
    let up = Btn::new("flat circular")
        .icon("go-up-symbolic")
        .tip("Join earlier")
        .build(w.act(move |w| w.move_follower(id, -1)));
    up.button.set_sensitive(n > 1);
    let down = Btn::new("flat circular")
        .icon("go-down-symbolic")
        .tip("Join later")
        .build(w.act(move |w| w.move_follower(id, 1)));
    down.button.set_sensitive(n < count);
    let unlink = Btn::new("flat circular")
        .icon("list-remove-symbolic")
        .tip("Stop auto-joining")
        .build(w.act(move |w| w.set_follow(id, false)));
    let picture = w.services().avatars.cached(&id.to_string());
    let row = gtk::ListBoxRow::builder().activatable(false).selectable(false).build();
    row.add_css_class("follower");
    row.set_child(Some(&hbox!(
        10,
        "",
        lbl(&n.to_string(), "order").xalign(0.5).centered(),
        avatar(a.name.as_str(), picture.as_deref(), 24),
        hbox!(
            8,
            "",
            lbl(a.name.as_str(), "").ellipsize(),
            lbl(group, "caption dimmed").ellipsize()
        )
        .hexpand()
        .centered(),
        status_box.clone(),
        hbox!(0, "", up.button.centered(), down.button.centered(), unlink.button.centered())
    )));
    w.watch_accounts(Box::new(move |s| {
        clear(&status_box);
        let chip = s.chip(id);
        if chip != Chip::Idle {
            let (css, text, live) = chip.look();
            status_box.append(&status(css, text, live));
        }
    }));
    row
}

/// A row that only says something, dimmed.
pub fn placeholder(ic: &str, text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::builder().activatable(false).selectable(false).build();
    row.add_css_class("placeholder");
    let line = hbox!(10, "dimmed", icon(ic), lbl(text, "").wrapped().hexpand());
    line.set_halign(Align::Fill);
    row.set_child(Some(&line));
    row
}
