use super::*;
use crate::macros::grammar;
use crate::macros::relay::event::Heard;

fn key(code: u16, down: bool) -> Heard {
    Heard::Key { code, down }
}

fn button(code: u16, down: bool) -> Heard {
    Heard::Button { code, down }
}

fn at(x: f64, y: f64) -> Heard {
    Heard::Motion { x, y }
}

fn wheel(value120: i32) -> Heard {
    Heard::Scroll { horizontal: false, value120 }
}

fn turn(dx: f64, dy: f64) -> Heard {
    Heard::Turn { dx, dy }
}

/// The timeline a recording of `events` adds, lasting `secs`, without its
/// note -- and proof that it parses.
fn text(events: &[(f64, Heard)], secs: f64) -> String {
    let events = events.iter().map(|(at, heard)| Event { at: *at, heard: heard.clone() });
    let rows = Recording { events: events.collect(), secs }.rows("Main");
    assert_eq!(rows[0].kind, "Note");
    assert_eq!(rows[1].kind, "Timeline");
    let text = grammar::to_text(&rows[1..], 0);
    assert!(grammar::parse(&text).is_ok(), "{text}");
    text
}

/// `secs` seconds of the pointer going from `from` to `to` at an even speed,
/// sampled every hundredth of a second, from `t` on.
fn sweep(t: f64, from: (f64, f64), to: (f64, f64), secs: f64) -> Vec<(f64, Heard)> {
    let ticks = (secs * 100.0).round() as i32;
    (1..=ticks)
        .map(|i| {
            let f = f64::from(i) / f64::from(ticks);
            (
                t + f64::from(i) / 100.0,
                at(from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f),
            )
        })
        .collect()
}

#[test]
fn a_recording_is_headed_by_where_and_how_long() {
    let rows = Recording { events: vec![], secs: 12.345 }.rows("Alt 2");
    assert_eq!(rows[0], Row { kind: "Note".into(), value: "recorded in Alt 2 (12.3 s)".into() });
    assert_eq!(rows[1], Row { kind: "Timeline".into(), value: "12.35".into() });
}

#[test]
fn a_recording_of_nothing_done_is_empty() {
    let opening = Event { at: 0.0, heard: at(5.0, 5.0) };
    assert!(Recording { events: vec![opening.clone()], secs: 4.0 }.is_empty());
    let pressed = Event { at: 1.0, heard: key(18, true) };
    assert!(!Recording { events: vec![opening, pressed], secs: 4.0 }.is_empty());
    let turned = Event { at: 0.0, heard: turn(1.0, 0.0) };
    assert!(!Recording { events: vec![turned], secs: 4.0 }.is_empty());
}

#[test]
fn a_recording_runs_from_its_start_to_its_stop() {
    let e = Event { at: 0.5, heard: key(18, true) };
    let mut reports =
        vec![Ok(Report::Started), Ok(Report::Heard(e.clone())), Ok(Report::Stopped(2.5))]
            .into_iter();
    let started = std::cell::Cell::new(0);
    let got = record(&mut || reports.next().unwrap(), &|| started.set(started.get() + 1));
    assert_eq!(got, Ok(Recording { events: vec![e], secs: 2.5 }));
    assert_eq!(started.get(), 1);
}

#[test]
fn a_report_that_ends_mid_recording_is_its_error() {
    let mut reports =
        vec![Ok(Report::Started), Err(MacroError::WentAway("its report ended".into()))].into_iter();
    let got = record(&mut || reports.next().unwrap(), &|| {});
    assert_eq!(got, Err(MacroError::WentAway("its report ended".into())));
}

#[test]
fn each_press_is_held_exactly_as_long_as_it_was_at_its_own_time() {
    let events =
        [(0.5, key(18, true)), (0.58, key(18, false)), (1.0, key(17, true)), (2.2, key(17, false))];
    assert_eq!(text(&events, 3.0), "timeline 3\nat 0.5 tap e 0.08\nat 1 hold w 1.2\n");
}

#[test]
fn keys_that_overlap_play_over_one_another() {
    let events =
        [(0.0, key(17, true)), (0.5, key(57, true)), (0.56, key(57, false)), (1.5, key(17, false))];
    assert_eq!(text(&events, 2.0), "timeline 2\nat 0 hold w 1.5\nat 0.5 tap space 0.06\n");
}

#[test]
fn a_click_is_its_button_held_where_the_path_has_the_pointer() {
    let events = [
        (0.0, at(100.0, 200.0)),
        (1.0, button(0x110, true)),
        (1.08, button(0x110, false)),
        (2.0, button(0x111, true)),
        (2.05, button(0x111, false)),
    ];
    assert_eq!(
        text(&events, 2.5),
        "timeline 2.5\nat 0 path 0 100 200\nat 1 tap mouse1 0.08\nat 2 tap mouse2 0.05\n"
    );
}

#[test]
fn each_movement_is_one_path_set_off_from_where_the_pointer_rested() {
    let mut events = vec![(0.0, at(0.0, 0.0)), (1.0, at(10.0, 0.0))];
    events.extend(sweep(1.0, (10.0, 0.0), (110.0, 0.0), 0.1));
    events.push((2.0, at(110.0, 10.0)));
    events.extend(sweep(2.0, (110.0, 10.0), (110.0, 60.0), 0.05));
    assert_eq!(
        text(&events, 3.0),
        "timeline 3\nat 0 path 0 0 0\nat 0.99 path 0 0 0, 0.11 110 0\n\
         at 1.99 path 0 110 0, 0.06 110 60\n"
    );
}

#[test]
fn a_path_keeps_the_point_it_bends_at() {
    let mut events = vec![(0.0, at(0.0, 0.0))];
    events.extend(sweep(0.0, (0.0, 0.0), (100.0, 0.0), 0.1));
    events.extend(sweep(0.1, (100.0, 0.0), (100.0, 100.0), 0.1));
    assert_eq!(text(&events, 0.2), "timeline 0.2\nat 0 path 0 0 0, 0.1 100 0, 0.2 100 100\n");
}

#[test]
fn a_drag_is_the_button_held_while_the_pointer_moves() {
    let mut events = vec![(0.0, at(50.0, 50.0)), (0.5, button(0x111, true))];
    events.extend(sweep(0.5, (50.0, 50.0), (150.0, 50.0), 0.1));
    events.push((0.7, button(0x111, false)));
    assert_eq!(
        text(&events, 1.0),
        "timeline 1\nat 0 path 0 50 50\nat 0.5 hold mouse2 0.2\nat 0.5 path 0 50 50, 0.1 150 50\n"
    );
}

#[test]
fn raw_movement_is_written_as_turns_and_the_pointer_left_to_follow() {
    let mut events = vec![(0.0, at(300.0, 300.0))];
    events.extend((0..10).map(|i| (0.1 + f64::from(i) / 100.0, turn(2.0, 0.0))));
    events.extend(sweep(0.1, (300.0, 300.0), (320.0, 300.0), 0.1));
    events.push((1.0, turn(-0.5, 0.25)));
    assert_eq!(
        text(&events, 1.5),
        "timeline 1.5\nat 0.09 turn 0.1 20 0\nat 0.99 turn 0.01 -0.5 0.25\n"
    );
}

#[test]
fn raw_movement_in_one_hundredth_is_summed() {
    let events = [(0.5, turn(1.0, 1.0)), (0.501, turn(1.0, -3.0)), (0.502, turn(0.125, 0.0))];
    assert_eq!(text(&events, 1.0), "timeline 1\nat 0.49 turn 0.01 2.13 -2\n");
}

#[test]
fn a_scroll_is_its_notches_together() {
    let events = [
        (1.0, wheel(120)),
        (1.05, wheel(120)),
        (1.1, wheel(120)),
        (2.0, wheel(-120)),
        (2.5, Heard::Scroll { horizontal: true, value120: 240 }),
    ];
    assert_eq!(
        text(&events, 3.0),
        "timeline 3\nat 1 scroll down 3\nat 2 scroll up\nat 2.5 scroll right 2\n"
    );
}

#[test]
fn what_is_still_held_when_the_recording_stops_is_let_go_at_its_end() {
    let events = [(0.5, key(17, true)), (1.0, key(18, true)), (1.05, key(18, false))];
    assert_eq!(text(&events, 2.0), "timeline 2\nat 0.5 hold w 1.5\nat 1 tap e 0.05\n");
}

#[test]
fn what_was_held_when_the_recording_started_is_pressed_from_its_start() {
    let events = [(0.0, at(5.0, 5.0)), (0.0, key(17, true)), (1.0, key(17, false))];
    assert_eq!(text(&events, 2.0), "timeline 2\nat 0 hold w 1\nat 0 path 0 5 5\n");
}

#[test]
fn a_release_of_what_was_never_pressed_is_passed_over() {
    assert_eq!(text(&[(0.5, key(18, false))], 1.0), "timeline 1\n");
}

#[test]
fn a_key_with_no_name_is_written_by_its_code() {
    let events = [(0.0, key(183, true)), (0.05, key(183, false))];
    assert_eq!(text(&events, 0.05), "timeline 0.05\nat 0 tap code183 0.05\n");
}

#[test]
fn minutes_of_play_are_a_few_steps_not_thousands() {
    // A minute of walking, turning the camera at a hundred samples a
    // second, now and then a jump.
    let mut events = Vec::new();
    for s in 0..60 {
        let t = f64::from(s);
        events.push((t, key(17, true)));
        events.push((t + 0.9, key(17, false)));
        if s % 5 == 0 {
            events.push((t + 0.3, key(57, true)));
            events.push((t + 0.4, key(57, false)));
        }
        events.extend((0..40).map(|i| (t + f64::from(i) / 100.0, turn(3.0, 0.5))));
    }
    let rows = text(&events, 60.0).lines().count();
    assert!(rows < 200, "{rows} rows");
}
