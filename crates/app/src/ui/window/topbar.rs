//! The top bar: the app's mark and how it is doing, where launches go (the
//! target and the recent ones), the search, and the tools at the end. It is
//! the window's title bar too: drag it to move the window.

use adw::prelude::*;
use gtk::{Align, Orientation, gio};

use super::WeakWindow;
use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name};

pub struct TopBar {
    pub root: gtk::WindowHandle,
    pub status_dot: gtk::Box,
    pub status: gtk::Label,
    pub target: gtk::MenuButton,
    pub target_art: gtk::Box,
    pub target_name: gtk::Label,
    pub recents: gtk::Box,
    pub search: gtk::SearchEntry,
    pub update: ds::Button,
    pub insp_toggle: gtk::ToggleButton,
    pub brand_text: gtk::Box,
    pub vsep: gtk::Box,
}

impl TopBar {
    pub fn build(w: &WeakWindow, inspector_shown: bool) -> Self {
        let bar = gtk::Box::new(Orientation::Horizontal, 12);
        bar.add_css_class("mn-top");
        bar.append(&gtk::WindowControls::new(gtk::PackType::Start));

        // -- the mark, the name and the status line --------------------------
        let logo = gtk::Image::from_icon_name("rm-brand");
        logo.set_pixel_size(28);
        let status_dot = gtk::Box::new(Orientation::Horizontal, 0).css("cx-dot");
        status_dot.set_valign(Align::Center);
        let status = lbl("All idle", "").ellipsize();
        let brand_text = gtk::Box::new(Orientation::Vertical, 0);
        brand_text.set_valign(Align::Center);
        brand_text.append(&lbl("Roblox Manager", "mn-brand-t"));
        let line = gtk::Box::new(Orientation::Horizontal, 4).css("mn-brand-s");
        line.append(&status_dot);
        line.append(&status);
        brand_text.append(&line);
        let brand = gtk::Box::new(Orientation::Horizontal, 8).css("mn-brand");
        brand.append(&logo);
        brand.append(&brand_text);
        bar.append(&brand);
        let vsep = gtk::Box::new(Orientation::Horizontal, 0).css("mn-vsep");
        vsep.set_valign(Align::Center);
        bar.append(&vsep);

        // -- where launches go -------------------------------------------------
        let target_art = gtk::Box::new(Orientation::Horizontal, 0);
        let target_name = lbl("Games browser", "mn-target-n").ellipsize().chars(22);
        let text = gtk::Box::new(Orientation::Vertical, 0);
        text.set_valign(Align::Center);
        text.append(&lbl("LAUNCH INTO", "mn-target-k"));
        text.append(&target_name);
        let inner = gtk::Box::new(Orientation::Horizontal, 8);
        inner.append(&target_art);
        inner.append(&text.hexpand());
        inner.append(&ds::icon("chev-down").css("chev s16"));
        let target = gtk::MenuButton::builder()
            .child(&inner)
            .tooltip_text("Where launches go (Ctrl+T)")
            .valign(Align::Center)
            .css_classes(["mn-target"])
            .build();
        name(&target, "Launch target");
        bar.append(&target);
        let recents = gtk::Box::new(Orientation::Horizontal, 4);
        recents.set_valign(Align::Center);
        name(&recents, "Recent targets");
        bar.append(&recents);

        // -- search --------------------------------------------------------------
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search name, @user or note")
            .valign(Align::Center)
            .halign(Align::End)
            .hexpand(true)
            .width_request(280)
            .css_classes(["search-field"])
            .build();
        name(&search, "Search accounts (Ctrl+F)");
        {
            let w = w.clone();
            search.connect_search_changed(move |e| {
                if let Some(w) = w.upgrade() {
                    w.set_filter(&e.text());
                }
            });
        }
        bar.append(&search);

        // -- tools ----------------------------------------------------------------
        let tools = gtk::Box::new(Orientation::Horizontal, 4).css("mn-tools");
        tools.set_valign(Align::Center);
        let update = ds::Button::with_icons("Update", Variant::Ghost, true, Some("refresh"), None)
            .action("win.update");
        update.button.set_valign(Align::Center);
        update.button.set_visible(false);
        tools.append(&update.button);
        let add = ds::ib("plus", "Add account (Ctrl+N)", false);
        add.set_action_name(Some("win.add-account"));
        tools.append(&add);
        let refresh = ds::ib("refresh", "Check sessions and reload favourites (Ctrl+R)", false);
        refresh.set_action_name(Some("win.refresh"));
        tools.append(&refresh);
        let insp_toggle = gtk::ToggleButton::builder()
            .child(&ds::icon("panel"))
            .active(inspector_shown)
            .tooltip_text("Show or hide the inspector (F9)")
            .valign(Align::Center)
            .css_classes(["ib"])
            .build();
        name(&insp_toggle, "Show or hide the inspector");
        tools.append(&insp_toggle);
        let burger = gtk::MenuButton::builder()
            .child(&ds::icon("menu"))
            .menu_model(&main_menu())
            .primary(true)
            .tooltip_text("Main menu (F10)")
            .valign(Align::Center)
            .css_classes(["burger"])
            .build();
        name(&burger, "Main menu");
        tools.append(&burger);
        bar.append(&tools);
        bar.append(&gtk::WindowControls::new(gtk::PackType::End));

        let root = gtk::WindowHandle::new();
        root.set_child(Some(&bar));
        TopBar {
            root,
            status_dot,
            status,
            target,
            target_art,
            target_name,
            recents,
            search,
            update,
            insp_toggle,
            brand_text,
            vsep,
        }
    }
}

fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let add = gio::Menu::new();
    add.append(Some("_Add account…"), Some("win.add-account"));
    add.append(Some("New _group"), Some("win.new-group"));
    add.append(Some("New _macro…"), Some("win.new-macro"));
    menu.append_section(None, &add);
    let roblox = gio::Menu::new();
    roblox.append(Some("_Refresh sessions and favourites"), Some("win.refresh"));
    roblox.append(Some("_Update all"), Some("win.update"));
    roblox.append(Some("Open Roblox _links here"), Some("win.open-links-here"));
    menu.append_section(None, &roblox);
    let windows = gio::Menu::new();
    windows.append(Some("_Hide all windows"), Some("win.hide-all"));
    windows.append(Some("_Show all windows"), Some("win.show-all"));
    menu.append_section(None, &windows);
    let help = gio::Menu::new();
    help.append(Some("Activity _log"), Some("win.activity-log"));
    help.append(Some("How _macros work"), Some("win.macro-help"));
    help.append(Some("_Keyboard shortcuts"), Some("win.shortcuts"));
    help.append(Some("_About Roblox Manager"), Some("app.about"));
    menu.append_section(None, &help);
    menu
}
