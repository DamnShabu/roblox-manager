//! The parts of the window that are built once, put together: the top bar,
//! what needs you, the command bar over the accounts table, the inspector
//! beside it, the status strip along the bottom and the activity drawer
//! that rises over it.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, glib};

use super::WeakWindow;
use super::inspector::{Inspector, NARROW};
use super::overview::Alert;
use super::strip::Strip;
use super::topbar::TopBar;
use crate::ui::activity::LogDrawer;
use crate::ui::panel::Panel;
use crate::ui::table::cmd::CmdBar;
use crate::ui::table::{Columns, header};
use crate::ui::target::TargetPopover;
use crate::ui::widgets::{Fluent, LabelFluent, lbl};

pub struct Chrome {
    pub top: TopBar,
    pub target_pop: Rc<TargetPopover>,
    pub busy_bar: gtk::ProgressBar,
    pub banner: adw::Banner,
    pub alert: Alert,
    pub cmd: CmdBar,
    /// "welcome" with no accounts, else "accounts".
    pub pages: gtk::Stack,
    /// The launch order and the groups' bands and rows.
    pub table: gtk::Box,
    pub cols: Columns,
    pub insp: Inspector,
    pub split: adw::OverlaySplitView,
    pub strip: Strip,
    pub log: LogDrawer,
    pub toasts: adw::ToastOverlay,
    pub shortcuts: gtk::ShortcutController,
    pub panel: RefCell<Option<Panel>>,
}

impl Chrome {
    pub fn build(win: &adw::ApplicationWindow, w: &WeakWindow, sidebar: bool) -> Self {
        let top = TopBar::build(w, sidebar);
        let target_pop = TargetPopover::build(w);
        top.target.set_popover(Some(&target_pop.popover));
        let busy_bar = gtk::ProgressBar::builder().css_classes(["loadbar"]).visible(false).build();
        busy_bar.set_valign(Align::Start);
        busy_bar.set_pulse_step(0.08);
        let banner =
            adw::Banner::new("Installing the newest Roblox build — this can take a few minutes");
        let alert = Alert::build(w);

        // -- the accounts: command bar, header, table ----------------------------
        let cmd = CmdBar::build(w);
        let cols = Columns::default();
        let table = gtk::Box::new(Orientation::Vertical, 0);
        let add_line = gtk::Box::new(Orientation::Horizontal, 8);
        add_line.set_margin_start(12);
        add_line.set_margin_top(8);
        add_line.set_margin_bottom(8);
        for (text, action, tip) in [
            ("Add account", "win.add-account", "Sign in another account with Quick Login (Ctrl+N)"),
            ("New group", "win.new-group", "A named set of accounts with a game of its own"),
        ] {
            let b = crate::ui::ds::Button::with_icons(
                text,
                crate::ui::ds::Variant::Ghost,
                true,
                Some("plus"),
                None,
            )
            .tip(tip)
            .action(action);
            add_line.append(&b.button);
        }
        let rows = gtk::Box::new(Orientation::Vertical, 0);
        rows.append(&table);
        rows.append(&add_line);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&rows)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .css_classes(["mn-table"])
            .build();
        let columns = gtk::Box::new(Orientation::Vertical, 0);
        columns.append(&header(&cols));
        columns.append(&scroller);
        // Narrower than its columns (beside a wide inspector), the table
        // scrolls sideways rather than widening the window.
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
        let center = gtk::Box::new(Orientation::Vertical, 0);
        center.append(&cmd.root);
        center.append(&pages);

        // -- the inspector beside it ---------------------------------------------------
        let insp = Inspector::build(w);
        let split = adw::OverlaySplitView::builder()
            .content(&center)
            .sidebar(&insp.root)
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(NARROW)
            .max_sidebar_width(NARROW)
            .sidebar_width_fraction(0.5)
            .show_sidebar(sidebar)
            .build();
        top.insp_toggle.bind_property("active", &split, "show-sidebar").bidirectional().build();

        // -- the drawer over the bottom, the strip under it ------------------------------
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
        root.append(&alert.root);
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
            alert,
            cmd,
            pages,
            table,
            cols,
            insp,
            split,
            strip,
            log,
            toasts,
            shortcuts,
            panel: RefCell::default(),
        };
        chrome.breakpoints(win, w);
        chrome
    }

    /// Narrower windows: fewer columns, then the inspector over the table
    /// rather than beside it, then the top bar's extras go.
    fn breakpoints(&self, win: &adw::ApplicationWindow, w: &WeakWindow) {
        let steps: [(&str, u8, bool, bool); 4] = [
            ("max-width: 1280sp", 1, false, false),
            ("max-width: 1100sp", 1, true, false),
            ("max-width: 860sp", 2, true, false),
            ("max-width: 720sp", 2, true, true),
        ];
        for (cond, level, overlay, phone) in steps {
            let Ok(cond) = adw::BreakpointCondition::parse(cond) else { continue };
            let bp = adw::Breakpoint::new(cond);
            bp.add_setter(&self.top.recents, "visible", Some(&false.to_value()));
            if overlay {
                bp.add_setter(&self.split, "collapsed", Some(&true.to_value()));
                bp.add_setter(&self.top.brand_text, "visible", Some(&false.to_value()));
                bp.add_setter(&self.top.search, "width-request", Some(&160.to_value()));
            }
            if level >= 2 {
                bp.add_setter(&self.cmd.extras, "visible", Some(&false.to_value()));
                bp.add_setter(&self.cmd.count, "visible", Some(&false.to_value()));
            }
            if phone {
                bp.add_setter(&self.top.vsep, "visible", Some(&false.to_value()));
                bp.add_setter(&self.top.search, "visible", Some(&false.to_value()));
                bp.add_setter(&self.strip.links, "visible", Some(&false.to_value()));
            }
            let weak = w.clone();
            bp.connect_apply(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.columns().set_level(level);
                }
            });
            let weak = w.clone();
            bp.connect_unapply(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.columns().set_level(0);
                }
            });
            win.add_breakpoint(bp);
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
