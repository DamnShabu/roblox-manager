//! `mimeapps.list`: which application the desktop opens a type (here, a link
//! scheme) with. Edited in place: every line that is not ours stays as it was.

const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";

/// The application `mime` opens with, if the list names one.
pub(super) fn default_for(text: &str, mime: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = header(line) {
            section = name;
        } else if section == DEFAULTS {
            if let Some(ids) = value(line, mime) {
                return ids.split(';').map(str::trim).find(|id| !id.is_empty()).map(str::to_owned);
            }
        }
    }
    None
}

/// The list with `id` as the default for `mime`, and first among its
/// associations.
pub(super) fn set_default(text: &str, mime: &str, id: &str) -> String {
    let text = set(text, DEFAULTS, mime, &format!("{id};"));
    let added = associations(&text, mime);
    let others = added.iter().filter(|a| *a != id).map(|a| format!("{a};"));
    let listed: String = [format!("{id};")].into_iter().chain(others).collect();
    set(&text, ADDED, mime, &listed)
}

/// The ids listed under [Added Associations] for `mime`.
fn associations(text: &str, mime: &str) -> Vec<String> {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = header(line) {
            section = name;
        } else if section == ADDED {
            if let Some(ids) = value(line, mime) {
                return ids
                    .split(';')
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
        }
    }
    Vec::new()
}

/// `text` with `key=value` in `[section]`: replacing the key's line where
/// it has one, else at the end of the section, else in a new section.
fn set(text: &str, section: &str, key: &str, value: &str) -> String {
    let entry = format!("{key}={value}");
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = header(trimmed) {
            if current == section && !done {
                // Before the blank lines that end the section.
                let at = out.len() - out.iter().rev().take_while(|l| l.trim().is_empty()).count();
                out.insert(at, entry.clone());
                done = true;
            }
            name.clone_into(&mut current);
        } else if current == section && !done && self::value(trimmed, key).is_some() {
            out.push(entry.clone());
            done = true;
            continue;
        } else if current == section && done && self::value(trimmed, key).is_some() {
            // A second line for the key would contradict the first.
            continue;
        }
        out.push(line.to_owned());
    }
    if !done {
        if current != section {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("[{section}]"));
        }
        out.push(entry);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

fn header(line: &str) -> Option<&str> {
    line.strip_prefix('[').and_then(|l| l.strip_suffix(']')).map(str::trim)
}

/// The value of `line` when it sets `key`.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once('=')?;
    (k.trim() == key).then(|| v.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIME: &str = "x-scheme-handler/roblox-player";

    #[test]
    fn an_empty_list_gets_both_sections() {
        let got = set_default("", MIME, "me.desktop");
        assert_eq!(
            got,
            "[Default Applications]\nx-scheme-handler/roblox-player=me.desktop;\n\n\
             [Added Associations]\nx-scheme-handler/roblox-player=me.desktop;\n"
        );
        assert_eq!(default_for(&got, MIME).as_deref(), Some("me.desktop"));
    }

    #[test]
    fn another_default_is_replaced_and_everything_else_kept() {
        let before = "\
# mine
[Default Applications]
text/html=firefox.desktop
x-scheme-handler/roblox-player=org.vinegarhq.Sober.desktop

[Added Associations]
x-scheme-handler/roblox-player=org.vinegarhq.Sober.desktop;me.desktop;
image/png=eog.desktop;
";
        assert_eq!(default_for(before, MIME).as_deref(), Some("org.vinegarhq.Sober.desktop"));
        let got = set_default(before, MIME, "me.desktop");
        assert_eq!(
            got,
            "\
# mine
[Default Applications]
text/html=firefox.desktop
x-scheme-handler/roblox-player=me.desktop;

[Added Associations]
x-scheme-handler/roblox-player=me.desktop;org.vinegarhq.Sober.desktop;
image/png=eog.desktop;
"
        );
    }

    #[test]
    fn a_missing_key_goes_at_the_end_of_its_section() {
        let before = "[Default Applications]\ntext/html=firefox.desktop\n\n[Other]\na=b\n";
        let got = set(before, DEFAULTS, MIME, "me.desktop;");
        assert_eq!(
            got,
            "[Default Applications]\ntext/html=firefox.desktop\n\
             x-scheme-handler/roblox-player=me.desktop;\n\n[Other]\na=b\n"
        );
    }

    #[test]
    fn a_repeated_key_is_left_once() {
        let before =
            "[Default Applications]\nx-scheme-handler/roblox=a;\nx-scheme-handler/roblox=b;\n";
        let got = set(before, DEFAULTS, "x-scheme-handler/roblox", "me.desktop;");
        assert_eq!(got, "[Default Applications]\nx-scheme-handler/roblox=me.desktop;\n");
    }

    #[test]
    fn a_default_in_another_section_does_not_count() {
        let text = "[Added Associations]\nx-scheme-handler/roblox-player=a.desktop;\n";
        assert_eq!(default_for(text, MIME), None);
    }
}
