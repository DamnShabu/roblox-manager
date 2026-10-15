//! One account as a table row: select it, see how it is (status, how long
//! it has run, its CPU and memory, its macro, its performance), hide its
//! window, launch or stop it, and a menu for the rest (also on a right
//! click). A click on the row shows it in the details pane. Every account
//! but the main one can be dragged onto another row or a group's row.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, gdk, gio, glib};
use rbxmgr_core::accounts::{Account, SessionState};
use rbxmgr_core::cordial::Window as ClientWindow;
use rbxmgr_core::types::UserId;

use super::format::{bytes, last_ran, percent, uptime};
use super::perf::PerfCell;
use super::{Col, row_button, table_row};
use crate::state::{AppState, Chip, Inspected};
use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name, toggle_class};
use crate::ui::window::{Window, can_hide, monitor_hz};

/// A client's CPU share from which its row says it is heavy, in percent.
const HEAVY_CPU: f32 = 15.0;
/// And its memory, in bytes.
const HEAVY_MEMORY: u64 = 1024 * 1024 * 1024;

pub fn account_row(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let cols = w.columns();
    let hz = monitor_hz();
    let picture = w.services().avatars.cached(&id.to_string());
    let row = table_row("mn-row");

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

    let (who, hidden_mark) = identity(acct, picture.as_deref());
    row.append(&cols.cell(Col::Name, &who));

    let dot = gtk::Box::new(Orientation::Horizontal, 0).css("mn-dot");
    dot.set_valign(Align::Center);
    let state = lbl("", "mn-state");
    let status = gtk::Box::new(Orientation::Horizontal, 8);
    status.append(&dot);
    status.append(&state);
    row.append(&cols.cell(Col::Status, &status));
    let ran = lbl("", "mn-num");
    let cpu = lbl("", "mn-num");
    let memory = lbl("", "mn-num");
    row.append(&cols.cell(Col::Uptime, &ran));
    row.append(&cols.cell(Col::Cpu, &cpu));
    row.append(&cols.cell(Col::Memory, &memory));
    let macro_cell = MacroCell::build(w, acct);
    row.append(&cols.cell(Col::Macro, &macro_cell.root));
    let perf = PerfCell::build(id, acct.name.as_str(), hz);
    row.append(&cols.cell(Col::Perf, &perf.root));

    let hide = row_button("eye", "Hide its window");
    hide.connect_clicked({
        let w = w.weak();
        move |_| {
            if let Some(w) = w.upgrade() {
                let hide = can_hide(&w.state(), id);
                w.set_window_hidden(id, hide);
            }
        }
    });
    let play = row_button("play-filled", "Launch into the target");
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
        .css_classes(["ib", "sm", "mn-act"])
        .build();
    name(&more, &format!("More for {}", acct.name));
    let acts = gtk::Box::new(Orientation::Horizontal, 2);
    acts.set_halign(Align::End);
    acts.append(&hide);
    acts.append(&play);
    acts.append(&more);
    row.append(&cols.cell(Col::Actions, &acts));
    context_menu(&row, &model);
    inspect_on_click(w, &row, id);
    if !acct.leader {
        drag_and_drop(w, &row, id);
    }

    let confirm = confirm_row(
        &format!("Remove {}?", acct.name),
        "Its session leaves the keyring and its client is closed. The Roblox account itself is \
         untouched; add it again any time with Quick Login.",
        "Remove",
        w.act(move |w| w.remove_account(id)),
    );
    w.register_confirm(id, &confirm);

    let shown = row.clone();
    let last_launch = acct.last_launch.clone();
    w.watch_accounts(Box::new(move |s| {
        let chip = s.chip(id);
        let (text, kind) = status_of(s, id, chip);
        state.set_label(text);
        for k in ["run", "start", "bad", "idle"] {
            toggle_class(&dot, k, k == kind);
            toggle_class(&state, k, k == kind);
        }
        let usage = s.usage.get(&id).filter(|_| chip == Chip::Running);
        match usage {
            Some(u) => {
                ran.set_label(&uptime(u.uptime));
                cpu.set_label(&u.cpu.map_or_else(|| "—".to_owned(), percent));
                memory.set_label(&bytes(u.memory));
            }
            None if chip == Chip::Running => {
                for l in [&ran, &cpu, &memory] {
                    l.set_label("—");
                }
            }
            None => {
                // Uptime is for what runs: when an idle one last did is its tip.
                for l in [&ran, &cpu, &memory] {
                    l.set_label("—");
                }
            }
        }
        ran.set_tooltip_text(
            match usage {
                Some(_) => None,
                None => Some(format!("Last launched: {}", last_ran(last_launch.as_deref()))),
            }
            .as_deref(),
        );
        toggle_class(&ran, "mn-dim", usage.is_none());
        toggle_class(&cpu, "heavy", usage.and_then(|u| u.cpu).is_some_and(|c| c >= HEAVY_CPU));
        toggle_class(&cpu, "mn-dim", usage.and_then(|u| u.cpu).is_none());
        toggle_class(&memory, "heavy", usage.is_some_and(|u| u.memory >= HEAVY_MEMORY));
        toggle_class(&memory, "mn-dim", usage.is_none());
        macro_cell.draw(s, id, chip == Chip::Running);
        perf.draw(s, id, hz);
        let live = chip == Chip::Running;
        ds::set_ib(
            &play,
            if live { "stop-filled" } else { "play-filled" },
            if live { "Close this account's client" } else { "Launch into the target" },
        );
        play.set_sensitive(!matches!(chip, Chip::Starting | Chip::Joining));
        let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
        hide.set_visible(hidden || can_hide(s, id));
        hidden_mark.set_visible(hidden);
        ds::set_ib(
            &hide,
            if hidden { "eye-off" } else { "eye" },
            if hidden { "Show its window" } else { "Hide its window; the game keeps running" },
        );
        toggle_class(&hide, "on", hidden);
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
    block.append(&confirm);
    block
}

/// Who it is: its picture, label, the star of the main account, its Roblox
/// name, its note and whether its window is hidden. Returns the hidden mark,
/// which shows while it is.
fn identity(acct: &Account, picture: Option<&std::path::Path>) -> (gtk::Box, gtk::Image) {
    let who = gtk::Box::new(Orientation::Horizontal, 8);
    who.append(&ds::av(acct.name.as_str(), picture, 26));
    who.append(&lbl(acct.name.as_str(), "mn-name").ellipsize());
    if acct.leader {
        who.append(&ds::icon("star-filled").css("mn-star s16").tip("Your main account"));
    }
    let user = acct.username.clone().unwrap_or_else(|| acct.name.to_string());
    who.append(&lbl(&format!("@{user}"), "mn-user").ellipsize());
    let note = acct.note.trim();
    if !note.is_empty() {
        who.append(&ds::icon("note").css("mn-dim s16").tip(note));
    }
    let hidden = ds::icon("eye-off").css("mn-dim s16").tip("Window hidden; the game keeps running");
    who.append(&hidden);
    (who, hidden)
}

/// The words and look of its status.
fn status_of(s: &AppState, id: UserId, chip: Chip) -> (&'static str, &'static str) {
    match chip {
        Chip::Running => ("Running", "run"),
        Chip::Starting => ("Starting", "start"),
        Chip::Joining => ("Joining", "start"),
        Chip::Expired => ("Signed out", "bad"),
        Chip::Idle if s.failures.contains_key(&id) => ("Failed", "bad"),
        Chip::Idle if s.accounts.session(id) == SessionState::Checking => ("Checking", "start"),
        Chip::Idle => ("Idle", "idle"),
    }
}

/// Its macro: run or stop it, and where it is; or, with none picked, a
/// menu to pick one.
struct MacroCell {
    root: gtk::Box,
    run: Option<gtk::Button>,
    at: gtk::Label,
}

impl MacroCell {
    fn build(w: &Window, acct: &Account) -> Self {
        let id = acct.user_id;
        let root = gtk::Box::new(Orientation::Horizontal, 6).css("mn-macro");
        let at = lbl("", "mn-dim").ellipsize();
        let names: Vec<String> = w.state().macros.names().map(str::to_owned).collect();
        let picked = acct.macro_name.clone().filter(|m| names.contains(m));
        let Some(m) = picked else {
            let menu = gio::Menu::new();
            for n in &names {
                let item = gio::MenuItem::new(Some(&n.replace('_', "__")), None);
                item.set_action_and_target_value(
                    Some("win.set-macro"),
                    Some(&(id.0, n.clone()).to_variant()),
                );
                menu.append_item(&item);
            }
            let new = gio::Menu::new();
            new.append(Some("New macro…"), Some("win.new-macro"));
            menu.append_section(None, &new);
            let inner = gtk::Box::new(Orientation::Horizontal, 6);
            inner.append(&lbl("Not set", ""));
            inner.append(&ds::icon("chev-down").css("mn-dim s16"));
            let add = gtk::MenuButton::builder()
                .child(&inner)
                .menu_model(&menu)
                .valign(Align::Center)
                .css_classes(["mn-quiet"])
                .tooltip_text("Pick the macro its Run plays")
                .build();
            root.append(&add);
            return MacroCell { root, run: None, at };
        };
        let run = row_button("play-circle", "Play its macro");
        run.connect_clicked({
            let w = w.weak();
            move |_| {
                if let Some(w) = w.upgrade() {
                    w.play_macro_here(id);
                }
            }
        });
        root.append(&run);
        root.append(&lbl(&m, "mn-macro-n").ellipsize());
        root.append(&at);
        MacroCell { root, run: Some(run), at }
    }

    /// Playing or not, and where.
    fn draw(&self, s: &AppState, id: UserId, live: bool) {
        let Some(run) = &self.run else { return };
        let playing = s.macro_runs.contains_key(&id);
        ds::set_ib(
            run,
            if playing { "stop-circle" } else { "play-circle" },
            if playing { "Stop its macro" } else { "Play its macro" },
        );
        toggle_class(run, "playing", playing);
        run.set_sensitive(playing || live);
        let at = s.macro_progress.get(&id).map(|p| short_progress(p)).unwrap_or("starting");
        self.at.set_label(if playing { at } else { "" });
        self.at.set_visible(playing);
    }
}

/// Where a macro is, as short as the column: "round 2, step 3/7: pressing
/// e" as "round 2, step 3/7".
fn short_progress(p: &str) -> &str {
    p.split_once(':').map_or(p, |(head, _)| head)
}

/// A confirmation that opens under a row or a group's row: what it asks,
/// Cancel, and the destructive button.
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
    let holder = gtk::Box::new(Orientation::Horizontal, 0).css("mn-confirm");
    holder.append(&confirm.hexpand());
    revealer.set_child(Some(&holder));
    revealer
}

/// The row's menu: window actions, aimed at this account.
pub fn menu(w: &Window, acct: &Account) -> gio::Menu {
    let id = acct.user_id.0;
    let (groups, current) = {
        let s = w.state();
        let groups: Vec<(String, String)> = std::iter::once((String::new(), "Ungrouped".into()))
            .chain(s.accounts.groups().iter().map(|g| {
                let name = if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                (g.id.clone(), name)
            }))
            .collect();
        let current = s.accounts.group_of(acct).unwrap_or_default().to_owned();
        (groups, current)
    };
    let item = |label: &str, action: &str| {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some(action), Some(&id.to_variant()));
        item
    };
    let model = gio::Menu::new();
    model.append_item(&item("Settings", "win.account-settings"));
    if !acct.leader {
        model.append_item(&item("Make main account", "win.make-main"));
    }
    let places = gio::Menu::new();
    if !acct.leader {
        for (gid, name) in groups.into_iter().filter(|(gid, _)| *gid != current) {
            // A menu label's underscores are mnemonics; a name's are text.
            let entry = gio::MenuItem::new(Some(&name.replace('_', "__")), None);
            entry.set_action_and_target_value(
                Some("win.move-account"),
                Some(&(id, gid).to_variant()),
            );
            places.append_item(&entry);
        }
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

/// A click on the row (not on one of its buttons) shows it in the details
/// pane.
pub(super) fn inspect_on_click(w: &Window, row: &gtk::Box, id: UserId) {
    let click = gtk::GestureClick::builder().button(gdk::BUTTON_PRIMARY).build();
    let weak = w.weak();
    click.connect_released(move |g, _, _, _| {
        g.set_state(gtk::EventSequenceState::Claimed);
        if let Some(w) = weak.upgrade() {
            w.inspect(Some(Inspected::Account(id)));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_cut_before_what_the_step_does() {
        assert_eq!(short_progress("round 2, step 3/7: pressing e"), "round 2, step 3/7");
        assert_eq!(short_progress("starting"), "starting");
    }
}
