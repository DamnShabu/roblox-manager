//! One account as a table row: select it, see how it is (status, role,
//! macro, performance, when it last ran), launch or stop it, and a menu for
//! the rest (also on a right click). Under it, when asked for, why its last
//! launch failed and the confirmation to remove it. Every account but the
//! leader can be dragged onto another row, a band or the launch order.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, SessionState};
use rbxmgr_core::cordial::{Performance, Window as ClientWindow};
use rbxmgr_core::types::UserId;

use super::{Col, short_since};
use crate::state::{Chip, Inspected};
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::ds::{self, Tone, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, name, sentence, toggle_class};
use crate::ui::window::{Window, can_hide};

pub fn account_row(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let cols = w.columns();
    let (macro_shown, picture) = {
        let s = w.state();
        let m = acct.macro_name.clone().filter(|m| s.macros.contains(m));
        (m, w.services().avatars.cached(&id.to_string()))
    };
    let row = gtk::Box::new(Orientation::Horizontal, 8).css("mn-row");

    // -- grip, check, avatar -------------------------------------------------
    let grip = if acct.leader {
        ds::icon("star-filled").css("mn-grip leader s16").tip("The leader")
    } else {
        ds::icon("grip").css("mn-grip s16").tip(
            "Drag onto another row to reorder, onto a band to move it, or onto the launch order",
        )
    };
    row.append(&cols.cell(Col::Grip, &grip));
    let quiet = Rc::new(Cell::new(false));
    let check = ds::check(acct.selected, &format!("Select {}", acct.name));
    {
        let (weak, quiet) = (w.weak(), quiet.clone());
        check.connect_toggled(move |c| {
            if let (false, Some(w)) = (quiet.get(), weak.upgrade()) {
                w.select_accounts(&[id], c.is_active());
            }
        });
    }
    row.append(&cols.cell(Col::Check, &check));
    row.append(&cols.cell(Col::Av, &ds::av(acct.name.as_str(), picture.as_deref(), 24)));

    // -- name -------------------------------------------------------------------
    let title = gtk::Box::new(Orientation::Horizontal, 4);
    title.append(&lbl(acct.name.as_str(), "mn-name-t").ellipsize());
    let note = acct.note.trim();
    if !note.is_empty() {
        title.append(&ds::icon("note").css("mn-note").tip(note));
    }
    let hidden_mark =
        ds::icon("eye-off").css("mn-note").tip("Window hidden; the game keeps running");
    title.append(&hidden_mark);
    let user = acct.username.clone().unwrap_or_else(|| acct.name.to_string());
    let text = gtk::Box::new(Orientation::Vertical, 0);
    text.append(&title);
    text.append(&lbl(&format!("@{user}"), "mn-name-s").ellipsize());
    let name_btn = gtk::Button::builder()
        .child(&text)
        .tooltip_text("Show its settings")
        .css_classes(["mn-name"])
        .halign(Align::Start)
        .build();
    name_btn.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                AccountSettings::open(&w, id);
            }
        }
    });
    row.append(&cols.cell(Col::Name, &name_btn));

    // -- status, role, macro, perf, last ---------------------------------------
    let status = gtk::Box::new(Orientation::Horizontal, 4);
    row.append(&cols.cell(Col::Status, &status));
    let role = gtk::Box::new(Orientation::Horizontal, 4).css("mn-role");
    match (acct.leader, acct.follow) {
        (true, _) => {
            role.add_css_class("leader");
            role.append(&ds::icon("star-filled"));
            role.append(&lbl("Leader", ""));
            role.set_tooltip_text(Some("Launch as group starts it first"));
        }
        (false, Some(n)) => {
            role.append(&lbl(&format!("Auto-join #{n}"), ""));
            role.set_tooltip_text(Some("Launch as group sends it into the leader's server"));
        }
        (false, None) => role.append(&lbl("—", "mn-dash")),
    }
    row.append(&cols.cell(Col::Role, &role));
    let macro_cell = gtk::Box::new(Orientation::Horizontal, 4).css("mn-macro");
    let macro_btn = ds::ib("play", "Play its macro", true);
    macro_btn.add_css_class("go");
    let macro_progress = lbl("", "mn-macro-s").ellipsize().visible(false);
    match &macro_shown {
        Some(m) => {
            macro_btn.connect_clicked({
                let w = w.weak();
                move |_| {
                    if let Some(w) = w.upgrade() {
                        w.play_macro_here(id);
                    }
                }
            });
            let t = gtk::Box::new(Orientation::Vertical, 0);
            t.set_valign(Align::Center);
            t.append(&lbl(m, "mn-macro-t").ellipsize());
            t.append(&macro_progress);
            macro_cell.append(&macro_btn);
            macro_cell.append(&t);
        }
        None => macro_cell.append(&lbl("—", "mn-dash")),
    }
    row.append(&cols.cell(Col::Macro, &macro_cell));
    let perf = acct.performance();
    let perf_short = match perf {
        Performance::Low => "Low",
        Performance::Medium => "Med",
        Performance::High => "High",
        Performance::Max => "Max",
    };
    row.append(&cols.cell(
        Col::Perf,
        &lbl(perf_short, "mn-perf").tip(&format!("Performance: {}", perf.label())),
    ));
    row.append(&cols.cell(
        Col::Last,
        &lbl(&short_since(acct.last_launch.as_deref()), "mn-last").tip("Last launched"),
    ));

    // -- actions -------------------------------------------------------------------
    let acts = gtk::Box::new(Orientation::Horizontal, 0);
    acts.set_halign(Align::End);
    acts.set_hexpand(true);
    let hide = ds::ib("eye-off", "Hide its window", true);
    hide.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                let hide = can_hide(&w.state(), id);
                w.set_window_hidden(id, hide);
            }
        }
    });
    let play = ds::ib("play", "Launch into the target", true);
    play.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                w.play_or_stop(id);
            }
        }
    });
    let model = menu(w, acct);
    let more = gtk::MenuButton::builder()
        .child(&ds::icon("more"))
        .menu_model(&model)
        .valign(Align::Center)
        .tooltip_text(format!("More for {}", acct.name))
        .css_classes(["ib", "sm", "mn-more"])
        .build();
    name(&more, &format!("More for {}", acct.name));
    acts.append(&hide);
    acts.append(&play);
    acts.append(&more);
    row.append(&cols.cell(Col::Actions, &acts));
    context_menu(&row, &model);
    if !acct.leader {
        drag_and_drop(w, &row, id);
    }

    // -- what opens under it ------------------------------------------------------------
    let reason_text = lbl("", "").wrapped().hexpand();
    let reason = gtk::Box::new(Orientation::Horizontal, 8).css("mn-reason");
    reason.append(&ds::icon("alert").css("s16"));
    reason.append(&reason_text);
    let reason_open = gtk::Revealer::builder().child(&reason).reveal_child(false).build();
    let again = ds::Button::new("Try again", Variant::Secondary, true)
        .on(w.act(move |w| w.play_or_stop(id)));
    let details = ds::Button::new("Details", Variant::Ghost, true).action("win.activity-log");
    let dismiss = ds::ib("x", "Dismiss", true);
    {
        let r = reason_open.clone();
        dismiss.connect_clicked(move |_| r.set_reveal_child(false));
    }
    for b in [&again.button, &details.button, &dismiss] {
        b.set_valign(Align::Center);
        reason.append(b);
    }
    let confirm = confirm_row(
        &format!("Remove {}?", acct.name),
        "Its session leaves the keyring and its client is closed. The Roblox account itself is \
         untouched; add it again any time with Quick Login.",
        "Remove",
        w.act(move |w| w.remove_account(id)),
    );
    w.register_confirm(id, &confirm);

    let leader = acct.leader;
    let shown = row.clone();
    let reason_shown = reason_open.clone();
    let failed_btn = {
        let r = reason_open.clone();
        move || {
            let b = gtk::Button::builder().css_classes(["mn-badge-btn"]).build();
            let inner = gtk::Box::new(Orientation::Horizontal, 4);
            inner.append(&ds::badge(Tone::Danger, "Launch failed", false, false));
            inner.append(&ds::icon("chev-down").css("s14"));
            b.set_child(Some(&inner));
            b.set_tooltip_text(Some("Why it did not launch"));
            let r = r.clone();
            b.connect_clicked(move |_| r.set_reveal_child(!r.reveals_child()));
            b
        }
    };
    w.watch_accounts(Box::new(move |s| {
        let chip = s.chip(id);
        clear(&status);
        let failure = s.failures.get(&id);
        match chip {
            Chip::Running => status.append(&ds::badge(Tone::Success, "Running", true, false)),
            Chip::Starting => status.append(&ds::badge(Tone::Warning, "Starting…", true, true)),
            Chip::Joining => status.append(&ds::badge(Tone::Warning, "Joining…", true, true)),
            Chip::Expired => {
                let b = gtk::Button::builder()
                    .child(&ds::badge(Tone::Danger, "Session expired", true, false))
                    .css_classes(["mn-badge-btn"])
                    .tooltip_text("Roblox no longer takes this session: sign in again")
                    .build();
                b.set_action_name(Some("win.sign-in-again"));
                b.set_action_target_value(Some(&id.0.to_variant()));
                status.append(&b);
            }
            Chip::Idle if failure.is_some() => status.append(&failed_btn()),
            Chip::Idle if s.accounts.session(id) == SessionState::Checking => {
                status.append(&ds::badge(Tone::Neutral, "Checking", true, true));
            }
            Chip::Idle => status.append(&lbl("Idle", "mn-idle")),
        }
        match failure {
            Some(why) => reason_text.set_label(&sentence(why)),
            None => reason_open.set_reveal_child(false),
        }
        let live = chip == Chip::Running;
        ds::set_ib(
            &play,
            if live { "stop" } else { "play" },
            if live {
                "Close this account's client"
            } else if leader {
                "Launch the leader, then its auto-join accounts into its server"
            } else {
                "Launch into the target"
            },
        );
        toggle_class(&play, "stop", live);
        toggle_class(&play, "go", !live);
        play.set_sensitive(!matches!(chip, Chip::Starting | Chip::Joining));
        let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
        hide.set_visible(hidden || can_hide(s, id));
        hidden_mark.set_visible(hidden);
        ds::set_ib(
            &hide,
            if hidden { "eye-off" } else { "eye" },
            if hidden {
                "Show this account's window"
            } else {
                "Hide its window; the game keeps running"
            },
        );
        toggle_class(&hide, "on", hidden);
        // A macro playing here: its stop, and where it is.
        let playing = s.macro_runs.get(&id);
        ds::set_ib(
            &macro_btn,
            if playing.is_some() { "stop" } else { "play" },
            if playing.is_some() { "Stop its macro" } else { "Play its macro" },
        );
        macro_btn.set_sensitive(playing.is_some() || live);
        match playing {
            Some(_) => {
                let at = s.macro_progress.get(&id).map_or("starting", String::as_str);
                macro_progress.set_label(at);
                macro_progress.set_visible(true);
            }
            None => macro_progress.set_visible(false),
        }
        if let Some(a) = s.accounts.get(id) {
            if a.selected != check.is_active() {
                quiet.set(true);
                check.set_active(a.selected);
                quiet.set(false);
            }
            toggle_class(&shown, "sel", a.selected);
        }
        toggle_class(&shown, "current", s.inspected == Some(Inspected::Account(id)));
    }));
    let block = gtk::Box::new(Orientation::Vertical, 0);
    block.append(&row);
    block.append(&reason_shown);
    block.append(&confirm);
    block
}

/// A confirmation that opens under a row or band: what it asks, Cancel,
/// and the destructive button.
pub fn confirm_row(title: &str, body: &str, verb: &str, run: impl Fn() + 'static) -> gtk::Revealer {
    let text = gtk::Label::new(None);
    text.set_markup(&format!(
        "<b>{}</b> {}",
        glib::markup_escape_text(title),
        glib::markup_escape_text(body)
    ));
    text.set_wrap(true);
    text.set_xalign(0.0);
    text.set_hexpand(true);
    let confirm = gtk::Box::new(Orientation::Horizontal, 12).css("cx-confirm");
    confirm.append(&text);
    let revealer = gtk::Revealer::new();
    let cancel = ds::Button::new("Cancel", Variant::Secondary, true).on({
        let r = revealer.clone();
        move || r.set_reveal_child(false)
    });
    let go = ds::Button::with_icons(verb, Variant::Danger, true, Some("trash"), None).on({
        let r = revealer.clone();
        move || {
            r.set_reveal_child(false);
            run();
        }
    });
    for b in [&cancel.button, &go.button] {
        b.set_valign(Align::Center);
        confirm.append(b);
    }
    let holder = gtk::Box::new(Orientation::Horizontal, 0).css("mn-reason plain");
    holder.append(&confirm.hexpand());
    revealer.set_child(Some(&holder));
    revealer
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
    model.append_item(&item("Settings", "win.account-settings"));
    if !acct.leader {
        let layout = gio::Menu::new();
        layout.append_item(&item("Make leader", "win.make-leader"));
        if has_leader {
            let follow =
                if acct.follow.is_some() { "Stop auto-joining" } else { "Auto-join the leader" };
            layout.append_item(&item(follow, "win.toggle-follow"));
        }
        model.append_section(None, &layout);
    }
    let places = gio::Menu::new();
    for (gid, name) in groups.into_iter().filter(|(gid, _)| *gid != current) {
        // A menu label's underscores are mnemonics; a name's are text.
        let entry = gio::MenuItem::new(Some(&name.replace('_', "__")), None);
        entry.set_action_and_target_value(Some("win.move-account"), Some(&(id, gid).to_variant()));
        places.append_item(&entry);
    }
    if places.n_items() > 0 {
        model.append_section(Some("Move to"), &places);
    }
    let session = gio::Menu::new();
    session.append_item(&item("Check session", "win.check-session"));
    session.append_item(&item("Sign in again…", "win.sign-in-again"));
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
fn drag_and_drop(w: &Window, row: &gtk::Box, id: UserId) {
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

fn mark(row: &gtk::Box, place: Option<&str>) {
    for c in ["drop-above", "drop-below"] {
        toggle_class(row, c, Some(c) == place);
    }
}
