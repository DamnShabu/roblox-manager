//! Picking the area a `when` looks for: an overlay opened inside a running
//! client's own nested display (its cage), dragged over what to wait for.
//! Once it is gone, that area of the client's frame is copied out as cage
//! shows it and kept as the image the `when` names. Its coordinates are
//! that display's, the ones a macro's clicks aim at too.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{Align, gdk, glib};

use rbxmgr_core::macros::grammar::when;
use rbxmgr_core::macros::sight::screencopy::Screencopy;
use rbxmgr_core::macros::sight::{Area, Eyes, Image};

use super::{clients, images};
use crate::ui::window::{WeakWindow, Window};
use crate::worker;

/// An area dragged over, None when the pick was called off, or what went
/// wrong.
type Picked = Result<Option<Area>, String>;

/// A rectangle being dragged: its corner and size, in the display's units.
type Rect = (f64, f64, f64, f64);

/// The smallest area worth looking for: anything less is a click, not a
/// drag.
const SMALLEST: f64 = 4.0;
/// How long the overlay is given to leave cage's frame before the area is
/// copied: it is gone from the next frame cage draws.
const OVERLAY_GONE: Duration = Duration::from_millis(250);

const STYLE: &str = "
window.area-picker { background: rgba(0, 0, 0, 0.2); }
.area-picker .hint {
  background: rgba(20, 20, 20, 0.85); color: white;
  border-radius: 999px; padding: 8px 18px; margin-top: 24px;
}";

/// A `when` step's button that picks its image: the image picked is kept,
/// and its name and point go into `value`; what went wrong into `err`.
pub fn button(window: &WeakWindow, value: &gtk::Entry, err: &gtk::Label) -> gtk::Button {
    const TIP: &str = "Pick the area to look for in a running client";
    let button = gtk::Button::builder()
        .icon_name("image-x-generic-symbolic")
        .tooltip_text(TIP)
        .valign(Align::Center)
        .css_classes(["flat", "circular"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(TIP)]);
    let (window, value, err) = (window.clone(), value.downgrade(), err.downgrade());
    button.connect_clicked(move |anchor| {
        let Some(w) = window.upgrade() else { return };
        let dir = w.services().paths.macro_images();
        let (value, err) = (value.clone(), err.clone());
        choose(&w, anchor, dir, move |picked| match picked {
            Ok(Some((name, area))) => {
                if let Some(value) = value.upgrade() {
                    value.set_text(&when::image_at(&value.text(), &name, area.x, area.y));
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

/// Pick an area in one of the running macro-ready clients -- the only one,
/// or the one chosen from a menu under `anchor` -- and keep it in `dir`.
fn choose(
    w: &Window,
    anchor: &gtk::Button,
    dir: PathBuf,
    done: impl FnOnce(Result<Option<(String, Area)>, String>) + 'static,
) {
    const NONE_UP: &str =
        "Launch an account with Macro-ready window on, then pick the area in its window";
    clients::choose(w, anchor, NONE_UP, move |chosen| {
        let client = match chosen {
            Ok(client) => client,
            Err(e) => return done(Err(e)),
        };
        let display = client.display.clone();
        pick(&client.display, move |picked| match picked {
            Ok(Some(area)) => keep(display, area, dir, done),
            Ok(None) => done(Ok(None)),
            Err(e) => done(Err(e)),
        });
    });
}

/// Copy `area` out of the client at `display` once the overlay has left its
/// frame, and keep it in `dir` under a name of its own.
fn keep(
    display: PathBuf,
    area: Area,
    dir: PathBuf,
    done: impl FnOnce(Result<Option<(String, Area)>, String>) + 'static,
) {
    glib::timeout_add_local_once(OVERLAY_GONE, move || {
        worker::run(
            move || -> std::io::Result<Image> { Screencopy::connect(&display)?.look(area) },
            move |copied| {
                let kept = copied
                    .map_err(|e| format!("Could not copy that area of its window: {e}"))
                    .and_then(|image| images::save(&dir, &image));
                done(kept.map(|name| Some((name, area))));
            },
        );
    });
}

/// The overlay, over the client in the display linked at `display`. A
/// drag with the primary button picks; any other button or Escape calls it
/// off.
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
        .label("Drag over what the macro should wait for · Esc to cancel")
        .halign(Align::Center)
        .valign(Align::Start)
        .css_classes(["hint"])
        .build();
    let dragged: Rc<Cell<Option<Rect>>> = Rc::new(Cell::new(None));
    let canvas = gtk::DrawingArea::builder().hexpand(true).vexpand(true).build();
    let shown = Rc::clone(&dragged);
    canvas.set_draw_func(move |_, cr, _, _| {
        let Some((x, y, w, h)) = shown.get() else { return };
        cr.rectangle(x, y, w, h);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.15);
        // A failed draw only leaves the rectangle off this frame.
        let _ = cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.9);
        cr.set_line_width(2.0);
        let _ = cr.stroke();
    });
    let layers = gtk::Overlay::builder().child(&canvas).build();
    layers.add_overlay(&hint);
    hint.set_can_target(false);
    let win = gtk::Window::builder()
        .display(&gdisplay)
        .decorated(false)
        .child(&layers)
        .css_classes(["area-picker"])
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

    let drag = gtk::GestureDrag::builder().button(0).build();
    let f = finish.clone();
    drag.connect_drag_begin(move |g, _, _| {
        if g.current_button() != gdk::BUTTON_PRIMARY {
            f(Ok(None));
        }
    });
    let (rect, area, at) = (Rc::clone(&dragged), canvas.downgrade(), hint.downgrade());
    drag.connect_drag_update(move |g, dx, dy| {
        let Some((x0, y0)) = g.start_point() else { return };
        let rect_now = (x0.min(x0 + dx), y0.min(y0 + dy), dx.abs(), dy.abs());
        rect.set(Some(rect_now));
        if let Some(a) = area.upgrade() {
            a.queue_draw();
        }
        if let Some(l) = at.upgrade() {
            let (x, y, w, h) = rect_now;
            l.set_label(&format!("{x:.0}, {y:.0} · {w:.0} × {h:.0} · let go to pick"));
        }
    });
    let f = finish.clone();
    let rect = Rc::clone(&dragged);
    drag.connect_drag_end(move |_, _, _| {
        match rect.get() {
            Some((x, y, w, h)) if w >= SMALLEST && h >= SMALLEST => f(Ok(Some(Area {
                x: x.round() as i32,
                y: y.round() as i32,
                w: w.round() as u32,
                h: h.round() as u32,
            }))),
            // Too small to be an area: drag again.
            _ => rect.set(None),
        }
    });
    win.add_controller(drag);

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
