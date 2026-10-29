//! The sidebar beside the table: the accounts by status, each group, the
//! macros and the advanced macros' scripts (each run or stopped from here),
//! and how much of the machine the clients take. A status or a group is a
//! view of the table.

use std::cell::RefCell;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, gdk, gio, glib};
use rbxmgr_core::accounts::Account;
use rbxmgr_core::macros::script::{self, Script};

use super::{WeakWindow, Window};
use crate::state::{AppState, View};
use crate::ui::ds;
use crate::ui::macros::editor::MacroDialog;
use crate::ui::table::format::bytes;
use crate::ui::table::row::{context_menu, dragged_id};
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, name, plural, toggle_class};

/// One entry: the button, the count beside it, and the view it shows.
struct Entry {
    button: gtk::Button,
    count: gtk::Label,
    view: View,
}

/// One macro's or script's entry: its run or stop, and where it plays.
struct MacroEntry {
    /// What its runs are called in `macro_runs`.
    name: String,
    run: gtk::Button,
    at: gtk::Label,
}

pub struct Sidebar {
    pub root: gtk::Box,
    status: Vec<Entry>,
    groups_box: gtk::Box,
    groups: RefCell<Vec<Entry>>,
    macros_box: gtk::Box,
    macros: RefCell<Vec<MacroEntry>>,
    scripts_box: gtk::Box,
    scripts: RefCell<Vec<MacroEntry>>,
    cpu: gtk::ProgressBar,
    cpu_text: gtk::Label,
    memory: gtk::ProgressBar,
    memory_text: gtk::Label,
    machine_note: gtk::Label,
}

impl Sidebar {
    pub fn build(w: &WeakWindow) -> Self {
        let list = gtk::Box::new(Orientation::Vertical, 2);
        list.append(&heading("Status", None));
        let status: Vec<Entry> = [
            (View::All, "view-list", "All accounts"),
            (View::Attention, "error", "Needs attention"),
            (View::Running, "play-circle", "Running"),
            (View::Launching, "hourglass", "Launching"),
            (View::Idle, "idle", "Idle"),
        ]
        .into_iter()
        .map(|(view, icon, text)| {
            let mark = ds::icon(icon);
            if view == View::Attention {
                mark.add_css_class("bad");
            }
            entry(w, view, mark.upcast_ref(), text)
        })
        .collect();
        for e in &status {
            list.append(&e.button);
        }
        list.append(&heading("Groups", None));
        let groups_box = gtk::Box::new(Orientation::Vertical, 2);
        list.append(&groups_box);
        let add = ds::ib("plus", "New macro (Ctrl+Shift+N)", true);
        add.set_action_name(Some("win.new-macro"));
        list.append(&heading("Macros", Some(add.upcast_ref())));
        let macros_box = gtk::Box::new(Orientation::Vertical, 2);
        list.append(&macros_box);
        let scripts_end = gtk::Box::new(Orientation::Horizontal, 0);
        for (icon, tip, action) in [
            ("refresh", "Look in the scripts folder again", "win.reload-scripts"),
            ("external", "Open the scripts folder", "win.open-scripts"),
        ] {
            let b = ds::ib(icon, tip, true);
            b.set_action_name(Some(action));
            scripts_end.append(&b);
        }
        list.append(&heading("Scripts", Some(scripts_end.upcast_ref())));
        let scripts_box = gtk::Box::new(Orientation::Vertical, 2);
        list.append(&scripts_box);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();

        let machine = gtk::Box::new(Orientation::Vertical, 10).css("sb-machine");
        machine.append(&lbl("Machine", "sb-h"));
        let (cpu, cpu_text) = (bar(), lbl("—", "sb-v"));
        let (memory, memory_text) = (bar(), lbl("—", "sb-v"));
        for (label, b, v) in [("CPU", &cpu, &cpu_text), ("Memory", &memory, &memory_text)] {
            let line = gtk::Box::new(Orientation::Horizontal, 10);
            line.append(&lbl(label, "sb-k").width(56));
            line.append(&b.clone().hexpand());
            line.append(v);
            machine.append(&line);
        }
        let machine_note = lbl("", "mn-dim sb-note").wrapped().chars(26);
        machine.append(&machine_note);

        let root = gtk::Box::new(Orientation::Vertical, 0).css("sb");
        // Its own width, set: its bars expand inside it, and would otherwise
        // have it share the window's spare width with the table.
        root.set_hexpand(false);
        root.set_width_request(240);
        name(&root, "Views of the accounts, the macros and the machine");
        root.append(&scroller);
        root.append(&machine);
        Sidebar {
            root,
            status,
            groups_box,
            groups: RefCell::default(),
            macros_box,
            macros: RefCell::default(),
            scripts_box,
            scripts: RefCell::default(),
            cpu,
            cpu_text,
            memory,
            memory_text,
            machine_note,
        }
    }

    /// The groups' entries, afresh: each group, then Ungrouped when any
    /// account is in none.
    pub fn draw_groups(&self, w: &Window) {
        clear(&self.groups_box);
        let mut made = Vec::new();
        let (groups, any_ungrouped) = {
            let s = w.state();
            let a = &s.accounts;
            let groups: Vec<(String, String, Option<std::path::PathBuf>, Option<String>)> = a
                .groups()
                .iter()
                .map(|g| {
                    let tile = g
                        .place_id
                        .as_ref()
                        .and_then(|p| s.game_list.iter().find(|t| &t.game.place_id == p));
                    let name = if g.name.is_empty() {
                        "Untitled group".to_owned()
                    } else {
                        g.name.clone()
                    };
                    (
                        g.id.clone(),
                        name,
                        g.place_id.as_ref().and_then(|p| w.game_icon(p, g.universe_id.as_deref())),
                        tile.map(|t| t.game.name.clone()).or_else(|| g.game.clone()),
                    )
                })
                .collect();
            let any = a.accounts().iter().any(|x| !x.leader && a.group_of(x).is_none());
            (groups, any)
        };
        let weak = w.weak();
        for (gid, title, icon, game) in groups {
            let art = match &game {
                Some(g) => ds::art(g, icon.as_deref(), 18),
                None => ds::art_icon("gamepad", 18),
            };
            let e = entry(&weak, View::Group(gid.clone()), &art, &title);
            drop_into_group(w, &e.button, Some(gid.clone()));
            let menu = gio::Menu::new();
            for (label, action) in [
                ("Name and game", "win.group-settings"),
                ("Select its accounts", "win.select-group"),
                ("Delete group…", "win.delete-group"),
            ] {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(Some(action), Some(&gid.to_variant()));
                menu.append_item(&item);
            }
            context_menu(&e.button, &menu);
            self.groups_box.append(&e.button);
            made.push(e);
        }
        if any_ungrouped {
            let e = entry(
                &weak,
                View::Group(String::new()),
                ds::icon("users").upcast_ref(),
                "Ungrouped",
            );
            drop_into_group(w, &e.button, None);
            self.groups_box.append(&e.button);
            made.push(e);
        }
        let new = gtk::Button::builder()
            .css_classes(["sb-it", "sb-new"])
            .action_name("win.new-group")
            .build();
        let inner = gtk::Box::new(Orientation::Horizontal, 10);
        inner.append(&ds::icon("plus"));
        inner.append(&lbl("New group", ""));
        new.set_child(Some(&inner));
        self.groups_box.append(&new);
        self.groups.replace(made);
    }

    /// The macros' entries, afresh.
    pub fn draw_macros(&self, w: &Window) {
        clear(&self.macros_box);
        let names: Vec<String> = w.state().macros.names().map(str::to_owned).collect();
        let mut made = Vec::new();
        for m in names {
            let run = ds::ib("play-circle", &format!("Run {m} on the selected accounts"), true);
            run.add_css_class("sb-run");
            run.set_action_name(Some("app.run-macro"));
            run.set_action_target_value(Some(&m.to_variant()));
            let open = gtk::Button::builder()
                .child(&lbl(&m, "").ellipsize().chars(16))
                .css_classes(["sb-it", "sb-macro"])
                .hexpand(true)
                .tooltip_text("Edit it")
                .build();
            let edit = m.clone();
            let weak = w.weak();
            open.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() {
                    MacroDialog::open(&w, Some(&edit));
                }
            });
            let at = lbl("", "sb-count");
            let line = gtk::Box::new(Orientation::Horizontal, 0).css("sb-mline");
            line.append(&run);
            line.append(&open);
            line.append(&at);
            let menu = gio::Menu::new();
            for (label, action) in [
                ("Run or stop on the selected", "app.run-macro"),
                ("Edit…", "win.edit-macro"),
                ("Switch on or off", "win.toggle-macro"),
            ] {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(Some(action), Some(&m.to_variant()));
                menu.append_item(&item);
            }
            context_menu(&line, &menu);
            self.macros_box.append(&line);
            made.push(MacroEntry { name: m, run, at });
        }
        if made.is_empty() {
            self.macros_box.append(&lbl("No macros yet: + makes one.", "mn-dim sb-note").wrapped());
        }
        self.macros.replace(made);
    }

    /// The scripts' entries, afresh, from what the scripts folder holds or
    /// why it could not be read.
    pub fn draw_scripts(&self, found: Result<Vec<Script>, String>) {
        clear(&self.scripts_box);
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                self.scripts_box.append(&lbl(&e, "mn-dim sb-note").wrapped());
                self.scripts.replace(Vec::new());
                return;
            }
        };
        let mut made = Vec::new();
        for s in found {
            let target = s.name.to_variant();
            let run_name = script::run_name(&s.name);
            let run =
                ds::ib("play-circle", &format!("Run {run_name} on the selected accounts"), true);
            run.add_css_class("sb-run");
            run.set_action_name(Some("win.run-script"));
            run.set_action_target_value(Some(&target));
            let open = gtk::Button::builder()
                .child(&lbl(&run_name, "").ellipsize().chars(16))
                .css_classes(["sb-it", "sb-macro"])
                .hexpand(true)
                .tooltip_text(s.about.as_deref().unwrap_or("Open it in your editor"))
                .action_name("win.edit-script")
                .action_target(&target)
                .build();
            let at = lbl("", "sb-count");
            let line = gtk::Box::new(Orientation::Horizontal, 0).css("sb-mline");
            line.append(&run);
            line.append(&open);
            line.append(&at);
            let menu = gio::Menu::new();
            for (label, action) in [
                ("Run or stop on the selected", "win.run-script"),
                ("Open in your editor", "win.edit-script"),
            ] {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_action_and_target_value(Some(action), Some(&target));
                menu.append_item(&item);
            }
            context_menu(&line, &menu);
            self.scripts_box.append(&line);
            made.push(MacroEntry { name: run_name, run, at });
        }
        if made.is_empty() {
            self.scripts_box.append(
                &lbl("No scripts yet: put a Python file in the folder.", "mn-dim sb-note")
                    .wrapped(),
            );
        }
        self.scripts.replace(made);
    }

    /// Counts, which view is on, where each macro plays, and the machine.
    pub fn draw(&self, s: &AppState) {
        let c = s.counts();
        for e in &self.status {
            let n = match e.view {
                View::All => c.all,
                View::Attention => c.attention,
                View::Running => c.running,
                View::Launching => c.launching,
                View::Idle => c.idle,
                View::Group(_) => 0,
            };
            e.count.set_label(&n.to_string());
            if e.view == View::Attention {
                toggle_class(&e.count, "sb-pill", n > 0);
            }
            toggle_class(&e.button, "on", s.view == e.view);
        }
        for e in self.groups.borrow().iter() {
            let View::Group(gid) = &e.view else { continue };
            let members: Vec<&Account> = s
                .accounts
                .accounts()
                .iter()
                .filter(|a| !a.leader && s.accounts.group_of(a).unwrap_or_default() == gid)
                .collect();
            let up = members.iter().filter(|a| s.running.contains(&a.user_id)).count();
            e.count.set_label(&format!("{up}/{}", members.len()));
            toggle_class(&e.button, "on", s.view == e.view);
        }
        for m in self.macros.borrow().iter() {
            let on = m.draw_run(s);
            let enabled = s.macros.enabled(&m.name);
            m.at.set_label(&if on > 0 {
                format!("on {on}")
            } else if enabled {
                String::new()
            } else {
                "off".to_owned()
            });
        }
        for m in self.scripts.borrow().iter() {
            let on = m.draw_run(s);
            m.at.set_label(&if on > 0 { format!("on {on}") } else { String::new() });
        }
        let clients = s.running.len();
        match &s.machine {
            Some(m) => {
                let cpu = m.cpu.unwrap_or(0.0);
                self.cpu.set_fraction(f64::from(cpu) / 100.0);
                self.cpu_text
                    .set_label(&m.cpu.map_or_else(|| "—".to_owned(), |c| format!("{c:.0}%")));
                let total = m.memory_total.max(1);
                self.memory.set_fraction(m.memory_used as f64 / total as f64);
                self.memory_text.set_label(&bytes(m.memory_used));
                self.machine_note.set_label(&format!(
                    "of {} · {}",
                    bytes(m.memory_total),
                    plural(clients, "client", "clients")
                ));
            }
            None => {
                self.cpu_text.set_label("—");
                self.memory_text.set_label("—");
                self.machine_note.set_label(&plural(clients, "client", "clients"));
            }
        }
    }
}

impl MacroEntry {
    /// Its run as a stop while it plays anywhere; on how many clients.
    fn draw_run(&self, s: &AppState) -> usize {
        let on = s.macro_runs.values().filter(|(_, n)| n == &self.name).count();
        ds::set_ib(
            &self.run,
            if on > 0 { "stop-circle" } else { "play-circle" },
            &if on > 0 {
                format!("Stop {} everywhere it plays", self.name)
            } else {
                format!("Run {} on the selected accounts", self.name)
            },
        );
        toggle_class(&self.run, "playing", on > 0);
        on
    }
}

/// A small heading, with a button at its end when given.
fn heading(text: &str, end: Option<&gtk::Widget>) -> gtk::Box {
    let h = gtk::Box::new(Orientation::Horizontal, 0).css("sb-h-row");
    h.append(&lbl(text, "sb-h").hexpand());
    if let Some(e) = end {
        h.append(e);
    }
    h
}

/// A view's entry: its mark, its words and its count; a click shows it.
fn entry(w: &WeakWindow, view: View, mark: &gtk::Widget, text: &str) -> Entry {
    let count = lbl("", "sb-count");
    let inner = gtk::Box::new(Orientation::Horizontal, 10);
    inner.append(mark);
    inner.append(&lbl(text, "").ellipsize().chars(18).hexpand());
    inner.append(&count);
    let button = gtk::Button::builder().child(&inner).css_classes(["sb-it"]).build();
    name(&button, text);
    let (weak, shown) = (w.clone(), view.clone());
    button.connect_clicked(move |_| {
        if let Some(w) = weak.upgrade() {
            w.set_view(shown.clone());
        }
    });
    Entry { button, count, view }
}

fn bar() -> gtk::ProgressBar {
    let b = gtk::ProgressBar::new();
    b.add_css_class("thin");
    b.set_valign(Align::Center);
    b
}

/// Dropping an account on a group's entry moves it into that group.
fn drop_into_group(w: &Window, target: &gtk::Button, gid: Option<String>) {
    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    let weak = w.weak();
    drop.connect_drop(move |_, value, _, _| {
        if let (Some(id), Some(w)) = (dragged_id(Some(value)), weak.upgrade()) {
            let gid = gid.clone();
            glib::idle_add_local_once(move || w.set_group(id, gid));
        }
        true
    });
    target.add_controller(drop);
}
