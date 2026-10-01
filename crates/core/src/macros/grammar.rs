//! A macro's text: one command per line. One tokenizer serves the editor
//! (text as rows, and back) and the player (text as steps).

use std::fmt;

use super::keys::{self, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT};

/// A tap is a short hold: its press length is random as well.
pub const TAP_PRESS: (f64, f64) = (0.04, 0.12);
/// Typed characters are this far apart.
pub const TYPE_GAP: (f64, f64) = (0.05, 0.16);
/// The longest any one duration may be: a day.
const LONGEST_SECS: f64 = 86_400.0;
/// The furthest a move or click may reach from where it starts.
const FURTHEST: i32 = 65_535;

/// The editor's step types and the command each is in a macro's text.
const STEP_TYPES: [(&str, &str); 8] = [
    ("Key", "tap"),
    ("Hold", "hold"),
    ("Type", "type"),
    ("Click", "click"),
    ("Move", "move"),
    ("Wait", "wait"),
    ("Start", "start"),
    ("Note", "#"),
];

/// One line as the editor shows it: a step type and its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: String,
    pub value: String,
}

/// One thing the player does. Durations are ranges in seconds, picked
/// afresh every time.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Keys pressed together, held, released in reverse.
    Hold {
        keys: Vec<u16>,
        lo: f64,
        hi: f64,
    },
    /// Characters as (key, shifted).
    Type(Vec<(u16, bool)>),
    /// A button, optionally at a point (from the display's corner).
    Click {
        button: u16,
        at: Option<(i32, i32)>,
    },
    Move(i32, i32),
    Wait(f64, f64),
    /// A wait before the first round only.
    Start(f64, f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    /// 0 is until stopped.
    pub loops: u32,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The line at fault, from 1; None for the macro as a whole.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(n) => write!(f, "line {n}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// One line that says anything.
struct Line<'a> {
    /// From 1.
    number: usize,
    /// Lower-cased; "#" for a note.
    command: String,
    rest: &'a str,
    /// The whole line, trimmed.
    text: &'a str,
}

fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    text.lines().enumerate().filter_map(|(i, raw)| {
        let line = raw.trim();
        if line.is_empty() {
            return None;
        }
        let (command, rest) = match line.strip_prefix('#') {
            Some(note) => ("#".to_owned(), note.trim()),
            None => {
                let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
                (alias(&cmd.to_lowercase()).to_owned(), rest.trim())
            }
        };
        Some(Line { number: i + 1, command, rest, text: line })
    })
}

/// A command by the name the editor shows its step type under ("key" for
/// "tap", "note" for "#"), as the help writes them: text written either way
/// reads the same.
fn alias(command: &str) -> &str {
    STEP_TYPES
        .iter()
        .find(|(kind, _)| kind.eq_ignore_ascii_case(command))
        .map_or(command, |(_, cmd)| cmd)
}

/// A macro as the editor's rows, and its loop count (0 until stopped). A
/// command the editor has no type for keeps its own name, so saving writes
/// it back unchanged.
pub fn rows(text: &str) -> (Vec<Row>, u32) {
    let mut rows = Vec::new();
    let mut loops = 0;
    for line in lines(text) {
        if line.command == "loop" {
            if let Ok(n) = line.rest.parse() {
                loops = n;
                continue;
            }
        }
        let kind = STEP_TYPES
            .iter()
            .find(|(_, cmd)| *cmd == line.command)
            .map_or_else(|| capitalize(&line.command), |(kind, _)| (*kind).to_owned());
        rows.push(Row { kind, value: line.rest.to_owned() });
    }
    (rows, loops)
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// [`rows`] backwards.
pub fn to_text(rows: &[Row], loops: u32) -> String {
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let cmd = STEP_TYPES
                .iter()
                .find(|(kind, _)| *kind == row.kind)
                .map_or_else(|| row.kind.to_lowercase(), |(_, cmd)| (*cmd).to_owned());
            format!("{cmd} {}", row.value).trim().to_owned()
        })
        .collect();
    if loops > 0 {
        lines.push(format!("loop {loops}"));
    }
    lines.join("\n") + "\n"
}

/// "Until stopped", "Once", "3 rounds".
pub fn loop_label(loops: u32) -> String {
    match loops {
        0 => "Until stopped".to_owned(),
        1 => "Once".to_owned(),
        n => format!("{n} rounds"),
    }
}

/// The steps a macro's text stands for. The error names the line at fault.
pub fn parse(text: &str) -> Result<Macro, ParseError> {
    let mut m = Macro { loops: 0, steps: Vec::new() };
    for line in lines(text) {
        if line.command == "#" {
            continue;
        }
        let at_line = |message: String| ParseError { line: Some(line.number), message };
        match step(&line).map_err(at_line)? {
            Parsed::Step(s) => m.steps.push(s),
            Parsed::Loops(n) => m.loops = n,
        }
    }
    if m.steps.is_empty() {
        return Err(ParseError { line: None, message: "the macro has no steps".into() });
    }
    Ok(m)
}

enum Parsed {
    Step(Step),
    Loops(u32),
}

fn step(line: &Line<'_>) -> Result<Parsed, String> {
    let args: Vec<&str> = line.rest.split_whitespace().collect();
    let step = match (line.command.as_str(), args.as_slice()) {
        ("type", _) if !line.rest.is_empty() => Step::Type(typed(line.rest)?),
        ("tap", [k]) => hold(k, TAP_PRESS)?,
        ("tap", [k, secs]) | ("hold", [k, secs]) => hold(k, seconds(secs)?)?,
        ("wait", [secs]) => {
            let (lo, hi) = seconds(secs)?;
            Step::Wait(lo, hi)
        }
        ("start", [secs]) => {
            let (lo, hi) = seconds(secs)?;
            Step::Start(lo, hi)
        }
        ("click", args) if args.len() <= 3 => click(args)?,
        ("move", [dx, dy]) => Step::Move(int(dx)?, int(dy)?),
        ("loop", [n]) if n.bytes().all(|b| b.is_ascii_digit()) => {
            return Ok(Parsed::Loops(n.parse().map_err(|_| format!("not a count: '{n}'"))?));
        }
        _ => return Err(format!("don't understand '{}'", line.text)),
    };
    Ok(Parsed::Step(step))
}

fn hold(token: &str, (lo, hi): (f64, f64)) -> Result<Step, String> {
    let keys = token
        .split('+')
        .map(|k| keys::key_code(k).ok_or_else(|| format!("unknown key '{token}'")))
        .collect::<Result<_, _>>()?;
    Ok(Step::Hold { keys, lo, hi })
}

fn typed(text: &str) -> Result<Vec<(u16, bool)>, String> {
    text.chars()
        .map(|c| {
            keys::char_code(c)
                .ok_or_else(|| format!("cannot type '{c}' -- US keyboard characters only"))
        })
        .collect()
}

fn seconds(token: &str) -> Result<(f64, f64), String> {
    let bad = || format!("not a duration: '{token}'");
    let (lo, hi) = token.split_once('-').unwrap_or((token, token));
    let (lo, hi): (f64, f64) = (lo.parse().map_err(|_| bad())?, hi.parse().map_err(|_| bad())?);
    if !(lo.is_finite() && hi.is_finite() && 0.0 <= lo && lo <= hi) {
        return Err(bad());
    }
    if hi > LONGEST_SECS {
        return Err(format!("'{token}' is longer than a day"));
    }
    Ok((lo, hi))
}

/// A distance in pixels, within reach of any screen.
fn int(token: &str) -> Result<i32, String> {
    let n: i32 = token.parse().map_err(|_| format!("not a number: '{token}'"))?;
    if n.unsigned_abs() > FURTHEST.unsigned_abs() {
        return Err(format!("'{token}' is too far -- at most {FURTHEST} pixels"));
    }
    Ok(n)
}

fn click(args: &[&str]) -> Result<Step, String> {
    let usage = || "expected click [left|right|middle] [X Y]".to_owned();
    let (button, point) = match args.split_first() {
        Some((first, rest)) if first.chars().all(char::is_alphabetic) => (*first, rest),
        _ => ("left", args),
    };
    let button = match button {
        "left" => BUTTON_LEFT,
        "right" => BUTTON_RIGHT,
        "middle" => BUTTON_MIDDLE,
        _ => return Err(usage()),
    };
    let at = match point {
        [] => None,
        [x, y] => Some((int(x)?, int(y)?)),
        _ => return Err(usage()),
    };
    Ok(Step::Click { button, at })
}

/// A click step's value aimed at (`x`, `y`): its button word stays, any
/// point it had is replaced.
pub fn click_at(value: &str, x: i32, y: i32) -> String {
    match value.split_whitespace().next() {
        Some(button) if button.chars().all(char::is_alphabetic) => format!("{button} {x} {y}"),
        _ => format!("{x} {y}"),
    }
}

/// What a key, text, click or move step does, for the status line.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Hold { keys, .. } => {
            let names: Vec<String> = keys.iter().map(|k| keys::key_name(*k)).collect();
            format!("pressing {}", names.join("+"))
        }
        Step::Type(_) => "typing".into(),
        Step::Click { .. } => "clicking".into(),
        Step::Move(..) => "moving the mouse".into(),
        Step::Wait(..) | Step::Start(..) => "waiting".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(keys: &[u16], lo: f64, hi: f64) -> Step {
        Step::Hold { keys: keys.to_vec(), lo, hi }
    }

    #[test]
    fn a_macro_parses_into_the_codes_a_us_keyboard_sends() {
        let m = parse(
            "# farm\nloop 3\ntap E\ntap f 0.2-0.3\nhold shift+w 1-2\n\ntype -Gg\nclick right 10 20\n\
             move -5 5\nwait 0.5\ntap F5\ntap /\n",
        )
        .unwrap();
        let (lo, hi) = TAP_PRESS;
        assert_eq!(m.loops, 3);
        assert_eq!(
            m.steps,
            vec![
                hold(&[18], lo, hi),
                hold(&[33], 0.2, 0.3),
                hold(&[42, 17], 1.0, 2.0),
                Step::Type(vec![(12, false), (34, true), (34, false)]),
                Step::Click { button: BUTTON_RIGHT, at: Some((10, 20)) },
                Step::Move(-5, 5),
                Step::Wait(0.5, 0.5),
                hold(&[63], lo, hi),
                hold(&[53], lo, hi),
            ]
        );
    }

    #[test]
    fn a_tap_is_its_own_keys_code_never_escape() {
        let (lo, hi) = TAP_PRESS;
        assert_eq!(parse("tap j").unwrap().steps, [hold(&[36], lo, hi)]);
        assert_eq!(parse("tap space").unwrap().steps, [hold(&[57], lo, hi)]);
    }

    #[test]
    fn values_too_large_to_play_are_refused_by_line() {
        for (bad, why) in [
            ("wait 1e13", "line 1"),
            ("tap e\nwait 1e20", "line 2"),
            ("hold w 100000", "longer than a day"),
            ("move 9000000 0", "too far"),
            ("click 70000 5", "too far"),
        ] {
            let err = parse(bad).unwrap_err().to_string();
            assert!(err.contains(why), "{bad:?}: {err}");
        }
        assert!(
            parse("wait 86400\nmove -65535 65535").is_ok(),
            "a day and a screen's width are fine"
        );
    }

    #[test]
    fn a_bad_macro_is_refused_and_says_where() {
        for (bad, why) in [
            ("tap -k", "unknown key"),
            ("tap e\nwait 3-1", "line 2"),
            ("click 5", "click"),
            ("jump", "don't understand"),
            ("type héllo", "cannot type"),
            ("# nothing", "no steps"),
        ] {
            let err = parse(bad).unwrap_err().to_string();
            assert!(err.contains(why), "{bad:?}: {err}");
        }
    }

    #[test]
    fn the_editors_rows_name_types_keep_notes_and_hold_the_loop_apart() {
        let src = "# note\nstart 5\ntap j\nhold shift+w 2\nwait 60-70\nfrob 1\nloop 3\n";
        let row = |k: &str, v: &str| Row { kind: k.into(), value: v.into() };
        assert_eq!(
            rows(src),
            (
                vec![
                    row("Note", "note"),
                    row("Start", "5"),
                    row("Key", "j"),
                    row("Hold", "shift+w 2"),
                    row("Wait", "60-70"),
                    row("Frob", "1"),
                ],
                3
            )
        );
        let (r, loops) = rows(src);
        assert_eq!(to_text(&r, loops), src, "rows go back to the very text they came from");
    }

    #[test]
    fn a_macro_with_no_loop_line_runs_until_stopped() {
        assert_eq!(rows("tap e").1, 0);
        assert_eq!(
            (loop_label(0), loop_label(1), loop_label(3)),
            ("Until stopped".into(), "Once".into(), "3 rounds".into())
        );
    }

    #[test]
    fn an_editor_made_macro_is_one_the_player_runs() {
        let row = |k: &str, v: &str| Row { kind: k.into(), value: v.into() };
        let text = to_text(&[row("Key", "e"), row("Wait", "0.5"), row("Click", "960 540")], 1);
        assert_eq!(parse(&text).unwrap().loops, 1);
    }

    #[test]
    fn the_editors_names_read_as_the_commands_they_stand_for() {
        let src = "Start 45\nKey j\nNote farm the boss\nWait 60-70\nloop 2\n";
        let m = parse(src).unwrap();
        assert_eq!((m.loops, m.steps.len()), (2, 3), "the note is no step");
        let (r, loops) = rows(src);
        assert_eq!(
            r.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
            ["Start", "Key", "Note", "Wait"]
        );
        assert_eq!(to_text(&r, loops), "start 45\ntap j\n# farm the boss\nwait 60-70\nloop 2\n");
    }

    #[test]
    fn a_picked_point_replaces_the_clicks_point_and_keeps_its_button() {
        assert_eq!(click_at("", 960, 540), "960 540");
        assert_eq!(click_at("10 20", 960, 540), "960 540");
        assert_eq!(click_at("right", 5, 6), "right 5 6");
        assert_eq!(click_at(" middle 1 2 ", 5, 6), "middle 5 6");
        assert!(click(&["right", "5", "6"]).is_ok(), "what it writes parses");
    }

    #[test]
    fn steps_describe_themselves() {
        assert_eq!(describe(&hold(&[42, 17], 1.0, 1.0)), "pressing shift+w");
        assert_eq!(describe(&Step::Click { button: BUTTON_LEFT, at: None }), "clicking");
        assert_eq!(describe(&Step::Type(vec![])), "typing");
    }
}
