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

/// The text a recording of `events` adds, lasting `secs`, without its note
/// -- and proof that it parses.
fn text(events: &[(f64, Heard)], secs: f64) -> String {
    let events = events.iter().map(|(at, heard)| Event { at: *at, heard: heard.clone() });
    let rows = Recording { events: events.collect(), secs }.rows("Main");
    assert_eq!(rows[0].kind, "Note");
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
fn a_quick_press_is_a_key_and_a_long_one_a_hold() {
    let events =
        [(0.5, key(18, true)), (0.58, key(18, false)), (1.0, key(17, true)), (2.2, key(17, false))];
    assert_eq!(text(&events, 3.0), "wait 0.5\ntap e\nwait 0.42\nhold w 1.2\nwait 0.8\n");
}

#[test]
fn keys_that_overlap_are_pressed_and_released_where_they_happened() {
    let events =
        [(0.0, key(17, true)), (0.5, key(57, true)), (0.56, key(57, false)), (1.5, key(17, false))];
    assert_eq!(
        text(&events, 2.0),
        "press w\nwait 0.5\ntap space\nwait 0.94\nrelease w\nwait 0.5\n"
    );
}

#[test]
fn a_click_keeps_the_point_it_was_made_at() {
    let events = [
        (0.0, at(100.0, 200.0)),
        (1.0, button(0x110, true)),
        (1.08, button(0x110, false)),
        (2.0, button(0x111, true)),
        (2.05, button(0x111, false)),
    ];
    assert_eq!(
        text(&events, 2.5),
        "move to 100 200\nwait 1\nclick 100 200\nwait 0.92\nclick right 100 200\nwait 0.45\n"
    );
}

#[test]
fn a_click_that_wobbles_a_pixel_is_still_a_click() {
    let events = [
        (0.0, at(100.0, 100.0)),
        (1.0, button(0x110, true)),
        (1.02, at(101.0, 100.5)),
        (1.06, button(0x110, false)),
    ];
    assert_eq!(text(&events, 1.06), "move to 100 100\nwait 1\nclick 100 100\n");
}

#[test]
fn a_long_click_is_a_held_button_where_it_was_made() {
    let events = [(0.0, at(30.0, 40.0)), (1.0, button(0x110, true)), (1.5, button(0x110, false))];
    assert_eq!(text(&events, 2.0), "move to 30 40\nwait 1\nhold mouse1 0.5\nwait 0.5\n");
}

#[test]
fn moving_the_mouse_glides_along_its_path_and_rests_where_it_stopped() {
    let mut events = vec![(0.0, at(0.0, 0.0)), (1.0, at(10.0, 0.0))];
    events.extend(sweep(1.0, (10.0, 0.0), (110.0, 0.0), 0.1));
    events.push((2.0, at(110.0, 10.0)));
    events.extend(sweep(2.0, (110.0, 10.0), (110.0, 60.0), 0.05));
    assert_eq!(
        text(&events, 3.0),
        "move to 0 0\nwait 0.99\nmove to 110 0 0.11\nwait 0.89\nmove to 110 60 0.06\nwait 0.95\n"
    );
}

#[test]
fn a_turn_keeps_the_point_the_path_bends_at() {
    let mut events = vec![(0.0, at(0.0, 0.0))];
    events.extend(sweep(0.0, (0.0, 0.0), (100.0, 0.0), 0.1));
    events.extend(sweep(0.1, (100.0, 0.0), (100.0, 100.0), 0.1));
    assert_eq!(text(&events, 0.2), "move to 0 0\nmove to 100 0 0.1\nmove to 100 100 0.1\n");
}

#[test]
fn a_drag_is_pressed_moved_and_released() {
    let mut events = vec![(0.0, at(50.0, 50.0)), (0.5, button(0x111, true))];
    events.extend(sweep(0.5, (50.0, 50.0), (150.0, 50.0), 0.1));
    events.push((0.7, button(0x111, false)));
    assert_eq!(
        text(&events, 1.0),
        "move to 50 50\nwait 0.5\npress mouse2\nmove to 150 50 0.1\nwait 0.1\nrelease mouse2\nwait 0.3\n"
    );
}

#[test]
fn walking_while_turning_holds_the_key_around_the_turn() {
    let mut events = vec![(0.0, at(0.0, 0.0)), (0.0, key(17, true))];
    events.extend(sweep(0.2, (0.0, 0.0), (300.0, 0.0), 0.3));
    events.push((1.0, key(17, false)));
    assert_eq!(
        text(&events, 1.0),
        "move to 0 0\npress w\nwait 0.2\nmove to 300 0 0.3\nwait 0.5\nrelease w\n"
    );
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
        "wait 1\nscroll down 3\nwait 0.9\nscroll up\nwait 0.5\nscroll right 2\nwait 0.5\n"
    );
}

#[test]
fn what_is_still_held_when_the_recording_stops_is_let_go_at_its_end() {
    assert_eq!(text(&[(0.5, key(17, true))], 2.0), "wait 0.5\nhold w 1.5\n");
    let events = [(0.5, key(17, true)), (1.0, key(18, true)), (1.05, key(18, false))];
    assert_eq!(text(&events, 2.0), "wait 0.5\npress w\nwait 0.5\ntap e\nwait 0.95\nrelease w\n");
}

#[test]
fn what_was_held_when_the_recording_started_is_pressed_from_its_start() {
    let events = [(0.0, at(5.0, 5.0)), (0.0, key(17, true)), (1.0, key(17, false))];
    assert_eq!(text(&events, 2.0), "move to 5 5\nhold w 1\nwait 1\n");
}

#[test]
fn a_release_of_what_was_never_pressed_is_passed_over() {
    assert_eq!(text(&[(0.5, key(18, false))], 1.0), "wait 1\n");
}

#[test]
fn a_key_with_no_name_is_written_by_its_code() {
    let events = [(0.0, key(183, true)), (0.05, key(183, false))];
    assert_eq!(text(&events, 0.05), "tap code183\n");
}

#[test]
fn a_recording_is_headed_by_where_and_how_long() {
    let rows = Recording { events: vec![], secs: 12.345 }.rows("Alt 2");
    assert_eq!(rows[0], Row { kind: "Note".into(), value: "recorded in Alt 2 (12.3 s)".into() });
}

#[test]
fn a_recording_of_nothing_done_is_empty() {
    let opening = Event { at: 0.0, heard: at(5.0, 5.0) };
    assert!(Recording { events: vec![opening.clone()], secs: 4.0 }.is_empty());
    let pressed = Event { at: 1.0, heard: key(18, true) };
    assert!(!Recording { events: vec![opening, pressed], secs: 4.0 }.is_empty());
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
