//! roblox-manager: several Roblox accounts, launched into one server.

mod services;
mod state;
mod ui;
mod worker;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use rbxmgr_core::accounts::AccountStore;
use rbxmgr_core::macros::MacroLibrary;

use crate::services::Services;
use crate::state::AppState;
use crate::ui::window::Window;

pub const APP_ID: &str = "io.github.mujo.RobloxManager";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|app| {
        gtk::Window::set_default_icon_name(APP_ID);
        load_style();
        // A macro's hotkey from anywhere: bind a compositor key to
        //   gapplication action io.github.mujo.RobloxManager run-macro "'NAME'"
        let run_macro = gio::SimpleAction::new("run-macro", Some(glib::VariantTy::STRING));
        run_macro.connect_activate(|_, param| {
            let name = param.and_then(|p| p.get::<String>());
            if let (Some(name), Some(w)) = (name, ui::window::current()) {
                w.run_macro_card(&name);
            }
        });
        app.add_action(&run_macro);
        let about = gio::SimpleAction::new("about", None);
        let weak = app.downgrade();
        about.connect_activate(move |_, _| {
            let parent = weak.upgrade().and_then(|a| a.active_window());
            ui::about::show(parent.as_ref());
        });
        app.add_action(&about);
        let quit = gio::SimpleAction::new("quit", None);
        let weak = app.downgrade();
        quit.connect_activate(move |_, _| {
            // Through the window, so playing macros are asked about first.
            match (ui::window::current(), weak.upgrade()) {
                (Some(w), _) => w.gtk_window().close(),
                (None, Some(app)) => app.quit(),
                (None, None) => {}
            }
        });
        app.add_action(&quit);
        ui::window::set_accels(app);
    });
    app.connect_activate(|app| {
        if let Some(w) = window(app) {
            w.present();
        }
    });
    // A join link from the browser (the desktop entry handles roblox-player:
    // and roblox:): its popup alone, not the whole window.
    app.connect_open(|app, links, _| {
        let Some(w) = window(app) else { return };
        match links.last() {
            Some(link) => w.open_link(&link.uri()),
            None => w.present(),
        }
    });
    app.run()
}

/// The window, made (and not yet shown) if there is none.
fn window(app: &adw::Application) -> Option<Window> {
    if let Some(w) = ui::window::current() {
        return Some(w);
    }
    let services = Services::new();
    let accounts = match AccountStore::load(&services.paths) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("roblox-manager: {e}");
            app.quit();
            return None;
        }
    };
    let macros = MacroLibrary::load(&services.paths.macros());
    Some(Window::new(app, AppState::new(accounts, macros), services))
}

/// The app's stylesheet, and its dark surfaces while the style is dark --
/// the split libadwaita itself makes between style.css and style-dark.css.
fn load_style() {
    let Some(display) = gdk::Display::default() else { return };
    let base = gtk::CssProvider::new();
    base.load_from_string(include_str!("../resources/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &base,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let dark = gtk::CssProvider::new();
    dark.load_from_string(include_str!("../resources/style-dark.css"));
    let follow = move |style: &adw::StyleManager| {
        if style.is_dark() {
            gtk::style_context_add_provider_for_display(
                &display,
                &dark,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        } else {
            gtk::style_context_remove_provider_for_display(&display, &dark);
        }
    };
    let style = adw::StyleManager::default();
    follow(&style);
    style.connect_dark_notify(follow);
}
