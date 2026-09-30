//! Editing one macro as steps. Saving writes the same text the player has
//! always read; the macro library checks it parses first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType, gdk, glib};
use rbxmgr_core::macros::grammar::{self, Row};

use super::card::step_icon;
use crate::ui::confirm;
use crate::ui::widgets::{
    Btn, Fluent, LabelFluent, hotkey_label, icon, keycaps, lbl, plural, wrap,
};
use crate::ui::window::{WeakWindow, Window};

const STEP_TYPES: [&str; 8] = ["Key", "Hold", "Type", "Click", "Move", "Wait", "Start", "Note"];

/// What a step's value looks like, as the entry's placeholder.
fn hint(kind: &str) -> &'static str {
    match kind {
        "Key" => "e  ·  shift+w  ·  space",
        "Hold" => "w 2  ·  shift+w 0.5-1",
        "Type" => "text to type",
        "Click" => "960 540  ·  right  ·  left 10 20",
        "Move" => "40 0",
        "Wait" => "0.5  ·  60-240",
        "Start" => "45",
        "Note" => "what this part does",
        _ => "",
    }
}

pub struct MacroDialog {
    window: WeakWindow,
    dialog: adw::Dialog,
    old: Option<String>,
    rows: RefCell<Vec<Row>>,
    loops: Cell<u32>,
    hotkey: RefCell<Option<String>>,
    capturing: Cell<bool>,
    name: adw::EntryRow,
    cap: gtk::Button,
    clear_key: gtk::Button,
    repeat: adw::ToggleGroup,
    rounds: adw::SpinRow,
    count: gtk::Label,
    list: gtk::ListBox,
    err: gtk::Label,
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
        let dialog = adw::Dialog::builder()
            .title(if new { "New Macro" } else { "Edit Macro" })
            .content_width(600)
            .build();
        let rounds = adw::SpinRow::with_range(2.0, 9999.0, 1.0);
        rounds.set_title("Rounds");
        rounds.set_value(if loops > 1 { f64::from(loops) } else { 10.0 });
        let d = Rc::new(MacroDialog {
            window: w.weak(),
            dialog,
            old: name.map(str::to_owned),
            rows: RefCell::new(rows),
            loops: Cell::new(loops),
            hotkey: RefCell::new(hotkey),
            capturing: Cell::new(false),
            name: adw::EntryRow::builder().title("Name").text(&title).build(),
            cap: gtk::Button::builder().valign(Align::Center).build(),
            clear_key: gtk::Button::builder()
                .icon_name("edit-clear-symbolic")
                .tooltip_text("No hotkey")
                .valign(Align::Center)
                .css_classes(["flat", "circular"])
                .build(),
            repeat: adw::ToggleGroup::builder().valign(Align::Center).build(),
            rounds,
            count: lbl("", "dimmed"),
            list: crate::ui::widgets::boxed_list(),
            err: lbl("", "error").wrapped().visible(false),
        });
        d.assemble(new);
        let held = RefCell::new(Some(d.clone()));
        d.dialog.connect_closed(move |_| {
            held.take();
        });
        d.dialog.present(Some(w.gtk_window()));
    }

    fn assemble(self: &Rc<Self>, new: bool) {
        // -- header bar: Cancel, the title, Save ------------------------------
        let header = adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build();
        let cancel = {
            let dialog = self.dialog.downgrade();
            Btn::new("").text("_Cancel").build(move || {
                if let Some(d) = dialog.upgrade() {
                    d.close();
                }
            })
        };
        let me = Rc::downgrade(self);
        let save = Btn::new("suggested-action").text("_Save").build(move || {
            if let Some(d) = me.upgrade() {
                d.save();
            }
        });
        header.pack_start(&cancel.button);
        header.pack_end(&save.button);
        header.pack_end(
            &gtk::Button::builder()
                .icon_name("help-about-symbolic")
                .tooltip_text("How Macros Work")
                .action_name("win.macro-help")
                .build(),
        );
        self.dialog.set_default_widget(Some(&save.button));

        // -- name and hotkey ------------------------------------------------------
        let about = adw::PreferencesGroup::new();
        about.add(&self.name);
        let key_row = adw::ActionRow::builder()
            .title("Hotkey")
            .subtitle("Runs or stops it on the selected accounts while this window has focus")
            .build();
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
        key_row.add_suffix(&self.clear_key);
        key_row.add_suffix(&self.cap);
        key_row.set_activatable_widget(Some(&self.cap));
        about.add(&key_row);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| match me.upgrade() {
            Some(d) => d.on_key(key, state),
            None => glib::Propagation::Proceed,
        });
        self.dialog.add_controller(keys);

        // -- repeat ---------------------------------------------------------------
        let repeat_group = adw::PreferencesGroup::builder().title("Repeat").build();
        for (name, label) in [("once", "Once"), ("rounds", "Rounds"), ("until", "Until Stopped")] {
            self.repeat.add(adw::Toggle::builder().name(name).label(label).build());
        }
        self.repeat.set_active_name(Some(match self.loops.get() {
            1 => "once",
            0 => "until",
            _ => "rounds",
        }));
        let repeat_row = adw::ActionRow::builder().title("Play the steps").build();
        repeat_row.add_suffix(&self.repeat);
        repeat_group.add(&repeat_row);
        repeat_group.add(&self.rounds);
        let me = Rc::downgrade(self);
        self.repeat.connect_active_name_notify(move |_| {
            if let Some(d) = me.upgrade() {
                d.repeat_changed();
            }
        });
        let me = Rc::downgrade(self);
        self.rounds.connect_value_notify(move |r| {
            if let Some(d) = me.upgrade() {
                if d.repeat.active_name().as_deref() == Some("rounds") {
                    d.loops.set(r.value().max(2.0) as u32);
                }
            }
        });
        self.rounds.set_visible(self.repeat.active_name().as_deref() == Some("rounds"));

        // -- steps ----------------------------------------------------------------
        let adds: Vec<gtk::Widget> = ["Key", "Wait", "Click", "Type", "Hold", "Move", "Note"]
            .into_iter()
            .map(|kind| {
                let me = Rc::downgrade(self);
                Btn::new("")
                    .text(kind)
                    .icon("list-add-symbolic")
                    .tip(&format!("Add a {} step", kind.to_lowercase()))
                    .build(move || {
                        if let Some(d) = me.upgrade() {
                            d.rows
                                .borrow_mut()
                                .push(Row { kind: kind.to_owned(), value: String::new() });
                            d.draw_steps();
                        }
                    })
                    .button
                    .upcast()
            })
            .collect();
        let steps_group =
            adw::PreferencesGroup::builder().title("Steps").header_suffix(&self.count).build();
        steps_group.add(&self.list);
        let add_box = wrap(6, &adds);
        add_box.set_margin_top(12);
        steps_group.add(&add_box);

        let page = vbox!(24, "", about, repeat_group, steps_group, self.err.clone()).margins(18);
        if !new {
            let delete = adw::ButtonRow::builder().title("Delete Macro…").build();
            delete.add_css_class("destructive-action");
            let me = Rc::downgrade(self);
            delete.connect_activated(move |_| {
                if let Some(d) = me.upgrade() {
                    d.delete();
                }
            });
            let danger = adw::PreferencesGroup::new();
            danger.add(&delete);
            page.append(&danger);
        }
        let clamp = adw::Clamp::builder().maximum_size(640).child(&page).build();
        let scroller = gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&scroller));
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

    // -- steps --------------------------------------------------------------
    fn draw_steps(self: &Rc<Self>) {
        self.list.remove_all();
        let rows = self.rows.borrow().clone();
        let real = rows.iter().filter(|r| r.kind != "Note").count();
        self.count.set_label(&plural(real, "step", "steps"));
        if rows.is_empty() {
            self.list.append(&crate::ui::accounts::leader::placeholder(
                "list-add-symbolic",
                "No steps yet. Add one below.",
            ));
        }
        for (i, r) in rows.iter().enumerate() {
            let mut kinds: Vec<&str> = STEP_TYPES.to_vec();
            if !kinds.contains(&r.kind.as_str()) {
                kinds.push(&r.kind);
            }
            let kind = gtk::DropDown::from_strings(&kinds);
            kind.set_valign(Align::Center);
            // One width for every type, so the values line up.
            kind.set_width_request(104);
            kind.set_selected(kinds.iter().position(|k| *k == r.kind).unwrap_or(0) as u32);
            let (me, owned): (_, Vec<String>) =
                (Rc::downgrade(self), kinds.iter().map(|k| (*k).to_owned()).collect());
            kind.connect_selected_notify(move |d| {
                if let (Some(me), Some(k)) = (me.upgrade(), owned.get(d.selected() as usize)) {
                    me.retype(i, k.clone());
                }
            });
            let value = gtk::Entry::builder()
                .text(&r.value)
                .placeholder_text(hint(&r.kind))
                .hexpand(true)
                .valign(Align::Center)
                .css_classes(["monospace"])
                .build();
            let me = Rc::downgrade(self);
            value.connect_changed(move |e| {
                if let Some(d) = me.upgrade() {
                    if let Some(row) = d.rows.borrow_mut().get_mut(i) {
                        row.value = e.text().to_string();
                    }
                }
            });
            let button = |ic: &str, tip: &str, on: bool, act: fn(&Rc<Self>, usize)| {
                let me = Rc::downgrade(self);
                let b = Btn::new("flat circular").icon(ic).tip(tip).build(move || {
                    if let Some(d) = me.upgrade() {
                        act(&d, i);
                    }
                });
                b.button.set_sensitive(on);
                b.button.set_valign(Align::Center);
                b.button
            };
            let len = rows.len();
            let line = hbox!(
                8,
                "",
                lbl(&format!("{}", i + 1), "number dimmed").xalign(1.0),
                icon(step_icon(&r.kind)).css("dimmed"),
                kind,
                value,
                button("go-up-symbolic", "Move up", i > 0, |d, i| d.swap(i, i - 1)),
                button("go-down-symbolic", "Move down", i + 1 < len, |d, i| d.swap(i, i + 1)),
                button("user-trash-symbolic", "Remove step", true, |d, i| {
                    d.rows.borrow_mut().remove(i);
                    d.draw_steps();
                })
            );
            let row = gtk::ListBoxRow::builder()
                .activatable(false)
                .selectable(false)
                .child(&line)
                .build();
            row.add_css_class("step-row");
            self.list.append(&row);
        }
    }

    fn retype(self: &Rc<Self>, i: usize, kind: String) {
        let changed = self.rows.borrow_mut().get_mut(i).is_some_and(|r| {
            let changed = r.kind != kind;
            r.kind = kind;
            changed
        });
        if changed {
            // Deferred: the dropdown whose popover just closed is one of the
            // widgets being replaced.
            let me = Rc::downgrade(self);
            glib::idle_add_local_once(move || {
                if let Some(d) = me.upgrade() {
                    d.draw_steps();
                }
            });
        }
    }

    fn swap(self: &Rc<Self>, i: usize, j: usize) {
        self.rows.borrow_mut().swap(i, j);
        self.draw_steps();
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
                self.cap.set_label("Set Hotkey…");
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
