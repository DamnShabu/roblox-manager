//! Editing one macro as steps. Saving writes the same text the player has
//! always read; the macro library checks it parses first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType, gdk, glib};
use rbxmgr_core::macros::grammar::{self, Row};

use super::record::Record;
use crate::ui::confirm;
use crate::ui::ds::{self, Variant};
use crate::ui::panel::Panel;
use crate::ui::widgets::{Fluent, LabelFluent, hotkey_label, keycaps, lbl, sentence};
use crate::ui::window::{WeakWindow, Window};

pub struct MacroDialog {
    window: WeakWindow,
    dialog: Panel,
    old: Option<String>,
    rows: RefCell<Vec<Row>>,
    loops: Cell<u32>,
    hotkey: RefCell<Option<String>>,
    capturing: Cell<bool>,
    name: gtk::Entry,
    cap: gtk::Button,
    clear_key: gtk::Button,
    repeat: adw::ToggleGroup,
    rounds: gtk::SpinButton,
    count: gtk::Label,
    list: gtk::ListBox,
    /// Steps as rows, or the macro as text to paste or copy.
    view: adw::ToggleGroup,
    views: gtk::Stack,
    text: gtk::TextView,
    repeat_group: gtk::Box,
    err: gtk::Label,
    /// Steps recorded from a running client.
    record: Rc<Record>,
}

impl MacroDialog {
    /// Edit macro `name`, or start a new one.
    pub fn open(w: &Window, name: Option<&str>) {
        let (text, title, hotkey) = {
            let s = w.state();
            match name {
                Some(n) => (
                    s.macros.text(n).unwrap_or_default().to_owned(),
                    n.to_owned(),
                    s.macros.hotkey(n).map(str::to_owned),
                ),
                None => {
                    let title = (1..)
                        .map(|n| format!("Macro {n}"))
                        .find(|t| !s.macros.contains(t))
                        .unwrap_or_default();
                    // Roblox kicks after 20 idle minutes; the starter keeps a client in.
                    ("# anti-AFK\ntap space\nwait 60-240\n".to_owned(), title, None)
                }
            }
        };
        let (rows, loops) = grammar::rows(&text);
        let new = name.is_none();
        let dialog = Panel::new(if new { "New macro" } else { "Edit macro" }).for_macros();
        let rounds = gtk::SpinButton::with_range(2.0, 9999.0, 1.0);
        rounds.add_css_class("cx-input");
        rounds.set_valign(Align::Center);
        rounds.set_value(if loops > 1 { f64::from(loops) } else { 10.0 });
        let d = Rc::new(MacroDialog {
            window: w.weak(),
            dialog,
            old: name.map(str::to_owned),
            rows: RefCell::new(rows),
            loops: Cell::new(loops),
            hotkey: RefCell::new(hotkey),
            capturing: Cell::new(false),
            name: crate::ui::ds::input(&title),
            cap: gtk::Button::builder()
                .valign(Align::Center)
                .css_classes(["ds", "secondary", "sm"])
                .build(),
            clear_key: gtk::Button::builder()
                .icon_name("rm-x-symbolic")
                .tooltip_text("No hotkey")
                .valign(Align::Center)
                .css_classes(["ib", "sm"])
                .build(),
            repeat: adw::ToggleGroup::builder().valign(Align::Center).build(),
            rounds,
            count: lbl("", "dimmed"),
            list: crate::ui::widgets::boxed_list(),
            view: adw::ToggleGroup::builder().valign(Align::Center).build(),
            views: gtk::Stack::builder().vhomogeneous(false).build(),
            text: gtk::TextView::builder()
                .monospace(true)
                .top_margin(10)
                .bottom_margin(10)
                .left_margin(12)
                .right_margin(12)
                .build(),
            repeat_group: gtk::Box::new(gtk::Orientation::Vertical, 4),
            err: lbl("", "error").wrapped().visible(false),
            record: Record::new(w.weak()),
        });
        d.assemble(new);
        let held = RefCell::new(Some(d.clone()));
        d.dialog.connect_closed(move |_| {
            held.take();
        });
        d.dialog.present(w);
    }

    fn assemble(self: &Rc<Self>, new: bool) {
        // -- header bar: Cancel, the title, Save ------------------------------
        let back =
            ds::Button::with_icons("All macros", Variant::Ghost, true, Some("chev-left"), None)
                .tip("Back to the list; nothing unsaved is kept");
        let cancel = ds::Button::new("Cancel", Variant::Ghost, true);
        for b in [&back.button, &cancel.button] {
            let dialog = self.dialog.downgrade();
            b.connect_clicked(move |_| {
                if let Some(d) = dialog.upgrade() {
                    d.close();
                }
            });
        }
        let me = Rc::downgrade(self);
        let save = ds::Button::new("Save", Variant::Primary, true).on(move || {
            if let Some(d) = me.upgrade() {
                d.save();
            }
        });
        self.name.set_hexpand(true);
        self.name.set_valign(Align::Center);
        for b in [&back.button, &cancel.button, &save.button] {
            b.set_valign(Align::Center);
        }
        let header =
            hbox!(8, "cx-head", back.button, self.name.clone(), cancel.button, save.button.clone());
        self.dialog.set_default_widget(Some(&save.button));

        // -- name and hotkey ------------------------------------------------------
        let me = Rc::downgrade(self);
        self.cap.connect_clicked(move |_| {
            if let Some(d) = me.upgrade() {
                d.capture();
            }
        });
        let me = Rc::downgrade(self);
        self.clear_key.connect_clicked(move |_| {
            if let Some(d) = me.upgrade() {
                d.hotkey.replace(None);
                d.capturing.set(false);
                d.draw_hotkey();
            }
        });
        let about = ds::sec("", None);
        about.add_css_class("first");
        about.append(&ds::field(
            "Hotkey",
            &hbox!(4, "", self.cap.clone(), self.clear_key.clone()),
            Some("Runs or stops it on the selected accounts while this window has focus"),
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| match me.upgrade() {
            Some(d) => d.on_key(key, state),
            None => glib::Propagation::Proceed,
        });
        self.dialog.add_controller(keys);

        // -- repeat ---------------------------------------------------------------
        let repeat_group = &self.repeat_group;
        for (name, label) in [("once", "Once"), ("rounds", "Rounds"), ("until", "Until Stopped")] {
            self.repeat.add(adw::Toggle::builder().name(name).label(label).build());
        }
        self.repeat.set_active_name(Some(match self.loops.get() {
            1 => "once",
            0 => "until",
            _ => "rounds",
        }));
        self.repeat.add_css_class("cx-seg");
        let rounds_label = lbl("rounds", "t-caption muted").centered();
        self.rounds.bind_property("visible", &rounds_label, "visible").sync_create().build();
        repeat_group.append(&ds::field(
            "Playback",
            &hbox!(8, "", self.repeat.clone(), self.rounds.clone(), rounds_label),
            Some("Once, a number of rounds, or round after round until stopped"),
        ));
        about.append(repeat_group);
        let me = Rc::downgrade(self);
        self.repeat.connect_active_name_notify(move |_| {
            if let Some(d) = me.upgrade() {
                d.repeat_changed();
            }
        });
        let me = Rc::downgrade(self);
        self.rounds.connect_value_changed(move |r| {
            if let Some(d) = me.upgrade() {
                if d.repeat.active_name().as_deref() == Some("rounds") {
                    d.loops.set(r.value().max(2.0) as u32);
                }
            }
        });
        self.rounds.set_visible(self.repeat.active_name().as_deref() == Some("rounds"));

        let steps_group = self.steps_group();

        let err = ds::sec("", None);
        err.append(&self.err);
        self.err.bind_property("visible", &err, "visible").sync_create().build();
        let page = vbox!(0, "", about, steps_group, err);
        if !new {
            let delete =
                ds::Button::with_icons("Delete macro…", Variant::Ghost, false, Some("trash"), None);
            delete.button.set_halign(Align::Start);
            let me = Rc::downgrade(self);
            delete.button.connect_clicked(move |_| {
                if let Some(d) = me.upgrade() {
                    d.delete();
                }
            });
            let danger = ds::sec("", None);
            danger.append(&delete.button);
            page.append(&danger);
        }
        let scroller = gtk::ScrolledWindow::builder()
            .child(&page)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();
        let view = vbox!(0, "cx-panel", header, scroller);
        self.dialog.set_child(Some(&view));
        self.draw_hotkey();
        self.draw_steps();
    }

    // -- repeat -----------------------------------------------------------
    fn repeat_changed(&self) {
        let mode = self.repeat.active_name();
        self.loops.set(match mode.as_deref() {
            Some("once") => 1,
            Some("rounds") => self.rounds.value().max(2.0) as u32,
            _ => 0,
        });
        self.rounds.set_visible(mode.as_deref() == Some("rounds"));
    }

    // -- steps or text ------------------------------------------------------
    /// Show the steps as rows or as text, carrying every change across.
    fn switch_view(self: &Rc<Self>) {
        self.err.set_visible(false);
        if self.view.active_name().as_deref() == Some("text") {
            let text = grammar::to_text(&self.rows.borrow(), self.loops.get());
            self.text.buffer().set_text(&text);
            self.views.set_visible_child_name("text");
            // The text's loop line is its repeat.
            self.repeat_group.set_visible(false);
            self.count.set_visible(false);
        } else {
            self.take_text();
            self.views.set_visible_child_name("steps");
            self.repeat_group.set_visible(true);
            self.count.set_visible(true);
            self.draw_steps();
        }
    }

    /// The text as rows and a repeat, into the steps view.
    fn take_text(&self) {
        let b = self.text.buffer();
        let (rows, loops) = grammar::rows(&b.text(&b.start_iter(), &b.end_iter(), false));
        self.rows.replace(rows);
        // Rounds before the mode: the mode reads them as it changes.
        if loops > 1 {
            self.rounds.set_value(f64::from(loops));
        }
        self.repeat.set_active_name(Some(match loops {
            1 => "once",
            0 => "until",
            _ => "rounds",
        }));
        self.loops.set(loops);
    }

    fn in_text(&self) -> bool {
        self.view.active_name().as_deref() == Some("text")
    }

    // -- hotkey ---------------------------------------------------------------
    fn draw_hotkey(&self) {
        let key = self.hotkey.borrow().clone();
        self.cap.remove_css_class("capturing");
        self.cap.add_css_class("hotkey");
        match key {
            Some(k) => {
                self.cap.set_child(Some(&keycaps(&k)));
                self.cap.set_tooltip_text(Some(&format!(
                    "{} — click to change",
                    hotkey_label(Some(&k))
                )));
            }
            None => {
                self.cap.set_label("Set hotkey…");
                self.cap.set_tooltip_text(Some("Click, then press the keys"));
            }
        }
        self.clear_key.set_visible(self.hotkey.borrow().is_some());
    }

    fn capture(&self) {
        self.capturing.set(true);
        self.cap.set_label("Press keys… (Esc cancels)");
        self.cap.add_css_class("capturing");
    }

    fn on_key(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        if !self.capturing.get() {
            return glib::Propagation::Proceed;
        }
        let name = key.name().map(|n| n.to_string()).unwrap_or_default();
        if ["Shift", "Control", "Alt", "Super", "Meta", "ISO_"].iter().any(|m| name.starts_with(m))
        {
            return glib::Propagation::Stop; // a modifier alone is not a hotkey yet
        }
        self.capturing.set(false);
        if name == "BackSpace" {
            self.hotkey.replace(None);
        } else if name != "Escape" {
            let mods = state & gtk::accelerator_get_default_mod_mask();
            self.hotkey.replace(Some(gtk::accelerator_name(key, mods).to_string()));
        }
        self.draw_hotkey();
        glib::Propagation::Stop
    }

    // -- save and delete ---------------------------------------------------------
    fn save(&self) {
        if self.in_text() {
            // Checked as typed, so an error names the line the user sees.
            let b = self.text.buffer();
            if let Err(e) = grammar::parse(&b.text(&b.start_iter(), &b.end_iter(), false)) {
                self.err.set_label(&sentence(&e.to_string()));
                self.err.set_visible(true);
                return;
            }
            self.take_text();
        }
        let text = grammar::to_text(&self.rows.borrow(), self.loops.get());
        let Some(w) = self.window.upgrade() else { return };
        let hotkey = self.hotkey.borrow().clone();
        match w.save_macro(self.old.as_deref(), &self.name.text(), &text, hotkey.as_deref()) {
            Some(err) => {
                self.err.set_label(&err);
                self.err.set_visible(true);
            }
            None => {
                self.dialog.close();
            }
        }
    }

    fn delete(&self) {
        let (Some(old), Some(w)) = (self.old.clone(), self.window.upgrade()) else { return };
        let dialog = self.dialog.clone();
        confirm::ask(
            &w,
            &format!("Delete {old}?"),
            "Accounts that play it are left with no macro.",
            "_Delete",
            move |w| {
                w.delete_macro(&old);
                dialog.close();
            },
        );
    }
}

mod steps;
mod timeline;
