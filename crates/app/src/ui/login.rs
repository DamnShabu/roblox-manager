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

use super::ds::{self, Variant};
use super::widgets::{Fluent, LabelFluent, lbl, sentence, toggle_class};
use super::window::{WeakWindow, Window};
use crate::ui::panel::Panel;
use crate::worker::Logger;

/// What the login worker tells the dialog, for the code it was started for.
enum Event {
    Code(u64, String),
    Status(u64, QuickLoginStatus),
}

pub struct AddAccountDialog {
    window: WeakWindow,
    dialog: Panel,
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
    copy: ds::Button,
    expiry: gtk::Label,
    bar: gtk::ProgressBar,
    pulse: gtk::ProgressBar,
    status: gtk::Label,
    status_box: gtk::Box,
    /// Where a new account goes once added, and whether it auto-joins.
    put_in: RefCell<Option<String>>,
    follow: Cell<bool>,
}

impl AddAccountDialog {
    pub fn open(w: &Window, relogin: Option<Account>) {
        let dialog = Panel::new(if relogin.is_some() { "Sign in again" } else { "Add account" })
            .with_sub("Roblox Quick Login");
        let (tx, rx) = async_channel::unbounded();
        let d = Rc::new_cyclic(|me: &std::rc::Weak<AddAccountDialog>| {
            let me = me.clone();
            let copy = ds::Button::new("Copy", Variant::Secondary, true).on(move || {
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
                expiry: lbl("Expires in –:––", "t-caption muted").hexpand(),
                bar: gtk::ProgressBar::builder().css_classes(["thin", "ok"]).fraction(1.0).build(),
                pulse: gtk::ProgressBar::builder().css_classes(["thin", "wait"]).build(),
                status: lbl("Requesting a code…", "t-label").wrapped().hexpand(),
                status_box: vbox!(8, ""),
                put_in: RefCell::default(),
                follow: Cell::new(false),
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
        d.dialog.present(w);
    }

    fn assemble(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let open = ds::Button::with_icons(
            "Open Quick Login",
            Variant::Primary,
            true,
            None,
            Some("arrow-right"),
        )
        .tip("Opens roblox.com/crossdevicelogin with no code in the address: you type the code")
        .on(move || {
            if let Some(d) = me.upgrade() {
                d.open_page();
            }
        });
        let weak = self.window.clone();
        let link =
            ds::Button::new("Copy link", Variant::Ghost, true).tip(CONFIRM_URL).on(move || {
                if let Some(w) = weak.upgrade() {
                    w.gtk_window().clipboard().set_text(CONFIRM_URL);
                    w.toast("Link copied");
                }
            });
        let links = hbox!(8, "", open.button, link.button);
        self.step1.add_css_class("now");
        let step1 = step(
            &self.step1,
            "Open Quick Login in a browser",
            Some("On any device where you are signed in to Roblox."),
            &links,
        );

        let me = Rc::downgrade(self);
        let new_code =
            ds::Button::with_icons("New code", Variant::Ghost, true, Some("refresh"), None).on(
                move || {
                    if let Some(d) = me.upgrade() {
                        d.request_code();
                    }
                },
            );
        let code = hbox!(16, "", self.code_a.clone(), self.code_b.clone()).hexpand().centered();
        let code_box = hbox!(12, "login-code-box", code, self.copy.button.clone());
        let expiry = hbox!(8, "", self.expiry.clone(), new_code.button.centered());
        let card = vbox!(8, "", code_box, expiry, self.bar.clone());
        let step2 = step(&self.step2, "Enter this code there", None, &card);

        let c3 = circle("3");
        self.status_box.append(&self.status);
        self.status_box.append(&self.pulse);
        let step3 = step(
            &c3,
            "Approve the sign-in",
            Some("The account shows up here by itself once you approve."),
            &self.status_box,
        );
        let me = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(120), move || match me.upgrade() {
            Some(d) if !d.closed.load(Ordering::Relaxed) => {
                if d.pulse.is_visible() {
                    d.pulse.pulse();
                }
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });

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
        let first = ds::sec("", None);
        first.add_css_class("first");
        first.append(&lbl(&intro, "t-body-sm muted").wrapped());
        let steps = ds::sec("", None);
        steps.set_spacing(20);
        steps.append(&step1);
        steps.append(&step2);
        steps.append(&step3);
        let body = vbox!(0, "", first, steps);
        if self.relogin.is_none() {
            body.append(&self.once_added());
        }
        let foot = hbox!(
            8,
            "cx-sec",
            ds::icon("lock").css("muted s14"),
            lbl("No password is typed here", "t-caption muted")
        );
        body.append(&foot);
        let close = self.dialog.close_button();
        let art = ds::art_icon("plus", 36);
        art.add_css_class("av");
        let head = ds::head(
            &art,
            lbl(&self.dialog.title(), "cx-head-title").upcast_ref(),
            lbl("Roblox Quick Login", "cx-head-sub").upcast_ref(),
            &[close.upcast()],
        );
        let root = vbox!(0, "cx-panel", head);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&body)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        self.dialog.set_child(Some(&root));
    }

    /// Where a new account goes once it is added, and whether it follows
    /// the leader.
    fn once_added(self: &Rc<Self>) -> gtk::Box {
        let sec = ds::sec("Once it is added", None);
        let Some(w) = self.window.upgrade() else { return sec };
        let (groups, leader) = {
            let s = w.state();
            let groups: Vec<(Option<String>, String)> = std::iter::once((None, "Ungrouped".into()))
                .chain(s.accounts.groups().iter().map(|g| {
                    let name =
                        if g.name.is_empty() { "Untitled group".into() } else { g.name.clone() };
                    (Some(g.id.clone()), name)
                }))
                .collect();
            (groups, s.accounts.leader().map(|l| l.name.to_string()))
        };
        let names: Vec<&str> = groups.iter().map(|(_, n)| n.as_str()).collect();
        let (me, groups) = (Rc::downgrade(self), groups.clone());
        let pick = ds::select(&names, 0, move |i| {
            if let Some(d) = me.upgrade() {
                d.put_in.replace(groups.get(i).and_then(|(g, _)| g.clone()));
            }
        });
        sec.append(&ds::sfield("Put it in", &pick, None));
        if let Some(l) = leader {
            let me = Rc::downgrade(self);
            let sw = crate::ui::widgets::switch(false, Some("Auto-join the leader"), move |on| {
                if let Some(d) = me.upgrade() {
                    d.follow.set(on);
                }
            });
            let line = hbox!(12, "", sw, lbl("Auto-join the leader", "t-label"));
            sec.append(&ds::sfield(
                "Role",
                &line,
                Some(&format!("Joins {l}'s server right after it launches.")),
            ));
        }
        sec
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
        self.step2.add_css_class("now");
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
        self.copy.set_text("Copied");
        let me = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(1600), move || {
            if let Some(d) = me.upgrade() {
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
        toggle_class(&self.status, "danger-text", error);
    }

    fn on_event(&self, ev: Event) {
        let current = self.generation.load(Ordering::Relaxed);
        match ev {
            Event::Code(code_gen, code) if code_gen == current => {
                let half = code.len().div_ceil(2);
                self.code_a.set_label(code.get(..half).unwrap_or(&code));
                self.code_b.set_label(code.get(half..).unwrap_or(""));
                self.code.replace(code);
                self.copy.button.set_sensitive(true);
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
        self.copy.button.set_sensitive(false);
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
        }
        self.expires.set(None);
        self.copy.button.set_sensitive(false);
        self.set_status(&sentence(why), true);
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
                    if new && let Some(d) = me.upgrade() {
                        let put_in = d.put_in.borrow().clone();
                        if put_in.is_some() {
                            w.set_group(user.id, put_in);
                        }
                        if d.follow.get() {
                            w.set_follow(user.id, true);
                        }
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
    circle.remove_css_class("now");
    circle.add_css_class("done");
}

/// One step: its number, then its title, help and content.
fn step(
    circle: &gtk::Label,
    title: &str,
    help: Option<&str>,
    content: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let body = vbox!(4, "", lbl(title, "t-label")).hexpand();
    if let Some(h) = help {
        body.append(&lbl(h, "t-caption muted").wrapped());
    }
    let content = content.clone().upcast::<gtk::Widget>();
    content.set_margin_top(8);
    body.append(&content);
    hbox!(14, "", circle.clone(), body)
}
