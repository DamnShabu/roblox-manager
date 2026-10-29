//! How a macro step is drawn wherever steps are listed: the editor, and the
//! details pane's Macro steps.

/// The icon for an editor step type.
pub fn step_icon(kind: &str) -> &'static str {
    match kind {
        "Key" | "Press" | "Release" => "keyboard",
        "Hold" => "hold",
        "Repeat" => "repeat",
        "Type" => "type",
        "Click" | "Scroll" | "Path" | "Turn" => "mouse",
        "Move" => "move",
        "Wait" => "clock",
        "Start" => "start",
        "Stagger" => "users",
        "Note" => "note",
        "Timeline" | "At" => "timeline",
        "When" => "image",
        "Do" => "do-arrow",
        "Exit" => "exit",
        _ => "info",
    }
}
