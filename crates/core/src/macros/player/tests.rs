use std::cell::RefCell;
use std::rc::Rc;

use chrono::TimeZone;

use super::*;
use crate::macros::grammar::parse;
use crate::macros::keys::{BUTTON_LEFT, BUTTON_RIGHT};

#[derive(Clone, Debug, PartialEq)]
enum Sent {
    Key(u16, bool),
    Button(u16, bool),
    Motion(f64, f64),
    MoveTo(i32, i32),
    Scroll(bool, i32),
}

/// An input that records what reaches it, can fail on one key's press, and
/// notes when it is let go of.
#[derive(Default)]
struct Recorder {
    sent: Rc<RefCell<Vec<Sent>>>,
    closed: Rc<RefCell<bool>>,
    fail_on: Option<u16>,
    /// Set when this is sent: a stop at a known point in a macro.
    stop_on: Option<(Sent, StopFlag)>,
}

impl Input for Recorder {
    fn key(&mut self, code: u16, down: bool) -> io::Result<()> {
        if down && Some(code) == self.fail_on {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe"));
        }
        self.note(Sent::Key(code, down));
        Ok(())
    }
    fn motion(&mut self, dx: f64, dy: f64) -> io::Result<()> {
        self.note(Sent::Motion(dx, dy));
        Ok(())
    }
    fn button(&mut self, code: u16, down: bool) -> io::Result<()> {
        self.note(Sent::Button(code, down));
        Ok(())
    }
    fn move_to(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.note(Sent::MoveTo(x, y));
        Ok(())
    }
    fn scroll(&mut self, horizontal: bool, notches: i32) -> io::Result<()> {
        self.note(Sent::Scroll(horizontal, notches));
        Ok(())
    }
}

impl Recorder {
    fn failing_on(code: u16) -> Self {
        Recorder { sent: Rc::default(), closed: Rc::default(), fail_on: Some(code), stop_on: None }
    }

    fn note(&self, sent: Sent) {
        if let Some((at, stop)) = &self.stop_on {
            if *at == sent {
                stop.set();
            }
        }
        self.sent.borrow_mut().push(sent);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        *self.closed.borrow_mut() = true;
    }
}

fn step(text: &str) -> Step {
    parse(text).unwrap().steps.remove(0)
}

fn no_eyes(_: &Path) -> io::Result<Box<dyn Eyes + Send>> {
    Err(io::Error::other("these tests do not look"))
}

fn no_image(name: &str) -> Result<Image, String> {
    Err(format!("no image named {name}"))
}

/// Sight for a macro with no `when`: never asked for anything.
fn blind() -> Sight<'static> {
    Sight { open: &no_eyes, image: &no_image }
}

/// The top of every range, so waits are known.
fn top(_lo: f64, hi: f64) -> f64 {
    hi
}

/// Play `lines`, one step each, into `r` with nothing held at the start.
fn steps(r: &mut Recorder, lines: &[&str]) -> io::Result<Held<'static>> {
    let mut held = Held::default();
    for line in lines {
        play_step(r, &step(line), &StopFlag::default(), &top, &mut held)?;
    }
    Ok(held)
}

#[test]
fn a_combo_is_pressed_in_order_held_and_released_in_reverse() {
    let mut r = Recorder::default();
    let mut held = Held::default();
    play_step(&mut r, &step("hold shift+w 1"), &StopFlag::default(), &|_, _| 0.0, &mut held)
        .unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [Sent::Key(42, true), Sent::Key(17, true), Sent::Key(17, false), Sent::Key(42, false)]
    );
}

#[test]
fn typed_text_shifts_what_a_us_keyboard_shifts() {
    let mut r = Recorder::default();
    let mut held = Held::default();
    play_step(&mut r, &step("type -G"), &StopFlag::default(), &|_, _| 0.0, &mut held).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [
            Sent::Key(12, true),
            Sent::Key(12, false),
            Sent::Key(42, true),
            Sent::Key(34, true),
            Sent::Key(34, false),
            Sent::Key(42, false),
        ]
    );
}

#[test]
fn a_click_at_a_point_puts_the_pointer_there_first() {
    let mut r = Recorder::default();
    steps(&mut r, &["click 10 20"]).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [Sent::MoveTo(10, 20), Sent::Button(BUTTON_LEFT, true), Sent::Button(BUTTON_LEFT, false)]
    );
}

#[test]
fn a_key_already_down_is_let_go_when_the_next_press_fails() {
    let mut r = Recorder::failing_on(17);
    let mut held = Held::default();
    let stop = StopFlag::default();
    assert!(play_step(&mut r, &step("hold shift+w 1"), &stop, &|_, _| 0.0, &mut held).is_err());
    assert_eq!(*r.sent.borrow(), [Sent::Key(42, true), Sent::Key(42, false)]);
}

#[test]
fn a_stop_cuts_a_hold_short_and_still_lets_go() {
    let stop = StopFlag::default();
    stop.set();
    let mut r = Recorder::default();
    play_step(&mut r, &step("hold w 600"), &stop, &top, &mut Held::default()).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Key(17, true), Sent::Key(17, false)]);
}

#[test]
fn a_press_leaves_its_key_down_until_a_release_and_buttons_go_to_the_pointer() {
    let mut r = Recorder::default();
    steps(&mut r, &["press w", "press mouse2", "hold mouse1 0", "release mouse2", "release w"])
        .unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [
            Sent::Key(17, true),
            Sent::Button(BUTTON_RIGHT, true),
            Sent::Button(BUTTON_LEFT, true),
            Sent::Button(BUTTON_LEFT, false),
            Sent::Button(BUTTON_RIGHT, false),
            Sent::Key(17, false),
        ]
    );
}

#[test]
fn a_key_is_pressed_once_and_only_what_is_down_is_released() {
    let mut r = Recorder::default();
    steps(&mut r, &["release e", "press w", "press w", "release w", "release w"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Key(17, true), Sent::Key(17, false)]);
}

#[test]
fn letting_go_of_everything_releases_the_last_pressed_first() {
    let mut r = Recorder::default();
    let mut held = steps(&mut r, &["press shift", "press w", "press mouse1"]).unwrap();
    r.sent.borrow_mut().clear();
    held.release_all(&mut r).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [Sent::Button(BUTTON_LEFT, false), Sent::Key(17, false), Sent::Key(42, false)]
    );
    held.release_all(&mut r).unwrap();
    assert_eq!(r.sent.borrow().len(), 3, "nothing is let go of twice");
}

#[test]
fn a_move_to_glides_from_the_last_point_in_small_even_steps() {
    let mut r = Recorder::default();
    steps(&mut r, &["move to 0 0", "move to 100 -50 0.05"]).unwrap();
    assert_eq!(
        *r.sent.borrow(),
        [
            Sent::MoveTo(0, 0),
            Sent::MoveTo(20, -10),
            Sent::MoveTo(40, -20),
            Sent::MoveTo(60, -30),
            Sent::MoveTo(80, -40),
            Sent::MoveTo(100, -50),
        ]
    );
}

#[test]
fn a_glide_sends_nothing_for_a_tick_that_moves_less_than_a_pixel() {
    let mut r = Recorder::default();
    steps(&mut r, &["move to 0 0", "move to 2 0 0.05"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::MoveTo(0, 0), Sent::MoveTo(1, 0), Sent::MoveTo(2, 0)]);
}

#[test]
fn a_move_to_with_no_point_to_start_from_goes_straight_there() {
    let mut r = Recorder::default();
    steps(&mut r, &["move to 100 50 0.02"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::MoveTo(100, 50)]);
}

#[test]
fn a_glide_starts_from_where_a_click_or_a_move_left_the_pointer() {
    let mut r = Recorder::default();
    steps(&mut r, &["click 10 10", "move 10 0", "move to 40 10 0.02"]).unwrap();
    let sent = r.sent.borrow();
    assert_eq!(&sent[sent.len() - 2..], [Sent::MoveTo(30, 10), Sent::MoveTo(40, 10)]);
}

#[test]
fn a_scroll_turns_the_wheel() {
    let mut r = Recorder::default();
    steps(&mut r, &["scroll up 3", "scroll right"]).unwrap();
    assert_eq!(*r.sent.borrow(), [Sent::Scroll(false, -3), Sent::Scroll(true, 1)]);
}

/// A player whose client is up when `up` says so and whose display is `input`.
fn run(
    m: &str,
    stop: &StopFlag,
    up: &dyn Fn() -> bool,
    connect: &dyn Fn(&Path) -> io::Result<Box<dyn Input>>,
) -> (Result<(), MacroError>, Vec<String>) {
    let reports = RefCell::new(Vec::new());
    // Just before a second boundary, so a short wait shows in the clock time.
    let now =
        || Local.with_ymd_and_hms(2026, 9, 29, 11, 59, 59).unwrap() + Duration::from_millis(700);
    let player = Player {
        display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        running: up,
        connect,
        report: &|line| reports.borrow_mut().push(line),
        pick: &top,
        now: &now,
        start: Instant::now(),
        sight: &blind(),
    };
    let got = player.play(&parse(m).unwrap(), stop);
    (got, reports.into_inner())
}

/// A display that records into `sent`, and notes in `closed` when it is let go of.
fn recording_into(
    sent: &Rc<RefCell<Vec<Sent>>>,
    closed: &Rc<RefCell<bool>>,
    fail_on: Option<u16>,
) -> impl Fn(&Path) -> io::Result<Box<dyn Input>> {
    let (sent, closed) = (Rc::clone(sent), Rc::clone(closed));
    move |_: &Path| -> io::Result<Box<dyn Input>> {
        let (sent, closed) = (Rc::clone(&sent), Rc::clone(&closed));
        Ok(Box::new(Recorder { sent, closed, fail_on, stop_on: None }))
    }
}

/// A display that records into `sent` and sets `stop` once `at` reaches it.
fn stopping_at(
    sent: &Rc<RefCell<Vec<Sent>>>,
    at: Sent,
    stop: &StopFlag,
) -> impl Fn(&Path) -> io::Result<Box<dyn Input>> {
    let (sent, stop_on) = (Rc::clone(sent), Some((at, stop.clone())));
    move |_: &Path| -> io::Result<Box<dyn Input>> {
        let sent = Rc::clone(&sent);
        let stop_on = stop_on.clone();
        Ok(Box::new(Recorder { sent, closed: Rc::default(), fail_on: None, stop_on }))
    }
}

#[test]
fn a_playing_macro_says_each_step_it_is_at_and_the_waits_worth_watching() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let stop = StopFlag::default();
    let reports = RefCell::new(Vec::new());
    let now =
        || Local.with_ymd_and_hms(2026, 9, 29, 11, 59, 59).unwrap() + Duration::from_millis(700);
    let player = Player {
        display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        running: &|| true,
        connect: &recording_into(&sent, &closed, None),
        // Stopped as soon as the long wait is told: it is never waited out.
        report: &|line: String| {
            if line.contains("waiting") {
                stop.set();
            }
            reports.borrow_mut().push(line);
        },
        pick: &top,
        now: &now,
        start: Instant::now(),
        sight: &blind(),
    };
    let m = parse("start 0.1\ntap e\ntap e\nwait 0\nmove to 1 1\nmove to 2 2\nwait 5\ntap e\n");
    player.play(&m.unwrap(), &stop).unwrap();
    assert_eq!(
        reports.into_inner(),
        [
            "round 1, step 2/8: pressing e",
            "round 1, step 3/8: pressing e",
            "round 1, step 5/8: moving the mouse",
            "round 1, step 6/8: moving the mouse",
            "round 1, step 7/8: waiting 5s, until 12:00:04",
        ]
    );
}

#[test]
fn a_finished_macro_lets_go_of_its_display() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, _) = run(
        "tap e\nloop 1\n",
        &StopFlag::default(),
        &|| true,
        &recording_into(&sent, &closed, None),
    );
    got.unwrap();
    assert!(*closed.borrow());
}

#[test]
fn every_round_lets_go_of_what_it_pressed_and_reports_itself() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, reports) = run(
        "press w\npress mouse1\nloop 2\n",
        &StopFlag::default(),
        &|| true,
        &recording_into(&sent, &closed, None),
    );
    got.unwrap();
    let round = [
        Sent::Key(17, true),
        Sent::Button(BUTTON_LEFT, true),
        Sent::Button(BUTTON_LEFT, false),
        Sent::Key(17, false),
    ];
    assert_eq!(*sent.borrow(), [round.clone(), round].concat());
    assert_eq!(
        reports,
        [
            "round 1, step 1/2: holding down w",
            "round 1, step 2/2: holding down mouse1",
            "round 2, step 1/2: holding down w",
            "round 2, step 2/2: holding down mouse1",
        ]
    );
}

#[test]
fn a_macro_stopped_mid_round_lets_go_of_what_it_pressed() {
    let sent = Rc::default();
    let stop = StopFlag::default();
    let (got, _) = run(
        "press w\nwait 600\ntap e\n",
        &stop,
        &|| true,
        &stopping_at(&sent, Sent::Key(17, true), &stop),
    );
    got.unwrap();
    assert_eq!(*sent.borrow(), [Sent::Key(17, true), Sent::Key(17, false)]);
}

#[test]
fn a_display_that_fails_mid_macro_still_gets_its_keys_let_go() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, _) = run(
        "press w\npress e\n",
        &StopFlag::default(),
        &|| true,
        &recording_into(&sent, &closed, Some(18)),
    );
    assert!(matches!(&got, Err(MacroError::WentAway(_))), "{got:?}");
    assert_eq!(*sent.borrow(), [Sent::Key(17, true), Sent::Key(17, false)]);
    assert!(*closed.borrow());
}

#[test]
fn a_client_that_is_not_running_gets_nothing() {
    let connect = |_: &Path| -> io::Result<Box<dyn Input>> { unreachable!("never connects") };
    let (got, _) = run("tap e", &StopFlag::default(), &|| false, &connect);
    assert_eq!(got, Err(MacroError::NotRunning));
}

#[test]
fn a_client_in_a_normal_window_is_named_as_such() {
    let connect = |_: &Path| -> io::Result<Box<dyn Input>> { Err(io::ErrorKind::NotFound.into()) };
    assert_eq!(
        run("tap e", &StopFlag::default(), &|| true, &connect).0,
        Err(MacroError::NotNested)
    );
    let refused =
        |_: &Path| -> io::Result<Box<dyn Input>> { Err(io::ErrorKind::ConnectionRefused.into()) };
    assert_eq!(
        run("tap e", &StopFlag::default(), &|| true, &refused).0,
        Err(MacroError::NotNested)
    );
}

#[test]
fn a_display_that_goes_away_ends_the_macro_with_a_reason() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, _) =
        run("tap e", &StopFlag::default(), &|| true, &recording_into(&sent, &closed, Some(18)));
    assert!(
        matches!(&got, Err(MacroError::WentAway(why)) if why.contains("Broken pipe")),
        "{got:?}"
    );
    assert!(*closed.borrow());
}

#[test]
fn a_stopped_macro_sends_nothing_more() {
    let stop = StopFlag::default();
    stop.set();
    let (sent, closed) = (Rc::default(), Rc::default());
    run("tap e\nclick right", &stop, &|| true, &recording_into(&sent, &closed, None)).0.unwrap();
    assert!(sent.borrow().is_empty());
}

#[test]
fn a_stop_flag_knows_its_own_clones_from_another_flag() {
    let a = StopFlag::default();
    assert!(a.same_as(&a.clone()));
    assert!(!a.same_as(&StopFlag::default()));
}

/// Every range at its top, and taps held as long as a Repeat lets them: half
/// of `every`.
fn quick(lo: f64, hi: f64) -> f64 {
    if (lo, hi) == TAP_PRESS { 1.0 } else { hi }
}

#[test]
fn a_repeat_taps_behind_the_steps_after_it() {
    let mut r = Recorder::default();
    let held = steps(&mut r, &["repeat e 0.3 0.05", "hold f 0.2"]);
    let mut held = held.unwrap();
    let f_down = Sent::Key(33, true);
    let sent = r.sent.borrow().clone();
    let (start, end) = (
        sent.iter().position(|s| *s == f_down).unwrap(),
        sent.iter().position(|s| *s == Sent::Key(33, false)).unwrap(),
    );
    assert_eq!(sent[0], Sent::Key(18, true), "the first tap is at once");
    assert!(
        sent[start..end].iter().filter(|s| **s == Sent::Key(18, true)).count() >= 2,
        "e is tapped while f is held: {sent:?}"
    );
    held.release_all(&mut r).unwrap();
    let sent = r.sent.borrow();
    let e: Vec<_> = sent.iter().filter(|s| matches!(s, Sent::Key(18, _))).collect();
    assert!(e.chunks(2).all(|t| *t == [&Sent::Key(18, true), &Sent::Key(18, false)]), "{e:?}");
}

#[test]
fn a_round_ends_once_its_repeats_have() {
    let started = Instant::now();
    let mut held = Held::default();
    let mut r = Recorder::default();
    let sent = Rc::clone(&r.sent);
    let stop = StopFlag::default();
    play_step(&mut r, &step("repeat e 0.2 0.05"), &stop, &quick, &mut held).unwrap();
    assert!(!held.finish_repeats(&mut r, &stop, &quick).unwrap());
    assert!(started.elapsed() >= Duration::from_millis(150));
    let taps = sent.borrow().iter().filter(|s| **s == Sent::Key(18, true)).count();
    assert!((3..=5).contains(&taps), "{taps} taps in 0.2 s, one every 0.05 s");
    assert_eq!(sent.borrow().last(), Some(&Sent::Key(18, false)));
}

#[test]
fn a_macro_stopped_mid_repeat_lets_go_of_the_key() {
    let sent = Rc::default();
    let stop = StopFlag::default();
    let (got, _) = run(
        "repeat e 600 10\nwait 600\n",
        &stop,
        &|| true,
        &stopping_at(&sent, Sent::Key(18, true), &stop),
    );
    got.unwrap();
    assert_eq!(*sent.borrow(), [Sent::Key(18, true), Sent::Key(18, false)]);
}

#[test]
fn a_wait_makes_up_for_the_time_a_slow_step_took() {
    let started = Instant::now();
    let mut held = Held::default();
    let mut r = Recorder::default();
    let stop = StopFlag::default();
    held.idle(&mut r, 0.1, &stop, &top).unwrap();
    // A step that took 50 ms to send, the display lagging.
    std::thread::sleep(Duration::from_millis(50));
    held.idle(&mut r, 0.1, &stop, &top).unwrap();
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(200), "{took:?}");
    assert!(took < Duration::from_millis(245), "{took:?}: two waits of 0.1 s, nothing added");
}

#[test]
fn a_macro_stalled_longer_than_it_can_make_up_carries_on_from_where_it_is() {
    let mut held = Held::default();
    let mut r = Recorder::default();
    let stop = StopFlag::default();
    held.idle(&mut r, 0.0, &stop, &top).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    let resumed = Instant::now();
    held.idle(&mut r, 0.1, &stop, &top).unwrap();
    assert!(resumed.elapsed() >= Duration::from_millis(100), "the wait is not skipped");
}

mod alike;
mod exit;
mod start;
mod when;
