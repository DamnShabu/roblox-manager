//! Picking a click's point in a running client: an overlay opened inside the
//! client's own nested display (its cage), clicked where the macro should
//! click. Its coordinates are that display's, the ones the player aims at, so
//! the point holds wherever the window sits on your desktop -- as long as it
//! keeps its size.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, gdk, glib};

use rbxmgr_core::macros::grammar;

use super::clients;
use crate::ui::window::{WeakWindow, Window};

/// A point picked, None when the pick was called off, or what went wrong.
type Picked = Result<Option<(i32, i32)>, String>;

const STYLE: &str = "
window.point-picker { background: rgba(0, 0, 0, 0.2); }
.point-picker .hint {
  background: rgba(20, 20, 20, 0.85); color: white;
  border-radius: 999px; padding: 8px 18px; margin-top: 24px;
}";

/// A click step's button that picks its point: the picked point goes into
/// `value` (whose own handler files it in its row), what went wrong into `err`.
pub fn button(window: &WeakWindow, value: &gtk::Entry, err: &gtk::Label) -> gtk::Button {
    const TIP: &str = "Pick the point in a running client";
    // Not a Btn: the button is the anchor of its own menu.
    let button = gtk::Button::builder()
        .icon_name("find-location-symbolic")
        .tooltip_text(TIP)
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(TIP)]);
    let (window, value, err) = (window.clone(), value.downgrade(), err.downgrade());
    button.connect_clicked(move |anchor| {
        let Some(w) = window.upgrade() else { return };
        let (value, err) = (value.clone(), err.clone());
        choose(&w, anchor, move |picked| match picked {
            Ok(Some((x, y))) => {
                if let Some(value) = value.upgrade() {
                    value.set_text(&grammar::click_at(&value.text(), x, y));
                }
            }
            Ok(None) => {}
            Err(e) => {
                if let Some(err) = err.upgrade() {
                    err.set_label(&e);
                    err.set_visible(true);
                }
            }
        });
    });
    button
}

/// Pick a point in one of the running macro-ready clients: the only one, or
/// the one chosen from a menu under `anchor`.
fn choose(w: &Window, anchor: &gtk::Button, done: impl FnOnce(Picked) + 'static) {
    const NONE_UP: &str =
        "Launch an account with Macro-ready window on, then pick the point in its window";
    clients::choose(w, anchor, NONE_UP, move |chosen| match chosen {
        Ok(client) => pick(&client.display, done),
        Err(e) => done(Err(e)),
    });
}

/// The overlay, over the client in the display linked at `display`. A
/// primary click picks; any other button or Escape calls it off.
fn pick(display: &Path, done: impl FnOnce(Picked) + 'static) {
    let Some(gdisplay) = display.to_str().and_then(|p| gdk::Display::open(Some(p))) else {
        return done(Err("Could not reach its window -- is the client still up?".into()));
    };
    let style = gtk::CssProvider::new();
    style.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &gdisplay,
        &style,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let hint = gtk::Label::builder()
        .label("Click where the macro should click · Esc to cancel")
        .halign(Align::Center)
        .valign(Align::Start)
        .css_classes(["hint"])
        .build();
    let win = gtk::Window::builder()
        .display(&gdisplay)
        .decorated(false)
        .child(&hint)
        .css_classes(["point-picker"])
        .build();
    win.set_cursor_from_name(Some("crosshair"));

    let finish = {
        let (win, done) = (win.downgrade(), once(done));
        Rc::new(move |picked: Picked| {
            let Some(f) = done.borrow_mut().take() else { return };
            // Torn down after the event that ended it; the display once the
            // window it held is gone.
            let (win, gdisplay) = (win.clone(), gdisplay.clone());
            glib::idle_add_local_once(move || {
                if let Some(win) = win.upgrade() {
                    win.destroy();
                }
                gdisplay.close();
            });
            f(picked);
        })
    };

    let click = gtk::GestureClick::builder().button(0).build();
    let f = finish.clone();
    click.connect_pressed(move |g, _, x, y| {
        // A fullscreen, undecorated window: its coordinates are the display's.
        f(Ok((g.current_button() == gdk::BUTTON_PRIMARY)
            .then(|| (x.round() as i32, y.round() as i32))));
    });
    win.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    let at = hint.downgrade();
    motion.connect_motion(move |_, x, y| {
        if let Some(l) = at.upgrade() {
            l.set_label(&format!("{:.0}, {:.0} · click to pick · Esc to cancel", x, y));
        }
    });
    win.add_controller(motion);

    let keys = gtk::EventControllerKey::new();
    let f = finish.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            f(Ok(None));
        }
        glib::Propagation::Stop
    });
    win.add_controller(keys);

    let f = finish;
    win.connect_close_request(move |_| {
        f(Ok(None));
        glib::Propagation::Proceed
    });
    win.fullscreen();
    win.present();
}

/// A callback that runs at most once, shared between the signals racing
/// to run it.
fn once<F: FnOnce(Picked)>(f: F) -> Rc<RefCell<Option<F>>> {
    Rc::new(RefCell::new(Some(f)))
}
