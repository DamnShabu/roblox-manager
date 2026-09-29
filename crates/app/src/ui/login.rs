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

use super::modal::Modal;
use super::widgets::{Btn, Fluent, IconButton, LabelFluent, dot, icon, lbl};
use super::window::{WeakWindow, Window};
use crate::worker::Logger;

/// What the login worker tells the dialog, for the code it was started for.
enum Event {
    Code(u64, String),
    Status(u64, QuickLoginStatus),
}

pub struct AddAccountDialog {
    window: WeakWindow,
    modal: Modal,
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
    pulse: gtk::Widget,
    status: gtk::Label,
    status_box: gtk::Box,
}

impl AddAccountDialog {
    pub fn open(w: &Window, relogin: Option<Account>) {
        let modal = match &relogin {
            Some(a) => Modal::new(
                "sync",
                "Sign in again",
                &format!("Approve as {}'s Roblox user", a.name),
                480,
            ),
            None => Modal::new("person_add", "Add account", "Sign in with Roblox Quick Login", 480),
        };
        let (tx, rx) = async_channel::unbounded();
        let d = Rc::new_cyclic(|me: &std::rc::Weak<AddAccountDialog>| {
            let me = me.clone();
            let copy =
                Btn::new("copy").text("Copy").icon("content_copy").size(17).build(move || {
                    if let Some(d) = me.upgrade() {
                        d.copy_code();
                    }
                });
            copy.button.set_valign(Align::Center);
            AddAccountDialog {
                window: w.weak(),
                modal,
                relogin,
                generation: Arc::new(AtomicU64::new(0)),
                closed: Arc::new(AtomicBool::new(false)),
                code: RefCell::default(),
                expires: Cell::new(None),
                events: tx,
                step1: circle("1"),
                step2: circle("2"),
                code_a: lbl("···", "code mono").selectable(),
                code_b: lbl("···", "code mono").selectable(),
                copy,
                expiry: lbl("Expires in –:––", "mono").hexpand(),
                bar: gtk::ProgressBar::builder().css_classes(["exp"]).fraction(1.0).build(),
                pulse: dot("wait", true, 8),
                status: lbl("Requesting a code…", "").wrapped().hexpand(),
                status_box: hbox!(10, "qlstatus"),
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
        // The dialog lives as long as it shows: its own close handler holds it.
        let keep = RefCell::new(Some(d.clone()));
        d.modal.dialog.connect_closed(move |_| {
            closed.store(true, Ordering::Relaxed);
            keep.take();
        });
        d.request_code();
        d.modal.present(w.gtk_window());
    }

    fn assemble(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let open = Btn::new("open")
            .text("Open in browser")
            .tip("Opens the page with no code in the address -- you type the code")
            .build(move || {
                if let Some(d) = me.upgrade() {
                    d.open_page();
                }
            });
        if let Some(content) = open.button.child().and_downcast::<gtk::Box>() {
            content.append(&icon("open_in_new", 18, ""));
        }
        let weak = self.window.clone();
        let url = Btn::new("url mono")
            .text("roblox.com/crossdevicelogin")
            .tip("Copy the full address")
            .build(move || {
                if let Some(w) = weak.upgrade() {
                    w.gtk_window().clipboard().set_text(CONFIRM_URL);
                    w.toast("Address copied");
                }
            });
        let links = hbox!(12, "", open.button, url.button);
        links.set_margin_top(10);
        let step1 = step(
            &self.step1,
            "Open Quick Login in your browser",
            Some("Use a device where you're already signed in."),
            &links,
            false,
        );

        let me = Rc::downgrade(self);
        let new_code =
            Btn::new("newcode").text("New code").icon("refresh").size(15).gap(4).build(move || {
                if let Some(d) = me.upgrade() {
                    d.request_code();
                }
            });
        let card = vbox!(
            0,
            "codecard",
            hbox!(
                14,
                "codetop",
                hbox!(14, "", self.code_a.clone(), self.code_b.clone()).hexpand(),
                self.copy.button.clone()
            ),
            hbox!(8, "expiry", self.expiry.clone(), new_code.button),
            self.bar.clone()
        );
        card.set_overflow(gtk::Overflow::Hidden);
        let step2 = step(&self.step2, "Enter this code", None, &card, false);

        let c3 = circle("3");
        c3.add_css_class("active");
        self.status_box.append(&self.pulse);
        self.status_box.append(&self.status);
        let step3 = step(
            &c3,
            "Approve the sign-in",
            Some("The account shows up in your list automatically."),
            &self.status_box,
            true,
        );

        let cancel = {
            let dialog = self.modal.dialog.clone();
            Btn::new("cancel").text("Cancel").build(move || {
                dialog.close();
            })
        };
        self.modal.build(
            &vbox!(0, "stepper", step1, step2, step3),
            &hbox!(
                0,
                "mfoot",
                gtk::Box::new(gtk::Orientation::Horizontal, 0).hexpand(),
                cancel.button
            ),
        );
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
        self.copy.set_icon("check");
        self.copy.set_text("Copied");
        self.copy.button.add_css_class("copied");
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(1600), move || {
            if let Some(d) = me.upgrade() {
                d.copy.set_icon("content_copy");
                d.copy.set_text("Copy");
                d.copy.button.remove_css_class("copied");
            }
        });
    }

    fn tick(&self) {
        let Some(expires) = self.expires.get() else { return };
        let left = expires.saturating_duration_since(Instant::now()).as_secs();
        self.expiry.set_label(&format!("Expires in {}:{:02}", left / 60, left % 60));
        self.bar.set_fraction(left as f64 / TIMEOUT.as_secs_f64());
        if left < 60 {
            self.bar.add_css_class("low");
        } else {
            self.bar.remove_css_class("low");
        }
    }

    fn set_status(&self, text: &str, error: bool) {
        self.status.set_label(text);
        self.pulse.set_visible(!error);
        if error {
            self.status_box.add_css_class("error");
        } else {
            self.status_box.remove_css_class("error");
        }
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
                        d.modal.dialog.close();
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
    c.add_css_class("circle");
    c.set_valign(Align::Start);
    c
}

fn done(circle: &gtk::Label) {
    circle.set_label("check");
    for c in ["ms", "bold", "done"] {
        circle.add_css_class(c);
    }
}

/// One step: its number on a rail, then its title, help and content.
fn step(
    circle: &gtk::Label,
    title: &str,
    help: Option<&str>,
    content: &impl IsA<gtk::Widget>,
    last: bool,
) -> gtk::Box {
    let rail = vbox!(0, "", circle.clone()).halign(Align::Center).width(26);
    if !last {
        rail.append(
            &gtk::Box::new(gtk::Orientation::Vertical, 0)
                .css("connector")
                .vexpand()
                .halign(Align::Center),
        );
    }
    let body = vbox!(4, "stepbody", lbl(title, "steptitle")).hexpand();
    match help {
        Some(h) => body.append(&lbl(h, "stephelp").wrapped()),
        None => body.set_spacing(12),
    }
    body.append(content);
    hbox!(14, "", rail, body)
}
