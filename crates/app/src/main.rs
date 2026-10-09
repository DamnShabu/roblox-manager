//! roblox-manager: several Roblox accounts, launched into one server.

mod services;
mod state;
mod ui;
mod worker;

use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use rbxmgr_core::accounts::AccountStore;
use rbxmgr_core::macros::{MacroLibrary, cage_window, relay};
use rbxmgr_core::update;

use crate::services::Services;
use crate::state::AppState;
use crate::ui::window::Window;

pub const APP_ID: &str = "io.github.mujo.RobloxManager";

fn main() -> glib::ExitCode {
    let mut args: Vec<String> = std::env::args().collect();
    // Started by a macro-ready launch, as its client's relay or its cage's
    // window relay: no window.
    match args.get(1).map(String::as_str) {
        Some(relay::FLAG) => std::process::exit(relay::run(&args[2..])),
        Some(cage_window::FLAG) => std::process::exit(cage_window::run(&args[2..])),
        _ => {}
    }
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
    // Started by Restart: the old copy may still hold the app's name, and
    // would take this launch over and then exit.
    if let Some(at) = args.iter().position(|a| a == update::RESTARTED) {
        args.remove(at);
        if !update::wait_for_predecessor(Duration::from_secs(20)) {
            eprintln!("roblox-manager: the previous copy was still running; starting anyway");
        }
    }
    app.run_with_args(&args)
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
            cannot_start(app, &e.to_string());
            return None;
        }
    };
    let macros = MacroLibrary::load(&services.paths.macros());
    Some(Window::new(app, AppState::new(accounts, macros), services))
}

/// Say why there is no window, rather than quitting with only a line on a
/// terminal nobody launching from the desktop has open. Nothing is saved:
/// the accounts that could not be read are still on disk as they were.
fn cannot_start(app: &adw::Application, why: &str) {
    eprintln!("roblox-manager: {why}");
    let dialog = adw::AlertDialog::new(
        Some("Roblox Manager Could Not Start"),
        Some(&format!("{why}\n\nNothing was changed or overwritten.")),
    );
    dialog.add_response("close", "_Close");
    // A dialog with no parent is a window the app does not count; held, the
    // app stays up until it is answered.
    let hold = std::cell::RefCell::new(Some(app.hold()));
    dialog.connect_closed(move |_| {
        hold.take();
    });
    dialog.present(None::<&gtk::Widget>);
}

/// The app's stylesheet. The app is dark only, so there is no light sheet to
/// switch between.
fn load_style() {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    // The design's icons; without them the theme's stand in, so a failure
    // is said and the app goes on.
    let icons = rbxmgr_core::Paths::from_env().cache().join("ui-icons");
    if let Err(e) = ui::icons::install(&icons) {
        eprintln!("roblox-manager: could not write the app's icons to {}: {e}", icons.display());
    }
    let Some(display) = gdk::Display::default() else { return };
    let base = gtk::CssProvider::new();
    base.load_from_string(include_str!("../resources/style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &base,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
