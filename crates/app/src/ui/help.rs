//! Help in the inspector: every keyboard shortcut, filtered as you type,
//! and about Roblox Manager.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType};

use super::ds::{self, Variant};
use super::panel::Panel;
use super::widgets::{Fluent, LabelFluent, hotkey_label, lbl};
use super::window::{SHORTCUTS, Window};

/// Which part of the window a shortcut belongs to.
fn section(action: &str) -> &'static str {
    match action {
        "win.add-account" | "win.search" | "win.refresh" => "Accounts",
        "win.launch-group" | "win.launch-selected" | "win.stop-all" | "win.pick-target" => "Launch",
        "win.new-macro" | "win.macro-help" => "Macros",
        _ => "Window",
    }
}

/// The keys of an accelerator, one cap each: "<Control>n" → ["Ctrl", "N"].
fn caps(accel: &str) -> Vec<String> {
    hotkey_label(Some(accel))
        .split('+')
        .filter(|k| !k.is_empty())
        .map(|k| if k == "Return" { "Enter".to_owned() } else { k.to_owned() })
        .collect()
}

/// Open help on its Shortcuts tab, or on About.
pub fn show(w: &Window, about: bool) {
    let panel = Panel::new("Help");
    let n = SHORTCUTS.len() + w.state().macros.hotkeys().count();
    let shortcuts = gtk::ToggleButton::with_label("Shortcuts");
    let about_tab = gtk::ToggleButton::with_label("About");
    about_tab.set_group(Some(&shortcuts));
    let seg = gtk::Box::new(Orientation::Horizontal, 0).css("cx-seg");
    seg.set_valign(Align::Center);
    seg.append(&shortcuts);
    seg.append(&about_tab);
    let title = lbl("Help", "cx-head-title");
    let sub = lbl(&format!("{n} shortcuts"), "cx-head-sub").ellipsize();
    let text = gtk::Box::new(Orientation::Vertical, 0);
    text.set_valign(Align::Center);
    text.append(&title);
    text.append(&sub);
    let head = gtk::Box::new(Orientation::Horizontal, 12).css("cx-head");
    head.append(&text.hexpand());
    head.append(&seg);
    head.append(&panel.close_button());

    let pages = gtk::Stack::builder().vexpand(true).build();
    pages.add_named(&scroll(&shortcuts_page(w)), Some("shortcuts"));
    pages.add_named(&scroll(&about_page()), Some("about"));
    {
        let p = pages.clone();
        shortcuts.connect_toggled(move |t| {
            if t.is_active() {
                p.set_visible_child_name("shortcuts");
            }
        });
        let p = pages.clone();
        about_tab.connect_toggled(move |t| {
            if t.is_active() {
                p.set_visible_child_name("about");
            }
        });
    }
    if about {
        about_tab.set_active(true);
    } else {
        shortcuts.set_active(true);
    }
    let root = gtk::Box::new(Orientation::Vertical, 0).css("cx-panel");
    root.append(&head);
    root.append(&pages);
    panel.set_child(Some(&root));
    panel.present(w);
}

fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build()
}

fn shortcuts_page(w: &Window) -> gtk::Box {
    let filter = gtk::SearchEntry::builder()
        .placeholder_text("Filter shortcuts")
        .css_classes(["search-field"])
        .build();
    let top = ds::sec("", None);
    top.add_css_class("first");
    top.append(&filter);
    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.append(&top);
    let mut rows: Vec<(gtk::Box, String, gtk::Box)> = Vec::new();
    let mut entries: Vec<(&'static str, String, Vec<Vec<String>>)> = SHORTCUTS
        .iter()
        .map(|(action, what, keys)| {
            (section(action), (*what).to_owned(), keys.iter().map(|k| caps(k)).collect())
        })
        .collect();
    for (name, accel) in w.state().macros.hotkeys() {
        entries.push(("Macros", format!("Run or stop {name}"), vec![caps(accel)]));
    }
    for title in ["Accounts", "Launch", "Macros", "Window"] {
        let sec = ds::sec(title, None);
        let mut any = false;
        for (_, what, keys) in entries.iter().filter(|(s, _, _)| *s == title) {
            any = true;
            let row = gtk::Box::new(Orientation::Horizontal, 8);
            row.set_margin_top(4);
            row.set_margin_bottom(4);
            row.append(&lbl(what, "t-body-sm").wrapped().hexpand());
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    row.append(&lbl("or", "t-caption muted").centered());
                }
                let refs: Vec<&str> = k.iter().map(String::as_str).collect();
                row.append(&ds::keys(&refs));
            }
            sec.append(&row);
            rows.push((row, what.to_lowercase(), sec.clone()));
        }
        if any {
            page.append(&sec);
        }
    }
    let rows = Rc::new(rows);
    filter.connect_search_changed(move |e| {
        let q = e.text().trim().to_lowercase();
        for (row, what, _) in rows.iter() {
            row.set_visible(q.is_empty() || what.contains(&q));
        }
        // A section whose shortcuts are all filtered out goes too.
        for (_, _, sec) in rows.iter() {
            let mut shown = false;
            let mut c = sec.first_child().and_then(|h| h.next_sibling());
            while let Some(r) = c {
                shown |= r.is_visible();
                c = r.next_sibling();
            }
            sec.set_visible(shown);
        }
    });
    page
}

fn about_page() -> gtk::Box {
    let logo = gtk::Image::from_icon_name("rm-brand");
    logo.set_pixel_size(64);
    let top = ds::sec("", None);
    top.add_css_class("first");
    let who = gtk::Box::new(Orientation::Horizontal, 16);
    who.append(&logo);
    let text = gtk::Box::new(Orientation::Vertical, 2);
    text.set_valign(Align::Center);
    text.append(&lbl("Roblox Manager", "t-h4"));
    text.append(&lbl(&format!("Version {}", env!("CARGO_PKG_VERSION")), "t-caption muted"));
    who.append(&text);
    top.append(&who);
    top.append(
        &lbl(
            "Several Roblox accounts, launched into one server. Roblox's rules treat running \
             several clients at once as a policy violation, and community reports tie it to \
             anti-cheat flags. The second client only ever starts when you ask for it.",
            "t-body-sm muted",
        )
        .wrapped(),
    );
    let links = gtk::Box::new(Orientation::Horizontal, 8);
    for (text, uri) in [
        ("Website", "https://github.com/DamnShabu/roblox-manager"),
        ("Report a problem", "https://github.com/DamnShabu/roblox-manager/issues"),
    ] {
        let b = ds::Button::with_icons(text, Variant::Secondary, true, None, Some("external"));
        b.button.set_tooltip_text(Some(uri));
        let uri = uri.to_owned();
        b.button.connect_clicked(move |b| {
            let root = b.root().and_downcast::<gtk::Window>();
            gtk::UriLauncher::new(&uri).launch(root.as_ref(), gtk::gio::Cancellable::NONE, |r| {
                if let (Err(e), Some(w)) = (r, crate::ui::window::current()) {
                    w.toast(&format!("Could not open the browser: {e}"));
                }
            });
        });
        links.append(&b.button);
    }
    top.append(&links);
    let legal = ds::sec("Licences", None);
    legal.append(&lbl("Roblox Manager is under the MIT licence.", "t-body-sm").wrapped());
    legal.append(
        &lbl("Each client runs in Stacked, a fork of Cordial, under the GPL 3.0.", "t-body-sm")
            .wrapped(),
    );
    let more =
        ds::Button::new("Credits and full licences", Variant::Ghost, true).action("app.about");
    more.button.set_halign(Align::Start);
    legal.append(&more.button);
    let page = gtk::Box::new(Orientation::Vertical, 0);
    page.append(&top);
    page.append(&legal);
    page
}
