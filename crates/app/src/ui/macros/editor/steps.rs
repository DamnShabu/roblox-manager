//! The editor's steps: a row each, typed and filled in, moved and removed --
//! the list the macro's text is written from -- or the text itself.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, glib};
use rbxmgr_core::macros::grammar::{self, Row};
use rbxmgr_core::macros::lanes;

use super::MacroDialog;
use crate::ui::macros::card::step_icon;
use crate::ui::macros::{area, point};
use crate::ui::widgets::{Btn, Fluent, LabelFluent, lbl, plural, wrap};

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
        let mut adds: Vec<gtk::Widget> = ["Key", "Wait", "Click", "Type", "Hold", "Move", "Note"]
            .into_iter()
            .map(|kind| {
                let me = Rc::downgrade(self);
                Btn::new("ds ghost sm")
                    .text(kind)
                    .icon("rm-plus-symbolic")
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
        adds.push(self.when_button().upcast());
        adds.push(self.record_button().upcast());
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
        self.list.add_css_class("cx-list");
        let add_box = wrap(6, &adds);
        add_box.set_margin_top(12);
        self.views.add_named(&vbox!(0, "", self.list.clone(), add_box), Some("steps"));
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
        steps_group.append(self.record.banner());
        steps_group.append(&self.views);
        steps_group
    }

    pub(super) fn draw_steps(self: &Rc<Self>) {
        self.list.remove_all();
        let rows = self.rows.borrow().clone();
        let real = rows.iter().filter(|r| !matches!(r.kind.as_str(), "Note" | "Timeline")).count();
        self.count.set_label(&plural(real, "step", "steps"));
        if rows.is_empty() {
            let row = gtk::ListBoxRow::builder().activatable(false).selectable(false).build();
            row.set_child(Some(&crate::ui::widgets::lbl(
                "No steps yet. Add one below.",
                "t-body-sm muted",
            )));
            row.add_css_class("step-row");
            self.list.append(&row);
        }
        let units = lanes::units(&rows);
        let count = units.len();
        for (u, &(i, len)) in units.iter().enumerate() {
            if rows[i].kind == "Timeline" {
                self.list.append(&self.timeline_row(u + 1, count, i, len));
                continue;
            }
            for (k, row) in (i..i + len).enumerate() {
                // A when's do rows go with it: numbered with it, moved with it.
                let number = if k == 0 { format!("{}", u + 1) } else { String::new() };
                let moves = if k == 0 { (u > 0, u + 1 < count) } else { (false, false) };
                self.list.append(&self.step_row(&rows, row, &number, moves));
            }
        }
    }

    /// Row `i` as a step row, numbered `number` (empty for a when's rows
    /// after its first), with its moves on or off.
    fn step_row(
        self: &Rc<Self>,
        rows: &[Row],
        i: usize,
        number: &str,
        (up, down): (bool, bool),
    ) -> gtk::ListBoxRow {
        let r = &rows[i];
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
            8,
            "",
            lbl(number, "number dimmed").xalign(1.0),
            crate::ui::ds::icon(step_icon(&r.kind)).css("muted s16"),
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
        let row =
            gtk::ListBoxRow::builder().activatable(false).selectable(false).child(&line).build();
        row.add_css_class("step-row");
        row
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

    /// When: a `when` and a first `do` under it, added at the end -- steps
    /// that play the moment something shows in the client, out of turn.
    fn when_button(self: &Rc<Self>) -> gtk::Button {
        let me = Rc::downgrade(self);
        Btn::new("ds ghost sm")
            .text("When")
            .icon("rm-plus-symbolic")
            .tip("Add steps that play the moment something shows in the client")
            .build(move || {
                if let Some(d) = me.upgrade() {
                    d.add_rows(vec![
                        Row { kind: "When".to_owned(), value: String::new() },
                        Row { kind: "Do".to_owned(), value: String::new() },
                    ]);
                }
            })
            .button
    }

    /// Record: steps from playing in a running client, added at the end.
    fn record_button(self: &Rc<Self>) -> gtk::Button {
        const TIP: &str = "Record steps by playing in a running client: press F8 in its \
                           window to start, and again to stop";
        let content = adw::ButtonContent::builder()
            .icon_name("media-record-symbolic")
            .label("_Record")
            .use_underline(true)
            .build();
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text(TIP)
            .css_classes(["ds", "ghost", "sm"])
            .build();
        let me = Rc::downgrade(self);
        button.connect_clicked(move |anchor| {
            let Some(d) = me.upgrade() else { return };
            d.err.set_visible(false);
            let me = Rc::downgrade(&d);
            d.record.start(anchor, move |recorded| {
                let Some(d) = me.upgrade() else { return };
                match recorded {
                    Ok(rows) => d.add_rows(rows),
                    Err(e) => {
                        d.err.set_label(&e);
                        d.err.set_visible(true);
                    }
                }
            });
        });
        button
    }

    /// `rows` after the last step, in whichever view is showing.
    fn add_rows(self: &Rc<Self>, rows: Vec<Row>) {
        if self.in_text() {
            let buffer = self.text.buffer();
            let before = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
            let lead = if before.is_empty() || before.ends_with('\n') { "" } else { "\n" };
            let added = format!("{lead}{}", grammar::to_text(&rows, 0));
            buffer.insert(&mut buffer.end_iter(), &added);
        } else {
            self.rows.borrow_mut().extend(rows);
            self.draw_steps();
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
