//! The command bar over the table: select all, what the selection can do
//! (launch, stop, hide, move), and on the right Stop all and the launch of
//! the whole order.

use adw::prelude::*;
use gtk::{Align, Orientation, gio};

use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, lbl, name};
use crate::ui::window::WeakWindow;

pub struct CmdBar {
    pub root: gtk::Box,
    pub select_all: gtk::CheckButton,
    /// Set while a redraw brings the box in line, so it is not a click.
    pub quiet: std::rc::Rc<std::cell::Cell<bool>>,
    pub count: gtk::Label,
    pub each: ds::Button,
    pub move_to: gtk::MenuButton,
    /// Stop, Hide and Move to: the first to go in a narrow window.
    pub extras: gtk::Box,
    pub stop_all: ds::Button,
    pub chain: ds::Button,
}

impl CmdBar {
    pub fn build(w: &WeakWindow) -> Self {
        let root = gtk::Box::new(Orientation::Horizontal, 8).css("mn-cmd");
        name(&root, "Act on the selected accounts");
        let quiet = std::rc::Rc::new(std::cell::Cell::new(false));
        let select_all = ds::check(false, "Select every account");
        {
            let (w, quiet) = (w.clone(), quiet.clone());
            select_all.connect_toggled(move |c| {
                if let (false, Some(w)) = (quiet.get(), w.upgrade()) {
                    w.select_every(c.is_active());
                }
            });
        }
        root.append(&select_all);
        let count = lbl("", "mn-cmd-count");
        root.append(&count);
        let each = ds::Button::new("Launch", Variant::Secondary, true)
            .tip("Each selected account into the target (Ctrl+Shift+Enter)")
            .action("win.launch-selected");
        let stop = ds::Button::new("Stop", Variant::Ghost, true)
            .tip("Close the selected accounts' clients")
            .action("win.stop-selected");
        let hide = ds::Button::new("Hide", Variant::Ghost, true)
            .tip("Hide or show the selected accounts' windows; games keep running")
            .action("win.hide-selected");
        let inner = gtk::Box::new(Orientation::Horizontal, 6);
        inner.append(&lbl("Move to", ""));
        inner.append(&ds::icon("chev-down"));
        let move_to = gtk::MenuButton::builder()
            .child(&inner)
            .css_classes(["ds", "ghost", "sm"])
            .tooltip_text("Move the selected accounts to a group")
            .build();
        move_to.set_menu_model(Some(&gio::Menu::new()));
        each.button.set_valign(Align::Center);
        root.append(&each.button);
        let extras = gtk::Box::new(Orientation::Horizontal, 8);
        for b in [&stop.button, &hide.button] {
            b.set_valign(Align::Center);
            extras.append(b);
        }
        move_to.set_valign(Align::Center);
        extras.append(&move_to);
        root.append(&extras);
        root.append(&gtk::Box::new(Orientation::Horizontal, 0).hexpand());
        let stop_all = ds::Button::new("Stop all", Variant::Ghost, true)
            .tip("Close every account's client and stop every macro (Ctrl+Shift+.)")
            .action("win.stop-all");
        let chain = ds::Button::with_icons(
            "Launch as group",
            Variant::Primary,
            true,
            None,
            Some("arrow-right"),
        )
        .tip("The leader first, then the auto-join accounts into its server (Ctrl+Enter)")
        .action("win.launch-group");
        for b in [&stop_all.button, &chain.button] {
            b.set_valign(Align::Center);
            root.append(b);
        }
        CmdBar { root, select_all, quiet, count, each, move_to, extras, stop_all, chain }
    }

    /// The groups the selection can move to.
    pub fn set_groups(&self, groups: &[(String, String)], n: usize) {
        let menu = gio::Menu::new();
        let places = gio::Menu::new();
        for (gid, name) in groups {
            let item = gio::MenuItem::new(Some(&name.replace('_', "__")), None);
            item.set_action_and_target_value(Some("win.move-selected"), Some(&gid.to_variant()));
            places.append_item(&item);
        }
        menu.append_section(Some(&format!("Move {n} to")), &places);
        self.move_to.set_menu_model(Some(&menu));
    }
}
