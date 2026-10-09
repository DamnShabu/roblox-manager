//! What needs you, said where it is seen: the alert strip under the top bar
//! (the first thing, until dismissed), and the inspector's overview while
//! nothing is open there -- everything that needs you, what is running,
//! and a tip.

use adw::prelude::*;
use gtk::{Align, Orientation};
use rbxmgr_core::accounts::SessionState;
use rbxmgr_core::cordial::Window as ClientWindow;
use rbxmgr_core::types::UserId;

use super::Window;
use crate::state::AppState;
use crate::ui::accounts::settings::AccountSettings;
use crate::ui::ds::{self, Variant};
use crate::ui::login::AddAccountDialog;
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, sentence};

/// Something that needs you: about which account, what, and its fix.
#[derive(Clone, PartialEq, Eq)]
pub struct Need {
    pub id: UserId,
    pub name: String,
    pub expired: bool,
    pub why: String,
}

impl Need {
    /// What the overview says.
    fn text(&self) -> String {
        if self.expired {
            format!("{}: Roblox no longer takes this session", self.name)
        } else {
            format!("{}: {}", self.name, sentence(&self.why))
        }
    }

    fn fix_label(&self) -> &'static str {
        if self.expired { "Sign in again" } else { "Try again" }
    }

    fn fix(&self, w: &Window) {
        if self.expired {
            let acct = w.state().accounts.get(self.id).cloned();
            if let Some(acct) = acct {
                AddAccountDialog::open(w, Some(acct));
            }
        } else {
            w.play_or_stop(self.id);
        }
    }
}

/// Everything that needs you now: expired sessions, then failed launches.
pub fn needs(s: &AppState) -> Vec<Need> {
    let mut out = Vec::new();
    for a in s.accounts.accounts() {
        if s.accounts.session(a.user_id) == SessionState::Expired {
            out.push(Need {
                id: a.user_id,
                name: a.name.to_string(),
                expired: true,
                why: String::new(),
            });
        }
    }
    for a in s.accounts.accounts() {
        if let Some(why) = s.failures.get(&a.user_id).filter(|_| !s.running.contains(&a.user_id)) {
            out.push(Need {
                id: a.user_id,
                name: a.name.to_string(),
                expired: false,
                why: why.clone(),
            });
        }
    }
    out
}

/// The strip under the top bar.
pub struct Alert {
    pub root: gtk::Revealer,
    text: gtk::Label,
    fix: ds::Button,
}

impl Alert {
    pub fn build(w: &super::WeakWindow) -> Self {
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let fix = ds::Button::new("", Variant::Primary, true).on({
            let w = w.clone();
            move || {
                if let Some(w) = w.upgrade() {
                    let first = needs(&w.state()).into_iter().next();
                    if let Some(n) = first {
                        n.fix(&w);
                    }
                }
            }
        });
        fix.button.set_valign(Align::Center);
        let dismiss = ds::ib("x", "Dismiss until something new happens", true);
        let strip = gtk::Box::new(Orientation::Horizontal, 12).css("mn-alert");
        strip.append(&ds::icon("alert").css("s18"));
        strip.append(&text);
        strip.append(&fix.button);
        strip.append(&dismiss);
        let root = gtk::Revealer::builder().child(&strip).build();
        dismiss.connect_clicked({
            let w = w.clone();
            move |_| {
                if let Some(w) = w.upgrade() {
                    let first = needs(&w.state()).into_iter().next();
                    w.0.alert_dismissed.replace(first);
                    w.0.ui.alert.root.set_reveal_child(false);
                }
            }
        });
        Alert { root, text, fix }
    }
}

impl Window {
    /// The alert strip and the overview, as the state is.
    pub(super) fn draw_needs(&self) {
        let needs = needs(&self.state());
        let alert = &self.0.ui.alert;
        let first = needs.first().cloned();
        let dismissed = *self.0.alert_dismissed.borrow() == first;
        match first.as_ref().filter(|_| !dismissed) {
            Some(n) => {
                let what = if n.expired {
                    format!("{}'s session expired", n.name)
                } else {
                    format!("{} did not launch", n.name)
                };
                alert.text.set_markup(&format!(
                    "<b>Needs you:</b> {}",
                    gtk::glib::markup_escape_text(&what)
                ));
                alert.fix.set_text(&if n.expired {
                    format!("Sign in {}", n.name)
                } else {
                    "Try again".to_owned()
                });
                alert.root.set_reveal_child(true);
            }
            None => alert.root.set_reveal_child(false),
        }
        self.draw_overview(&needs);
    }

    fn draw_overview(&self, needs: &[Need]) {
        let page = &self.0.ui.insp.overview;
        clear(page);
        let first = ds::sec("Needs you", None);
        first.add_css_class("first");
        if needs.is_empty() {
            first.append(&lbl("Nothing needs you right now.", "t-body-sm muted"));
        }
        for n in needs {
            let note = ds::note(ds::Tone::Danger, "alert", &n.text());
            let fix = ds::Button::new(n.fix_label(), Variant::Secondary, true);
            let (weak, n2) = (self.weak(), n.clone());
            fix.button.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() {
                    n2.fix(&w);
                }
            });
            fix.button.set_valign(Align::Center);
            note.append(&fix.button);
            first.append(&note);
        }
        page.append(&first);

        let s = self.state();
        let running: Vec<_> =
            s.accounts.accounts().iter().filter(|a| s.running.contains(&a.user_id)).collect();
        let count = lbl(&format!("{} running", running.len()), "t-caption muted");
        let sec = ds::sec("Running", Some(count.upcast_ref()));
        if running.is_empty() {
            sec.append(&lbl("No client is running.", "t-body-sm muted"));
        } else {
            let list = gtk::Box::new(Orientation::Vertical, 0).css("cx-list");
            for a in running {
                let id = a.user_id;
                let mut sub: Vec<String> = Vec::new();
                if let Some(p) = s.playing.get(&id) {
                    sub.push(s.game_name(p).map_or_else(|| format!("Place {p}"), str::to_owned));
                }
                if let Some((_, m)) = s.macro_runs.get(&id) {
                    sub.push(m.clone());
                }
                let row = gtk::Box::new(Orientation::Horizontal, 8).css("cx-row");
                let pic = self.services().avatars.cached(&id.to_string());
                row.append(&ds::av(a.name.as_str(), pic.as_deref(), 24));
                let text = gtk::Box::new(Orientation::Vertical, 0);
                text.append(&lbl(a.name.as_str(), "cx-row-t").ellipsize());
                text.append(&lbl(&sub.join(" · "), "cx-row-s").ellipsize());
                let open = gtk::Button::builder()
                    .child(&text)
                    .css_classes(["mn-name"])
                    .hexpand(true)
                    .build();
                let weak = self.weak();
                open.connect_clicked(move |_| {
                    if let Some(w) = weak.upgrade() {
                        AccountSettings::open(&w, id);
                    }
                });
                row.append(&open);
                let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
                if hidden || super::can_hide(&s, id) {
                    let hide = ds::ib(
                        if hidden { "eye-off" } else { "eye" },
                        if hidden { "Show its window" } else { "Hide its window" },
                        true,
                    );
                    if hidden {
                        hide.add_css_class("on");
                    }
                    let weak = self.weak();
                    hide.connect_clicked(move |_| {
                        if let Some(w) = weak.upgrade() {
                            w.set_window_hidden(id, !hidden);
                        }
                    });
                    row.append(&hide);
                }
                let stop = ds::ib("stop", &format!("Close {}'s client", a.name), true);
                stop.add_css_class("stop");
                let weak = self.weak();
                stop.connect_clicked(move |_| {
                    if let Some(w) = weak.upgrade() {
                        w.stop_account(id);
                    }
                });
                row.append(&stop);
                list.append(&row);
            }
            sec.append(&list);
        }
        page.append(&sec);
        let tip = ds::sec("Tip", None);
        tip.append(
            &lbl(
                "Click an account or a group to edit it here. Ctrl+N adds an account; Ctrl+? \
                 lists every shortcut.",
                "t-body-sm muted",
            )
            .wrapped(),
        );
        page.append(&tip);
    }
}
