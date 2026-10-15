//! The status bar along the bottom: what running several clients risks,
//! whether browser links open here, the latest activity line (a click opens
//! the log), and the Activity toggle for the drawer.

use adw::prelude::*;
use gtk::{Align, Orientation};

use crate::state::Activity;
use crate::ui::activity;
use crate::ui::ds;
use crate::ui::widgets::{Fluent, LabelFluent, lbl, toggle_class};

/// Said wherever accounts are launched together: PRODUCT.md's risk line.
pub const RISK: &str = "Running more than one client breaks Roblox's terms, and anti-cheat flags \
                        have been reported. Use alts you can afford to lose.";

pub struct Strip {
    pub root: gtk::Box,
    last_icon: gtk::Image,
    last_time: gtk::Label,
    last_text: gtk::Label,
    pub last: gtk::Button,
    pub links: gtk::Box,
    pub log_toggle: gtk::ToggleButton,
}

impl Strip {
    pub fn build() -> Self {
        let root = gtk::Box::new(Orientation::Horizontal, 12).css("mn-strip");
        let risk = gtk::Box::new(Orientation::Horizontal, 8);
        risk.append(&ds::icon("info").css("s16"));
        risk.append(&lbl(RISK, "").ellipsize().tip(RISK));
        risk.set_hexpand(true);
        root.append(&risk);
        let links = gtk::Box::new(Orientation::Horizontal, 6).css("mn-strip-item");
        links.append(&ds::icon("check").css("s16"));
        links.append(&lbl("Links open here", ""));
        links.set_tooltip_text(Some("Roblox links from the browser open in this app"));
        links.set_visible(false);
        root.append(&links);
        let last_icon = ds::icon("history").css("s16");
        let last_time = lbl("", "");
        let last_text = lbl("Ready", "").ellipsize().chars(48);
        let inner = gtk::Box::new(Orientation::Horizontal, 8);
        inner.append(&last_icon);
        inner.append(&last_time);
        inner.append(&last_text);
        let last = gtk::Button::builder()
            .child(&inner)
            .tooltip_text("Open the activity log (Ctrl+L)")
            .css_classes(["mn-strip-btn"])
            .valign(Align::Center)
            .build();
        last.set_action_name(Some("win.activity-log"));
        root.append(&last);
        let inner = gtk::Box::new(Orientation::Horizontal, 4);
        inner.append(&lbl("Activity", ""));
        inner.append(&ds::icon("chev-up").css("s16"));
        let log_toggle = gtk::ToggleButton::builder()
            .child(&inner)
            .tooltip_text("Show or hide the activity drawer (Ctrl+L)")
            .valign(Align::Center)
            .css_classes(["mn-strip-btn", "drawer"])
            .build();
        root.append(&log_toggle);
        Strip { root, last_icon, last_time, last_text, last, links, log_toggle }
    }

    /// Show the newest line.
    pub fn show(&self, newest: Option<&Activity>) {
        let Some(e) = newest else { return };
        let kind = activity::kind(&e.line);
        self.last_icon.set_icon_name(Some(&crate::ui::icons::name(activity::icon_for(kind))));
        let tone = match activity::tone_class(kind) {
            Some("danger-text") => Some("bad"),
            Some("success-text") => Some("run"),
            Some("warning-text") => Some("start"),
            _ => None,
        };
        for k in ["bad", "run", "start"] {
            toggle_class(&self.last_icon, k, tone == Some(k));
        }
        self.last_time.set_label(&e.time);
        self.last_text.set_label(&e.line);
        self.last.set_tooltip_text(Some(&e.line));
    }
}
