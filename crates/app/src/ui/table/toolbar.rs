//! The toolbar over the table: what the table shows and how many, what the
//! selected accounts can do (their performance, a macro, a group, stop),
//! then Stop all, where launches go, and Launch beside it. In a narrow
//! window what the selection can do folds into one menu.

use adw::prelude::*;
use gtk::{Align, Orientation, gio};
use rbxmgr_core::cordial::Performance;

use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name};

pub struct Toolbar {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub count: gtk::Label,
    pub selected: gtk::Label,
    /// The rule between the view's count and the selection.
    pub sep: gtk::Box,
    /// What the selection can do; greyed out while nothing is selected.
    pub tools: gtk::Box,
    /// The same, as one menu, for a narrow window: hidden until then.
    pub compact: gtk::MenuButton,
    compact_label: gtk::Label,
    /// The macros and the groups both of them offer, kept in step.
    macros: gio::Menu,
    groups: gio::Menu,
    pub stop_all: ds::Button,
    pub target: gtk::MenuButton,
    pub target_art: gtk::Box,
    pub target_name: gtk::Label,
    pub launch: ds::Button,
}

impl Toolbar {
    pub fn build() -> Self {
        let root = gtk::Box::new(Orientation::Horizontal, 2).css("mn-tb");
        name(&root, "The table's view, and what the selected accounts can do");
        let title = lbl("All accounts", "mn-tb-t");
        let count = lbl("", "mn-dim");
        count.set_margin_start(6);
        let sep = gtk::Box::new(Orientation::Horizontal, 0).css("mn-tb-sep");
        sep.set_valign(Align::Center);
        let selected = lbl("", "mn-tb-sel");
        let (perf, macros, groups) = (perf_menu(), gio::Menu::new(), gio::Menu::new());
        let tools = gtk::Box::new(Orientation::Horizontal, 2);
        tools.append(&menu_button(
            "speed",
            "Performance",
            &perf,
            "Set the selected accounts' performance; it applies when each next launches",
        ));
        tools.append(&menu_button(
            "play-circle",
            "Run macro",
            &macros,
            "Run a macro on the selected accounts, or stop it",
        ));
        tools.append(&menu_button(
            "move-to",
            "Move to",
            &groups,
            "Move the selected accounts to a group",
        ));
        let stop = ds::Button::with_icons("Stop", Variant::Ghost, true, Some("stop-filled"), None)
            .tip("Close the selected accounts' clients")
            .action("win.stop-selected");
        let hide = ds::Button::with_icons("Hide", Variant::Ghost, true, Some("eye-off"), None)
            .tip("Hide or show the selected accounts' windows; games keep running")
            .action("win.hide-selected");
        tools.append(&stop.button);
        tools.append(&hide.button);

        let all = gio::Menu::new();
        all.append_submenu(Some("Performance"), &perf);
        all.append_submenu(Some("Run macro"), &macros);
        all.append_submenu(Some("Move to"), &groups);
        let acts = gio::Menu::new();
        acts.append(Some("Stop"), Some("win.stop-selected"));
        acts.append(Some("Hide or show windows"), Some("win.hide-selected"));
        all.append_section(None, &acts);
        let compact_label = lbl("", "");
        let inner = gtk::Box::new(Orientation::Horizontal, 6);
        inner.append(&compact_label);
        inner.append(&ds::icon("chev-down").css("mn-dim s16"));
        let compact = gtk::MenuButton::builder()
            .child(&inner)
            .menu_model(&all)
            .valign(Align::Center)
            .visible(false)
            .tooltip_text("What the selected accounts can do")
            .css_classes(["ds", "ghost", "sm"])
            .build();
        name(&compact, "Selected accounts");

        root.append(&title);
        root.append(&count);
        root.append(&sep);
        root.append(&selected);
        root.append(&tools);
        root.append(&compact);
        root.append(&gtk::Box::new(Orientation::Horizontal, 0).hexpand());
        let stop_all =
            ds::Button::with_icons("Stop all", Variant::Ghost, true, Some("stop-filled"), None)
                .tip("Close every account's client and stop every macro (Ctrl+Shift+.)")
                .action("win.stop-all");
        root.append(&stop_all.button);

        let target_art = gtk::Box::new(Orientation::Horizontal, 0);
        let target_name = lbl("Games browser", "").ellipsize().chars(24);
        let inner = gtk::Box::new(Orientation::Horizontal, 8);
        inner.append(&target_art);
        inner.append(&target_name);
        inner.append(&ds::icon("chev-down").css("mn-dim s16"));
        let target = gtk::MenuButton::builder()
            .child(&inner)
            .tooltip_text("Where launches go (Ctrl+T)")
            .valign(Align::Center)
            .css_classes(["mn-target"])
            .build();
        name(&target, "Launch target");
        root.append(&target);
        let launch =
            ds::Button::with_icons("Launch", Variant::Primary, true, Some("play-filled"), None)
                .tip("Each selected account into the target (Ctrl+Shift+Enter)")
                .action("win.launch-selected");
        root.append(&launch.button);
        for b in [&stop.button, &hide.button, &stop_all.button, &launch.button] {
            b.set_valign(Align::Center);
        }
        Toolbar {
            root,
            title,
            count,
            selected,
            sep,
            tools,
            compact,
            compact_label,
            macros,
            groups,
            stop_all,
            target,
            target_art,
            target_name,
            launch,
        }
    }

    /// How many are selected, as both forms say it.
    pub fn set_selected(&self, n: usize) {
        let text = if n == 0 { "None selected".to_owned() } else { format!("{n} selected") };
        self.selected.set_label(&text);
        self.compact_label.set_label(&text);
        self.tools.set_sensitive(n >= 1);
        self.compact.set_sensitive(n >= 1);
    }

    /// The groups the selection can move to.
    pub fn set_groups(&self, groups: &[(String, String)], n: usize) {
        self.groups.remove_all();
        let places = gio::Menu::new();
        for (gid, name) in groups {
            let item = gio::MenuItem::new(Some(&name.replace('_', "__")), None);
            item.set_action_and_target_value(Some("win.move-selected"), Some(&gid.to_variant()));
            places.append_item(&item);
        }
        self.groups.append_section(Some(&format!("Move {n} to")), &places);
    }

    /// The macros Run macro offers.
    pub fn set_macros(&self, names: &[String]) {
        self.macros.remove_all();
        let list = gio::Menu::new();
        for n in names {
            let item = gio::MenuItem::new(Some(&n.replace('_', "__")), None);
            item.set_action_and_target_value(Some("app.run-macro"), Some(&n.to_variant()));
            list.append_item(&item);
        }
        self.macros.append_section(None, &list);
        let new = gio::Menu::new();
        new.append(Some("New macro…"), Some("win.new-macro"));
        self.macros.append_section(None, &new);
    }
}

/// A quiet button with an icon and words that opens `menu`.
fn menu_button(icon: &str, text: &str, menu: &gio::Menu, tip: &str) -> gtk::MenuButton {
    let inner = gtk::Box::new(Orientation::Horizontal, 6);
    inner.append(&ds::icon(icon));
    inner.append(&lbl(text, ""));
    inner.append(&ds::icon("chev-down").css("mn-dim s16"));
    let b = gtk::MenuButton::builder()
        .child(&inner)
        .menu_model(menu)
        .valign(Align::Center)
        .tooltip_text(tip)
        .css_classes(["ds", "ghost", "sm"])
        .build();
    name(&b, text);
    b
}

/// The four levels, for the selected accounts.
fn perf_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    for level in Performance::ALL {
        let item = gio::MenuItem::new(Some(level.label()), None);
        item.set_action_and_target_value(
            Some("win.perf-selected"),
            Some(&level.label().to_variant()),
        );
        menu.append_item(&item);
    }
    menu
}
