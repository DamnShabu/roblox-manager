//! The details pane under the table: the account the table shows as current
//! -- who it is and how it is -- then its controls (its performance, its
//! macro, its window, its session), its macro's steps, and its own lines of
//! the activity log. It folds away to give the table the room.

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};
use rbxmgr_core::accounts::{Account, SessionState, relative_time};
use rbxmgr_core::cordial::{Performance, Window as ClientWindow};
use rbxmgr_core::macros::grammar;

use super::{Window, can_hide};
use crate::state::{Chip, Inspected};
use crate::ui::accounts::settings::performance_detail;
use crate::ui::ds::{self, Variant};
use crate::ui::login::AddAccountDialog;
use crate::ui::macros::editor::MacroDialog;
use crate::ui::macros::kinds::step_icon;
use crate::ui::table::format::{bytes, percent, uptime};
use crate::ui::table::row::menu;
use crate::ui::widgets::{Fluent, LabelFluent, clear, lbl, name, switch, toggle_class};

/// How many of its macro's steps the Macro steps tab lists; the editor has
/// the rest.
const SHOWN_STEPS: usize = 40;
/// How many of its log lines the Log tab lists.
const SHOWN_LINES: usize = 200;

pub struct Details {
    pub root: gtk::Box,
    head: gtk::Box,
    pages: gtk::Stack,
    content: gtk::Revealer,
    pub fold: gtk::ToggleButton,
    tabs: Vec<(gtk::ToggleButton, &'static str)>,
}

impl Details {
    pub fn build() -> Self {
        let head = gtk::Box::new(Orientation::Horizontal, 12).css("dt-head-who");
        let tabs_box = gtk::Box::new(Orientation::Horizontal, 0);
        let mut tabs: Vec<(gtk::ToggleButton, &'static str)> = Vec::new();
        for (key, text) in [("controls", "Controls"), ("steps", "Macro steps"), ("log", "Log")] {
            let t = gtk::ToggleButton::builder().label(text).css_classes(["dt-tab"]).build();
            if let Some((first, _)) = tabs.first() {
                t.set_group(Some(first));
            } else {
                t.set_active(true);
            }
            tabs_box.append(&t);
            tabs.push((t, key));
        }
        let pages = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .vhomogeneous(false)
            .build();
        for (t, key) in &tabs {
            let (pages, key) = (pages.clone(), *key);
            t.connect_toggled(move |t| {
                if t.is_active() {
                    pages.set_visible_child_name(key);
                }
            });
        }
        let fold = gtk::ToggleButton::builder()
            .child(&ds::icon("chev-down"))
            .active(true)
            .tooltip_text("Fold the details away (F7)")
            .valign(Align::Center)
            .css_classes(["ib", "fold"])
            .build();
        name(&fold, "Show or fold the details");
        let bar = gtk::Box::new(Orientation::Horizontal, 8).css("dt-head");
        bar.append(&head.clone().hexpand());
        bar.append(&tabs_box);
        bar.append(&fold);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&pages)
            .hscrollbar_policy(PolicyType::Never)
            .min_content_height(150)
            .max_content_height(320)
            .propagate_natural_height(true)
            .build();
        let content = gtk::Revealer::builder().child(&scroller).reveal_child(true).build();
        fold.bind_property("active", &content, "reveal-child").sync_create().build();
        {
            let tabs_box = tabs_box.clone();
            fold.connect_toggled(move |f| {
                toggle_class(f, "folded", !f.is_active());
                tabs_box.set_visible(f.is_active());
            });
        }
        let root = gtk::Box::new(Orientation::Vertical, 0).css("dt");
        name(&root, "Details of the current account");
        root.append(&bar);
        root.append(&content);
        Details { root, head, pages, content, fold, tabs }
    }

    fn page(&self, key: &str, child: &impl IsA<gtk::Widget>) {
        if let Some(old) = self.pages.child_by_name(key) {
            self.pages.remove(&old);
        }
        self.pages.add_named(child, Some(key));
        let on = self.tabs.iter().find(|(t, _)| t.is_active()).map_or("controls", |(_, k)| *k);
        self.pages.set_visible_child_name(on);
    }
}

impl Window {
    /// The details pane, for the current account; or what to do with none.
    pub fn draw_details(&self) {
        let d = &self.0.ui.details;
        clear(&d.head);
        let current = match &self.state().inspected {
            Some(Inspected::Account(id)) => self.state().accounts.get(*id).cloned(),
            _ => None,
        };
        let Some(acct) = current else {
            d.head.append(&lbl("Click an account to see its controls here", "mn-dim"));
            for key in ["controls", "steps", "log"] {
                d.page(key, &lbl("", ""));
            }
            d.content.set_visible(false);
            return;
        };
        d.content.set_visible(true);
        d.head.append(&who(self, &acct));
        d.page("controls", &controls(self, &acct));
        d.page("steps", &steps(self, &acct));
        d.page("log", &log(self, &acct));
    }
}

/// Who it is and how it is, live.
fn who(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let picture = w.services().avatars.cached(&id.to_string());
    let b = gtk::Box::new(Orientation::Horizontal, 12);
    b.append(&ds::av(acct.name.as_str(), picture.as_deref(), 30));
    b.append(&lbl(acct.name.as_str(), "dt-name").ellipsize());
    let user = acct.username.clone().unwrap_or_else(|| acct.name.to_string());
    let group = {
        let s = w.state();
        if acct.leader {
            "Main account".to_owned()
        } else {
            s.accounts
                .group_of(acct)
                .and_then(|g| s.accounts.groups().iter().find(|x| x.id == g))
                .map_or_else(|| "Ungrouped".to_owned(), |g| g.name.clone())
        }
    };
    b.append(&lbl(&format!("@{user} · {group}"), "mn-dim").ellipsize());
    let dot = gtk::Box::new(Orientation::Horizontal, 0).css("mn-dot");
    dot.set_valign(Align::Center);
    let state = lbl("", "mn-state");
    let summary = lbl("", "mn-dim");
    let status = gtk::Box::new(Orientation::Horizontal, 8);
    status.append(&dot);
    status.append(&state);
    b.append(&status);
    b.append(&summary);
    let (dot, state, summary) = (dot.downgrade(), state.downgrade(), summary.downgrade());
    w.watch_while(Box::new(move |s| {
        let (Some(dot), Some(state), Some(summary)) =
            (dot.upgrade(), state.upgrade(), summary.upgrade())
        else {
            return false;
        };
        let chip = s.chip(id);
        let (text, kind) = match chip {
            Chip::Running => ("Running", "run"),
            Chip::Starting | Chip::Joining => ("Starting", "start"),
            Chip::Expired => ("Signed out", "bad"),
            Chip::Idle if s.failures.contains_key(&id) => ("Failed", "bad"),
            Chip::Idle => ("Idle", "idle"),
        };
        state.set_label(text);
        for k in ["run", "start", "bad", "idle"] {
            toggle_class(&dot, k, k == kind);
            toggle_class(&state, k, k == kind);
        }
        summary.set_label(&match s.usage.get(&id).filter(|_| chip == Chip::Running) {
            Some(u) => {
                let cpu = u.cpu.map_or_else(|| "—".to_owned(), percent);
                format!("{} · {cpu} CPU · {}", uptime(u.uptime), bytes(u.memory))
            }
            None => String::new(),
        });
        true
    }));
    b
}

/// A field: its small label over its controls.
fn field(label: &str, body: &gtk::Box) -> gtk::Box {
    let f = gtk::Box::new(Orientation::Vertical, 8).css("dt-f");
    f.append(&lbl(label, "dt-lb"));
    f.append(body);
    f
}

/// Its controls, each applying as it is changed.
fn controls(w: &Window, acct: &Account) -> gtk::FlowBox {
    // Side by side when there is room; a narrow window takes them a line
    // or two at a time.
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(false)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(36)
        .row_spacing(16)
        .css_classes(["dt-grid"])
        .build();
    for f in [performance(w, acct), macro_field(w, acct), window_field(w, acct), session(w, acct)] {
        grid.insert(&f, -1);
    }
    grid
}

fn performance(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let detail = lbl(performance_detail(acct.performance()), "mn-dim dt-help").wrapped().chars(40);
    let opts: Vec<(&str, &str)> = Performance::ALL.iter().map(|p| (p.label(), p.label())).collect();
    let (weak, d) = (w.weak(), detail.clone());
    let seg = ds::seg(&opts, acct.performance().label(), move |key| {
        let Some(level) = Performance::ALL.into_iter().find(|p| p.label() == key) else { return };
        d.set_label(performance_detail(level));
        if let Some(w) = weak.upgrade() {
            w.set_performance(id, level);
        }
    });
    let when = lbl("A change applies the next time it starts.", "mn-dim dt-help").wrapped();
    let body = gtk::Box::new(Orientation::Vertical, 8);
    body.append(&seg);
    body.append(&detail);
    body.append(&when);
    let when = when.downgrade();
    w.watch_while(Box::new(move |s| {
        let Some(when) = when.upgrade() else { return false };
        when.set_label(&match s.stale_performance(id) {
            Some(running) => format!("Running at {} until it starts again.", running.label()),
            None => "A change applies the next time it starts.".to_owned(),
        });
        true
    }));
    field("Performance", &body)
}

fn macro_field(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let names: Vec<String> = w.state().macros.names().map(str::to_owned).collect();
    let mine = acct.macro_name.clone().filter(|m| names.contains(m));
    let options: Vec<&str> =
        std::iter::once("None").chain(names.iter().map(String::as_str)).collect();
    let at = mine.as_ref().and_then(|m| names.iter().position(|n| n == m)).map_or(0, |i| i + 1);
    let (weak, picks) = (w.weak(), names.clone());
    let pick = ds::select(&options, at, move |i| {
        let name = i.checked_sub(1).and_then(|i| picks.get(i)).cloned();
        if let Some(w) = weak.upgrade() {
            w.pick_macro(id, name);
        }
    });
    pick.set_width_request(180);
    let run = ds::Button::with_icons("Run", Variant::Secondary, true, Some("play-circle"), None)
        .on(w.act(move |w| w.play_macro_here(id)));
    let edit_name = mine.clone();
    let edit = ds::Button::with_icons("Edit", Variant::Ghost, true, Some("pencil"), None)
        .on(w.act(move |w| MacroDialog::open(w, edit_name.as_deref())));
    edit.button.set_sensitive(mine.is_some());
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.append(&pick);
    line.append(&run.button);
    line.append(&edit.button);
    let at_now = lbl("", "dt-help").ellipsize();
    let progress = gtk::ProgressBar::new();
    progress.add_css_class("thin");
    let body = gtk::Box::new(Orientation::Vertical, 8);
    body.append(&line);
    body.append(&at_now);
    body.append(&progress);
    let (run, at_now, progress) = (run.clone(), at_now.downgrade(), progress.downgrade());
    w.watch_while(Box::new(move |s| {
        let (Some(at_now), Some(progress)) = (at_now.upgrade(), progress.upgrade()) else {
            return false;
        };
        let playing = s.macro_runs.get(&id).map(|(_, m)| m.clone());
        let chosen =
            s.accounts.get(id).and_then(|a| a.macro_name.clone()).filter(|m| s.macros.contains(m));
        run.set_text(if playing.is_some() { "Stop" } else { "Run" });
        run.set_icon(if playing.is_some() { "stop-circle" } else { "play-circle" });
        let live = s.chip(id) == Chip::Running;
        run.button.set_sensitive(playing.is_some() || (live && chosen.is_some()));
        let at = s.macro_progress.get(&id).map(String::as_str);
        match (&playing, at) {
            (Some(_), Some(at)) => at_now.set_label(&at.replace(": ", " · ")),
            (Some(_), None) => at_now.set_label("Starting"),
            (None, _) if chosen.is_some() && !live => {
                at_now.set_label("Launch it to run its macro")
            }
            (None, _) => at_now.set_label(""),
        }
        let fraction = playing.as_ref().and(at).and_then(step_fraction);
        progress.set_visible(fraction.is_some());
        progress.set_fraction(fraction.unwrap_or(0.0));
        true
    }));
    field("Macro", &body)
}

/// How far through its round a macro is, from where it says it is:
/// "round 2, step 3/7: pressing e" is 3 of 7.
fn step_fraction(at: &str) -> Option<f64> {
    let (_, rest) = at.split_once("step ")?;
    let (n, rest) = rest.split_once('/')?;
    let of: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let (n, of): (f64, f64) = (n.trim().parse().ok()?, of.parse().ok()?);
    (of > 0.0).then(|| (n / of).clamp(0.0, 1.0))
}

fn window_field(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let hide =
        ds::Button::with_icons("Hide window", Variant::Secondary, true, Some("eye-off"), None).on(
            w.act(move |w| {
                let hide = can_hide(&w.state(), id);
                w.set_window_hidden(id, hide);
            }),
        );
    hide.button.set_halign(Align::Start);
    let weak = w.weak();
    let ready = switch(acct.nested, Some("Macro-ready window"), move |on| {
        if let Some(w) = weak.upgrade() {
            w.set_nested(id, on);
        }
    });
    let line = gtk::Box::new(Orientation::Horizontal, 10);
    line.append(&ready);
    line.append(&lbl("Macro-ready", ""));
    let body = gtk::Box::new(Orientation::Vertical, 10);
    body.append(&hide.button);
    body.append(&line);
    // The switch's line stands for the page: gone with it, the redraw goes.
    let (hide, alive) = (hide.clone(), line.downgrade());
    w.watch_while(Box::new(move |s| {
        if alive.upgrade().is_none() {
            return false;
        }
        let hidden = s.windows.get(&id) == Some(&ClientWindow::Hidden);
        hide.set_text(if hidden { "Show window" } else { "Hide window" });
        hide.set_icon(if hidden { "eye" } else { "eye-off" });
        hide.button.set_sensitive(hidden || can_hide(s, id));
        true
    }));
    field("Window", &body)
}

fn session(w: &Window, acct: &Account) -> gtk::Box {
    let id = acct.user_id;
    let mark = ds::icon("ok-circle").css("s16");
    let state = lbl("", "dt-help").ellipsize();
    let line = gtk::Box::new(Orientation::Horizontal, 8);
    line.append(&mark);
    line.append(&state);
    let check =
        ds::Button::with_icons("Check now", Variant::Secondary, true, Some("refresh"), None)
            .on(w.act(move |w| w.check_sessions(vec![id])));
    let again = acct.clone();
    let sign_in =
        ds::Button::with_icons("Sign in again", Variant::Secondary, true, Some("login"), None)
            .on(w.act(move |w| AddAccountDialog::open(w, Some(again.clone()))));
    let inner = gtk::Box::new(Orientation::Horizontal, 6);
    inner.append(&ds::icon("move-to"));
    inner.append(&lbl("Move to", ""));
    let move_to = gtk::MenuButton::builder()
        .child(&inner)
        .menu_model(&menu(w, acct))
        .css_classes(["ds", "ghost", "sm"])
        .tooltip_text("Settings, its group, its session, removing it")
        .build();
    let buttons = gtk::Box::new(Orientation::Horizontal, 8);
    buttons.append(&check.button);
    buttons.append(&sign_in.button);
    buttons.append(&move_to);
    let body = gtk::Box::new(Orientation::Vertical, 10);
    body.append(&line);
    body.append(&buttons);
    let (mark, state) = (mark.downgrade(), state.downgrade());
    let (check, sign_in) = (check.clone(), sign_in.clone());
    w.watch_while(Box::new(move |s| {
        let (Some(mark), Some(state)) = (mark.upgrade(), state.upgrade()) else { return false };
        let session = s.accounts.session(id);
        let (icon, kind, text) = match &session {
            SessionState::Checking => ("hourglass", "start", "Checking with Roblox…".to_owned()),
            SessionState::Expired => {
                ("error", "bad", "Signed out · Roblox no longer takes it".to_owned())
            }
            SessionState::Ok { checked: Some(when) } => (
                "ok-circle",
                "run",
                format!("Signed in · checked {}", relative_time(Some(when), chrono::Utc::now())),
            ),
            SessionState::Ok { checked: None } => ("ok-circle", "run", "Signed in".to_owned()),
        };
        mark.set_icon_name(Some(&crate::ui::icons::name(icon)));
        for k in ["run", "start", "bad"] {
            toggle_class(&mark, k, k == kind);
        }
        state.set_label(&text);
        check.button.set_sensitive(session != SessionState::Checking);
        sign_in.button.set_visible(session == SessionState::Expired);
        true
    }));
    field("Session", &body)
}

/// Its macro's steps, as the editor names them.
fn steps(w: &Window, acct: &Account) -> gtk::Box {
    let page = gtk::Box::new(Orientation::Vertical, 6).css("dt-page");
    let s = w.state();
    let Some(text) = acct.macro_name.as_deref().and_then(|m| s.macros.text(m).map(|t| (m, t)))
    else {
        page.append(&lbl(
            "No macro picked. Pick one under Controls to see its steps here.",
            "mn-dim",
        ));
        return page;
    };
    let (name, text) = text;
    let (rows, loops) = grammar::rows(text);
    page.append(&lbl(&format!("{name} · {}", grammar::loop_label(loops)), "dt-lb"));
    let list = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(4)
        .min_children_per_line(2)
        .column_spacing(24)
        .row_spacing(4)
        .build();
    for r in rows.iter().take(SHOWN_STEPS) {
        let line = gtk::Box::new(Orientation::Horizontal, 8);
        line.append(&ds::icon(step_icon(&r.kind)).css("mn-dim s16"));
        line.append(&lbl(&r.kind, "dt-step-k"));
        line.append(&lbl(&r.value, "dt-step-v mono").ellipsize());
        list.insert(&line, -1);
    }
    page.append(&list);
    if rows.len() > SHOWN_STEPS {
        page.append(&lbl(
            &format!("and {} more in its editor", rows.len() - SHOWN_STEPS),
            "mn-dim",
        ));
    }
    page
}

/// Its own lines of the activity log, newest first.
fn log(w: &Window, acct: &Account) -> gtk::Box {
    let page = gtk::Box::new(Orientation::Vertical, 4).css("dt-page");
    let label = acct.name.to_string();
    let s = w.state();
    let mine: Vec<_> =
        s.activity.iter().filter(|a| a.line.contains(&label)).take(SHOWN_LINES).collect();
    if mine.is_empty() {
        page.append(&lbl(&format!("Nothing about {label} in this run's activity yet."), "mn-dim"));
    }
    for a in mine {
        let line = gtk::Box::new(Orientation::Horizontal, 12);
        line.append(&lbl(&a.time, "mn-dim mono"));
        line.append(&lbl(&a.line, "").ellipsize().hexpand());
        page.append(&line);
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_counts_how_far_through_its_round_it_is() {
        assert_eq!(step_fraction("round 2, step 3/4: pressing e"), Some(0.75));
        assert_eq!(step_fraction("round 2, step 9/4"), Some(1.0));
        assert_eq!(step_fraction("starting"), None);
        assert_eq!(step_fraction("step 1/0"), None);
    }
}
