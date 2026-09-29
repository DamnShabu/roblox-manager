//! The Roblox Manager window: one handle every widget keeps, the state it
//! draws from, and the redraws.

mod accounts;
mod chrome;
mod launching;
mod macros;

use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use rbxmgr_core::types::{Profile, UserId};

use self::chrome::Chrome;
use super::activity;
use super::widgets::{Fluent, LabelFluent, clear, dot, icon, lbl};
use crate::services::Services;
use crate::state::AppState;
use crate::worker::{self, Logger};

/// Brings one drawn widget up to date with the state, without rebuilding it.
pub type Redraw = Box<dyn Fn(&AppState)>;

/// A cheap handle on the window; widgets keep one to act on it.
#[derive(Clone)]
pub struct Window(Rc<Inner>);

pub struct Inner {
    pub win: adw::ApplicationWindow,
    state: RefCell<AppState>,
    pub services: Services,
    log: Logger,
    ui: Chrome,
    /// Redraw each drawn account's status, without rebuilding the rows.
    chips: RefCell<Vec<Redraw>>,
    /// Redraw each macro card's running state and meta line.
    cards: RefCell<Vec<Redraw>>,
    polling: std::cell::Cell<bool>,
}

impl Window {
    pub fn new(app: &adw::Application, state: AppState, services: Services) -> Self {
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| {
            let handle = WeakWindow(weak.clone());
            let log = {
                let handle = handle.clone();
                Logger::new(move |line| {
                    if let Some(w) = handle.upgrade() {
                        w.show_log(line);
                    }
                })
            };
            let win = adw::ApplicationWindow::builder()
                .application(app)
                .title("Roblox Manager")
                .default_width(1320)
                .default_height(860)
                .build();
            win.add_css_class("rbx");
            let ui = Chrome::build(&win, &handle);
            Inner {
                win,
                state: RefCell::new(state),
                services,
                log,
                ui,
                chips: RefCell::default(),
                cards: RefCell::default(),
                polling: std::cell::Cell::new(false),
            }
        });
        let w = Window(inner);
        // The app owns the window's handle until the window closes: every
        // widget and task holds only a weak one.
        CURRENT.with_borrow_mut(|c| *c = Some(w.clone()));
        w.0.win.connect_close_request(|_| {
            CURRENT.with_borrow_mut(|c| c.take());
            glib::Propagation::Proceed
        });
        w.refresh();
        w.log("Ready");
        for path in w.state().accounts.set_aside().iter().chain(w.state().macros.set_aside()) {
            w.toast(&format!("A settings file did not read; it was kept as {}", path.display()));
        }
        let handle = w.weak();
        glib::timeout_add_seconds_local(2, move || match handle.upgrade() {
            Some(w) => {
                w.poll_running();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        let profiles = w.0.services.profiles.clone();
        let log = w.0.log.clone();
        w.run_task(
            move || profiles.migrate_flatpak(&|l| log.line(l)).map_err(|e| e.to_string()),
            |w, done: Result<(), String>| {
                if let Err(e) = done {
                    w.log(&format!("Could not move the old Cordial profiles: {e}"));
                }
            },
        );
        w
    }

    pub fn present(&self) {
        self.0.win.present();
    }

    pub fn weak(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.0))
    }

    /// A callback that acts on the window while it lives.
    pub fn act(&self, f: impl Fn(&Window) + 'static) -> impl Fn() + 'static {
        let weak = self.weak();
        move || {
            if let Some(w) = weak.upgrade() {
                f(&w);
            }
        }
    }

    pub fn state(&self) -> Ref<'_, AppState> {
        self.0.state.borrow()
    }

    pub fn state_mut(&self) -> RefMut<'_, AppState> {
        self.0.state.borrow_mut()
    }

    pub fn services(&self) -> &Services {
        &self.0.services
    }

    pub fn gtk_window(&self) -> &adw::ApplicationWindow {
        &self.0.win
    }

    /// A logger for workers.
    pub fn logger(&self) -> Logger {
        self.0.log.clone()
    }

    pub fn log(&self, line: &str) {
        self.0.log.line(line);
    }

    fn show_log(&self, line: String) {
        let stamp = chrono::Local::now().format("%H:%M").to_string();
        self.state_mut().log(stamp, line);
        let log_box = &self.0.ui.log_box;
        clear(log_box);
        for (t, m) in &self.state().activity {
            let kind = activity::kind(m);
            let row = hbox!(
                8,
                "",
                icon(activity::icon(kind), 16, &format!("k-{kind}")),
                lbl(m, "logline").hexpand().ellipsize().chars(1),
                lbl(t, "logtime mono")
            )
            .tip(m);
            log_box.append(&row);
        }
    }

    /// Plain text: messages carry labels the user typed and error text,
    /// which a toast would otherwise parse as markup.
    pub fn toast(&self, msg: &str) {
        let toast = adw::Toast::new(msg);
        toast.set_use_markup(false);
        self.0.ui.toasts.add_toast(toast);
    }

    /// Run `work` on a thread under the busy count; `done` on the main loop.
    pub fn run_task<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&Window, T) + 'static,
    ) {
        self.set_busy(true);
        let weak = self.weak();
        worker::run(work, move |result| {
            if let Some(w) = weak.upgrade() {
                done(&w, result);
                w.set_busy(false);
            }
        });
    }

    /// Counted, not a flag: a launch, a reload and a login can overlap, and
    /// the first to finish must not end the spin under the others.
    fn set_busy(&self, on: bool) {
        let busy = {
            let mut s = self.state_mut();
            s.busy = if on { s.busy + 1 } else { s.busy.saturating_sub(1) };
            s.busy > 0
        };
        if busy {
            self.0.win.add_css_class("busy");
        } else {
            self.0.win.remove_css_class("busy");
        }
    }

    // -- redraws ----------------------------------------------------------
    pub fn refresh(&self) {
        self.show_games();
        self.refresh_accounts();
        self.refresh_macros();
    }

    /// Every drawn status: rows' chips, the pill, macro cards.
    pub fn refresh_states(&self) {
        let s = self.state();
        for chip in self.0.chips.borrow().iter() {
            chip(&s);
        }
        for card in self.0.cards.borrow().iter() {
            card(&s);
        }
        drop(s);
        self.update_pill();
    }

    pub(super) fn add_chip(&self, redraw: Redraw) {
        redraw(&self.state());
        self.0.chips.borrow_mut().push(redraw);
    }

    pub(super) fn add_card(&self, redraw: Redraw) {
        redraw(&self.state());
        self.0.cards.borrow_mut().push(redraw);
    }

    fn update_pill(&self) {
        let (text, n) = {
            let s = self.state();
            (s.pill(), s.running.len())
        };
        let ui = &self.0.ui;
        ui.pill_label.set_label(&text);
        if n > 0 {
            ui.pill.add_css_class("on");
        } else {
            ui.pill.remove_css_class("on");
        }
        clear(&ui.pill_dot);
        ui.pill_dot.append(&dot(if n > 0 { "running" } else { "idlepill" }, n > 0, 7));
    }

    /// Keeps every row's status and play/stop honest. pgrep runs off the
    /// main loop, at most one at a time.
    fn poll_running(&self) {
        if self.0.polling.replace(true) {
            return;
        }
        let ids: Vec<UserId> = self.state().accounts.accounts().iter().map(|a| a.user_id).collect();
        let profiles = self.0.services.profiles.clone();
        let weak = self.weak();
        worker::run(
            move || {
                let live = profiles.running().ok()?;
                Some(
                    ids.into_iter()
                        .filter(|id| live.contains(&Profile::of(*id)))
                        .collect::<HashSet<_>>(),
                )
            },
            move |found| {
                let Some(w) = weak.upgrade() else { return };
                w.0.polling.set(false);
                let Some(found) = found else { return };
                if found != w.state().running {
                    w.state_mut().running = found;
                    w.refresh_states();
                }
            },
        );
    }
}

/// A handle that does not keep the window alive: for callbacks.
#[derive(Clone)]
pub struct WeakWindow(Weak<Inner>);

impl WeakWindow {
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(Window)
    }

    pub fn act(&self, f: impl Fn(&Window) + 'static) -> impl Fn() + 'static {
        let weak = self.clone();
        move || {
            if let Some(w) = weak.upgrade() {
                f(&w);
            }
        }
    }
}

thread_local! {
    /// The one open window: the application's actions reach it here, and
    /// this is what keeps its handle alive.
    static CURRENT: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// The window, if one is open.
pub fn current() -> Option<Window> {
    CURRENT.with_borrow(Clone::clone)
}
