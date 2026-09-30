//! The activity log: each line with an icon for its kind -- the latest in
//! the side pane, all of them in a window of their own.

use adw::prelude::*;
use gtk::{Align, PolicyType};

use super::widgets::{Btn, Fluent, LabelFluent, boxed_list, icon, lbl};
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

/// The icon for a kind.
pub fn icon_for(kind: &str) -> &'static str {
    match kind {
        "launch" => "media-playback-start-symbolic",
        "stop" => "media-playback-stop-symbolic",
        "macro" => "input-keyboard-symbolic",
        "update" => "software-update-available-symbolic",
        "friend" => "avatar-default-symbolic",
        "error" => "dialog-warning-symbolic",
        "join" => "insert-link-symbolic",
        _ => "dialog-information-symbolic",
    }
}

/// One line: its icon, its text, its time. `full` wraps long lines rather
/// than cutting them.
pub fn line(entry: &Activity, full: bool) -> gtk::Box {
    let k = kind(&entry.line);
    let text = lbl(&entry.line, "");
    if full {
        text.set_selectable(true);
        text.set_focusable(false);
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    } else {
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.set_tooltip_text(Some(&entry.line));
    }
    let glyph = icon(icon_for(k)).css(k).valign(Align::Start);
    glyph.set_margin_top(2);
    hbox!(
        10,
        "activity-line",
        glyph,
        text.hexpand(),
        lbl(&entry.time, "time caption dimmed").valign(Align::Start).top(2)
    )
}

/// A line as a row of the log window.
pub fn row(entry: &Activity) -> gtk::ListBoxRow {
    let r = gtk::ListBoxRow::builder().activatable(false).selectable(false).build();
    r.set_child(Some(&line(entry, true).margins(10)));
    r
}

/// Every line kept this run, newest first, with a button to copy them all.
pub fn open_log(w: &Window) {
    let list = boxed_list();
    for entry in &w.state().activity {
        list.append(&row(entry));
    }
    w.watch_log(&list);
    let weak = w.weak();
    let copy =
        Btn::new("flat").icon("edit-copy-symbolic").tip("Copy the whole log").build(move || {
            if let Some(w) = weak.upgrade() {
                let all: Vec<String> = w
                    .state()
                    .activity
                    .iter()
                    .rev()
                    .map(|e| format!("{}  {}", e.time, e.line))
                    .collect();
                w.gtk_window().clipboard().set_text(&all.join("\n"));
                w.toast("Log copied");
            }
        });
    let header = adw::HeaderBar::new();
    header.pack_start(&copy.button);
    let page = vbox!(
        12,
        "",
        lbl(
            "What happened this run, newest first. A client's own output is in \
             ~/.cache/rbxmgr/logs.",
            "caption dimmed"
        )
        .wrapped(),
        list
    )
    .margins(18);
    page.set_margin_top(6);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&adw::Clamp::builder().maximum_size(720).child(&page).build())
            .hscrollbar_policy(PolicyType::Never)
            .build(),
    ));
    adw::Dialog::builder()
        .title("Activity Log")
        .child(&view)
        .content_width(640)
        .content_height(640)
        .build()
        .present(Some(w.gtk_window()));
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
