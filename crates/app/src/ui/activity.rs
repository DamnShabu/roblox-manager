//! The activity list: the latest lines, each with an icon for its kind.

/// A line's kind, worked out from its words: log lines have dozens of
/// callers, most in the core, so none of them passes one.
pub fn kind(line: &str) -> &'static str {
    let m = line.to_lowercase();
    let kinds: [(&str, &[&str]); 7] = [
        ("error", &["failed", "could not", "expired", "error", "cannot"]),
        ("update", &["up to date"]),
        ("stop", &["stopped", "removed", "was not running"]),
        ("join", &["joined", " into ", "in server"]),
        ("friend", &["join ", "joining"]),
        ("launch", &["launch"]),
        ("macro", &["macro", "playing", "round ", "saved"]),
    ];
    kinds.iter().find(|(_, words)| words.iter().any(|w| m.contains(w))).map_or("info", |(k, _)| k)
}

/// The icon for a kind.
pub fn icon(kind: &str) -> &'static str {
    match kind {
        "launch" => "rocket_launch",
        "stop" => "stop_circle",
        "macro" => "bolt",
        "update" => "download_done",
        "friend" => "person_search",
        "error" => "error",
        "join" => "link",
        _ => "info",
    }
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
