//! The design system's pieces as GTK widgets: its Button and IconButton,
//! Badge, avatar and game art, sections and field rows, the segmented
//! control. Their look is in style.css under the same names.

use std::path::Path;

use adw::prelude::*;
use gtk::{Align, Orientation};

use super::icons;
use super::widgets::{Fluent, LabelFluent, classes, lbl, name, texture};

/// A design icon at the size its CSS class sets.
pub fn icon(name: &str) -> gtk::Image {
    gtk::Image::from_icon_name(&icons::name(name))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

impl Variant {
    fn css(self) -> &'static str {
        match self {
            Variant::Primary => "primary",
            Variant::Secondary => "secondary",
            Variant::Ghost => "ghost",
            Variant::Danger => "danger",
        }
    }
}

/// The system's Button: a label, and an icon either side. Kept so its
/// words and look can change as the state does.
#[derive(Clone)]
pub struct Button {
    pub button: gtk::Button,
    label: gtk::Label,
}

impl Button {
    pub fn new(text: &str, variant: Variant, small: bool) -> Self {
        Self::with_icons(text, variant, small, None, None)
    }

    pub fn with_icons(
        text: &str,
        variant: Variant,
        small: bool,
        left: Option<&str>,
        right: Option<&str>,
    ) -> Self {
        let button = gtk::Button::new();
        classes(&button, &format!("ds {}", variant.css()));
        if small {
            button.add_css_class("sm");
        }
        let label = gtk::Label::new(Some(text));
        label.set_use_underline(true);
        let inner = gtk::Box::new(Orientation::Horizontal, 6);
        inner.set_halign(Align::Center);
        if let Some(l) = left {
            inner.append(&icon(l));
        }
        inner.append(&label);
        if let Some(r) = right {
            inner.append(&icon(r));
        }
        button.set_child(Some(&inner));
        Button { button, label }
    }

    pub fn on(self, f: impl Fn() + 'static) -> Self {
        self.button.connect_clicked(move |_| f());
        self
    }

    pub fn tip(self, t: &str) -> Self {
        self.button.set_tooltip_text(Some(t));
        self
    }

    pub fn action(self, a: &str) -> Self {
        self.button.set_action_name(Some(a));
        self
    }

    pub fn set_text(&self, t: &str) {
        self.label.set_label(t);
    }
}

/// The system's IconButton: a design icon, named for screen readers by its
/// tip.
pub fn ib(icon_name: &str, tip: &str, small: bool) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&icon(icon_name)));
    b.add_css_class("ib");
    if small {
        b.add_css_class("sm");
    }
    b.set_valign(Align::Center);
    b.set_tooltip_text(Some(tip));
    name(&b, tip);
    b
}

/// Swap an icon button's icon.
pub fn set_ib(b: &gtk::Button, icon_name: &str, tip: &str) {
    b.set_child(Some(&icon(icon_name)));
    b.set_tooltip_text(Some(tip));
    name(b, tip);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Danger,
    Accent,
}

/// The system's Badge: a word on a pill, with a dot that may blink.
pub fn badge(tone: Tone, text: &str, dot: bool, live: bool) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Horizontal, 6);
    b.add_css_class("badge");
    b.add_css_class(match tone {
        Tone::Neutral => "neutral",
        Tone::Success => "success",
        Tone::Warning => "warning",
        Tone::Danger => "danger",
        Tone::Accent => "accent",
    });
    b.set_valign(Align::Center);
    b.set_halign(Align::Start);
    if dot {
        let d = gtk::Box::new(Orientation::Horizontal, 0);
        d.add_css_class("dot");
        if live {
            d.add_css_class("live");
        }
        d.set_valign(Align::Center);
        b.append(&d);
    }
    b.append(&gtk::Label::new(Some(text)));
    b
}

/// An account's avatar: its headshot when one is cached, else its first
/// letter on a tone picked from its name.
pub fn av(label: &str, picture: Option<&Path>, px: i32) -> gtk::Widget {
    let frame = gtk::Box::new(Orientation::Horizontal, 0);
    frame.add_css_class("av");
    frame.set_size_request(px, px);
    frame.set_valign(Align::Center);
    frame.set_halign(Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    match picture.and_then(texture) {
        Some(t) => {
            frame.append(&gtk::Image::builder().paintable(&t).pixel_size(px).build());
        }
        None => {
            let tone = ["", "t1", "t2", "t3"][label.bytes().map(usize::from).sum::<usize>() % 4];
            if !tone.is_empty() {
                frame.add_css_class(tone);
            }
            if px >= 48 {
                frame.add_css_class("lg");
            }
            let first: String = label.chars().take(1).flat_map(char::to_uppercase).collect();
            frame.set_homogeneous(true);
            frame.append(&gtk::Label::new(Some(&first)));
        }
    }
    frame.upcast()
}

/// A game's art: its icon when cached, else two letters of its name.
pub fn art(name: &str, icon_path: Option<&Path>, px: i32) -> gtk::Widget {
    let frame = gtk::Box::new(Orientation::Horizontal, 0);
    frame.add_css_class("art");
    frame.set_size_request(px, px);
    frame.set_valign(Align::Center);
    frame.set_halign(Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    match icon_path.and_then(texture) {
        Some(t) => frame.append(&gtk::Image::builder().paintable(&t).pixel_size(px).build()),
        None => {
            let tone = ["n", "s", "w"][name.bytes().map(usize::from).sum::<usize>() % 3];
            frame.add_css_class(tone);
            if px >= 48 {
                frame.add_css_class("lg");
            }
            frame.set_homogeneous(true);
            frame.append(&gtk::Label::new(Some(&monogram(name))));
        }
    }
    frame.upcast()
}

/// A design icon in a game art's frame, for targets that are not a game.
pub fn art_icon(icon_name: &str, px: i32) -> gtk::Widget {
    let frame = gtk::CenterBox::new();
    frame.add_css_class("art");
    frame.set_size_request(px, px);
    frame.set_valign(Align::Center);
    frame.set_center_widget(Some(&icon(icon_name).css("muted s16")));
    frame.upcast()
}

/// "Grand Piece Online" → "GP"; "DOORS" → "DO".
pub fn monogram(name: &str) -> String {
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| w.chars().next().is_some_and(char::is_alphanumeric))
        .collect();
    let letters: String = if words.len() >= 2 {
        words.iter().take(2).filter_map(|w| w.chars().next()).collect()
    } else {
        name.chars().filter(|c| c.is_alphanumeric()).take(2).collect()
    };
    letters.to_uppercase()
}

/// An overline: the small upper-case label over a section.
pub fn overline(text: &str) -> gtk::Label {
    lbl(&text.to_uppercase(), "t-overline")
}

/// A section of a panel: its overline, anything on its right, then rows.
pub fn sec(title: &str, right: Option<&gtk::Widget>) -> gtk::Box {
    let s = gtk::Box::new(Orientation::Vertical, 8);
    s.add_css_class("cx-sec");
    if !title.is_empty() || right.is_some() {
        let h = gtk::Box::new(Orientation::Horizontal, 8);
        h.add_css_class("cx-sec-h");
        h.append(&overline(title).hexpand());
        if let Some(r) = right {
            h.append(r);
        }
        s.append(&h);
    }
    s
}

/// A field row: its label on the left, its control, and help under it.
pub fn field(label: &str, control: &impl IsA<gtk::Widget>, help: Option<&str>) -> gtk::Grid {
    let g = gtk::Grid::builder().column_spacing(12).row_spacing(4).build();
    g.add_css_class("cx-field");
    let l = lbl(label, "cx-lab").wrapped();
    l.set_size_request(96, -1);
    l.set_valign(Align::Center);
    g.attach(&l, 0, 0, 1, 1);
    control.set_hexpand(true);
    g.attach(control, 1, 0, 1, 1);
    if let Some(h) = help {
        g.attach(&lbl(h, "cx-help").wrapped(), 1, 1, 1, 1);
    }
    g
}

/// The compact segmented control: one of `options` (name, label) chosen.
pub fn seg(options: &[(&str, &str)], active: &str, on_pick: impl Fn(&str) + 'static) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Horizontal, 0);
    b.add_css_class("cx-seg");
    b.set_halign(Align::Start);
    b.set_valign(Align::Center);
    let on_pick = std::rc::Rc::new(on_pick);
    let mut first: Option<gtk::ToggleButton> = None;
    for (key, text) in options {
        let t = gtk::ToggleButton::with_label(text);
        if let Some(f) = &first {
            t.set_group(Some(f));
        } else {
            first = Some(t.clone());
        }
        t.set_active(*key == active);
        let (key, on_pick) = (key.to_string(), on_pick.clone());
        t.connect_toggled(move |t| {
            if t.is_active() {
                on_pick(&key);
            }
        });
        b.append(&t);
    }
    b
}

/// A check box at the system's 16 px.
pub fn check(active: bool, tip: &str) -> gtk::CheckButton {
    let c = gtk::CheckButton::builder().active(active).valign(Align::Center).build();
    c.set_tooltip_text(Some(tip));
    name(&c, tip);
    c
}

/// A panel's head: art on the left, a title and a line under it, then
/// `tail` (buttons) and the panel's close.
pub fn head(
    art: &gtk::Widget,
    title: &gtk::Widget,
    sub: &gtk::Widget,
    tail: &[gtk::Widget],
) -> gtk::Box {
    let h = gtk::Box::new(Orientation::Horizontal, 12);
    h.add_css_class("cx-head");
    h.append(art);
    let text = gtk::Box::new(Orientation::Vertical, 0);
    text.set_valign(Align::Center);
    text.set_hexpand(true);
    text.append(title);
    text.append(sub);
    h.append(&text);
    for t in tail {
        h.append(t);
    }
    h
}

/// An inline notice: an icon and words, tinted by `tone`.
pub fn note(tone: Tone, icon_name: &str, text: &str) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Horizontal, 8);
    b.add_css_class("cx-note");
    b.add_css_class(match tone {
        Tone::Danger => "danger",
        Tone::Warning => "warning",
        Tone::Success => "success",
        Tone::Neutral | Tone::Accent => "neutral",
    });
    let i = icon(icon_name).css("s16");
    i.set_valign(Align::Start);
    b.append(&i);
    b.append(&lbl(text, "").wrapped().hexpand());
    b
}

/// Keys as keycaps: ["Ctrl", "F7"].
pub fn keys(keys: &[&str]) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Horizontal, 4);
    b.set_valign(Align::Center);
    for k in keys {
        b.append(&lbl(k, "cx-kbd").xalign(0.5));
    }
    b
}

/// A field stacked for the inspector's width: its label, the control, and
/// help under it.
pub fn sfield(label: &str, control: &impl IsA<gtk::Widget>, help: Option<&str>) -> gtk::Box {
    let b = gtk::Box::new(Orientation::Vertical, 6);
    b.append(&lbl(label, "t-label"));
    b.append(control);
    if let Some(h) = help {
        b.append(&lbl(h, "cx-help").wrapped());
    }
    b
}

/// "From the next launch": a setting the running client does not take.
pub fn next_launch() -> gtk::Box {
    let b = gtk::Box::new(Orientation::Horizontal, 4);
    b.append(&icon("clock").css("warning-text s14"));
    b.append(&lbl("From the next launch", "t-caption"));
    b
}

/// A drop-down of `options`, `at` chosen, in the system's select look.
pub fn select(options: &[&str], at: usize, on_pick: impl Fn(usize) + 'static) -> gtk::DropDown {
    let d = gtk::DropDown::from_strings(options);
    d.add_css_class("cx-select");
    d.set_selected(at as u32);
    d.connect_selected_notify(move |d| on_pick(d.selected() as usize));
    d
}

/// A one-line text input in the system's look.
pub fn input(text: &str) -> gtk::Entry {
    let e = gtk::Entry::builder().text(text).build();
    e.add_css_class("cx-input");
    e
}
