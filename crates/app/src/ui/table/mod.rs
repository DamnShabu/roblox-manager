//! The accounts as a table: a header of columns, what needs attention, the
//! main account, then each group's row and its accounts. Narrower windows
//! drop columns, the least needed first.

pub mod attention;
pub mod band;
mod fixed;
pub mod format;
pub mod perf;
pub mod row;
pub mod toolbar;

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gtk::{Align, Orientation, glib};

use self::fixed::fixed_natural;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, lbl};

/// A column of the table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Check,
    Name,
    Status,
    Uptime,
    Cpu,
    Memory,
    Macro,
    Perf,
    Actions,
}

/// The space between two columns, as the stylesheet sets it on a row.
const GAP: i32 = 12;

impl Col {
    /// Its width in px; the actions take what is left, at least this.
    fn width(self) -> i32 {
        match self {
            Col::Check => 20,
            Col::Name => 300,
            Col::Status => 124,
            Col::Uptime => 80,
            Col::Cpu => 60,
            Col::Memory => 76,
            Col::Macro => 200,
            Col::Perf => 140,
            Col::Actions => 128,
        }
    }

    /// The narrowness at which it goes: 1 below 1280 px, 2 below 1100 px,
    /// 3 below 860 px.
    fn gone_at(self) -> u8 {
        match self {
            Col::Uptime | Col::Memory => 1,
            Col::Cpu | Col::Macro => 2,
            Col::Perf => 3,
            Col::Check | Col::Name | Col::Status | Col::Actions => u8::MAX,
        }
    }

    /// Whether its numbers line up on the right.
    fn numeric(self) -> bool {
        matches!(self, Col::Cpu | Col::Memory)
    }
}

/// A cell that comes and goes with the window's width: one column's, or
/// several columns' taken as one.
enum Fit {
    One(u8),
    Span(Vec<Col>),
}

/// The cells that come and go with the window's width.
#[derive(Default)]
pub struct Columns {
    level: Cell<u8>,
    cells: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Fit)>>,
}

impl Columns {
    /// `child` in its column's cell, shown or not for the width now.
    pub fn cell(&self, col: Col, child: &impl IsA<gtk::Widget>) -> gtk::Box {
        let b = gtk::Box::new(Orientation::Horizontal, 6);
        b.set_valign(Align::Center);
        b.set_size_request(col.width(), -1);
        // The spare width goes before the row's actions. Set, not left to
        // GTK: a cell would otherwise expand with whatever expands inside it,
        // and each row would share its spare width out differently.
        b.set_hexpand(col == Col::Actions);
        if col.numeric() || col == Col::Actions {
            // One child across the whole cell, at its end.
            b.set_homogeneous(true);
            child.set_halign(Align::End);
        }
        if col == Col::Name {
            b.append(&fixed_natural(child, col.width()));
        } else {
            b.append(child);
        }
        let at = col.gone_at();
        if at != u8::MAX {
            b.set_visible(self.level.get() < at);
            self.cells.borrow_mut().push((b.upcast_ref::<gtk::Widget>().downgrade(), Fit::One(at)));
        }
        b
    }

    /// `child` across `cols` as one cell, as wide as those of them the
    /// window's width shows.
    pub fn span(&self, cols: &[Col], child: &impl IsA<gtk::Widget>) -> gtk::Box {
        let b = gtk::Box::new(Orientation::Horizontal, 8);
        b.set_valign(Align::Center);
        b.set_hexpand(false);
        b.set_homogeneous(true);
        b.append(child);
        b.set_size_request(span_width(cols, self.level.get()), -1);
        self.cells
            .borrow_mut()
            .push((b.upcast_ref::<gtk::Widget>().downgrade(), Fit::Span(cols.to_vec())));
        b
    }

    /// The window's width changed narrowness: show, hide and size to match.
    pub fn set_level(&self, level: u8) {
        self.level.set(level);
        self.cells.borrow_mut().retain(|(w, sized)| match w.upgrade() {
            Some(w) => {
                match sized {
                    Fit::One(at) => w.set_visible(level < *at),
                    Fit::Span(cols) => w.set_size_request(span_width(cols, level), -1),
                }
                true
            }
            None => false,
        });
    }

    pub fn level(&self) -> u8 {
        self.level.get()
    }
}

/// How wide `cols` are together at a narrowness: their widths and the gaps
/// between those shown.
fn span_width(cols: &[Col], level: u8) -> i32 {
    let shown: Vec<Col> = cols.iter().copied().filter(|c| level < c.gone_at()).collect();
    let gaps = i32::try_from(shown.len().saturating_sub(1)).unwrap_or(0) * GAP;
    shown.iter().map(|c| c.width()).sum::<i32>() + gaps
}

/// The header over the rows. `select_all` sits in the check column.
pub fn header(cols: &Columns, select_all: &gtk::CheckButton) -> gtk::Box {
    let h = gtk::Box::new(Orientation::Horizontal, GAP).css("mn-thead");
    h.append(&cols.cell(Col::Check, select_all));
    for (col, text) in [
        (Col::Name, "Account"),
        (Col::Status, "Status"),
        (Col::Uptime, "Uptime"),
        (Col::Cpu, "CPU"),
        (Col::Memory, "Memory"),
        (Col::Macro, "Macro"),
        (Col::Perf, "Performance"),
        (Col::Actions, ""),
    ] {
        h.append(&cols.cell(col, &lbl(text, "")));
    }
    h
}

/// A row of the table, its cells spaced as the header's.
pub fn table_row(css: &str) -> gtk::Box {
    gtk::Box::new(Orientation::Horizontal, GAP).css(css)
}

/// An icon button for a row: its icon, and a tip that names it for screen
/// readers.
pub fn row_button(icon: &str, tip: &str) -> gtk::Button {
    let b = ds::ib(icon, tip, true);
    b.add_css_class("mn-act");
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_is_as_wide_as_its_shown_columns_and_their_gaps() {
        let cols = [Col::Status, Col::Uptime, Col::Cpu];
        assert_eq!(span_width(&cols, 0), 124 + 80 + 60 + 2 * GAP);
        assert_eq!(span_width(&cols, 1), 124 + 60 + GAP, "uptime goes first");
        assert_eq!(span_width(&cols, 2), 124);
    }
}
