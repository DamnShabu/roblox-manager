//! A timeline as the editor draws it: a lane for each key or button it
//! presses, and one each for the pointer, the camera and the wheel, with a
//! bar in its lane for each step from when it starts to when it ends.

use super::grammar::{self, Row, Step, timeline::length};
use super::keys;

#[derive(Clone, Debug, PartialEq)]
pub struct Lanes {
    /// How long the timeline lasts, at the top of its ranges.
    pub secs: f64,
    /// In the order each is first used.
    pub lanes: Vec<Lane>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub name: String,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub from: f64,
    pub to: f64,
    /// The step's line, as written after `at`.
    pub line: String,
}

/// The rows from `start`, a Timeline row, that belong to it: it, and the
/// At rows after it -- with any notes between them, as the text reads.
pub fn block(rows: &[Row], start: usize) -> usize {
    let mut end = start + 1;
    let mut last_at = end;
    while let Some(r) = rows.get(end) {
        match r.kind.as_str() {
            "At" => last_at = end + 1,
            "Note" => {}
            _ => break,
        }
        end += 1;
    }
    last_at - start
}

/// The editor's steps as it lists them: (first row, how many rows) -- a
/// row each, but a timeline and its rows as one.
pub fn units(rows: &[Row]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let len = if rows[i].kind == "Timeline" { block(rows, i) } else { 1 };
        out.push((i, len));
        i += len;
    }
    out
}

/// The step listed at row `start` swapped with the one before it (`up`) or
/// after it, whole; false when there is none there.
pub fn shift(rows: &mut Vec<Row>, start: usize, up: bool) -> bool {
    let units = units(rows);
    let Some(k) = units.iter().position(|u| u.0 == start) else { return false };
    let other = if up { k.checked_sub(1) } else { Some(k + 1) };
    let Some(&(b_start, b_len)) = other.and_then(|o| units.get(o)) else { return false };
    let ((first, first_len), (second, second_len)) =
        if up { ((b_start, b_len), units[k]) } else { (units[k], (b_start, b_len)) };
    let moved: Vec<Row> = rows[second..second + second_len]
        .iter()
        .chain(&rows[first..first + first_len])
        .cloned()
        .collect();
    rows.splice(first..second + second_len, moved);
    true
}

/// The lanes of the timeline whose rows are `block`, a Timeline row first,
/// and how many steps it has.
pub fn of_rows(block: &[Row]) -> (Lanes, usize) {
    let items: Vec<&str> =
        block.iter().filter(|r| r.kind == "At").map(|r| r.value.as_str()).collect();
    let timeline = block.first().map_or("", |r| r.value.as_str());
    (lanes(timeline, &items), items.len())
}

/// The lanes of a timeline: `timeline` is its row's value, `items` its At
/// rows' values. A step that does not parse is in a lane of its own, so it
/// is seen.
pub fn lanes(timeline: &str, items: &[&str]) -> Lanes {
    let mut secs = grammar::parse(&format!("timeline {timeline}"))
        .ok()
        .map_or(0.0, |m| m.steps.first().map_or(0.0, length));
    let mut lanes: Vec<Lane> = Vec::new();
    let mut bars: Vec<(String, Bar)> = items
        .iter()
        .map(|line| {
            let parsed = grammar::parse(&format!("timeline\nat {line}"));
            let item = parsed.ok().and_then(|mut m| match m.steps.pop() {
                Some(Step::Timeline { mut items, .. }) => items.pop(),
                _ => None,
            });
            let Some(item) = item else {
                let bar = Bar { from: 0.0, to: 0.0, line: (*line).to_owned() };
                return ("Not understood".to_owned(), bar);
            };
            let from = item.at.0;
            let bar = Bar { from, to: from + length(&item.step), line: (*line).to_owned() };
            (lane(&item.step), bar)
        })
        .collect();
    bars.sort_by(|a, b| a.1.from.total_cmp(&b.1.from));
    for (name, bar) in bars {
        secs = secs.max(bar.to);
        match lanes.iter_mut().find(|l| l.name == name) {
            Some(lane) => lane.bars.push(bar),
            None => lanes.push(Lane { name, bars: vec![bar] }),
        }
    }
    Lanes { secs, lanes }
}

/// The lane a step goes in.
fn lane(step: &Step) -> String {
    let names =
        |codes: &[u16]| codes.iter().map(|c| keys::key_name(*c)).collect::<Vec<_>>().join("+");
    match step {
        Step::Hold { keys, .. } | Step::Press(keys) | Step::Release(keys) => names(keys),
        Step::Click { button, .. } => keys::key_name(*button),
        Step::Move(..) | Step::MoveTo { .. } | Step::Path(_) => "Pointer".to_owned(),
        Step::Turn(_) => "Camera".to_owned(),
        Step::Scroll { .. } => "Wheel".to_owned(),
        _ => "Other".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, value: &str) -> Row {
        Row { kind: kind.to_owned(), value: value.to_owned() }
    }

    #[test]
    fn each_key_button_and_the_mouse_has_a_lane_in_the_order_first_used() {
        let got = lanes(
            "2",
            &[
                "0.5 tap space 0.1",
                "0 hold w 1.5",
                "0.2 turn 0.5 10 0",
                "1 click 5 5",
                "0.7 hold w 0.1",
            ],
        );
        let names: Vec<&str> = got.lanes.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["w", "Camera", "space", "mouse1"]);
        assert_eq!(got.lanes[0].bars.len(), 2);
        assert_eq!((got.lanes[1].bars[0].from, got.lanes[1].bars[0].to), (0.2, 0.7));
        assert_eq!(got.secs, 2.0);
    }

    #[test]
    fn a_timeline_is_as_long_as_its_last_step_and_shows_what_it_cannot_read() {
        let got = lanes("1", &["0.5 hold w 3", "1 frobnicate"]);
        assert_eq!(got.secs, 3.5);
        assert_eq!(got.lanes[0].name, "Not understood");
        assert_eq!(got.lanes[0].bars[0].line, "1 frobnicate");
    }

    #[test]
    fn a_timeline_s_rows_run_through_its_notes_to_its_last_at() {
        let rows = [
            row("Timeline", "2"),
            row("At", "0 tap e"),
            row("Note", "inside"),
            row("At", "1 tap e"),
            row("Note", "after"),
            row("Wait", "1"),
        ];
        assert_eq!(block(&rows, 0), 4);
        assert_eq!(block(&[row("Timeline", "")], 0), 1);
        assert_eq!(units(&rows), [(0, 4), (4, 1), (5, 1)]);
    }

    #[test]
    fn a_step_moved_past_a_timeline_hops_it_whole() {
        let kinds = |rows: &[Row]| rows.iter().map(|r| r.kind.clone()).collect::<Vec<_>>();
        let mut rows =
            vec![row("Wait", "1"), row("Timeline", "2"), row("At", "0 tap e"), row("Key", "f")];
        assert!(shift(&mut rows, 0, false));
        assert_eq!(kinds(&rows), ["Timeline", "At", "Wait", "Key"]);
        assert!(shift(&mut rows, 0, false), "the timeline down, whole");
        assert_eq!(kinds(&rows), ["Wait", "Timeline", "At", "Key"]);
        assert!(shift(&mut rows, 3, true));
        assert_eq!(kinds(&rows), ["Wait", "Key", "Timeline", "At"]);
        assert!(!shift(&mut rows, 0, true), "nothing before the first");
        assert!(!shift(&mut rows, 2, false), "nothing after the last");
    }
}
