//! The design's icons: 24 px line drawings at a 1.75 stroke, drawn in the
//! text colour like any symbolic icon. Written once a run where the icon
//! theme finds them, and named `rm-NAME-symbolic`.

use std::path::Path;

use gtk::gdk;

/// (name, path data)
const ICONS: &[(&str, &str)] = &[
    (
        "alert",
        "M12 8v5 M12 16.5v.01 M10.3 3.9L2.6 17.2A2 2 0 0 0 4.3 20h15.4a2 2 0 0 0 1.7-2.8L13.7 3.9a2 2 0 0 0-3.4 0z",
    ),
    ("arrow-right", "M5 12h14 M13 6l6 6-6 6"),
    ("check", "M5 12.5l4.5 4.5L19 7.5"),
    ("chev-down", "M6 9l6 6 6-6"),
    ("chev-left", "M15 6l-6 6 6 6"),
    ("chev-right", "M9 6l6 6-6 6"),
    ("chev-up", "M6 15l6-6 6 6"),
    ("clock", "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2"),
    (
        "copy",
        "M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1",
    ),
    ("crosshair", "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 3v4M12 17v4M3 12h4M17 12h4"),
    ("do-arrow", "M9 5v6a3 3 0 0 0 3 3h7M15 10l4 4-4 4"),
    ("exit", "M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 17l-5-5 5-5M5 12h11"),
    ("external", "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"),
    (
        "eye",
        "M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
    ),
    (
        "eye-off",
        "M3 3l18 18M10.6 5.1A9.6 9.6 0 0 1 12 5c6 0 9.5 7 9.5 7a16 16 0 0 1-2.3 3.2M6.6 6.6A16 16 0 0 0 2.5 12S6 19 12 19a9.4 9.4 0 0 0 5.4-1.6M9.9 9.9a3 3 0 0 0 4.2 4.2",
    ),
    (
        "gamepad",
        "M6 11h4M8 9v4M15 12h.01M18 10h.01M17.3 5H6.7a4 4 0 0 0-4 3.6L2 15a3 3 0 0 0 5.2 2.6L8.5 16h7l1.3 1.6A3 3 0 0 0 22 15l-.7-6.4A4 4 0 0 0 17.3 5z",
    ),
    ("grid", "M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z"),
    (
        "grip",
        "M8.2 6a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M14.2 6a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M8.2 12a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M14.2 12a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M8.2 18a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0M14.2 18a.8.8 0 1 0 1.6 0a.8.8 0 1 0-1.6 0",
    ),
    (
        "hold",
        "M8 13V5.5a1.5 1.5 0 0 1 3 0V12M11 11.5v-7a1.5 1.5 0 0 1 3 0V12M14 11.5V6.5a1.5 1.5 0 0 1 3 0V13M17 9.5a1.5 1.5 0 0 1 3 0V15a6 6 0 0 1-6 6h-1.6a6 6 0 0 1-4.6-2.2L4.4 15.6a1.5 1.5 0 0 1 2.3-1.9L8 15",
    ),
    (
        "image",
        "M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM9 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3zM20 15l-5-5L5 20",
    ),
    ("info", "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01"),
    (
        "keyboard",
        "M4 6h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1zM7 10h.01M11 10h.01M15 10h.01M8 14h8",
    ),
    (
        "link",
        "M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7",
    ),
    ("list", "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01"),
    (
        "lock",
        "M6 11h12a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1zM8 11V7a4 4 0 0 1 8 0v4",
    ),
    ("menu", "M4 7h16 M4 12h16 M4 17h16"),
    (
        "more",
        "M11 5a1 1 0 1 0 2 0a1 1 0 1 0-2 0M11 12a1 1 0 1 0 2 0a1 1 0 1 0-2 0M11 19a1 1 0 1 0 2 0a1 1 0 1 0-2 0",
    ),
    ("mouse", "M12 3a6 6 0 0 1 6 6v6a6 6 0 0 1-12 0V9a6 6 0 0 1 6-6zM12 7v3"),
    ("move", "M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"),
    ("note", "M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5M9 13h6M9 17h4"),
    (
        "offline",
        "M2 8.8a15 15 0 0 1 4.2-2.7M10.7 5.1A15 15 0 0 1 22 8.8M5 12.9a10 10 0 0 1 5.2-2.7M16.8 11.2a10 10 0 0 1 2.2 1.7M8.5 16.4a5 5 0 0 1 7 0M12 20h.01M3 3l18 18",
    ),
    ("panel", "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zM15 4v16"),
    ("pencil", "M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16v4zM13.5 6.5l4 4"),
    ("play", "M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.3-7.2a.8.8 0 0 0 0-1.4L8.2 4.1A.8.8 0 0 0 7 4.8z"),
    ("plus", "M12 5v14 M5 12h14"),
    (
        "refresh",
        "M20 11a8 8 0 0 0-14.6-4.5L4 8 M4 4v4h4 M4 13a8 8 0 0 0 14.6 4.5L20 16 M20 20v-4h-4",
    ),
    ("repeat", "M17 2l4 4-4 4M3 11v-1a4 4 0 0 1 4-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 0 1-4 4H3"),
    ("search", "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4"),
    (
        "server",
        "M5 4h14a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM5 14h14a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-4a1 1 0 0 1 1-1zM8 7h.01M8 17h.01",
    ),
    (
        "settings",
        "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
    ),
    ("star", "M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"),
    ("start", "M5 4v16M9 6l10 6-10 6z"),
    ("stop", "M7 6h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z"),
    ("timeline", "M3 6h8M7 12h12M5 18h9"),
    ("trash", "M4 7h16 M10 11v6 M14 11v6 M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12 M9 7V4h6v3"),
    ("type", "M4 7V5h16v2M12 5v14M9 19h6"),
    ("user", "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1"),
    (
        "users",
        "M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM2 21v-1a6 6 0 0 1 12 0v1M16 3.1a4 4 0 0 1 0 7.8M22 21v-1a6 6 0 0 0-4-5.7",
    ),
    ("x", "M6 6l12 12M18 6L6 18"),
];

/// Icons drawn filled as well as outlined: the leader's star, a play.
const FILLED: &[&str] = &["star", "play"];

/// The app's own mark, in colour.
const BRAND: &str = include_str!("../../../../packaging/icons/roblox-manager.svg");

fn svg(d: &str, filled: bool) -> String {
    let class = if filled { "foreground-stroke" } else { "transparent-fill foreground-stroke" };
    let fill = if filled { "#000" } else { "none" };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\">\
         <path class=\"{class}\" fill=\"{fill}\" stroke=\"#000\" stroke-width=\"1.75\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\" d=\"{d}\"/></svg>"
    )
}

/// Write the icons under `dir` and have the icon theme look there. A
/// failure leaves the theme's own icons, so it is said and not fatal.
pub fn install(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, d) in ICONS {
        std::fs::write(dir.join(format!("rm-{name}-symbolic.svg")), svg(d, false))?;
        if FILLED.contains(name) {
            std::fs::write(dir.join(format!("rm-{name}-filled-symbolic.svg")), svg(d, true))?;
        }
    }
    std::fs::write(dir.join("rm-brand.svg"), BRAND)?;
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display).add_search_path(dir);
    }
    Ok(())
}

/// The icon theme name of a design icon.
pub fn name(icon: &str) -> String {
    format!("rm-{icon}-symbolic")
}
