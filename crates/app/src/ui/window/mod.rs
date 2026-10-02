//! The Roblox Manager window: one handle every widget keeps, the state it
//! draws from, and the redraws.

mod accounts;
mod actions;
mod chrome;
mod launching;
mod links;
mod macros;
mod roblox;
mod updates;

pub use self::macros::ReadyClient;

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::{gio, glib};
use rbxmgr_core::types::{Profile, UserId};
use rbxmgr_core::window_state::WindowState;

use self::chrome::Chrome;
use super::activity;
use super::widgets::clear;
use crate::services::Services;
use crate::state::AppState;
use crate::worker::{self, Logger};

/// Brings one drawn widget up to date with the state, without rebuilding it.
pub type Redraw = Box<dyn Fn(&AppState)>;

/// A redraw for a widget that may go away on its own (a dialog's): it says
/// whether its widget is still there, and is dropped once it is not.
pub type LiveRedraw = Box<dyn Fn(&AppState) -> bool>;

/// How many activity lines the side pane shows; the log window has the rest.
const ACTIVITY_SHOWN: usize = 6;

/// A cheap handle on the window; widgets keep one to act on it.
#[derive(Clone)]
pub struct Window(Rc<Inner>);

pub struct Inner {
    pub win: adw::ApplicationWindow,
    state: RefCell<AppState>,
    pub services: Services,
    log: Logger,
    ui: Chrome,
    /// Redraw each drawn account's live parts, without rebuilding the rows.
    rows: RefCell<Vec<Redraw>>,
    /// Redraw each macro's running state.
    cards: RefCell<Vec<Redraw>>,
    /// Redraw open dialogs' live parts.
    dialogs: RefCell<Vec<LiveRedraw>>,
    /// The open activity log, which takes each new line as it comes.
    log_view: RefCell<Option<glib::WeakRef<gtk::ListBox>>>,
    /// The open join link popup.
    link_popup: RefCell<Option<glib::WeakRef<adw::Window>>>,
    /// Whether the window has been shown: one opened for a join link alone
    /// stays hidden, and goes once the link needs nothing more of it.
    seen: Cell<bool>,
    polling: Cell<bool>,
}

impl Window {
    pub fn new(app: &adw::Application, state: AppState, services: Services) -> Self {
        let saved = WindowState::load(&services.paths.window_state());
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
                .default_width(saved.width)
                .default_height(saved.height)
                .maximized(saved.maximized)
                .width_request(360)
                .height_request(320)
                .build();
            let ui = Chrome::build(&win, &handle, saved.sidebar);
            Inner {
                win,
                state: RefCell::new(state),
                services,
                log,
                ui,
                rows: RefCell::default(),
                cards: RefCell::default(),
                dialogs: RefCell::default(),
                log_view: RefCell::default(),
                link_popup: RefCell::default(),
                seen: Cell::new(false),
                polling: Cell::new(false),
            }
        });
        let w = Window(inner);
        w.install_actions();
        w.install_style(saved.style);
        // The app owns the window's handle until the window closes: every
        // widget and task holds only a weak one.
        CURRENT.with_borrow_mut(|c| *c = Some(w.clone()));
        let weak = w.weak();
        w.0.win.connect_close_request(move |_| match weak.upgrade() {
            Some(w) => w.on_close_request(),
            None => glib::Propagation::Proceed,
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
        w.check_link_handler();
        w.schedule_update_checks();
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
        self.0.seen.set(true);
        self.0.win.present();
    }

    /// Whether the window has been shown since it was made.
    pub fn seen(&self) -> bool {
        self.0.seen.get()
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
        let s = self.state();
        for entry in s.activity.iter().take(ACTIVITY_SHOWN) {
            log_box.append(&activity::line(entry, false));
        }
        let open = self.0.log_view.borrow().as_ref().and_then(glib::WeakRef::upgrade);
        if let (Some(list), Some(newest)) = (open, s.activity.first()) {
            list.prepend(&activity::row(newest));
        }
    }

    /// Let the open log window take new lines.
    pub fn watch_log(&self, list: &gtk::ListBox) {
        self.0.log_view.replace(Some(list.downgrade()));
    }

    /// Plain text: messages carry labels the user typed and error text,
    /// which a toast would otherwise parse as markup.
    pub fn toast(&self, msg: &str) {
        let toast = adw::Toast::new(msg);
        toast.set_use_markup(false);
        self.0.ui.toasts.add_toast(toast);
    }

    /// A toast with a button that runs `action` on the window.
    pub fn toast_with(&self, msg: &str, button: &str, action: impl Fn(&Window) + 'static) {
        let toast = adw::Toast::new(msg);
        toast.set_use_markup(false);
        toast.set_button_label(Some(button));
        let act = self.act(action);
        toast.connect_button_clicked(move |_| act());
        self.0.ui.toasts.add_toast(toast);
    }

    /// Tell the desktop, for something that took a while and ended while
    /// the window was not the one in use; the toast covers the other case.
    pub fn notify(&self, title: &str, body: &str) {
        if self.0.win.is_active() {
            return;
        }
        if let Some(app) = self.0.win.application() {
            let n = gio::Notification::new(title);
            n.set_body(Some(body));
            app.send_notification(Some("rbxmgr"), &n);
        }
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
                w.leave_if_unseen();
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
        self.0.ui.spinner.set_visible(busy);
    }

    // -- redraws ----------------------------------------------------------
    pub fn refresh(&self) {
        self.show_games();
        self.refresh_accounts();
        self.refresh_macros();
    }

    /// Every drawn status: rows, group headers, macro cards, the title.
    pub fn refresh_states(&self) {
        let s = self.state();
        for redraw in self.0.rows.borrow().iter() {
            redraw(&s);
        }
        for card in self.0.cards.borrow().iter() {
            card(&s);
        }
        // Taken out while they run: a redraw that sets a widget may reach
        // code that watches a new one.
        let mut dialogs = self.0.dialogs.take();
        dialogs.retain(|redraw| redraw(&s));
        let mut added = self.0.dialogs.replace(dialogs);
        self.0.dialogs.borrow_mut().append(&mut added);
        self.0.ui.title.set_subtitle(&s.status_line());
        let live = !s.running.is_empty() || !s.launching.is_empty() || !s.macro_runs.is_empty();
        drop(s);
        self.set_action_enabled("stop-all", live);
    }

    /// Keep `redraw` up to date with the accounts' state until they are next
    /// drawn afresh.
    pub(super) fn watch_accounts(&self, redraw: Redraw) {
        redraw(&self.state());
        self.0.rows.borrow_mut().push(redraw);
    }

    /// Keep `redraw` up to date until the macros are next drawn afresh.
    pub(super) fn watch_macros(&self, redraw: Redraw) {
        redraw(&self.state());
        self.0.cards.borrow_mut().push(redraw);
    }

    /// Keep a dialog's widget up to date while `redraw` says it is there.
    pub fn watch_while(&self, redraw: LiveRedraw) {
        if redraw(&self.state()) {
            self.0.dialogs.borrow_mut().push(redraw);
        }
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

    // -- closing ------------------------------------------------------------
    /// Macros play from this process, so closing stops them: asked first.
    /// Clients are processes of their own and keep running.
    fn on_close_request(&self) -> glib::Propagation {
        let playing = self.state().macro_runs.len();
        if playing == 0 {
            self.close_now();
            return glib::Propagation::Proceed;
        }
        let body = format!(
            "{} playing into {}. Closing Roblox Manager stops {}; the game clients keep running.",
            if playing == 1 { "A macro is" } else { "Macros are" },
            if playing == 1 { "a client" } else { "clients" },
            if playing == 1 { "it" } else { "them" },
        );
        super::confirm::ask(self, "Stop Macros and Close?", &body, "_Close", |w| {
            w.close_now();
            w.0.win.destroy();
        });
        glib::Propagation::Stop
    }

    /// Stop what plays, remember how the window was, and let go of it. A
    /// window never shown has nothing of its size to remember.
    fn close_now(&self) {
        for (stop, _) in self.state().macro_runs.values() {
            stop.set();
        }
        if self.seen() {
            self.remember_size();
        }
        CURRENT.with_borrow_mut(|c| c.take());
    }

    fn remember_size(&self) {
        let (width, height) = self.0.win.default_size();
        let state = WindowState {
            width,
            height,
            maximized: self.0.win.is_maximized(),
            // A narrow window hides the pane by itself: that is no choice.
            sidebar: self.0.ui.split.is_collapsed() || self.0.ui.split.shows_sidebar(),
            style: self.style(),
        };
        if let Err(e) = state.save(&self.services().paths.window_state()) {
            eprintln!("roblox-manager: could not remember the window's size: {e}");
        }
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

pub use actions::set_accels;
