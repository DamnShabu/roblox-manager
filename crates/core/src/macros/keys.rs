//! Linux evdev key codes, laid out as a US keyboard. These are what reach the
//! game: Roblox's in-game key path takes the raw evdev code, not the keysym.

/// The keyboard rows and the code of each row's first key.
const ROWS: [(u16, &str); 4] =
    [(2, "1234567890-="), (16, "qwertyuiop[]"), (30, "asdfghjkl;'`"), (43, "\\zxcvbnm,./")];

/// Shifted characters and the unshifted key that types them.
const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    (':', ';'),
    ('"', '\''),
    ('~', '`'),
    ('|', '\\'),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
];

pub const SHIFT: u16 = 42;
pub const SPACE: u16 = 57;

/// Keys by name, in the order names are preferred when a code has several.
const NAMED: &[(&str, u16)] = &[
    ("esc", 1),
    ("escape", 1),
    ("backspace", 14),
    ("tab", 15),
    ("enter", 28),
    ("return", 28),
    ("ctrl", 29),
    ("control_l", 29),
    ("shift", SHIFT),
    ("shift_l", SHIFT),
    ("shift_r", 54),
    ("alt", 56),
    ("alt_l", 56),
    ("space", SPACE),
    ("capslock", 58),
    ("caps_lock", 58),
    ("f1", 59),
    ("f2", 60),
    ("f3", 61),
    ("f4", 62),
    ("f5", 63),
    ("f6", 64),
    ("f7", 65),
    ("f8", 66),
    ("f9", 67),
    ("f10", 68),
    ("f11", 87),
    ("f12", 88),
    ("control_r", 97),
    ("alt_r", 100),
    ("home", 102),
    ("up", 103),
    ("pageup", 104),
    ("prior", 104),
    ("left", 105),
    ("right", 106),
    ("end", 107),
    ("down", 108),
    ("pagedown", 109),
    ("next", 109),
    ("insert", 110),
    ("delete", 111),
    ("minus", 12),
    ("equal", 13),
    ("comma", 51),
    ("period", 52),
    ("slash", 53),
    ("super", 125),
    ("super_l", 125),
    ("super_r", 126),
    ("menu", 127),
    ("print", 99),
    ("scrolllock", 70),
    ("pause", 119),
    ("numlock", 69),
    ("kp0", 82),
    ("kp1", 79),
    ("kp2", 80),
    ("kp3", 81),
    ("kp4", 75),
    ("kp5", 76),
    ("kp6", 77),
    ("kp7", 71),
    ("kp8", 72),
    ("kp9", 73),
    ("kpdot", 83),
    ("kpenter", 96),
    ("kpplus", 78),
    ("kpminus", 74),
    ("kpasterisk", 55),
    ("kpslash", 98),
    // Roblox's MouseButton1-3, and a mouse's two side buttons.
    ("mouse1", BUTTON_LEFT),
    ("mouse2", BUTTON_RIGHT),
    ("mouse3", BUTTON_MIDDLE),
    ("mouse4", 0x113),
    ("mouse5", 0x114),
];

/// The highest code a key or button can have (evdev's KEY_MAX).
const LAST_CODE: u16 = 0x2ff;

pub const BUTTON_LEFT: u16 = 0x110;
pub const BUTTON_RIGHT: u16 = 0x111;
pub const BUTTON_MIDDLE: u16 = 0x112;

/// Whether `code` is a mouse button, which goes to the pointer, not the
/// keyboard.
pub fn is_button(code: u16) -> bool {
    (BUTTON_LEFT..=0x117).contains(&code)
}

/// A modifier key's bit in the keymap's state: Shift, Control, Mod1 (Alt).
pub fn modifier_mask(code: u16) -> Option<u32> {
    match code {
        42 | 54 => Some(1),
        29 | 97 => Some(4),
        56 | 100 => Some(8),
        _ => None,
    }
}

/// The key a US keyboard types `c` with, unshifted.
fn plain_code(c: char) -> Option<u16> {
    if c == ' ' {
        return Some(SPACE);
    }
    ROWS.iter().find_map(|(first, row)| row.chars().position(|r| r == c).map(|i| first + i as u16))
}

/// A key or mouse button by name (`space`, `F5`, `mouse1`), by the
/// character it types (`j`, `/`), or by its code (`code183`).
pub fn key_code(name: &str) -> Option<u16> {
    let name = name.to_lowercase();
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => plain_code(c),
        _ => NAMED.iter().find(|(n, _)| *n == name).map(|(_, code)| *code).or_else(|| {
            let digits = name.strip_prefix("code")?;
            if !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.parse().ok().filter(|code| (1..=LAST_CODE).contains(code))
        }),
    }
}

/// How a US keyboard types `c`: its key, and whether Shift is held.
pub fn char_code(c: char) -> Option<(u16, bool)> {
    if let Some(code) = plain_code(c) {
        return Some((code, false));
    }
    let unshifted = if c.is_ascii_uppercase() {
        c.to_ascii_lowercase()
    } else {
        SHIFTED.iter().find(|(s, _)| *s == c)?.1
    };
    plain_code(unshifted).map(|code| (code, true))
}

/// A code's name as a macro writes it: the character it types, its name, or
/// failing both its code. Every one reads back as the same key.
pub fn key_name(code: u16) -> String {
    if code == SPACE {
        return "space".to_owned();
    }
    let typed = ROWS.iter().find_map(|(first, row)| {
        let i = usize::from(code.checked_sub(*first)?);
        row.chars().nth(i).map(String::from)
    });
    typed
        .or_else(|| NAMED.iter().find(|(_, c)| *c == code).map(|(n, _)| (*n).to_owned()))
        .unwrap_or_else(|| format!("code{code}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_found_by_name_or_character_ignoring_case() {
        assert_eq!(key_code("j"), Some(36));
        assert_eq!(key_code("J"), Some(36));
        assert_eq!(key_code("space"), Some(SPACE));
        assert_eq!(key_code("F5"), Some(63));
        assert_eq!(key_code("/"), Some(53));
        assert_eq!(key_code("esc"), Some(1));
        assert_eq!(key_code("-k"), None);
        assert_eq!(key_code("_"), None, "not a key name, and _ is a shifted character");
    }

    #[test]
    fn characters_type_as_a_us_keyboard_types_them() {
        assert_eq!(char_code('-'), Some((12, false)));
        assert_eq!(char_code('G'), Some((34, true)));
        assert_eq!(char_code('?'), Some((53, true)));
        assert_eq!(char_code(' '), Some((SPACE, false)));
        assert_eq!(char_code('é'), None);
    }

    #[test]
    fn codes_have_their_best_name() {
        assert_eq!(key_name(1), "esc");
        assert_eq!(key_name(SPACE), "space");
        assert_eq!(key_name(18), "e");
        assert_eq!(key_name(SHIFT), "shift");
        assert_eq!(key_name(125), "super");
        assert_eq!(key_name(79), "kp1");
    }

    #[test]
    fn mouse_buttons_are_keys_by_their_roblox_numbers() {
        assert_eq!(key_code("mouse1"), Some(BUTTON_LEFT));
        assert_eq!(key_code("Mouse2"), Some(BUTTON_RIGHT));
        assert_eq!(key_code("mouse3"), Some(BUTTON_MIDDLE));
        assert_eq!(key_code("mouse4"), Some(0x113));
        assert_eq!(key_name(BUTTON_RIGHT), "mouse2");
        assert!(is_button(BUTTON_LEFT) && is_button(0x114));
        assert!(!is_button(30) && !is_button(0x2ff));
    }

    #[test]
    fn a_key_with_no_name_is_written_by_its_code() {
        assert_eq!(key_name(183), "code183");
        assert_eq!(key_code("code183"), Some(183));
        assert_eq!(key_code("CODE30"), Some(30), "a named key by number is still that key");
        for bad in ["code", "code0", "code768", "code-1", "codex", "code 5"] {
            assert_eq!(key_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_code_a_recording_meets_reads_back_as_that_code() {
        let keyboard = 1..=248u16;
        let mouse = 0x110..=0x117u16;
        for code in keyboard.chain(mouse) {
            assert_eq!(key_code(&key_name(code)), Some(code), "{code} as {:?}", key_name(code));
        }
    }
}
