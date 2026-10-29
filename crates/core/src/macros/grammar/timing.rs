//! When each of a macro's steps starts, from the round's start: what the
//! editor draws its steps along. A range, as the waits are.

use super::{Parsed, Row, Step, TYPE_GAP, lines, step};
use crate::macros::lanes;

/// Seconds, the least and the most.
pub type Span = (f64, f64);

/// How long `row` keeps the round from going on: (0, 0) for a row that
/// takes no time, plays out of turn (a `when` and its `do`s), plays beside
/// the steps after it (a repeat), or does not parse yet.
fn span(row: &Row) -> Span {
    if matches!(row.kind.as_str(), "Note" | "At" | "When" | "Do") {
        return (0.0, 0.0);
    }
    let text = super::to_text(std::slice::from_ref(row), 0);
    let Some(line) = lines(&text).next() else { return (0.0, 0.0) };
    match step(&line) {
        Ok(Parsed::Step(s)) => of_step(&s),
        _ => (0.0, 0.0),
    }
}

fn of_step(step: &Step) -> Span {
    match step {
        Step::Hold { lo, hi, .. } | Step::MoveTo { lo, hi, .. } => (*lo, *hi),
        Step::Wait(lo, hi) | Step::Start(lo, hi) => (*lo, *hi),
        Step::Type(chars) => {
            let n = chars.len() as f64;
            (n * TYPE_GAP.0, n * TYPE_GAP.1)
        }
        Step::Path(points) => last(points.iter().map(|p| p.0)),
        Step::Turn(points) => last(points.iter().map(|p| p.0)),
        Step::Timeline { secs, .. } => *secs,
        Step::Click { .. } | Step::Move(..) | Step::Press(_) => (0.0, 0.0),
        Step::Release(_) | Step::Repeat { .. } | Step::Scroll { .. } | Step::Exit => (0.0, 0.0),
    }
}

fn last(times: impl Iterator<Item = f64>) -> Span {
    let t = times.fold(0.0, f64::max);
    (t, t)
}

/// When each row starts, from the round's start, and when the round ends:
/// the rows' spans one after another. A timeline lasts until its last step
/// has played, if that is later than its length; a repeat plays beside the
/// steps after it, but the round lasts until it has.
pub fn starts(rows: &[Row]) -> (Vec<Span>, Span) {
    let mut at = (0.0, 0.0);
    // The latest a repeat keeps the round going.
    let mut held = (0.0f64, 0.0f64);
    let starts = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let start = at;
            let (mut lo, mut hi) = span(row);
            match row.kind.as_str() {
                "Timeline" => {
                    let block = &rows[i..i + lanes::block(rows, i)];
                    let played = lanes::of_rows(block).0.secs;
                    (lo, hi) = (lo.max(played), hi.max(played));
                }
                "Repeat" => {
                    let (rlo, rhi) = repeated(row);
                    held = (held.0.max(at.0 + rlo), held.1.max(at.1 + rhi));
                }
                _ => {}
            }
            at = (at.0 + lo, at.1 + hi);
            start
        })
        .collect();
    (starts, (at.0.max(held.0), at.1.max(held.1)))
}

/// How long a repeat row taps for, or (0, 0).
fn repeated(row: &Row) -> Span {
    let text = super::to_text(std::slice::from_ref(row), 0);
    match lines(&text).next().map(|line| step(&line)) {
        Some(Ok(Parsed::Step(Step::Repeat { lo, hi, .. }))) => (lo, hi),
        _ => (0.0, 0.0),
    }
}

/// A time on the timeline: "0 s", "2.5 s", "1:30", or a range of them,
/// "1:00–4:00".
pub fn label((lo, hi): Span) -> String {
    let one = |secs: f64| {
        if secs < 60.0 {
            let tenths = (secs * 10.0).round() / 10.0;
            format!("{tenths} s")
        } else {
            let whole = secs.round() as u64;
            format!("{}:{:02}", whole / 60, whole % 60)
        }
    };
    if (hi - lo).abs() < 0.05 {
        one(lo)
    } else if hi < 60.0 {
        format!("{}–{}", one(lo).trim_end_matches(" s"), one(hi))
    } else {
        format!("{}–{}", one(lo), one(hi))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, value: &str) -> Row {
        Row { kind: kind.to_owned(), value: value.to_owned() }
    }

    #[test]
    fn the_starter_macro_waits_a_minute_to_four_after_its_tap() {
        let rows = [row("Note", "anti-AFK"), row("Key", "space"), row("Wait", "60-240")];
        let (starts, end) = starts(&rows);
        assert_eq!(starts[0], (0.0, 0.0));
        assert_eq!(starts[1], (0.0, 0.0));
        assert!((starts[2].0 - 0.08).abs() < 1e-9);
        assert!((end.0 - 60.08).abs() < 1e-9 && (end.1 - 240.08).abs() < 1e-9);
    }

    #[test]
    fn out_of_turn_and_unfinished_rows_take_no_time() {
        for r in
            [row("When", "image coin"), row("Do", "wait 5"), row("Wait", ""), row("Repeat", "e 10")]
        {
            assert_eq!(span(&r), (0.0, 0.0), "{r:?}");
        }
        assert_eq!(span(&row("Hold", "w 1-2")), (1.0, 2.0));
        assert_eq!(span(&row("Timeline", "3.5")), (3.5, 3.5));
    }

    #[test]
    fn a_timeline_lasts_until_its_last_step_and_a_repeat_holds_the_round() {
        let rows = [row("Timeline", ""), row("At", "1 hold w 2"), row("Key", "e")];
        let (starts, _) = starts(&rows);
        assert!(starts[2].0 >= 3.0, "{starts:?}");
        let (_, end) = super::starts(&[row("Repeat", "e 10"), row("Key", "f")]);
        assert_eq!(end, (10.0, 10.0));
    }

    #[test]
    fn times_read_as_seconds_then_minutes() {
        assert_eq!(label((0.0, 0.0)), "0 s");
        assert_eq!(label((2.5, 2.5)), "2.5 s");
        assert_eq!(label((0.5, 1.0)), "0.5–1 s");
        assert_eq!(label((90.0, 90.0)), "1:30");
        assert_eq!(label((60.0, 240.0)), "1:00–4:00");
        assert_eq!(label((30.0, 90.0)), "30 s–1:30");
    }
}
