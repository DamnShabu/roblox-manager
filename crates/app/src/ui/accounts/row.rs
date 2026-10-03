//! One account as a row: select it, see how it is, launch or stop it, and a
//! menu for everything else (also on a right click). Every account but the
//! leader can be dragged -- onto another row to reorder or regroup, onto a
//! group, or onto the leader to auto-join it.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, relative_time};
use rbxmgr_core::cordial::Window as ClientWindow;
use rbxmgr_core::types::UserId;

use crate::state::Chip;
use crate::ui::widgets::{Btn, Fluent, avatar, clear, icon, lbl, name, status, toggle_class};
use crate::ui::window::{Window, can_hide};

pub fn account_row(w: &Window, acct: &Account) -> adw::ActionRow {
    let id = acct.user_id;
    let (macro_shown, picture) = {
        let s = w.state();
        let m = acct.macro_name.clone().filter(|m| s.macros.contains(m));
        (m, w.services().avatars.cached(&id.to_string()))
    };
    let about = subtitle(acct, macro_shown.as_deref());
    let row = adw::ActionRow::builder()
        .title(acct.name.as_str())
        .subtitle(&about)
        .use_markup(false)
        .title_lines(1)
        .subtitle_lines(1)
        .build();
    row.add_css_class("account");

    // A prefix goes in front of those already there: added last to first.
    row.add_prefix(&avatar(acct.name.as_str(), picture.as_deref(), 34));
    // Set while a redraw brings the box in line with the state, so the
    // change is not taken for a click.
    let quiet = Rc::new(Cell::new(false));
    let check = gtk::CheckButton::builder()
        .active(acct.selected)
        .valign(Align::Center)
        .tooltip_text("Include in Launch Selected")
        .build();
    {
        let (weak, quiet) = (w.weak(), quiet.clone());
        check.connect_toggled(move |c| {
            if let (false, Some(w)) = (quiet.get(), weak.upgrade()) {
                w.select_accounts(&[id], c.is_active());
            }
        });
    }
    row.add_prefix(&check);
    row.set_activatable_widget(Some(&check));
    if acct.leader {
        row.add_prefix(&icon("starred-symbolic").css("leader-star").tip("The leader"));
    } else {
        row.add_prefix(
            &icon("list-drag-handle-symbolic").css("dimmed").tip(
                "Drag to reorder, onto a group to move it, or onto the leader to auto-join it",
            ),
        );
    }

    if let Some(n) = acct.follow.filter(|_| !acct.leader) {
        row.add_suffix(
            &lbl(&format!("Auto-join #{n}"), "tag")
                .centered()
                .tip("Launch as Group sends it into the leader's server, in this place"),
        );
    }
    let note = acct.note.trim();
    if !note.is_empty() {
        row.add_suffix(&icon("text-x-generic-symbolic").css("dimmed").tip(note));
    }
    if acct.low_power {
        row.add_suffix(
            &icon("power-profile-power-saver-symbolic").css("dimmed").tip("Low-power client"),
        );
    }
    let failure = Btn::new("flat circular error")
        .icon("dialog-warning-symbolic")
        .build(w.act(move |w| w.show_failure(id)));
    failure.button.set_valign(Align::Center);
    let status_box = hbox!(6, "").centered();
    let play = Btn::new("flat circular")
        .icon("media-playback-start-symbolic")
        .build(w.act(move |w| w.play_or_stop(id)));
    play.button.set_valign(Align::Center);
    let hide = Btn::new("flat circular").icon("view-conceal-symbolic").build(w.act(move |w| {
        let hide = can_hide(&w.state(), id);
        w.set_window_hidden(id, hide);
    }));
    hide.button.set_valign(Align::Center);
    let model = menu(w, acct);
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&model)
        .valign(Align::Center)
        .tooltip_text("More")
        .css_classes(["flat", "circular"])
        .build();
    name(&more, &format!("More for {}", acct.name));
    row.add_suffix(&failure.button);
    row.add_suffix(&status_box);
    row.add_suffix(&hide.button);
    row.add_suffix(&play.button);
    row.add_suffix(&more);
    context_menu(&row, &model);
    if !acct.leader {
        drag_and_drop(w, &row, id);
    }

    let leader = acct.leader;
    let shown = row.clone();
    w.watch_accounts(Box::new(move |s| {
        let chip = s.chip(id);
        clear(&status_box);
        // While a macro plays, the subtitle says where this account's run
        // is: each account its own, however many play the same macro.
        match s.macro_runs.get(&id) {
            Some((_, playing)) => {
                status_box.append(&status("macro", playing, true).tip("The macro playing on it"));
                let at = s.macro_progress.get(&id).map_or("starting", String::as_str);
                shown.set_subtitle(&format!("{playing}: {at}"));
            }
            None => shown.set_subtitle(&about),
        }
        if chip != Chip::Idle {
            let (css, text, live) = chip.look();
            status_box.append(&status(css, text, live));
        }
        let live = chip == Chip::Running;
        play.set_icon(if live {
            "media-playback-stop-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        play.button.set_sensitive(!matches!(chip, Chip::Starting | Chip::Joining));
        let tip = if live {
            "Close this account's client"
        } else if leader {
            "Launch the leader, then its auto-join accounts into its server"
        } else {
            "Launch into the target"
        };
        play.button.set_tooltip_text(Some(tip));
        name(&play.button, tip);
        let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
        hide.button.set_visible(hidden || can_hide(s, id));
        hide.set_icon(if hidden { "view-reveal-symbolic" } else { "view-conceal-symbolic" });
        let tip = if hidden {
            "Show this account's window"
        } else {
            "Hide this account's window; the game keeps running"
        };
        hide.button.set_tooltip_text(Some(tip));
        name(&hide.button, tip);
        if let Some(a) = s.accounts.get(id).filter(|a| a.selected != check.is_active()) {
            quiet.set(true);
            check.set_active(a.selected);
            quiet.set(false);
        }
        match s.failures.get(&id) {
            Some(why) => {
                let tip = format!("The last launch failed: {why}");
                failure.button.set_visible(true);
                failure.button.set_tooltip_text(Some(&tip));
                name(&failure.button, &tip);
            }
            None => failure.button.set_visible(false),
        }
    }));
    row
}

/// "@alt_one · launched 3h ago · plays Anti-AFK"
fn subtitle(acct: &Account, macro_name: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(user) = acct.username.as_deref().filter(|u| *u != acct.name.as_str()) {
        parts.push(format!("@{user}"));
    }
    parts.push(match acct.last_launch.as_deref() {
        None => "never launched".to_owned(),
        when => format!("launched {}", relative_time(when, chrono::Utc::now())),
    });
    if let Some(m) = macro_name {
        parts.push(format!("plays {m}"));
    }
    parts.join(" · ")
}

/// The row's menu: window actions, aimed at this account.
fn menu(w: &Window, acct: &Account) -> gio::Menu {
    let id = acct.user_id.0;
    let (has_leader, groups, current) = {
        let s = w.state();
        let groups: Vec<(String, String)> = std::iter::once((String::new(), "Ungrouped".into()))
            .chain(s.accounts.groups().iter().map(|g| {
                let name = if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                (g.id.clone(), name)
            }))
            .collect();
        let current = s.accounts.group_of(acct).unwrap_or_default().to_owned();
        (s.accounts.leader().is_some(), groups, current)
    };
    let item = |label: &str, action: &str| {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some(action), Some(&id.to_variant()));
        item
    };
    let model = gio::Menu::new();
    model.append_item(&item("Settings…", "win.account-settings"));
    if !acct.leader {
        let layout = gio::Menu::new();
        layout.append_item(&item("Make Leader", "win.make-leader"));
        if has_leader {
            let follow =
                if acct.follow.is_some() { "Stop Auto-joining" } else { "Auto-join the Leader" };
            layout.append_item(&item(follow, "win.toggle-follow"));
        }
        let places = gio::Menu::new();
        for (gid, name) in groups.into_iter().filter(|(gid, _)| *gid != current) {
            // A menu label's underscores are mnemonics; a name's are text.
            let entry = gio::MenuItem::new(Some(&name.replace('_', "__")), None);
            entry.set_action_and_target_value(
                Some("win.move-account"),
                Some(&(id, gid).to_variant()),
            );
            places.append_item(&entry);
        }
        if places.n_items() > 0 {
            layout.append_submenu(Some("Move To"), &places);
        }
        model.append_section(None, &layout);
    }
    let session = gio::Menu::new();
    session.append_item(&item("Check Session", "win.check-session"));
    session.append_item(&item("Sign In Again…", "win.sign-in-again"));
    model.append_section(None, &session);
    let danger = gio::Menu::new();
    danger.append_item(&item("Remove…", "win.remove-account"));
    model.append_section(None, &danger);
    model
}

/// The same menu where the row is right-clicked.
pub fn context_menu(row: &impl IsA<gtk::Widget>, model: &gio::Menu) {
    let click = gtk::GestureClick::builder().button(gdk::BUTTON_SECONDARY).build();
    let model = model.clone();
    click.connect_pressed(move |g, _, x, y| {
        let Some(widget) = g.widget() else { return };
        g.set_state(gtk::EventSequenceState::Claimed);
        let pop = gtk::PopoverMenu::from_model(Some(&model));
        pop.set_parent(&widget);
        pop.set_has_arrow(false);
        pop.set_halign(Align::Start);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        // Unparented once any chosen action has found its group through it.
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    });
    row.add_controller(click);
}

/// The row as a drag source (its user id), and a drop target that marks
/// where a dragged account would land: after this row when it comes from
/// above, before it when from below.
fn drag_and_drop(w: &Window, row: &adw::ActionRow, id: UserId) {
    // Weak: the row owns these handlers, and a handler that owned the row
    // would keep every rebuilt list alive.
    let weak_row = row.downgrade();
    let src = gtk::DragSource::new();
    src.set_actions(gdk::DragAction::MOVE);
    src.connect_prepare(move |_, _, _| {
        Some(gdk::ContentProvider::for_value(&id.0.to_string().to_value()))
    });
    let dragged = weak_row.clone();
    src.connect_drag_begin(move |s, _| {
        if let Some(row) = dragged.upgrade() {
            s.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 20, 20);
            row.add_css_class("dragging");
        }
    });
    let dragged = weak_row.clone();
    src.connect_drag_end(move |_, _, _| {
        if let Some(row) = dragged.upgrade() {
            row.remove_css_class("dragging");
        }
    });
    row.add_controller(src);

    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    drop.set_preload(true);
    let (marked, weak) = (weak_row.clone(), w.weak());
    drop.connect_motion(move |t, _, _| {
        let Some(row) = marked.upgrade() else { return gdk::DragAction::MOVE };
        mark(&row, None);
        let from = dragged_id(t.value().as_ref());
        if let (Some(from), Some(w)) = (from.filter(|f| *f != id), weak.upgrade()) {
            let order: Vec<UserId> =
                w.state().accounts.visual_order().iter().map(|a| a.user_id).collect();
            if let (Some(a), Some(b)) =
                (order.iter().position(|x| *x == from), order.iter().position(|x| *x == id))
            {
                mark(&row, Some(if a < b { "drop-below" } else { "drop-above" }));
            }
        }
        gdk::DragAction::MOVE
    });
    let marked = weak_row.clone();
    drop.connect_leave(move |_| {
        if let Some(row) = marked.upgrade() {
            mark(&row, None);
        }
    });
    let (marked, weak) = (weak_row, w.weak());
    drop.connect_drop(move |_, value, _, _| {
        if let Some(row) = marked.upgrade() {
            mark(&row, None);
        }
        if let (Some(from), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
            // Deferred: the row that took the drop is rebuilt by it.
            glib::idle_add_local_once(move || w.drop_on_row(from, id));
        }
        true
    });
    row.add_controller(drop);
}

/// The account a drag carries.
pub fn dragged_id(value: Option<&glib::Value>) -> Option<UserId> {
    value.and_then(|v| v.get::<String>().ok()).and_then(|v| v.parse().ok()).map(UserId)
}

fn mark(row: &adw::ActionRow, place: Option<&str>) {
    for c in ["drop-above", "drop-below"] {
        toggle_class(row, c, Some(c) == place);
    }
}
