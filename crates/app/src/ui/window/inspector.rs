//! The panel beside the table, for what needs more room than a row or the
//! details pane: an account's or a group's settings, signing an account in,
//! and editing a macro. One panel at a time; the side opens with it and
//! closes with it -- never a window over the accounts.

use adw::prelude::*;
use gtk::Orientation;

use super::{WeakWindow, Window};
use crate::ui::panel::Panel;
use crate::ui::widgets::{Fluent, name};

/// The panel's width, and the most it widens to while a macro is being
/// edited.
pub const NARROW: f64 = 400.0;
pub const WIDE: f64 = 600.0;

pub struct Inspector {
    pub root: gtk::Box,
    stack: gtk::Stack,
}

impl Inspector {
    pub fn build(w: &WeakWindow) -> Self {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .vexpand(true)
            .build();
        let root = gtk::Box::new(Orientation::Vertical, 0).css("mn-insp");
        name(&root, "Panel");
        root.append(&stack);
        // Esc closes the open panel, once whatever has focus in it (a hotkey
        // being captured) has had the key.
        let keys = gtk::EventControllerKey::new();
        let w = w.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(w) = w.upgrade()
                && w.close_panel()
            {
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        root.add_controller(keys);
        Inspector { root, stack }
    }
}

impl Window {
    /// Show `panel` beside the table, closing the one there before.
    pub fn show_panel(&self, panel: &Panel) {
        let old = self.0.ui.panel.borrow_mut().take();
        if let Some(old) = old
            && &old != panel
        {
            old.close();
        }
        let stack = &self.0.ui.insp.stack;
        if panel.parent().is_none() {
            stack.add_child(panel);
        }
        stack.set_visible_child(panel);
        self.0.ui.panel.replace(Some(panel.clone()));
        let (weak, me) = (self.weak(), panel.downgrade());
        panel.set_closer(Box::new(move || {
            if let (Some(w), Some(p)) = (weak.upgrade(), me.upgrade()) {
                w.drop_panel(&p);
            }
        }));
        // The macro editor takes up to WIDE, half the window at most, and
        // never less than any other panel: a narrow or tiled window keeps it
        // whole rather than drawing it past its edge.
        let max = if panel.is_for_macros() { WIDE } else { NARROW };
        let split = &self.0.ui.split;
        split.set_min_sidebar_width(NARROW);
        split.set_max_sidebar_width(max);
        split.set_show_sidebar(true);
        if let Some(button) = panel.default_widget() {
            self.0.win.set_default_widget(Some(&button));
        }
        if let Some(target) = panel.focus_target() {
            target.grab_focus();
        }
    }

    /// Close the open panel; false when there is none.
    pub fn close_panel(&self) -> bool {
        let open = self.0.ui.panel.borrow().clone();
        match open {
            Some(p) => {
                p.close();
                true
            }
            None => false,
        }
    }

    fn drop_panel(&self, panel: &Panel) {
        if panel.parent().is_some() {
            self.0.ui.insp.stack.remove(panel);
        }
        let mut open = self.0.ui.panel.borrow_mut();
        if open.as_ref() == Some(panel) {
            *open = None;
        }
        let none = open.is_none();
        drop(open);
        self.0.win.set_default_widget(None::<&gtk::Widget>);
        if none {
            self.0.ui.split.set_show_sidebar(false);
        }
    }

    /// Mark the current account or group: drawn current in the table, and
    /// the account shown in the details pane.
    pub fn inspect(&self, what: Option<crate::state::Inspected>) {
        if self.state().inspected == what {
            return;
        }
        self.state_mut().inspected = what;
        self.draw_details();
        self.refresh_states();
    }
}
