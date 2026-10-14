//! The header bar: the app's mark, its name and how things are, the search,
//! and the tools at the end. It is the window's title bar too: drag it to
//! move the window.

use adw::prelude::*;
use gtk::{Align, Orientation, gio};

use super::WeakWindow;
use crate::ui::ds::{self, Variant};
use crate::ui::widgets::{Fluent, LabelFluent, lbl, name};

pub struct TopBar {
    pub root: gtk::WindowHandle,
    pub status: gtk::Label,
    pub search: gtk::SearchEntry,
    pub update: ds::Button,
}

impl TopBar {
    pub fn build(w: &WeakWindow) -> Self {
        let bar = gtk::Box::new(Orientation::Horizontal, 12).css("mn-top");
        bar.append(&gtk::WindowControls::new(gtk::PackType::Start));
        let logo = gtk::Image::from_icon_name("rm-brand");
        logo.set_pixel_size(24);
        bar.append(&logo);
        bar.append(&lbl("Stacked", "mn-brand"));
        let status = lbl("All idle", "mn-dim").ellipsize();
        bar.append(&status);

        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search accounts")
            .valign(Align::Center)
            .halign(Align::Center)
            .hexpand(true)
            .width_request(340)
            .css_classes(["search-field"])
            .build();
        name(&search, "Search accounts by name, Roblox user or note (Ctrl+F)");
        {
            let w = w.clone();
            search.connect_search_changed(move |e| {
                if let Some(w) = w.upgrade() {
                    w.set_filter(&e.text());
                }
            });
        }
        bar.append(&search);

        let tools = gtk::Box::new(Orientation::Horizontal, 2);
        tools.set_valign(Align::Center);
        let update = ds::Button::with_icons("Update", Variant::Ghost, true, Some("refresh"), None)
            .action("win.update");
        update.button.set_valign(Align::Center);
        update.button.set_visible(false);
        tools.append(&update.button);
        for (icon, tip, action) in [
            ("plus", "Add account (Ctrl+N)", "win.add-account"),
            ("refresh", "Check sessions and reload favourites (Ctrl+R)", "win.refresh"),
        ] {
            let b = ds::ib(icon, tip, false);
            b.set_action_name(Some(action));
            tools.append(&b);
        }
        let burger = gtk::MenuButton::builder()
            .child(&ds::icon("menu"))
            .menu_model(&main_menu())
            .primary(true)
            .tooltip_text("Main menu (F10)")
            .valign(Align::Center)
            .css_classes(["ib"])
            .build();
        name(&burger, "Main menu");
        tools.append(&burger);
        bar.append(&tools);
        bar.append(&gtk::WindowControls::new(gtk::PackType::End));

        let root = gtk::WindowHandle::new();
        root.set_child(Some(&bar));
        TopBar { root, status, search, update }
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
    windows.append(Some("Show or hide the _sidebar"), Some("win.toggle-sidebar"));
    menu.append_section(None, &windows);
    let help = gio::Menu::new();
    help.append(Some("Activity _log"), Some("win.activity-log"));
    help.append(Some("How _macros work"), Some("win.macro-help"));
    help.append(Some("_Keyboard shortcuts"), Some("win.shortcuts"));
    help.append(Some("_About Stacked"), Some("app.about"));
    menu.append_section(None, &help);
    menu
}
