//! The launch order above the groups: the leader, then each account that
//! auto-joins its server, in turn. Launch as group follows it. A chip opens
//! its account; the arrow moves a follower earlier, the cross stops it
//! following. Dropping a row here makes it auto-join.

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, glib};
use rbxmgr_core::accounts::Account;
use rbxmgr_core::types::UserId;

use super::row::dragged_id;
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name};
use crate::ui::window::Window;

pub fn launch_order(w: &Window, leader: Option<&Account>, followers: &[Account]) -> gtk::Box {
    let wrap = adw::WrapBox::builder().child_spacing(8).line_spacing(8).hexpand(true).build();
    wrap.append(&ds::overline("Launch order").centered());
    match leader {
        Some(l) => {
            let chip = gtk::Box::new(Orientation::Horizontal, 0).css("mn-chain-chip leader");
            chip.append(&chip_button(
                w,
                l,
                None,
                "Launch as group starts it first; its auto-join accounts follow into its server",
            ));
            wrap.append(&chip.centered());
        }
        None => wrap.append(
            &lbl("No leader yet: choose Make leader in an account's menu.", "t-caption muted")
                .centered(),
        ),
    }
    for (i, f) in followers.iter().enumerate() {
        wrap.append(&ds::icon("chev-right").css("muted s14").centered());
        let id = f.user_id;
        let chip = gtk::Box::new(Orientation::Horizontal, 0).css("mn-chain-chip");
        chip.append(&chip_button(
            w,
            f,
            Some(i + 1),
            &format!("Joins the leader's server, number {} in turn", i + 1),
        ));
        let earlier = ds::ib("chev-left", "Join earlier", true);
        name(&earlier, &format!("Move {} earlier", f.name));
        earlier.set_sensitive(i > 0);
        earlier.connect_clicked({
            let w = w.weak();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.move_follower(id, -1);
                }
            }
        });
        let stop = ds::ib("x", "Stop auto-joining", true);
        name(&stop, &format!("{} stops auto-joining", f.name));
        stop.connect_clicked({
            let w = w.weak();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.set_follow(id, false);
                }
            }
        });
        for b in [&earlier, &stop] {
            b.remove_css_class("ib");
            b.remove_css_class("sm");
            chip.append(b);
        }
        wrap.append(&chip.centered());
    }
    let drop_zone = lbl("Drop a row to auto-join", "mn-chain-drop").xalign(0.5);
    drop_zone.set_tooltip_text(Some("Drag a row here, or choose Auto-join the leader in its menu"));
    wrap.append(&drop_zone.centered());

    let summary = lbl("", "t-caption muted").valign(Align::Center);
    let strip = gtk::Box::new(Orientation::Horizontal, 8).css("mn-chain");
    strip.append(&wrap);
    strip.append(&summary);
    let mut ids: Vec<UserId> = leader.map(|l| l.user_id).into_iter().collect();
    ids.extend(followers.iter().map(|f| f.user_id));
    w.watch_accounts(Box::new(move |s| {
        let up = ids.iter().filter(|id| s.running.contains(id)).count();
        summary.set_label(&format!("{up} of {} running", ids.len()));
        summary.set_visible(!ids.is_empty());
    }));

    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let weak = w.weak();
    drop.connect_drop(move |_, value, _, _| {
        if let (Some(id), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
            glib::idle_add_local_once(move || w.set_follow(id, true));
        }
        true
    });
    {
        let b = strip.downgrade();
        drop.connect_enter(move |_, _, _| {
            if let Some(b) = b.upgrade() {
                b.add_css_class("drop");
            }
            gdk::DragAction::MOVE
        });
        let b = strip.downgrade();
        drop.connect_leave(move |_| {
            if let Some(b) = b.upgrade() {
                b.remove_css_class("drop");
            }
        });
    }
    strip.add_controller(drop);
    strip
}

/// A chip's main part: its number (for a follower), avatar and name; a
/// click opens the account in the inspector.
fn chip_button(w: &Window, a: &Account, n: Option<usize>, tip: &str) -> gtk::Button {
    let id = a.user_id;
    let inner = gtk::Box::new(Orientation::Horizontal, 4);
    match n {
        Some(n) => inner.append(&lbl(&format!("#{n}"), "t-caption muted")),
        None => inner.append(&ds::icon("star-filled").css("star")),
    }
    let picture = w.services().avatars.cached(&id.to_string());
    inner.append(&ds::av(a.name.as_str(), picture.as_deref(), 20));
    inner.append(&lbl(a.name.as_str(), "").ellipsize().chars(16));
    let b = gtk::Button::builder().child(&inner).tooltip_text(tip).build();
    b.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                AccountSettings::open(&w, id);
            }
        }
    });
    b
}
