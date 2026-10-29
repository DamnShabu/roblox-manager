//! The activity log: every line of this run, newest first, in the drawer
//! that rises over the bottom of the window; filters by kind, and a copy.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use super::ds::{self, Variant};
use super::widgets::{Fluent, LabelFluent, lbl, plural};
use super::window::Window;
use crate::state::Activity;

/// A line's kind, worked out from its words: log lines have dozens of
/// callers, most in the core, so none of them passes one.
pub fn kind(line: &str) -> &'static str {
    let m = line.to_lowercase();
    let kinds: [(&str, &[&str]); 7] = [
        ("error", &["failed", "could not", "expired", "error", "cannot"]),
        ("update", &["up to date"]),
        ("stop", &["stopped", "removed", "was not running", "shut down"]),
        ("join", &["joined", " into ", "in server"]),
        ("friend", &["join ", "joining"]),
        ("launch", &["launch"]),
        ("macro", &["macro", "playing", "round ", "saved"]),
    ];
    kinds.iter().find(|(_, words)| words.iter().any(|w| m.contains(w))).map_or("info", |(k, _)| k)
}

/// The design icon for a kind.
pub fn icon_for(kind: &str) -> &'static str {
    match kind {
        "launch" => "play",
        "stop" => "stop",
        "macro" => "keyboard",
        "update" => "check",
        "friend" => "user",
        "error" => "alert",
        "join" => "link",
        _ => "info",
    }
}

/// The colour class a kind's icon takes, if any.
pub fn tone_class(kind: &str) -> Option<&'static str> {
    match kind {
        "error" => Some("danger-text"),
        "update" => Some("success-text"),
        "friend" => Some("warning-text"),
        "stop" | "info" => Some("muted"),
        _ => None,
    }
}

/// The drawer's filters: (name, label, the kinds it shows).
const FILTERS: [(&str, &str, &[&str]); 5] = [
    ("all", "All", &[]),
    ("error", "Errors", &["error"]),
    ("launch", "Launches", &["launch", "join", "friend", "stop"]),
    ("macro", "Macros", &["macro"]),
    ("update", "Updates", &["update"]),
];

/// One line as a row of the drawer.
pub fn row(entry: &Activity) -> gtk::ListBoxRow {
    let k = kind(&entry.line);
    let b = gtk::Box::new(Orientation::Horizontal, 12).css(&format!("lg-row {k}"));
    b.append(&lbl(&entry.time, "t"));
    let i = ds::icon(icon_for(k));
    if let Some(c) = tone_class(k).filter(|_| k != "error") {
        i.add_css_class(c);
    }
    b.append(&i);
    let text = lbl(&entry.line, "").ellipsize().hexpand();
    text.set_tooltip_text(Some(&entry.line));
    b.append(&text);
    let r = gtk::ListBoxRow::builder().activatable(false).selectable(false).child(&b).build();
    r.set_widget_name(k);
    r
}

pub struct LogDrawer {
    pub root: gtk::Box,
    list: gtk::ListBox,
    count: gtk::Label,
    counts: Vec<(&'static [&'static str], gtk::Label)>,
}

impl LogDrawer {
    pub fn build(w: &crate::ui::window::WeakWindow) -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("lg-list");
        let filter: Rc<RefCell<&'static [&'static str]>> = Rc::new(RefCell::new(&[]));
        {
            let filter = filter.clone();
            list.set_filter_func(move |r| {
                let f = filter.borrow();
                f.is_empty() || f.contains(&r.widget_name().as_str())
            });
        }
        let count = lbl("", "cx-count");
        let title = gtk::Box::new(Orientation::Horizontal, 8);
        title.append(&lbl("Activity", "t-h4"));
        title.append(&count.clone().valign(Align::Center));
        let text = gtk::Box::new(Orientation::Vertical, 0);
        text.append(&title);
        text.append(
            &lbl(
                "What happened this run, newest first. A client's own output is in \
                 ~/.cache/rbxmgr/logs.",
                "t-caption muted",
            )
            .ellipsize(),
        );
        let head = gtk::Box::new(Orientation::Horizontal, 12).css("lg-head");
        head.append(&text.hexpand());
        let seg = gtk::Box::new(Orientation::Horizontal, 0).css("cx-seg");
        seg.set_valign(Align::Center);
        let mut counts = Vec::new();
        let mut first: Option<gtk::ToggleButton> = None;
        for (_, label, kinds) in FILTERS {
            let n = lbl("", "cx-count");
            let inner = gtk::Box::new(Orientation::Horizontal, 6);
            inner.append(&lbl(label, ""));
            inner.append(&n);
            let t = gtk::ToggleButton::builder().child(&inner).build();
            match &first {
                Some(f) => t.set_group(Some(f)),
                None => {
                    t.set_active(true);
                    first = Some(t.clone());
                }
            }
            let (filter, list) = (filter.clone(), list.clone());
            t.connect_toggled(move |t| {
                if t.is_active() {
                    filter.replace(kinds);
                    list.invalidate_filter();
                }
            });
            seg.append(&t);
            counts.push((kinds, n));
        }
        head.append(&seg);
        let copy = ds::Button::new("Copy log", Variant::Secondary, true).on({
            let w = w.clone();
            move || {
                if let Some(w) = w.upgrade() {
                    copy_all(&w);
                }
            }
        });
        copy.button.set_valign(Align::Center);
        head.append(&copy.button);
        let close = ds::ib("x", "Close the activity drawer", true);
        close.set_action_name(Some("win.activity-log"));
        head.append(&close);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .build();
        let root = gtk::Box::new(Orientation::Vertical, 0).css("mn-drawer");
        root.set_size_request(-1, 300);
        root.append(&head);
        root.append(&scroller);
        LogDrawer { root, list, count, counts }
    }

    /// The newest line, at the top; the oldest kept goes.
    pub fn prepend(&self, lines: &[Activity]) {
        if let Some(newest) = lines.first() {
            self.list.prepend(&row(newest));
        }
        if let Some(oldest) = self.list.row_at_index(crate::state::ACTIVITY_KEPT as i32) {
            self.list.remove(&oldest);
        }
        self.recount(lines);
    }

    fn recount(&self, lines: &[Activity]) {
        self.count.set_label(&plural(lines.len(), "line", "lines"));
        for (kinds, label) in &self.counts {
            let n =
                lines.iter().filter(|e| kinds.is_empty() || kinds.contains(&kind(&e.line))).count();
            label.set_label(&n.to_string());
        }
    }
}

/// The whole log on the clipboard, oldest first.
fn copy_all(w: &Window) {
    let all: Vec<String> =
        w.state().activity.iter().rev().map(|e| format!("{}  {}", e.time, e.line)).collect();
    w.gtk_window().clipboard().set_text(&all.join("\n"));
    w.toast("Log copied");
}

/// Open or close the drawer.
pub fn open_log(w: &Window) {
    w.toggle_drawer();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_sorted_into_kinds_by_their_words() {
        for (line, want) in [
            ("alt: FAILED -- 401", "error"),
            ("Roblox is up to date", "update"),
            ("Stopped 2 client(s)", "stop"),
            ("Shut down Fruit farm", "stop"),
            ("alt: launched into s-1", "join"),
            ("Target: join Pal", "friend"),
            ("alt: launched", "launch"),
            ("alt: playing Macro 1", "macro"),
            ("Ready", "info"),
        ] {
            assert_eq!(kind(line), want, "{line}");
        }
    }
}
