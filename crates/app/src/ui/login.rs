//! Add account / Sign in again: Roblox Quick Login as three steps -- open the
//! page, enter the code, approve. The code is asked for as soon as the dialog
//! opens and renewed when it expires. The app never opens a browser on its
//! own; its button goes to the bare page, with no code in the address.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{Align, gio, glib};
use rbxmgr_core::accounts::Account;
use rbxmgr_core::roblox::quick_login::{CONFIRM_URL, TIMEOUT};
use rbxmgr_core::roblox::{QuickLoginError, QuickLoginEvents, QuickLoginStatus, quick_login};
use rbxmgr_core::types::{Cookie, User};

use super::widgets::{Btn, Fluent, IconButton, LabelFluent, lbl, toggle_class};
use super::window::{WeakWindow, Window};
use crate::worker::Logger;

/// What the login worker tells the dialog, for the code it was started for.
enum Event {
    Code(u64, String),
    Status(u64, QuickLoginStatus),
}

pub struct AddAccountDialog {
    window: WeakWindow,
    dialog: adw::Dialog,
    /// The account being signed in again, if any.
    relogin: Option<Account>,
    /// Which code is current; older workers stop at their next poll.
    generation: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    code: RefCell<String>,
    expires: Cell<Option<Instant>>,
    events: async_channel::Sender<Event>,
    step1: gtk::Label,
    step2: gtk::Label,
    code_a: gtk::Label,
    code_b: gtk::Label,
    copy: IconButton,
    expiry: gtk::Label,
    bar: gtk::ProgressBar,
    pulse: adw::Spinner,
    status: gtk::Label,
    status_box: gtk::Box,
}

impl AddAccountDialog {
    pub fn open(w: &Window, relogin: Option<Account>) {
        let dialog = adw::Dialog::builder()
            .title(if relogin.is_some() { "Sign In Again" } else { "Add Account" })
            .content_width(500)
            .build();
        let (tx, rx) = async_channel::unbounded();
        let d = Rc::new_cyclic(|me: &std::rc::Weak<AddAccountDialog>| {
            let me = me.clone();
            let copy = Btn::new("").text("Copy").icon("edit-copy-symbolic").build(move || {
                if let Some(d) = me.upgrade() {
                    d.copy_code();
                }
            });
            copy.button.set_valign(Align::Center);
            AddAccountDialog {
                window: w.weak(),
                dialog,
                relogin,
                generation: Arc::new(AtomicU64::new(0)),
                closed: Arc::new(AtomicBool::new(false)),
                code: RefCell::default(),
                expires: Cell::new(None),
                events: tx,
                step1: circle("1"),
                step2: circle("2"),
                code_a: lbl("···", "login-code").selectable(),
                code_b: lbl("···", "login-code").selectable(),
                copy,
                expiry: lbl("Expires in –:––", "caption dimmed numeric").hexpand(),
                bar: gtk::ProgressBar::builder().css_classes(["expiry"]).fraction(1.0).build(),
                pulse: adw::Spinner::new(),
                status: lbl("Requesting a code…", "").wrapped().hexpand(),
                status_box: hbox!(10, "login-status"),
            }
        });
        d.assemble();
        let me = Rc::downgrade(&d);
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                match me.upgrade() {
                    Some(d) => d.on_event(ev),
                    None => break,
                }
            }
        });
        let me = Rc::downgrade(&d);
        glib::timeout_add_seconds_local(1, move || match me.upgrade() {
            Some(d) if !d.closed.load(Ordering::Relaxed) => {
                d.tick();
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });
        let closed = d.closed.clone();
        let held = RefCell::new(Some(d.clone()));
        d.dialog.connect_closed(move |_| {
            closed.store(true, Ordering::Relaxed);
            // The dialog's widgets hold only weak handles on its state.
            held.take();
        });
        d.request_code();
        d.dialog.present(Some(w.gtk_window()));
    }

    fn assemble(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let open = Btn::new("suggested-action")
            .text("Open Quick Login")
            .icon("adw-external-link-symbolic")
            .tip("Opens roblox.com/crossdevicelogin with no code in the address: you type the code")
            .build(move || {
                if let Some(d) = me.upgrade() {
                    d.open_page();
                }
            });
        let weak = self.window.clone();
        let link =
            Btn::new("flat").text("Copy Link").icon("edit-copy-symbolic").tip(CONFIRM_URL).build(
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.gtk_window().clipboard().set_text(CONFIRM_URL);
                        w.toast("Link copied");
                    }
                },
            );
        let links = hbox!(8, "", open.button, link.button);
        let step1 = step(
            &self.step1,
            "Open Quick Login in a browser",
            Some("On any device where you are signed in to Roblox."),
            &links,
        );

        let me = Rc::downgrade(self);
        let new_code =
            Btn::new("flat").text("New Code").icon("view-refresh-symbolic").build(move || {
                if let Some(d) = me.upgrade() {
                    d.request_code();
                }
            });
        let code = hbox!(16, "", self.code_a.clone(), self.code_b.clone()).hexpand().centered();
        let top = hbox!(12, "", code, self.copy.button.clone());
        let bottom = vbox!(
            6,
            "",
            hbox!(8, "", self.expiry.clone(), new_code.button.centered()),
            self.bar.clone()
        );
        top.set_margin_top(14);
        for part in [&top, &bottom] {
            part.set_margin_start(18);
            part.set_margin_end(14);
        }
        bottom.set_margin_bottom(14);
        let card = vbox!(10, "card", top, bottom);
        let step2 = step(&self.step2, "Enter this code there", None, &card);

        let c3 = circle("3");
        self.status_box.append(&self.pulse);
        self.status_box.append(&self.status);
        let step3 = step(
            &c3,
            "Approve the sign-in",
            Some("The account shows up here by itself once you approve."),
            &self.status_box,
        );

        let intro = match &self.relogin {
            Some(a) => format!(
                "Approve with {}'s own Roblox user{}. Its new session replaces the old one in your keyring.",
                a.name,
                a.username.as_deref().map(|u| format!(" (@{u})")).unwrap_or_default()
            ),
            None => "Quick Login signs this app in with a short code. No password is typed \
                     here, and the session is kept in your keyring."
                .to_owned(),
        };
        let body = vbox!(22, "", lbl(&intro, "dimmed").wrapped(), step1, step2, step3).margins(24);
        body.set_margin_top(6);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&body));
        self.dialog.set_child(Some(&view));
    }

    fn open_page(&self) {
        // The bare confirmation page: no code in the query, nothing prefilled.
        if let Err(e) =
            gio::AppInfo::launch_default_for_uri(CONFIRM_URL, None::<&gio::AppLaunchContext>)
        {
            self.set_status(&format!("Could not open the browser: {e}"), true);
            return;
        }
        done(&self.step1);
    }

    fn copy_code(self: &Rc<Self>) {
        let code = self.code.borrow().clone();
        if code.is_empty() {
            return;
        }
        if let Some(w) = self.window.upgrade() {
            w.gtk_window().clipboard().set_text(&code);
        }
        done(&self.step2);
        self.copy.set_icon("object-select-symbolic");
        self.copy.set_text("Copied");
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(1600), move || {
            if let Some(d) = me.upgrade() {
                d.copy.set_icon("edit-copy-symbolic");
                d.copy.set_text("Copy");
            }
        });
    }

    fn tick(&self) {
        let Some(expires) = self.expires.get() else { return };
        let left = expires.saturating_duration_since(Instant::now()).as_secs();
        self.expiry.set_label(&format!("Expires in {}:{:02}", left / 60, left % 60));
        self.bar.set_fraction(left as f64 / TIMEOUT.as_secs_f64());
        toggle_class(&self.bar, "low", left < 60);
    }

    fn set_status(&self, text: &str, error: bool) {
        self.status.set_label(text);
        self.pulse.set_visible(!error);
        toggle_class(&self.status_box, "error", error);
    }

    fn on_event(&self, ev: Event) {
        let current = self.generation.load(Ordering::Relaxed);
        match ev {
            Event::Code(code_gen, code) if code_gen == current => {
                let half = code.len().div_ceil(2);
                self.code_a.set_label(code.get(..half).unwrap_or(&code));
                self.code_b.set_label(code.get(half..).unwrap_or(""));
                self.code.replace(code);
                self.expires.set(Some(Instant::now() + TIMEOUT));
                self.tick();
                self.set_status("Waiting for approval…", false);
            }
            Event::Status(code_gen, QuickLoginStatus::UserLinked) if code_gen == current => {
                self.set_status("Code entered -- now approve the sign-in at Roblox", false);
            }
            _ => {}
        }
    }

    /// Ask Roblox for a code -- on opening, on "New code", and when the last
    /// one expired. A worker still polling an older code sees it is stale at
    /// its next poll and ends without a word.
    fn request_code(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let code_gen = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.code.replace(String::new());
        self.expires.set(None);
        self.code_a.set_label("···");
        self.code_b.set_label("···");
        self.expiry.set_label("Expires in –:––");
        self.bar.set_fraction(1.0);
        self.set_status("Requesting a code…", false);
        let roblox = w.services().roblox.clone();
        let (generation, closed) = (self.generation.clone(), self.closed.clone());
        let mut relay = Relay { code_gen, events: self.events.clone(), log: w.logger() };
        let me = Rc::downgrade(self);
        w.run_task(
            move || {
                let stale = || {
                    closed.load(Ordering::Relaxed) || generation.load(Ordering::Relaxed) != code_gen
                };
                quick_login(&*roblox, &mut relay, &stale, &std::thread::sleep)
            },
            move |_, result| {
                if let Some(d) = me.upgrade() {
                    d.finished(code_gen, result);
                }
            },
        );
    }

    fn finished(self: &Rc<Self>, code_gen: u64, result: Result<(Cookie, User), QuickLoginError>) {
        let stale = self.closed.load(Ordering::Relaxed)
            || self.generation.load(Ordering::Relaxed) != code_gen;
        match result {
            Err(QuickLoginError::CodeExpired) if !stale => self.request_code(),
            Err(_) if stale => {}
            Err(e) => self.failed(&e.to_string()),
            Ok((cookie, user)) => self.store(cookie, user),
        }
    }

    /// Keep the dialog open and say what went wrong on it: a failure that
    /// closed it read as "it worked but no account appeared".
    fn failed(&self, why: &str) {
        let name =
            self.relogin.as_ref().map_or_else(|| "new account".to_owned(), |a| a.name.to_string());
        if let Some(w) = self.window.upgrade() {
            w.log(&format!("Could not add '{name}': {why}"));
            w.toast(&format!("Could not add '{name}'"));
        }
        self.expires.set(None);
        self.set_status(why, true);
    }

    /// Put the approved session in the keyring. The same Roblox user added
    /// twice would share one Cordial profile, so an approval for an account
    /// already here refreshes that one. A new account's label is taken now,
    /// on the main loop, so two dialogs cannot claim the same one.
    fn store(self: &Rc<Self>, cookie: Cookie, user: User) {
        let Some(w) = self.window.upgrade() else { return };
        if let Some(a) = self.relogin.as_ref().filter(|a| a.user_id != user.id) {
            // Storing it would silently turn this label into someone else.
            return self.failed(&format!(
                "that code was approved by {}, not the account '{}' belongs to -- sign in as that user",
                user.name, a.name
            ));
        }
        let existing = w.state().accounts.get(user.id).map(|a| a.name.clone());
        let (label, new) = match existing {
            Some(label) => (label, false),
            None => w.state_mut().accounts.add_or_refresh(&user, chrono::Utc::now()),
        };
        let keyring = w.services().keyring.clone();
        let (to, me) = (label.clone(), Rc::downgrade(self));
        w.run_task(
            move || keyring.set_cookie(&to, &cookie),
            move |w, stored| match stored {
                Ok(()) => {
                    if !new {
                        w.state_mut().accounts.add_or_refresh(&user, chrono::Utc::now());
                    }
                    w.after_sign_in(&label, &user, new);
                    if let Some(d) = me.upgrade() {
                        d.dialog.close();
                    }
                }
                Err(e) => {
                    if new {
                        w.state_mut().accounts.remove(user.id);
                    }
                    if let Some(d) = me.upgrade() {
                        d.failed(&e.to_string());
                    }
                }
            },
        );
    }
}

/// The worker's side of the flow: codes and statuses to the dialog, lines to
/// the activity log.
struct Relay {
    code_gen: u64,
    events: async_channel::Sender<Event>,
    log: Logger,
}

impl QuickLoginEvents for Relay {
    fn code(&mut self, code: &str) {
        let _ = self.events.send_blocking(Event::Code(self.code_gen, code.to_owned()));
    }
    fn tick(&mut self, _secs_left: u64) {}
    fn status(&mut self, status: &QuickLoginStatus) {
        let _ = self.events.send_blocking(Event::Status(self.code_gen, status.clone()));
    }
    fn log(&mut self, line: String) {
        self.log.line(line);
    }
}

fn circle(n: &str) -> gtk::Label {
    let c = gtk::Label::new(Some(n));
    c.add_css_class("step-number");
    c.set_valign(Align::Start);
    c.set_halign(Align::Center);
    c
}

/// A step done: its number becomes a tick.
fn done(circle: &gtk::Label) {
    circle.set_label("✓");
    circle.add_css_class("done");
}

/// One step: its number, then its title, help and content.
fn step(
    circle: &gtk::Label,
    title: &str,
    help: Option<&str>,
    content: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let body = vbox!(4, "", lbl(title, "heading")).hexpand();
    if let Some(h) = help {
        body.append(&lbl(h, "caption dimmed").wrapped());
    }
    let content = content.clone().upcast::<gtk::Widget>();
    content.set_margin_top(8);
    body.append(&content);
    hbox!(14, "", circle.clone(), body)
}
