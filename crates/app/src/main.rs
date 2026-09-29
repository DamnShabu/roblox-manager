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

const APP_ID: &str = "io.github.mujo.RobloxManager";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        // The design is dark only; the stock widgets it leaves alone follow.
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        gtk::Window::set_default_icon_name("roblox-manager");
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("../resources/style.css"));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
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
    });
    app.connect_activate(|app| {
        if let Some(w) = ui::window::current() {
            return w.present();
        }
        let services = Services::new();
        let accounts = match AccountStore::load(&services.paths) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("roblox-manager: {e}");
                return app.quit();
            }
        };
        let macros = MacroLibrary::load(&services.paths.macros());
        let w = Window::new(app, AppState::new(accounts, macros), services);
        w.present();
    });
    app.run()
}
