//! The editor's steps: a stop each on a timeline, typed and filled in, moved
//! and removed -- the list the macro's text is written from -- or the text
//! itself.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, glib};
use rbxmgr_core::macros::grammar::{self, Row, timing};
use rbxmgr_core::macros::lanes;

use super::MacroDialog;
use super::rail::{self, Stop};
use crate::ui::macros::kinds::step_icon;
use crate::ui::macros::{area, point};
use crate::ui::widgets::{Btn, Fluent, LabelFluent, lbl, plural, sentence};

const STEP_TYPES: [&str; 16] = [
    "Key", "Hold", "Press", "Release", "Repeat", "Type", "Click", "Move", "Scroll", "Wait",
    "Start", "Stagger", "Note", "When", "Do", "Exit",
];

/// What a step's value looks like, as the entry's placeholder.
fn hint(kind: &str) -> &'static str {
    match kind {
        "Key" => "e  ·  shift+w  ·  space",
        "Hold" => "w 2  ·  shift+w 0.5-1  ·  mouse1 1",
        "Press" | "Release" => "w  ·  shift  ·  mouse2",
        "Repeat" => "e 10  ·  e 10 0.5  ·  mouse1 30 0.2-0.4",
        "Type" => "text to type",
        "Click" => "960 540  ·  right  ·  left 10 20",
        "Move" => "40 0  ·  to 960 540  ·  to 960 540 0.3",
        "Scroll" => "down  ·  up 3",
        "Wait" => "0.5  ·  60-240",
        "Start" => "45",
        "Stagger" => "5  ·  seconds between the accounts it starts on",
        "Note" => "what this part does",
        "Timeline" => "3.5  ·  its length in seconds",
        "At" => "0.5 hold w 1  ·  under a Timeline step",
        "Path" => "0 400 300, 0.5 520 310",
        "Turn" => "0.5 120 -10, 1 200 -15",
        "When" => "image coin  ·  not image coin in 0 0 400 300 95%  ·  color 960 30 #ff3030",
        "Do" => "tap e  ·  click 400 300  ·  exit  ·  under a When step",
        "Exit" => "nothing to fill in  ·  ends the round, and the next starts",
        _ => "",
    }
}

impl MacroDialog {
    /// The Steps group: the rows or the text, and the buttons that add to
    /// them.
    pub(super) fn steps_group(self: &Rc<Self>) -> gtk::Box {
        for (name, label) in [("steps", "Steps"), ("text", "Text")] {
            self.view.add(adw::Toggle::builder().name(name).label(label).build());
        }
        self.view.set_active_name(Some("steps"));
        let me = Rc::downgrade(self);
        self.view.connect_active_name_notify(move |_| {
            if let Some(d) = me.upgrade() {
                d.switch_view();
            }
        });
        self.view.add_css_class("cx-seg");
        let steps_group = crate::ui::ds::sec(
            "Steps",
            Some(hbox!(12, "", self.count.clone().centered(), self.view.clone()).upcast_ref()),
        );
        self.list.add_css_class("rail-list");
        let end = rail::end(&self.end_time, &self.end_then, "repeat");
        let adds = self.add_bar();
        adds.set_margin_top(12);
        self.views.add_named(&vbox!(0, "", self.list.clone(), end, adds), Some("steps"));
        let text = gtk::ScrolledWindow::builder()
            .child(&self.text)
            .min_content_height(220)
            .max_content_height(420)
            .propagate_natural_height(true)
            .build();
        let frame = gtk::Frame::builder().child(&text).build();
        frame.add_css_class("view");
        self.views.add_named(
            &vbox!(
                8,
                "",
                frame,
                lbl(
                    "One step a line, as How Macros Work writes them: Key e, Wait 60-240, \
                     Click 960 540. A last line “loop 5” plays it five times; with none it \
                     plays until stopped.",
                    "caption dimmed"
                )
                .wrapped()
            ),
            Some("text"),
        );
        steps_group.append(&self.record_bar());
        steps_group.append(self.record.banner());
        steps_group.append(&self.views);
        steps_group
    }

    pub(super) fn draw_steps(self: &Rc<Self>) {
        self.list.remove_all();
        self.entries.borrow_mut().clear();
        self.times.borrow_mut().clear();
        let rows = self.rows.borrow().clone();
        let units = lanes::units(&rows);
        // As many as there are stops: a note, a timeline or a when with its
        // dos is one each.
        self.count.set_label(&plural(units.len(), "step", "steps"));
        if rows.is_empty() {
            let empty = lbl("No steps yet: add one below, or record some.", "t-body-sm muted");
            empty.set_xalign(0.0);
            let stop = rail::stop(&rail::time(""), "info", &empty, Stop::First, false);
            self.list.append(&self.stop_row(&stop));
        }
        let count = units.len();
        for (u, &(i, len)) in units.iter().enumerate() {
            let at = if u == 0 { Stop::First } else { Stop::Next };
            if rows[i].kind == "Timeline" {
                let card = self.timeline_row(u + 1, count, i, len);
                let time = self.time_of(i, &rows[i]);
                self.list.append(&self.stop_row(&rail::stop(&time, "timeline", &card, at, false)));
                continue;
            }
            for (k, row) in (i..i + len).enumerate() {
                // A when's do rows go with it: under it on the rail, moved
                // with it.
                let moves = if k == 0 { (u > 0, u + 1 < count) } else { (false, false) };
                let line = self.step_row(&rows, row, moves);
                let (at, time) = match k {
                    0 => (at, self.time_of(row, &rows[row])),
                    _ => (Stop::Under, rail::time("")),
                };
                let waits = matches!(rows[row].kind.as_str(), "Wait" | "Start");
                let stop = rail::stop(&time, step_icon(&rows[row].kind), &line, at, waits);
                self.list.append(&self.stop_row(&stop));
            }
        }
        self.retime();
    }

    /// `stop` as a row of the list.
    fn stop_row(&self, stop: &gtk::Box) -> gtk::ListBoxRow {
        let row =
            gtk::ListBoxRow::builder().activatable(false).selectable(false).child(stop).build();
        row.add_css_class("step-row");
        row
    }

    /// The time beside row `i` on the rail, kept to be set as the steps
    /// before it change: none for a note, "on sight" for a when -- it plays
    /// out of turn.
    fn time_of(&self, i: usize, row: &Row) -> gtk::Label {
        match row.kind.as_str() {
            "Note" => rail::time(""),
            "When" => rail::time("on sight"),
            _ => {
                let label = rail::time("");
                self.times.borrow_mut().push((i, label.clone()));
                label
            }
        }
    }

    /// Set each stop's time, and the round's end, from the steps as they
    /// are now.
    pub(super) fn retime(&self) {
        let (starts, end) = timing::starts(&self.rows.borrow());
        for (i, label) in self.times.borrow().iter() {
            match starts.get(*i) {
                Some(Some(at)) => label.set_label(&timing::label(*at)),
                // After an exit: the round never gets here.
                Some(None) => label.set_label("never"),
                None => {}
            }
        }
        self.end_time.set_label(&timing::label(end));
        self.end_then.set_label(&match self.loops.get() {
            0 => "Round ends, then plays again until stopped".to_owned(),
            1 => "Round ends, and it stops".to_owned(),
            n => format!("Round ends, then plays again: {n} rounds in all"),
        });
    }

    /// Row `i` as a step's line, with its moves on or off.
    fn step_row(self: &Rc<Self>, rows: &[Row], i: usize, (up, down): (bool, bool)) -> gtk::Box {
        let r = &rows[i];
        let mut kinds: Vec<&str> = STEP_TYPES.to_vec();
        if !kinds.contains(&r.kind.as_str()) {
            kinds.push(&r.kind);
        }
        let kind = gtk::DropDown::from_strings(&kinds);
        kind.set_valign(Align::Center);
        // One width for every type, so the values line up.
        kind.set_width_request(92);
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
            // Room for a few characters at the least, so the row still fits
            // a narrow panel; it widens with the panel.
            .width_chars(4)
            .valign(Align::Center)
            .css_classes(["monospace"])
            .build();
        check(&value, r);
        let me = Rc::downgrade(self);
        value.connect_changed(move |e| {
            if let Some(d) = me.upgrade() {
                if let Some(row) = d.rows.borrow_mut().get_mut(i) {
                    row.value = e.text().to_string();
                    check(e, row);
                }
                d.retime();
            }
        });
        self.entries.borrow_mut().push(value.clone());
        let button = |ic: &str, tip: &str, on: bool, act: fn(&Rc<Self>, usize)| {
            let me = Rc::downgrade(self);
            let b = Btn::new("ib sm").icon(ic).tip(tip).build(move || {
                if let Some(d) = me.upgrade() {
                    act(&d, i);
                }
            });
            b.button.set_sensitive(on);
            b.button.set_valign(Align::Center);
            b.button
        };
        let line = hbox!(
            6,
            "",
            kind,
            value,
            button("rm-chev-up-symbolic", "Move up", up, |d, i| d.shift(i, true)),
            button("rm-chev-down-symbolic", "Move down", down, |d, i| d.shift(i, false)),
            button("rm-trash-symbolic", "Remove step", true, |d, i| {
                d.rows.borrow_mut().remove(i);
                d.draw_steps();
            })
        );
        if r.kind == "Click" {
            let pick = point::button(&self.window, &value, &self.err);
            line.insert_child_after(&pick, Some(&value));
        }
        if r.kind == "When" {
            let pick = area::button(&self.window, &value, &self.err);
            line.insert_child_after(&pick, Some(&value));
        }
        line
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

    /// The step listed at row `start` swapped with the one before or after
    /// it -- a timeline, whole.
    pub(super) fn shift(self: &Rc<Self>, start: usize, up: bool) {
        if lanes::shift(&mut self.rows.borrow_mut(), start, up) {
            self.draw_steps();
        }
    }
}

/// Mark `entry` when its row would not parse, with why in its tooltip. An
/// empty value is still being typed, unless the step takes none.
fn check(entry: &gtk::Entry, row: &Row) {
    let typed = !row.value.trim().is_empty() || row.kind == "Exit";
    match grammar::check_row(row).filter(|_| typed) {
        Some(why) => {
            entry.add_css_class("error");
            entry.set_tooltip_text(Some(&sentence(&why)));
        }
        None => {
            entry.remove_css_class("error");
            entry.set_tooltip_text(None);
        }
    }
}
