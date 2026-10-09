//! The inspector beside the accounts: Details (an overview, or the account,
//! group or sign-in being looked at) and Macros (the list, or the macro
//! being edited). One panel at a time -- never a window over the accounts.

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use super::{WeakWindow, Window};
use crate::state::Inspected;
use crate::ui::ds;
use crate::ui::panel::Panel;
use crate::ui::widgets::{Fluent, lbl, name};

/// The inspector's width, and its width while a macro is being edited.
pub const NARROW: f64 = 380.0;
pub const WIDE: f64 = 600.0;

pub struct Inspector {
    pub root: gtk::Box,
    pub details_tab: gtk::ToggleButton,
    pub macros_tab: gtk::ToggleButton,
    pub macros_count: gtk::Label,
    pub details: gtk::Stack,
    pub overview: gtk::Box,
    pub macros: gtk::Stack,
    pub macros_list: gtk::Box,
}

impl Inspector {
    pub fn build(w: &WeakWindow) -> Self {
        let details_tab = gtk::ToggleButton::builder().label("Details").active(true).build();
        details_tab.add_css_class("cx-tab");
        let macros_count = lbl("", "cx-count success-text").visible(false);
        let inner = gtk::Box::new(Orientation::Horizontal, 8);
        inner.append(&lbl("Macros", ""));
        inner.append(&macros_count.clone().valign(Align::Center));
        let macros_tab = gtk::ToggleButton::builder().child(&inner).group(&details_tab).build();
        macros_tab.add_css_class("cx-tab");
        let tabs = gtk::Box::new(Orientation::Horizontal, 4).css("cx-tabs");
        tabs.append(&details_tab);
        tabs.append(&macros_tab);
        tabs.append(&gtk::Box::new(Orientation::Horizontal, 0).hexpand());
        let help = ds::ib("info", "Keyboard shortcuts and about (Ctrl+?)", true);
        help.set_action_name(Some("win.shortcuts"));
        tabs.append(&help);
        let hide = ds::ib("panel", "Hide the inspector (F9)", true);
        hide.set_action_name(Some("win.toggle-sidebar"));
        tabs.append(&hide);
        for t in [&details_tab, &macros_tab] {
            t.set_valign(Align::Fill);
        }

        let overview = gtk::Box::new(Orientation::Vertical, 0);
        let details = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .vhomogeneous(false)
            .build();
        details.add_named(&scrolled(&overview), Some("overview"));
        let macros_list = gtk::Box::new(Orientation::Vertical, 0);
        let macros = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .vhomogeneous(false)
            .build();
        macros.add_named(&scrolled(&macros_list), Some("list"));
        let pages = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .vexpand(true)
            .build();
        pages.add_named(&details, Some("details"));
        pages.add_named(&macros, Some("macros"));
        {
            let (pages, w) = (pages.clone(), w.clone());
            details_tab.connect_toggled(move |t| {
                if t.is_active() {
                    pages.set_visible_child_name("details");
                    if let Some(w) = w.upgrade() {
                        w.fit_inspector();
                    }
                }
            });
        }
        {
            let (pages, w) = (pages.clone(), w.clone());
            macros_tab.connect_toggled(move |t| {
                if t.is_active() {
                    pages.set_visible_child_name("macros");
                    if let Some(w) = w.upgrade() {
                        w.fit_inspector();
                    }
                }
            });
        }
        let root = gtk::Box::new(Orientation::Vertical, 0).css("mn-insp");
        name(&root, "Inspector");
        root.append(&tabs);
        root.append(&pages);
        {
            // Esc closes the open panel, once whatever has focus in it (a
            // hotkey being captured) has had the key.
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
        }
        Inspector {
            root,
            details_tab,
            macros_tab,
            macros_count,
            details,
            overview,
            macros,
            macros_list,
        }
    }
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build()
}

impl Window {
    /// Show `panel` in the inspector, under its tab, closing the one there
    /// before.
    pub fn show_panel(&self, panel: &Panel) {
        let old = self.0.ui.panel.borrow_mut().take();
        if let Some(old) = old
            && &old != panel
        {
            old.close();
        }
        let insp = &self.0.ui.insp;
        let stack = if panel.is_for_macros() { &insp.macros } else { &insp.details };
        if panel.parent().is_none() {
            stack.add_child(panel);
        }
        stack.set_visible_child(panel);
        if panel.is_for_macros() {
            insp.macros_tab.set_active(true);
        } else {
            insp.details_tab.set_active(true);
        }
        self.0.ui.split.set_show_sidebar(true);
        self.0.ui.panel.replace(Some(panel.clone()));
        let (weak, me) = (self.weak(), panel.downgrade());
        panel.set_closer(Box::new(move || {
            if let (Some(w), Some(p)) = (weak.upgrade(), me.upgrade()) {
                w.drop_panel(&p);
            }
        }));
        if let Some(button) = panel.default_widget() {
            self.0.win.set_default_widget(Some(&button));
        }
        if let Some(target) = panel.focus_target() {
            target.grab_focus();
        }
        self.fit_inspector();
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
        let insp = &self.0.ui.insp;
        let stack = if panel.is_for_macros() { &insp.macros } else { &insp.details };
        stack.set_visible_child_name(if panel.is_for_macros() { "list" } else { "overview" });
        if panel.parent().is_some() {
            stack.remove(panel);
        }
        let mut open = self.0.ui.panel.borrow_mut();
        if open.as_ref() == Some(panel) {
            *open = None;
        }
        drop(open);
        self.0.win.set_default_widget(None::<&gtk::Widget>);
        if !panel.is_for_macros() {
            self.inspect(None);
        }
        self.fit_inspector();
    }

    /// Mark what the Details tab shows, so its row or band is drawn current.
    pub fn inspect(&self, what: Option<Inspected>) {
        if self.state().inspected == what {
            return;
        }
        self.state_mut().inspected = what;
        self.refresh_states();
    }

    /// The inspector's width: wide while a macro is open under Macros.
    pub fn fit_inspector(&self) {
        let insp = &self.0.ui.insp;
        let wide = insp.macros_tab.is_active()
            && insp.macros.visible_child_name().is_none_or(|n| n != "list");
        let px = if wide { WIDE } else { NARROW };
        let split = &self.0.ui.split;
        split.set_min_sidebar_width(px);
        split.set_max_sidebar_width(px);
    }

    /// Show the inspector on its Macros tab.
    pub fn show_macros_tab(&self) {
        self.0.ui.split.set_show_sidebar(true);
        self.0.ui.insp.macros_tab.set_active(true);
    }
}
