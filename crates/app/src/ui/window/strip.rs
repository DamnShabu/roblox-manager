//! The status strip along the bottom: the latest activity line (a click
//! opens the drawer), whether browser links open here, how many run, and
//! the Activity toggle for the drawer.

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::state::Activity;
use crate::ui::activity;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, LabelFluent, lbl, toggle_class};

pub struct Strip {
    pub root: gtk::Box,
    last_icon: gtk::Image,
    last_time: gtk::Label,
    last_text: gtk::Label,
    pub last: gtk::Button,
    pub links: gtk::Box,
    pub status: gtk::Label,
    pub log_toggle: gtk::ToggleButton,
}

impl Strip {
    pub fn build() -> Self {
        let root = gtk::Box::new(Orientation::Horizontal, 12).css("mn-strip");
        let last_icon = ds::icon("info");
        let last_time = lbl("", "t");
        let last_text = lbl("Ready", "x").ellipsize();
        let inner = gtk::Box::new(Orientation::Horizontal, 8);
        inner.append(&last_icon);
        inner.append(&last_time);
        inner.append(&last_text);
        let last = gtk::Button::builder()
            .child(&inner)
            .tooltip_text("Open the activity log (Ctrl+L)")
            .css_classes(["mn-strip-last"])
            .hexpand(true)
            .halign(Align::Fill)
            .build();
        last.set_action_name(Some("win.activity-log"));
        root.append(&last);
        let links = gtk::Box::new(Orientation::Horizontal, 4).css("mn-strip-item");
        links.append(&ds::icon("check"));
        links.append(&lbl("Links open here", ""));
        links.set_tooltip_text(Some("Roblox links from the browser open in this app"));
        links.set_visible(false);
        root.append(&links);
        let status = lbl("All idle", "mn-strip-item");
        root.append(&status);
        let inner = gtk::Box::new(Orientation::Horizontal, 4);
        inner.append(&lbl("Activity", ""));
        inner.append(&ds::icon("chev-up"));
        let log_toggle = gtk::ToggleButton::builder()
            .child(&inner)
            .tooltip_text("Show or hide the activity drawer (Ctrl+L)")
            .valign(Align::Center)
            .css_classes(["mn-strip-btn"])
            .build();
        root.append(&log_toggle);
        Strip { root, last_icon, last_time, last_text, last, links, status, log_toggle }
    }

    /// Show the newest line.
    pub fn show(&self, newest: Option<&Activity>) {
        let Some(e) = newest else { return };
        let kind = activity::kind(&e.line);
        self.last_icon.set_icon_name(Some(&crate::ui::icons::name(activity::icon_for(kind))));
        for k in ["danger-text", "success-text", "warning-text", "muted"] {
            toggle_class(&self.last_icon, k, false);
        }
        if let Some(c) = activity::tone_class(kind) {
            self.last_icon.add_css_class(c);
        }
        self.last_time.set_label(&e.time);
        self.last_text.set_label(&e.line);
        self.last.set_tooltip_text(Some(&e.line));
    }
}
