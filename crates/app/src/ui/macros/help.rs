//! "How macros work": what a macro is, why a macro-ready window, and every
//! step it can take.

use adw::prelude::*;
use gtk::{Align, PolicyType};

use crate::ui::widgets::{Fluent, LabelFluent, lbl};
use crate::ui::window::Window;

enum Block {
    Heading(&'static str),
    /// Pango markup.
    Text(&'static str),
    /// Shown as typed, in a box.
    Code(&'static str),
    /// Each step: how it is written, and what it does.
    Steps(&'static [(&'static str, &'static str)]),
}

use Block::{Code, Heading, Steps, Text};

const PAGE: &[Block] = &[
    Text(
        "A macro presses keys and clicks for one account, on its own, for as long as you let \
         it — while you use other windows, or step away.",
    ),
    Heading("Why a macro-ready window"),
    Text(
        "Roblox ignores the keyboard whenever its window is not the focused one. A \
         macro-ready account's client runs on a small display of its own, inside a normal \
         window on your desktop, where it always has focus. The macro types into that \
         display with emulated input — a virtual keyboard and mouse. Your real keyboard and \
         mouse are never used or read, and nothing touches the game itself.",
    ),
    Heading("Using it"),
    Text(
        "1. Make a macro: <b>New Macro</b>, add its steps, then <b>Save</b>.\n\
         2. In the account's <b>Settings</b>, pick the macro. That turns on \
         <i>Macro-Ready Window</i>.\n\
         3. Launch the account (again, if it was already running).\n\
         4. Press <b>Run</b> beside the macro in its settings; the same button stops it.\n\n\
         Or press <b>Run</b> on a macro in the side pane to play it on every selected account \
         at once; that turns on their macro-ready windows too. A macro switched off cannot run.",
    ),
    Heading("Hotkeys"),
    Text(
        "A macro's hotkey runs it on the selected accounts, or stops it, while this window has \
         focus. To use it from anywhere, bind a key in your desktop's keyboard settings to:",
    ),
    Code("gapplication action io.github.mujo.RobloxManager run-macro \"'Macro 1'\""),
    Heading("Steps"),
    Steps(&[
        ("Key KEY [SECONDS]", "press and release a key"),
        ("Hold KEY SECONDS", "keep a key down"),
        ("Type TEXT", "type text, e.g. into chat"),
        (
            "Click [left|right|middle] [X Y]",
            "click, optionally at a point you pick in a running client",
        ),
        ("Move DX DY", "move the mouse by an amount"),
        ("Wait SECONDS", "pause"),
        ("Start SECONDS", "pause once, before the first round only"),
        ("Note TEXT", "a reminder; does nothing"),
    ]),
    Text(
        "<b>Repeat</b> plays the steps once, a set number of rounds, or until stopped.\n\n\
         Keys are letters, digits and punctuation, or names such as <tt>space</tt>, \
         <tt>enter</tt>, <tt>esc</tt>, <tt>tab</tt>, <tt>shift</tt>, <tt>ctrl</tt>, \
         <tt>alt</tt>, <tt>up</tt>, <tt>down</tt>, <tt>left</tt>, <tt>right</tt>, \
         <tt>F1</tt>. Combine them with +: <tt>Hold shift+w 2</tt>. Keys and typed text \
         follow a US keyboard layout.",
    ),
    Heading("Randomness"),
    Text(
        "Any SECONDS can be a range — <tt>Wait 60-240</tt> — picked afresh every time. Every \
         key press is held for a random moment too (0.04–0.12 s for a tap unless you give \
         one), and typed characters are 0.05–0.16 s apart.",
    ),
    Heading("Example"),
    Code("Start  45\nWait   60-70\nKey    j\nWait   340-341\n\nRepeat: until stopped"),
    Heading("Good to know"),
    Text(
        "Keep the window on a workspace you can see; on a hidden one the game can stall. Stop \
         lets go of any held key at once. Roblox games have their own rules on macros — AFK \
         use can be against them.",
    ),
];

pub fn show(w: &Window) {
    let page = vbox!(10, "").margins(24);
    page.set_margin_top(6);
    for block in PAGE {
        page.append(&draw(block));
    }
    let clamp = adw::Clamp::builder().maximum_size(620).child(&page).build();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(PolicyType::Never)
            .propagate_natural_height(true)
            .build(),
    ));
    adw::Dialog::builder()
        .title("How Macros Work")
        .child(&view)
        .content_width(600)
        .content_height(720)
        .build()
        .present(Some(w.gtk_window()));
}

fn draw(block: &Block) -> gtk::Widget {
    match block {
        Heading(text) => lbl(text, "title-4").top(12).upcast(),
        Text(markup) => {
            let l = lbl("", "body").wrapped();
            l.set_markup(markup);
            l.upcast()
        }
        Code(text) => {
            // Selectable to copy, but not focused on opening, which would
            // select it all.
            let l = lbl(text, "monospace").selectable().wrapped();
            l.set_focusable(false);
            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("card");
            frame.append(&l.margins(12));
            frame.upcast()
        }
        Steps(steps) => {
            let grid = gtk::Grid::builder().row_spacing(6).column_spacing(18).build();
            for (i, (form, what)) in steps.iter().enumerate() {
                let row = i as i32;
                grid.attach(&lbl(form, "monospace").valign(Align::Start), 0, row, 1, 1);
                grid.attach(&lbl(what, "dimmed").wrapped().valign(Align::Start), 1, row, 1, 1);
            }
            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("card");
            frame.append(&grid.margins(12));
            frame.upcast()
        }
    }
}
