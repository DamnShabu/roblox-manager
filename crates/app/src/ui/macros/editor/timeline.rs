//! A timeline in the editor's step list: one card for it and every step
//! under it, a lane for each key, button and the mouse, a bar for each step
//! from when it starts to when it ends. Its steps are edited as text.

use std::rc::Rc;

use adw::prelude::*;
use gtk::Align;
use rbxmgr_core::macros::lanes::{self, Bar, Lane};

use super::MacroDialog;
use crate::ui::widgets::{Btn, Fluent, LabelFluent, icon, lbl, plural};

/// How tall a lane is.
const LANE_HEIGHT: i32 = 14;
/// How near a bar the pointer has to be for its tooltip, in pixels.
const NEAR: f64 = 3.0;
/// The most steps one tooltip names.
const SHOWN: usize = 6;

/// What one of the card's buttons does to the dialog.
type Act = Box<dyn Fn(&Rc<MacroDialog>)>;

impl MacroDialog {
    /// The card for the timeline whose rows are `len` from `start`: listed
    /// `number`th of `count` steps.
    pub(super) fn timeline_row(
        self: &Rc<Self>,
        number: usize,
        count: usize,
        start: usize,
        len: usize,
    ) -> gtk::ListBoxRow {
        let (drawn, steps) = lanes::of_rows(&self.rows.borrow()[start..start + len]);

        let button = |ic: &str, tip: &str, on: bool, act: Act| {
            let me = Rc::downgrade(self);
            let b = Btn::new("flat circular").icon(ic).tip(tip).build(move || {
                if let Some(d) = me.upgrade() {
                    act(&d);
                }
            });
            b.button.set_sensitive(on);
            b.button.set_valign(Align::Center);
            b.button
        };
        let about = format!("{:.1} s · {}", drawn.secs, plural(steps, "step", "steps"));
        let header = hbox!(
            8,
            "",
            lbl(&format!("{number}"), "number dimmed").xalign(1.0),
            icon("document-open-recent-symbolic").css("dimmed"),
            lbl("Timeline", "heading"),
            lbl(&about, "dimmed").hexpand().xalign(0.0),
            button(
                "document-edit-symbolic",
                "Edit its steps as text",
                true,
                Box::new(|d| d.view.set_active_name(Some("text")))
            ),
            button(
                "go-up-symbolic",
                "Move up",
                number > 1,
                Box::new(move |d| d.shift(start, true))
            ),
            button(
                "go-down-symbolic",
                "Move down",
                number < count,
                Box::new(move |d| d.shift(start, false))
            ),
            button(
                "user-trash-symbolic",
                "Remove the timeline and its steps",
                true,
                Box::new(move |d| {
                    d.rows.borrow_mut().drain(start..start + len);
                    d.draw_steps();
                })
            )
        );
        let grid = gtk::Grid::builder().row_spacing(3).column_spacing(10).margin_top(8).build();
        for (i, lane) in drawn.lanes.iter().enumerate() {
            let name = lbl(&lane.name, "caption dimmed").xalign(1.0).ellipsize();
            name.set_width_chars(8);
            name.set_max_width_chars(12);
            grid.attach(&name, 0, i as i32, 1, 1);
            grid.attach(&lane_area(lane, drawn.secs), 1, i as i32, 1, 1);
        }
        let ends = hbox!(0, "", lbl("0 s", "caption dimmed").hexpand().xalign(0.0));
        ends.append(&lbl(&format!("{:.1} s", drawn.secs), "caption dimmed"));
        grid.attach(&ends, 1, drawn.lanes.len() as i32, 1, 1);
        let row = gtk::ListBoxRow::builder()
            .activatable(false)
            .selectable(false)
            .child(&vbox!(0, "", header, grid))
            .build();
        row.add_css_class("step-row");
        row
    }
}

/// One lane: its bars across a timeline `secs` long, each named in a
/// tooltip.
fn lane_area(lane: &Lane, secs: f64) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder().hexpand(true).content_height(LANE_HEIGHT).build();
    let span = secs.max(0.01);
    let bars = lane.bars.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        let fg = area.color();
        cr.set_source_rgba(fg.red().into(), fg.green().into(), fg.blue().into(), 0.08);
        cr.rectangle(0.0, 0.0, w, h);
        fill(cr);
        let accent = adw::StyleManager::default().accent_color_rgba();
        cr.set_source_rgba(
            accent.red().into(),
            accent.green().into(),
            accent.blue().into(),
            accent.alpha().into(),
        );
        for bar in &bars {
            let (x, width) = place(bar, span, w);
            cr.rectangle(x, 1.0, width, h - 2.0);
        }
        fill(cr);
    });
    let bars = lane.bars.clone();
    area.set_has_tooltip(true);
    area.connect_query_tooltip(move |area, x, _, _, tip| {
        let w = f64::from(area.width());
        let x = f64::from(x);
        let near: Vec<String> = bars
            .iter()
            .filter(|bar| {
                let (from, width) = place(bar, span, w);
                x >= from - NEAR && x <= from + width + NEAR
            })
            .take(SHOWN)
            .map(|bar| format!("at {}", bar.line))
            .collect();
        if near.is_empty() {
            return false;
        }
        tip.set_text(Some(&near.join("\n")));
        true
    });
    area
}

/// Where a bar is drawn in a lane `w` wide: its left edge and width, never
/// too thin to see.
fn place(bar: &Bar, span: f64, w: f64) -> (f64, f64) {
    let x = bar.from / span * w;
    (x, ((bar.to - bar.from) / span * w).max(2.0))
}

/// Fill the path drawn so far; a lane cairo cannot fill is left blank, and
/// told.
fn fill(cr: &gtk::cairo::Context) {
    if let Err(e) = cr.fill() {
        eprintln!("rbxmgr: could not draw a timeline lane: {e}");
    }
}
