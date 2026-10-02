//! Recording steps into the editor from a running client: the banner that
//! says what to do in the client's window -- press F8 to start, F8 again to
//! stop -- while a recording is armed and while it runs, and the thread that
//! hears the client's relay meanwhile.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::thread;

use adw::prelude::*;
use gtk::{Align, glib};
use rbxmgr_core::macros::grammar::Row;
use rbxmgr_core::macros::recording::{self, RECORD_KEY, Recording};
use rbxmgr_core::macros::relay::{self, report};
use rbxmgr_core::macros::{MacroError, keys};

use super::clients;
use crate::ui::widgets::{Btn, Fluent, LabelFluent, lbl, plural, sentence};
use crate::ui::window::{ReadyClient, WeakWindow};

/// What the recording thread tells the editor, in order.
enum Said {
    /// The relay is armed; this ends the wait.
    Armed(report::Closer),
    /// The record key was pressed: the window is being recorded.
    Started,
    Done(Result<Recording, MacroError>),
}

/// The steps recorded, or why there are none.
type Recorded = Result<Vec<Row>, String>;

/// A recording armed or under way.
struct Active {
    client: ReadyClient,
    /// What disarms the relay, once it has been reached.
    closer: Option<report::Closer>,
    /// The button that asked, kept from asking twice meanwhile.
    anchor: glib::WeakRef<gtk::Button>,
    done: Box<dyn FnOnce(Recorded)>,
}

pub struct Record {
    window: WeakWindow,
    banner: gtk::Box,
    status: gtk::Label,
    active: RefCell<Option<Active>>,
}

impl Record {
    pub fn new(window: WeakWindow) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Record>| {
            let dot = gtk::Box::builder().valign(Align::Center).css_classes(["dot"]).build();
            let status = lbl("", "").wrapped().hexpand();
            let me = me.clone();
            // No mnemonic: the dialog's own Cancel has Alt+C.
            let cancel = Btn::new("flat").text("Cancel").build(move || {
                if let Some(r) = me.upgrade() {
                    r.cancel();
                }
            });
            let banner = hbox!(12, "recording-banner", dot, status.clone(), cancel.button);
            banner.set_visible(false);
            banner.set_margin_bottom(12);
            Record { window, banner, status, active: RefCell::new(None) }
        })
    }

    /// Where it says what to do while a recording is armed or runs.
    pub fn banner(&self) -> &gtk::Box {
        &self.banner
    }

    /// Record a running client, the only one or one picked from a menu under
    /// `anchor`: `done` hears the steps once the record key has been pressed
    /// twice in its window, or why there are none. Cancelled, it hears
    /// nothing.
    pub fn start(self: &Rc<Self>, anchor: &gtk::Button, done: impl FnOnce(Recorded) + 'static) {
        let Some(w) = self.window.upgrade() else { return };
        if self.active.borrow().is_some() {
            return;
        }
        const NONE_UP: &str =
            "Launch an account with Macro-ready window on, then record in its window";
        let (me, asked) = (Rc::downgrade(self), anchor.downgrade());
        clients::choose(&w, anchor, NONE_UP, move |chosen| match (chosen, me.upgrade()) {
            (Ok(client), Some(me)) => me.arm(client, asked, Box::new(done)),
            (Err(e), _) => done(Err(e)),
            (Ok(_), None) => {}
        });
    }

    fn arm(
        self: &Rc<Self>,
        client: ReadyClient,
        anchor: glib::WeakRef<gtk::Button>,
        done: Box<dyn FnOnce(Recorded)>,
    ) {
        let Some(w) = self.window.upgrade() else { return };
        if w.state().macro_runs.contains_key(&client.id) {
            let label = &client.label;
            return done(Err(format!(
                "Stop the macro playing on {label} first: the recording would hear it as well"
            )));
        }
        w.set_recording(Some(client.id));
        let (tell, heard) = async_channel::unbounded();
        let record_file = relay::record_file(&client.display);
        thread::spawn(move || {
            // The editor going away first leaves nobody to tell.
            let say = |said| drop(tell.send_blocking(said));
            let mut recorder = match report::Recorder::arm(&record_file, RECORD_KEY) {
                Ok(recorder) => recorder,
                Err(e) => return say(Said::Done(Err(e))),
            };
            match recorder.closer() {
                Ok(closer) => say(Said::Armed(closer)),
                Err(e) => return say(Said::Done(Err(MacroError::WentAway(e.to_string())))),
            }
            let got = recording::record(&mut || recorder.hear(), &|| say(Said::Started));
            say(Said::Done(got));
        });
        if let Some(b) = anchor.upgrade() {
            b.set_sensitive(false);
        }
        let label = client.label.clone();
        self.active.replace(Some(Active { client, closer: None, anchor, done }));
        self.show(&format!("Press {} in {label}'s window to start recording.", key_label()), false);
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(said) = heard.recv().await {
                match (me.upgrade(), said) {
                    (Some(me), said) => me.heard(said),
                    // The editor closed before the relay was reached: disarm it.
                    (None, Said::Armed(closer)) => closer.close(),
                    (None, _) => {}
                }
            }
        });
    }

    fn heard(&self, said: Said) {
        let mut active = self.active.borrow_mut();
        let Some(a) = active.as_mut() else {
            // Cancelled already; a late relay is disarmed.
            if let Said::Armed(closer) = said {
                closer.close();
            }
            return;
        };
        match said {
            Said::Armed(closer) => a.closer = Some(closer),
            Said::Started => {
                let (label, key) = (a.client.label.clone(), key_label());
                self.show(
                    &format!("Recording {label}'s window. Press {key} there again to stop."),
                    true,
                );
                if let Some(w) = self.window.upgrade() {
                    w.notify(
                        "Recording",
                        &format!("Press {key} again in {label}'s window to stop."),
                    );
                }
            }
            Said::Done(result) => {
                let Some(a) = active.take() else { return };
                drop(active);
                self.finish(a, result);
            }
        }
    }

    fn finish(&self, active: Active, result: Result<Recording, MacroError>) {
        self.banner.set_visible(false);
        if let Some(b) = active.anchor.upgrade() {
            b.set_sensitive(true);
        }
        let label = active.client.label;
        let recorded = match result {
            Ok(r) if r.is_empty() => {
                Err(format!("Nothing was pressed or moved in {label}'s window"))
            }
            Ok(r) => Ok(r.rows(&label)),
            Err(e) => Err(sentence(&format!("could not record {label}: {e}"))),
        };
        if let Some(w) = self.window.upgrade() {
            w.set_recording(None);
            match &recorded {
                Ok(rows) => {
                    let steps =
                        plural(rows.iter().filter(|r| r.kind != "Note").count(), "step", "steps");
                    w.notify(
                        "Recording added",
                        &format!("{steps} from {label}: review them, then Save."),
                    );
                }
                Err(e) => w.notify("Nothing recorded", e),
            }
        }
        (active.done)(recorded);
    }

    /// Stop waiting, or stop recording: the relay disarms, nothing is added.
    pub fn cancel(&self) {
        let Some(a) = self.active.borrow_mut().take() else { return };
        if let Some(closer) = &a.closer {
            closer.close();
        }
        if let Some(b) = a.anchor.upgrade() {
            b.set_sensitive(true);
        }
        if let Some(w) = self.window.upgrade() {
            w.set_recording(None);
        }
        self.banner.set_visible(false);
    }

    fn show(&self, text: &str, live: bool) {
        self.status.set_label(text);
        if live {
            self.banner.add_css_class("live");
        } else {
            self.banner.remove_css_class("live");
        }
        self.banner.set_visible(true);
    }
}

impl Drop for Record {
    /// The editor closing ends its recording.
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The record key as the keyboard shows it: F8.
fn key_label() -> String {
    keys::key_name(RECORD_KEY).to_uppercase()
}
