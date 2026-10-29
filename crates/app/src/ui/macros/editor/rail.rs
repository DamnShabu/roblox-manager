//! The editor's steps drawn as a timeline: each at a stop on a rail down
//! the left, with when it starts beside it, and the round's end at the
//! bottom.

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::ui::widgets::{Fluent, LabelFluent, lbl};

/// How a stop sits on the rail.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    /// The first: no rail above it.
    First,
    /// Any after it.
    Next,
    /// A step under the one before (a when's dos): no dot of its own.
    Under,
}

/// `content` at a stop on the rail: `time` (a [`time`] label) beside it, a dot with `icon` on
/// the rail. After a wait the rail below is dashed: time passing.
pub(super) fn stop(
    time: &gtk::Label,
    icon: &str,
    content: &impl IsA<gtk::Widget>,
    at: Stop,
    waits: bool,
) -> gtk::Box {
    let above = line(at == Stop::First, false);
    let below = line(false, waits);
    if waits {
        below.set_size_request(-1, 18);
    }
    let rail = gtk::Box::new(Orientation::Vertical, 0);
    rail.add_css_class("rail");
    rail.append(&above);
    if at == Stop::Under {
        // The rail runs straight past it.
        above.set_vexpand(true);
    } else {
        rail.append(&dot(icon));
    }
    rail.append(&below);
    let content = content.as_ref();
    content.set_hexpand(true);
    content.set_valign(Align::Center);
    content.set_margin_top(3);
    content.set_margin_bottom(3);
    hbox!(6, "rail-stop", time.clone(), rail, content.clone())
}

/// The round's end, after the last stop: when it comes, and what follows.
pub(super) fn end(time: &gtk::Label, then: &gtk::Label, icon: &str) -> gtk::Box {
    let rail = gtk::Box::new(Orientation::Vertical, 0);
    rail.add_css_class("rail");
    // A short length to it: the stops above take what room there is.
    let above = line(false, false);
    above.set_vexpand(false);
    above.set_size_request(-1, 10);
    rail.append(&above);
    rail.append(&dot(icon).css("end"));
    then.set_hexpand(true);
    then.set_valign(Align::Center);
    then.set_margin_top(10);
    let end = hbox!(6, "rail-stop", time.clone(), rail, then.clone());
    time.set_margin_top(10);
    end.set_vexpand(false);
    end
}

/// Room where a stop's time and dot go, for what lines up with the stops.
pub(super) fn indent(content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let rail = gtk::Box::new(Orientation::Vertical, 0);
    rail.add_css_class("rail");
    hbox!(6, "", time(""), rail, content.as_ref().clone())
}

/// The label a stop's time is in.
pub(super) fn time(text: &str) -> gtk::Label {
    let l = lbl(text, "rail-time").xalign(1.0);
    l.set_valign(Align::Center);
    l.set_width_chars(8);
    l
}

/// A length of the rail, to fill the stop's height above or below its dot.
fn line(hidden: bool, dashed: bool) -> gtk::Box {
    let l = gtk::Box::new(Orientation::Vertical, 0);
    l.add_css_class("rail-line");
    if hidden {
        l.add_css_class("hidden");
    }
    if dashed {
        l.add_css_class("dashed");
    }
    l.set_halign(Align::Center);
    l.set_vexpand(true);
    l.set_size_request(-1, 4);
    l
}

fn dot(icon: &str) -> gtk::Box {
    let d = hbox!(0, "rail-dot", crate::ui::ds::icon(icon).css("s16"));
    d.set_halign(Align::Center);
    d.set_valign(Align::Center);
    d
}
