//! Editing one macro as steps. Saving writes the same text the player has
//! always read; the macro library checks it parses first.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, PolicyType, gdk, glib};
use rbxmgr_core::macros::grammar::{self, Row};

use super::card::step_icon;
use crate::ui::modal::Modal;
use crate::ui::widgets::{Btn, Fluent, IconButton, LabelFluent, clear, hotkey_label, lbl, wrap};
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
    modal: Modal,
    old: Option<String>,
    rows: RefCell<Vec<Row>>,
    loops: Cell<u32>,
    hotkey: RefCell<Option<String>>,
    capturing: Cell<bool>,
    name: gtk::Entry,
    cap: IconButton,
    seg: gtk::Box,
    spin: gtk::SpinButton,
    count: gtk::Label,
    list: gtk::Box,
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
        let modal = Modal::new(
            "keyboard",
            if new { "New macro" } else { "Edit macro" },
            "Changes apply when you save",
            560,
        );
        let name_entry =
            gtk::Entry::builder().text(&title).placeholder_text("Macro name").hexpand(true).build();
        name_entry.add_css_class("efield");
        let spin = gtk::SpinButton::with_range(2.0, 9999.0, 1.0);
        spin.add_css_class("rounds");
        spin.set_value(if loops > 1 { f64::from(loops) } else { 10.0 });
        let dialog = Rc::new_cyclic(|me: &std::rc::Weak<MacroDialog>| {
            let me = me.clone();
            let cap = Btn::new("hkcap mono")
                .text(
                    &hotkey
                        .as_deref()
                        .map_or_else(|| "Set hotkey".to_owned(), |k| hotkey_label(Some(k))),
                )
                .icon("keyboard")
                .size(17)
                .gap(8)
                .tip("Press to set. While it waits: Esc keeps the old one, Backspace clears it.")
                .build(move || {
                    if let Some(d) = me.upgrade() {
                        d.capture();
                    }
                });
            if let Some(child) = cap.button.child() {
                child.set_halign(Align::Start);
            }
            MacroDialog {
                window: w.weak(),
                modal,
                old: name.map(str::to_owned),
                rows: RefCell::new(rows),
                loops: Cell::new(loops),
                hotkey: RefCell::new(hotkey),
                capturing: Cell::new(false),
                name: name_entry,
                cap,
                seg: hbox!(2, "seg").halign(Align::Start),
                spin,
                count: lbl("", "phint2"),
                list: vbox!(6, ""),
                err: lbl("", "merr").wrapped().visible(false),
            }
        });
        dialog.assemble(w, new);
        dialog.modal.keep_alive(dialog.clone());
        dialog.modal.present(w.gtk_window());
    }

    fn assemble(self: &Rc<Self>, w: &Window, new: bool) {
        let me = Rc::downgrade(self);
        self.spin.connect_value_changed(move |s| {
            if let Some(d) = me.upgrade() {
                d.set_loops(s.value_as_int().max(0).unsigned_abs());
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| match me.upgrade() {
            Some(d) => d.on_key(key, state),
            None => glib::Propagation::Proceed,
        });
        self.modal.dialog.add_controller(keys);

        let steps = gtk::ScrolledWindow::builder()
            .child(&self.list)
            .max_content_height(300)
            .propagate_natural_height(true)
            .hscrollbar_policy(PolicyType::Never)
            .build();
        let adds: Vec<gtk::Widget> = ["Key", "Wait", "Click", "Type", "Hold", "Note"]
            .into_iter()
            .map(|kind| {
                let me = Rc::downgrade(self);
                Btn::new("addstep")
                    .text(kind)
                    .icon("add")
                    .size(17)
                    .gap(5)
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
        let body = vbox!(
            18,
            "ebody",
            hbox!(
                12,
                "",
                vbox!(6, "", lbl("Name", "elabel"), self.name.clone()).hexpand(),
                vbox!(6, "", lbl("Hotkey", "elabel"), self.cap.button.clone()).width(160)
            ),
            vbox!(
                6,
                "",
                lbl("Repeat", "elabel"),
                hbox!(10, "", self.seg.clone(), self.spin.clone())
            ),
            vbox!(
                8,
                "",
                hbox!(8, "", lbl("Steps", "elabel"), self.count.clone()),
                steps,
                wrap(8, &adds),
                self.err.clone()
            )
        );
        let (me, me2) = (Rc::downgrade(self), Rc::downgrade(self));
        let discard = Btn::new("mdel")
            .text(if new { "Discard" } else { "Delete macro" })
            .icon("delete")
            .gap(5)
            .build(move || {
                if let Some(d) = me.upgrade() {
                    d.delete_or_discard();
                }
            });
        let cancel = {
            let dialog = self.modal.dialog.clone();
            Btn::new("cancel").text("Cancel").build(move || {
                dialog.close();
            })
        };
        let save = Btn::new("save").text("Save").icon("check").build(move || {
            if let Some(d) = me2.upgrade() {
                d.save();
            }
        });
        let footer = hbox!(
            8,
            "mfoot",
            discard.button,
            gtk::Box::new(gtk::Orientation::Horizontal, 0).hexpand(),
            cancel.button,
            save.button
        );
        footer.set_margin_top(20);
        let help =
            Btn::new("mclose").icon("help").size(20).tip("How macros work").build(w.act(show_help));
        help.button.set_valign(Align::Start);
        if let Some(first) = self.modal.header.first_child() {
            self.modal.header.insert_child_after(&help.button, first.next_sibling().as_ref());
        }
        self.modal.build(&body, &footer);
        self.draw_seg();
        self.draw_steps();
    }

    // -- repeat -----------------------------------------------------------
    fn set_loops(self: &Rc<Self>, n: u32) {
        self.loops.set(n);
        self.draw_seg();
    }

    fn draw_seg(self: &Rc<Self>) {
        clear(&self.seg);
        let loops = self.loops.get();
        let mode = match loops {
            1 => "once",
            0 => "until",
            _ => "rounds",
        };
        let rounds = self.spin.value_as_int().max(2).unsigned_abs();
        for (key, label, ic, n) in [
            ("once", "Once", "looks_one", 1),
            ("rounds", "Rounds", "pin", rounds),
            ("until", "Until stopped", "repeat", 0),
        ] {
            let me = Rc::downgrade(self);
            let b = Btn::new(if key == mode { "segopt on" } else { "segopt" })
                .text(label)
                .icon(ic)
                .size(16)
                .build(move || {
                    if let Some(d) = me.upgrade() {
                        d.set_loops(n);
                    }
                });
            self.seg.append(&b.button);
        }
        self.spin.set_visible(mode == "rounds");
    }

    // -- steps --------------------------------------------------------------
    fn draw_steps(self: &Rc<Self>) {
        clear(&self.list);
        let rows = self.rows.borrow().clone();
        let real = rows.iter().filter(|r| r.kind != "Note").count();
        self.count.set_label(&format!("{real} step{}", if real == 1 { "" } else { "s" }));
        if rows.is_empty() {
            self.list.append(&lbl("No steps yet. Add one below.", "noSteps").xalign(0.5));
        }
        for (i, r) in rows.iter().enumerate() {
            let mut kinds: Vec<&str> = STEP_TYPES.to_vec();
            if !kinds.contains(&r.kind.as_str()) {
                kinds.push(&r.kind);
            }
            let kind = gtk::DropDown::from_strings(&kinds);
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
                .build();
            value.add_css_class("sval");
            value.add_css_class("mono");
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
                let b = Btn::new(if ic == "delete" { "mini unlink" } else { "mini" })
                    .icon(ic)
                    .tip(tip)
                    .build(move || {
                        if let Some(d) = me.upgrade() {
                            act(&d, i);
                        }
                    });
                b.button.set_sensitive(on);
                b.button
            };
            let len = rows.len();
            self.list.append(&hbox!(
                8,
                "estep",
                lbl(&format!("{:02}", i + 1), "estepn mono").xalign(0.5).width(22),
                hbox!(0, "stype", crate::ui::widgets::icon(step_icon(&r.kind), 16, ""), kind)
                    .centered(),
                value,
                hbox!(
                    2,
                    "",
                    button("arrow_upward", "Move up", i > 0, |d, i| d.swap(i, i - 1)),
                    button("arrow_downward", "Move down", i + 1 < len, |d, i| d.swap(i, i + 1)),
                    button("delete", "Remove step", true, |d, i| {
                        d.rows.borrow_mut().remove(i);
                        d.draw_steps();
                    })
                )
                .centered()
            ));
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
    fn capture(&self) {
        self.capturing.set(true);
        self.cap.set_text("Press a key…");
        self.cap.button.add_css_class("capturing");
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
        self.cap.button.remove_css_class("capturing");
        if name == "BackSpace" {
            self.hotkey.replace(None);
        } else if name != "Escape" {
            let mods = state & gtk::accelerator_get_default_mod_mask();
            self.hotkey.replace(Some(gtk::accelerator_name(key, mods).to_string()));
        }
        let shown = self
            .hotkey
            .borrow()
            .as_deref()
            .map_or_else(|| "Set hotkey".to_owned(), |k| hotkey_label(Some(k)));
        self.cap.set_text(&shown);
        glib::Propagation::Stop
    }

    // -- save -----------------------------------------------------------------
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
                self.modal.dialog.close();
            }
        }
    }

    fn delete_or_discard(&self) {
        if let (Some(old), Some(w)) = (&self.old, self.window.upgrade()) {
            w.delete_macro(old);
        }
        self.modal.dialog.close();
    }
}

/// "How macros work": the help page.
pub fn show_help(w: &Window) {
    let text = gtk::Label::builder()
        .label(include_str!("../../../resources/macro-help.markup"))
        .use_markup(true)
        .wrap(true)
        .xalign(0.0)
        .selectable(true)
        .margin_top(6)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let page = adw::ToolbarView::new();
    page.add_top_bar(&adw::HeaderBar::new());
    page.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&text)
            .propagate_natural_height(true)
            .hscrollbar_policy(PolicyType::Never)
            .build(),
    ));
    adw::Dialog::builder()
        .title("How macros work")
        .child(&page)
        .content_width(520)
        .content_height(640)
        .build()
        .present(Some(w.gtk_window()));
}
