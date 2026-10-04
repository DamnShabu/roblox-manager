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
         mouse are never used, they are read only while you record in that one window, and \
         nothing touches the game itself.",
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
    Heading("Recording"),
    Text(
        "Press <b>Record</b> in a macro's editor, then <b>F8</b> in a running macro-ready \
         client's window, and play. Everything that window receives — keys pressed and held, \
         clicks, mouse movement and camera turns, scrolling, and the pauses between — is \
         recorded until you press <b>F8</b> there again. It is added to the editor as one \
         <b>timeline</b>, to look over before you <b>Save</b>: a lane for each key and \
         button, held exactly as long as you held it, and one each for the pointer, the \
         camera and the wheel. Hover a bar to see its step; edit them as text.\n\n\
         Camera turns are replayed as the raw mouse movement the game turned by, so they \
         turn as far as they did. A game never plays out exactly the same twice, though: \
         start each run from where the recording started, and expect small drift over \
         long recordings.\n\n\
         F8 never reaches the game while a recording is armed. Only that window is heard, \
         and only until the second F8. A client launched before recording existed has to be \
         launched again first.",
    ),
    Heading("When something shows"),
    Text(
        "A <b>When</b> step plays the <b>Do</b> steps under it the moment something shows in \
         the client, wherever the macro is in its own steps. Press the image button on a When \
         row and drag over what to wait for in a running client; that area is kept as an \
         image, and the macro looks for it twenty times a second. Its own steps pause while \
         a When plays and carry on after, keys they hold still down. A When plays once each \
         time what it waits for appears, and keeps seeing with the window hidden.",
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
        ("Press KEY", "put a key down and leave it down, while other steps play"),
        ("Release KEY", "let go of a key a Press put down"),
        (
            "Repeat KEY SECONDS [EVERY]",
            "tap a key every EVERY seconds (0.1–0.2 unless you give one) for SECONDS, while \
             other steps play; the round ends once it has",
        ),
        ("Type TEXT", "type text, e.g. into chat"),
        (
            "Click [left|right|middle] [X Y]",
            "click, optionally at a point you pick in a running client",
        ),
        ("Move DX DY", "move the mouse by an amount"),
        ("Move to X Y [SECONDS]", "put the mouse at a point, or glide it there"),
        ("Scroll up|down|left|right [N]", "turn the wheel N notches"),
        ("Wait SECONDS", "pause"),
        ("Start SECONDS", "pause once, before the first round only"),
        ("Note TEXT", "a reminder; does nothing"),
        (
            "Timeline [SECONDS]",
            "play the At steps under it, each at its own time, over one another; it lasts \
             SECONDS, or until its last step ends",
        ),
        (
            "At SECONDS STEP",
            "a Key, Hold, Press, Release, Click, Move, Scroll, Path or Turn, SECONDS after the \
             timeline starts",
        ),
        ("Path T X Y, T X Y, …", "glide the mouse through points, T seconds in"),
        (
            "Turn T DX DY, T DX DY, …",
            "move the mouse raw, as a game turns its camera: DX DY all told by T seconds in",
        ),
        (
            "When [not] image NAME X Y [90%]",
            "play the Do steps under it the moment a picked image shows at X Y (within a few \
             pixels), or with not, the moment it goes",
        ),
        (
            "When [not] color X Y #RRGGBB [24]",
            "play the Do steps under it the moment the pixel at X Y turns that colour, or stops \
             being it",
        ),
        ("Do STEP", "a step a When plays: any but Repeat, Start, Stagger or Timeline"),
    ]),
    Text(
        "<b>Playback</b> plays the steps once, a set number of rounds, or until stopped.\n\n\
         Keys are letters, digits and punctuation, or names such as <tt>space</tt>, \
         <tt>enter</tt>, <tt>esc</tt>, <tt>tab</tt>, <tt>shift</tt>, <tt>ctrl</tt>, \
         <tt>alt</tt>, <tt>up</tt>, <tt>down</tt>, <tt>left</tt>, <tt>right</tt>, \
         <tt>F1</tt>. Combine them with +: <tt>Hold shift+w 2</tt>. The mouse buttons are \
         keys too: <tt>mouse1</tt> (left), <tt>mouse2</tt> (right), <tt>mouse3</tt> \
         (middle) — <tt>Hold mouse1 2</tt>. Keys and typed text follow a US keyboard \
         layout.\n\n\
         Whatever a macro presses is let go of when each round ends, and when it stops.",
    ),
    Heading("Randomness"),
    Text(
        "Any SECONDS can be a range — <tt>Wait 60-240</tt> — picked afresh every time. Every \
         key press is held for a random moment too (0.04–0.12 s for a tap unless you give \
         one), and typed characters are 0.05–0.16 s apart. The accounts one Run plays it on \
         all pick the same, so they play alike; the next Run picks anew.",
    ),
    Heading("Example"),
    Code("Start  45\nWait   60-70\nKey    j\nWait   340-341\n\nPlayback: until stopped"),
    Heading("Good to know"),
    Text(
        "A macro keeps playing with its window on another workspace, scrolled out of view, \
         behind others or hidden with the account's hide button; launch the client again if \
         it was started before this version. Stop lets go of any held key at once. Points are \
         the client window's own, from its corner: \
         keep the window the size it was when a macro was recorded or its points were picked. \
         Roblox games have their own rules on macros — AFK use can be against them.",
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
