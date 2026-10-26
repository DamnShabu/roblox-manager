//! The parts of the window that are built once, put together: the header
//! bar; the sidebar beside the main column (the toolbar, the table and the
//! details pane under it); the panel that opens beside them for settings
//! and editors; the status bar along the bottom and the activity drawer
//! that rises over it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, glib};

use super::WeakWindow;
use super::details::Details;
use super::inspector::{Inspector, NARROW};
use super::sidebar::Sidebar;
use super::strip::Strip;
use super::topbar::TopBar;
use crate::ui::activity::LogDrawer;
use crate::ui::ds;
use crate::ui::panel::Panel;
use crate::ui::table::toolbar::Toolbar;
use crate::ui::table::{Columns, header};
use crate::ui::target::TargetPopover;
use crate::ui::widgets::{Fluent, LabelFluent, lbl};

pub struct Chrome {
    pub top: TopBar,
    pub target_pop: Rc<TargetPopover>,
    pub busy_bar: gtk::ProgressBar,
    pub banner: adw::Banner,
    pub sidebar: Sidebar,
    pub toolbar: Toolbar,
    pub select_all: gtk::CheckButton,
    /// Set while a redraw brings the select-all box in line, so it is not a
    /// click.
    pub select_quiet: Rc<Cell<bool>>,
    /// "welcome" with no accounts, else "accounts".
    pub pages: gtk::Stack,
    /// What needs attention, the main account, the groups and their rows.
    pub table: gtk::Box,
    pub cols: Columns,
    pub details: Details,
    pub insp: Inspector,
    pub split: adw::OverlaySplitView,
    pub strip: Strip,
    pub log: LogDrawer,
    pub toasts: adw::ToastOverlay,
    pub shortcuts: gtk::ShortcutController,
    pub panel: RefCell<Option<Panel>>,
}

impl Chrome {
    pub fn build(win: &adw::ApplicationWindow, w: &WeakWindow, sidebar_shown: bool) -> Self {
        let top = TopBar::build(w);
        let toolbar = Toolbar::build();
        let target_pop = TargetPopover::build(w);
        toolbar.target.set_popover(Some(&target_pop.popover));
        let busy_bar = gtk::ProgressBar::builder().css_classes(["loadbar"]).visible(false).build();
        busy_bar.set_valign(Align::Start);
        busy_bar.set_pulse_step(0.08);
        let banner =
            adw::Banner::new("Installing the newest Roblox build — this can take a few minutes");

        // -- the table: its header, then the rows ----------------------------------
        let cols = Columns::default();
        let select_quiet = Rc::new(Cell::new(false));
        let select_all = ds::check(false, "Select every account shown");
        {
            let (w, quiet) = (w.clone(), select_quiet.clone());
            select_all.connect_toggled(move |c| {
                if let (false, Some(w)) = (quiet.get(), w.upgrade()) {
                    w.select_every(c.is_active());
                }
            });
        }
        let table = gtk::Box::new(Orientation::Vertical, 0);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&table)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .css_classes(["mn-table"])
            .build();
        let columns = gtk::Box::new(Orientation::Vertical, 0);
        columns.append(&header(&cols, &select_all));
        columns.append(&scroller);
        // Narrower than its columns, the table scrolls sideways rather than
        // widening the window.
        let accounts = gtk::ScrolledWindow::builder()
            .child(&columns)
            .vscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();

        let logo = gtk::Image::from_icon_name("rm-brand");
        logo.set_pixel_size(72);
        let welcome = gtk::Box::new(Orientation::Vertical, 16).css("mn-tempty");
        welcome.set_valign(Align::Center);
        welcome.set_vexpand(true);
        welcome.append(&logo);
        welcome.append(&lbl("Add your first account", "mn-tempty-t").xalign(0.5));
        welcome.append(
            &lbl(
                "Accounts sign in with Roblox Quick Login: you approve the sign-in on a device \
                 where you are already signed in, and no password is typed here. The code is \
                 ready in the panel on the right.",
                "mn-tempty-s",
            )
            .wrapped()
            .chars(56)
            .xalign(0.5)
            .justify(),
        );
        welcome.set_halign(Align::Center);
        let pages = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        pages.add_named(&welcome, Some("welcome"));
        pages.add_named(&accounts, Some("accounts"));
        let details = Details::build();
        let main = gtk::Box::new(Orientation::Vertical, 0);
        main.append(&toolbar.root);
        main.append(&pages);
        main.append(&details.root);
        main.set_hexpand(true);

        // -- the sidebar beside it, the panel beside them ----------------------------
        let sidebar = Sidebar::build(w);
        sidebar.root.set_visible(sidebar_shown);
        let content = gtk::Box::new(Orientation::Horizontal, 0);
        content.append(&sidebar.root);
        content.append(&main);
        // The size the table area's own steps go by; it asks for no more
        // than this, and its steps make what it holds fit.
        let area = adw::BreakpointBin::builder()
            .child(&content)
            .width_request(360)
            .height_request(200)
            .build();
        let insp = Inspector::build(w);
        let split = adw::OverlaySplitView::builder()
            .content(&area)
            .sidebar(&insp.root)
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(NARROW)
            .max_sidebar_width(NARROW)
            .sidebar_width_fraction(0.5)
            .show_sidebar(false)
            // Shown only with a panel in it: libadwaita would otherwise show it
            // by itself whenever the window widens past collapsing.
            .pin_sidebar(true)
            .build();

        // -- the drawer over the bottom, the status bar under it ----------------------
        let log = LogDrawer::build(w);
        let drawer = gtk::Revealer::builder()
            .child(&log.root)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .valign(Align::End)
            .build();
        let strip = Strip::build();
        strip.log_toggle.bind_property("active", &drawer, "reveal-child").bidirectional().build();
        let body = gtk::Overlay::new();
        body.set_child(Some(&split));
        body.add_overlay(&drawer);
        body.add_overlay(&busy_bar);
        body.set_vexpand(true);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&body));

        let root = gtk::Box::new(Orientation::Vertical, 0);
        root.append(&top.root);
        root.append(&banner);
        root.append(&toasts);
        root.append(&strip.root);
        win.set_content(Some(&root));
        win.add_css_class("rm");

        let shortcuts = gtk::ShortcutController::new();
        win.add_controller(shortcuts.clone());

        let chrome = Chrome {
            top,
            target_pop,
            busy_bar,
            banner,
            sidebar,
            toolbar,
            select_all,
            select_quiet,
            pages,
            table,
            cols,
            details,
            insp,
            split,
            strip,
            log,
            toasts,
            shortcuts,
            panel: RefCell::default(),
        };
        chrome.breakpoints(win);
        chrome.content_breakpoints(&area, w);
        chrome
    }

    /// Narrower windows: the header's and the status bar's extras go, then
    /// the panel lies over the table rather than beside it.
    fn breakpoints(&self, win: &adw::ApplicationWindow) {
        let steps: [(&str, u8); 3] =
            [("max-width: 1100sp", 1), ("max-width: 860sp", 2), ("max-width: 720sp", 3)];
        for (cond, level) in steps {
            let Ok(cond) = adw::BreakpointCondition::parse(cond) else { continue };
            let bp = adw::Breakpoint::new(cond);
            bp.add_setter(&self.split, "collapsed", Some(&true.to_value()));
            // Over the table, the panel takes the window's width up to its
            // own most, rather than half of it.
            bp.add_setter(&self.split, "sidebar-width-fraction", Some(&1.0.to_value()));
            bp.add_setter(&self.top.search, "width-request", Some(&180.to_value()));
            // The risk line keeps its room; the activity's line gives way.
            bp.add_setter(&self.strip.last, "visible", Some(&false.to_value()));
            if level >= 2 {
                bp.add_setter(&self.top.status, "visible", Some(&false.to_value()));
            }
            if level >= 3 {
                bp.add_setter(&self.top.search, "visible", Some(&false.to_value()));
                bp.add_setter(&self.strip.links, "visible", Some(&false.to_value()));
            }
            win.add_breakpoint(bp);
        }
    }

    /// A narrower table area -- a narrow window, or a wide one with a panel
    /// open beside it: fewer columns, then what the selection can do folds
    /// into one menu, then no sidebar, then the toolbar's extras go.
    ///
    /// Measured on the area itself, not the window: steps that went by the
    /// window's width left the area as wide as its parts needed (1,472 px
    /// with the sidebar and the full toolbar, 2,072 with the macro editor
    /// open) on a window narrower than that, so a fullscreen or maximised
    /// window drew past the screen's edge and had its controls cut off.
    fn content_breakpoints(&self, bin: &adw::BreakpointBin, w: &WeakWindow) {
        // (the width, how much of the toolbar and sidebar goes, the table's
        // narrowness): each step leaves the table's columns room to fit.
        let steps: [(&str, u8, u8); 5] = [
            ("max-width: 1700sp", 0, 1),
            ("max-width: 1500sp", 1, 2),
            ("max-width: 1100sp", 2, 2),
            ("max-width: 800sp", 3, 3),
            ("max-width: 620sp", 4, 3),
        ];
        for (cond, level, columns) in steps {
            let Ok(cond) = adw::BreakpointCondition::parse(cond) else { continue };
            let bp = adw::Breakpoint::new(cond);
            if level >= 1 {
                bp.add_setter(&self.toolbar.tools, "visible", Some(&false.to_value()));
                bp.add_setter(&self.toolbar.selected, "visible", Some(&false.to_value()));
                bp.add_setter(&self.toolbar.sep, "visible", Some(&false.to_value()));
                bp.add_setter(&self.toolbar.compact, "visible", Some(&true.to_value()));
            }
            if level >= 2 {
                bp.add_setter(&self.sidebar.root, "visible", Some(&false.to_value()));
            }
            if level >= 3 {
                bp.add_setter(&self.toolbar.stop_all.button, "visible", Some(&false.to_value()));
            }
            if level >= 4 {
                bp.add_setter(&self.toolbar.count, "visible", Some(&false.to_value()));
            }
            let weak = w.clone();
            bp.connect_apply(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.columns().set_level(columns);
                }
            });
            let weak = w.clone();
            bp.connect_unapply(move |_| {
                if let Some(w) = weak.upgrade() {
                    // A narrower step applies after this, and sets its own.
                    w.columns().set_level(0);
                }
            });
            bin.add_breakpoint(bp);
        }
    }

    /// Pulse the busy bar while it shows.
    pub fn pulse(&self) {
        let bar = self.busy_bar.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
            match bar.upgrade() {
                Some(b) if b.is_visible() => {
                    b.pulse();
                    glib::ControlFlow::Continue
                }
                _ => glib::ControlFlow::Break,
            }
        });
    }
}
