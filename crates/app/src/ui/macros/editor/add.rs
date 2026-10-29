//! What adds to the editor's steps: one menu of every step type, by what
//! it works -- the keyboard, the mouse, time, the round itself -- and
//! recording, apart from them, at the top: steps played rather than typed.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use rbxmgr_core::macros::grammar::{self, Row};
use rbxmgr_core::macros::lanes;

use super::MacroDialog;
use crate::ui::widgets::{Btn, LabelFluent, lbl};

/// The step types the Add step menu offers, a section for each thing they
/// work, each with what it does.
const KINDS: [(&str, &[(&str, &str)]); 4] = [
    (
        "Keyboard",
        &[
            ("Key", "Tap a key"),
            ("Hold", "Hold a key down for a time"),
            ("Type", "Type some text"),
            ("Repeat", "Tap a key over and over, while the next steps play"),
            ("Press", "Press a key or button down and leave it down"),
            ("Release", "Let go of a key or button pressed earlier"),
        ],
    ),
    (
        "Mouse",
        &[
            ("Click", "Click, where the mouse is or at a point"),
            ("Move", "Move the mouse"),
            ("Scroll", "Scroll the mouse wheel"),
        ],
    ),
    (
        "Time",
        &[
            ("Wait", "Wait, a set time or a random one in a range"),
            ("Start", "Wait before the first round only"),
            ("Stagger", "Start each account a few seconds after the last"),
            ("Timeline", "Steps at set times, together: keys held while the mouse moves"),
        ],
    ),
    (
        "Round",
        &[
            ("When", "Steps that play the moment something shows in the client"),
            ("Note", "A heading for the steps under it"),
            ("Exit", "End the round here; the next one starts"),
        ],
    ),
];

/// The rows a new step of `kind` starts as: a timeline comes with its first
/// step under it, as a when does.
fn starting_rows(kind: &str) -> Vec<Row> {
    let row = |kind: &str, value: &str| Row { kind: kind.to_owned(), value: value.to_owned() };
    match kind {
        "Timeline" => vec![row("Timeline", "2"), row("At", "0 hold w 1")],
        "When" => vec![row("When", ""), row("Do", "")],
        _ => vec![row(kind, "")],
    }
}

impl MacroDialog {
    /// Under the timeline: Add step, every type in a menu, and Wait beside
    /// it, the step most often added between two others.
    pub(super) fn add_bar(self: &Rc<Self>) -> gtk::Box {
        let group = gio::SimpleActionGroup::new();
        let add = gio::SimpleAction::new("add", Some(glib::VariantTy::STRING));
        let me = Rc::downgrade(self);
        add.connect_activate(move |_, kind| {
            if let (Some(d), Some(kind)) = (me.upgrade(), kind.and_then(|k| k.get::<String>())) {
                d.add_rows(starting_rows(&kind));
            }
        });
        group.add_action(&add);
        let menu = gio::Menu::new();
        for (section, kinds) in KINDS {
            let items = gio::Menu::new();
            for (kind, about) in kinds {
                let item = gio::MenuItem::new(Some(&format!("{kind} \u{2014} {about}")), None);
                item.set_action_and_target_value(Some("steps.add"), Some(&kind.to_variant()));
                items.append_item(&item);
            }
            menu.append_section(Some(section), &items);
        }
        let content =
            adw::ButtonContent::builder().icon_name("rm-plus-symbolic").label("Add step").build();
        let button = gtk::MenuButton::builder()
            .child(&content)
            .menu_model(&menu)
            .tooltip_text("Add a step at the end: a key, the mouse, a wait…")
            .css_classes(["ds", "secondary", "sm"])
            .build();
        button.insert_action_group("steps", Some(&group));
        let me = Rc::downgrade(self);
        let wait = Btn::new("ds ghost sm")
            .text("Wait")
            .icon("rm-plus-symbolic")
            .tip("Add a wait at the end")
            .build(move || {
                if let Some(d) = me.upgrade() {
                    d.add_rows(starting_rows("Wait"));
                }
            })
            .button;
        super::rail::indent(&hbox!(6, "", button, wait))
    }

    /// Above the timeline: Record, and what it does -- steps played in a
    /// running client, added at the end.
    pub(super) fn record_bar(self: &Rc<Self>) -> gtk::Box {
        let content = adw::ButtonContent::builder()
            .icon_name("rm-record-symbolic")
            .label("_Record")
            .use_underline(true)
            .build();
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text("Record steps by playing in a running client")
            .css_classes(["ds", "secondary", "sm"])
            .build();
        button.set_valign(gtk::Align::Center);
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
        let about = lbl(
            "Play it yourself in a running client; the steps land at the end. \
             F8 in its window starts and stops.",
            "cx-help",
        )
        .wrapped();
        about.set_hexpand(true);
        hbox!(12, "record-bar", button, about)
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
            let first = self.rows.borrow().len();
            self.rows.borrow_mut().extend(rows);
            self.draw_steps();
            // Straight on to what it does: the new step's value, ready to type.
            if let Some(entry) = self.entry_of(first) {
                entry.grab_focus();
            }
        }
    }

    /// The value entry drawn for row `i`, when it has one (a timeline's
    /// steps are edited as text).
    fn entry_of(&self, i: usize) -> Option<gtk::Entry> {
        let entries = self.entries.borrow();
        // Drawn in row order, a timeline's rows skipped: count the rows
        // before `i` that have one.
        let units = lanes::units(&self.rows.borrow());
        let mut drawn = 0;
        for (start, len) in units {
            if self.rows.borrow().get(start).is_some_and(|r| r.kind == "Timeline") {
                if (start..start + len).contains(&i) {
                    return None;
                }
                continue;
            }
            if (start..start + len).contains(&i) {
                return entries.get(drawn + i - start).cloned();
            }
            drawn += len;
        }
        None
    }
}
