//! The accounts as a table: a header of columns, the launch order, then
//! each group's band and its rows. Narrow windows drop columns, widest
//! first: perf and last, then the grip, role and macro.

pub mod band;
pub mod chain;
pub mod cmd;
pub mod row;

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use chrono::Utc;
use gtk::{Align, Orientation, glib};
use rbxmgr_core::accounts::relative_time;

use crate::ui::widgets::{Fluent, lbl};

/// A column of the table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Grip,
    Check,
    Av,
    Name,
    Status,
    Role,
    Macro,
    Perf,
    Last,
    Actions,
}

impl Col {
    /// Its width in px; the name takes what is left.
    fn width(self) -> i32 {
        match self {
            Col::Grip => 16,
            Col::Check => 20,
            Col::Av => 24,
            Col::Name => 150,
            Col::Status => 132,
            Col::Role => 96,
            Col::Macro => 176,
            Col::Perf => 48,
            Col::Last => 44,
            Col::Actions => 100,
        }
    }

    /// The narrowness at which it goes: 1 below 1280 px, 2 below 860 px.
    fn gone_at(self) -> u8 {
        match self {
            Col::Perf | Col::Last => 1,
            Col::Grip | Col::Role | Col::Macro => 2,
            _ => u8::MAX,
        }
    }
}

/// The cells that come and go with the window's width.
#[derive(Default)]
pub struct Columns {
    level: Cell<u8>,
    cells: RefCell<Vec<(glib::WeakRef<gtk::Widget>, u8)>>,
}

impl Columns {
    /// `child` in its column's cell, shown or not for the width now.
    pub fn cell(&self, col: Col, child: &impl IsA<gtk::Widget>) -> gtk::Box {
        let b = gtk::Box::new(Orientation::Horizontal, 4);
        b.set_valign(Align::Center);
        if col == Col::Name {
            b.set_hexpand(true);
            b.set_size_request(col.width(), -1);
        } else {
            b.set_size_request(col.width(), -1);
        }
        b.append(child);
        let at = col.gone_at();
        if at != u8::MAX {
            b.set_visible(self.level.get() < at);
            self.cells.borrow_mut().push((b.upcast_ref::<gtk::Widget>().downgrade(), at));
        }
        b
    }

    /// The window's width changed narrowness: show and hide to match.
    pub fn set_level(&self, level: u8) {
        self.level.set(level);
        self.cells.borrow_mut().retain(|(w, at)| match w.upgrade() {
            Some(w) => {
                w.set_visible(level < *at);
                true
            }
            None => false,
        });
    }

    pub fn level(&self) -> u8 {
        self.level.get()
    }
}

/// The header over the rows.
pub fn header(cols: &Columns) -> gtk::Box {
    let h = gtk::Box::new(Orientation::Horizontal, 8).css("mn-thead");
    for (col, text) in [
        (Col::Grip, ""),
        (Col::Check, ""),
        (Col::Av, ""),
        (Col::Name, "ACCOUNT"),
        (Col::Status, "STATUS"),
        (Col::Role, "ROLE"),
        (Col::Macro, "MACRO"),
        (Col::Perf, "PERF"),
        (Col::Last, "LAST"),
        (Col::Actions, "ACTIONS"),
    ] {
        let l = lbl(text, "");
        if col == Col::Actions {
            l.set_hexpand(true);
            l.set_xalign(1.0);
        }
        h.append(&cols.cell(col, &l));
    }
    h
}

/// When an account last launched, as short as the column: "34m", "3h".
pub fn short_since(iso: Option<&str>) -> String {
    let r = relative_time(iso, Utc::now());
    if iso.is_none() || r == "never launched" {
        return "—".to_owned();
    }
    let r = r.trim_end_matches(" ago");
    if r == "just now" || r.ends_with('s') {
        return "now".to_owned();
    }
    // A date: its month and day fit the column.
    if r.len() == 10 { r[5..].to_owned() } else { r.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_column_says_how_long_ago_in_a_few_letters() {
        assert_eq!(short_since(None), "—");
        let ago = |secs: i64| (Utc::now() - chrono::Duration::seconds(secs)).to_rfc3339();
        assert_eq!(short_since(Some(&ago(5))), "now");
        assert_eq!(short_since(Some(&ago(34 * 60))), "34m");
        assert_eq!(short_since(Some(&ago(3 * 3600))), "3h");
        assert_eq!(short_since(Some(&ago(2 * 86_400))), "2d");
        assert_eq!(short_since(Some("2020-03-04T00:00:00Z")), "03-04");
    }
}
