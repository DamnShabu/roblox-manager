//! A macro's `when`s, played as the player answers them: straight away,
//! wherever it is in its own steps.

use std::sync::{Arc, Mutex};

use super::*;
use crate::macros::sight::Area;

/// Eyes that see `before` until `after` has passed, then `then`.
struct Turns {
    from: Instant,
    after: Duration,
    before: [u8; 3],
    then: [u8; 3],
    looks: Arc<Mutex<u32>>,
}

impl Eyes for Turns {
    fn look(&mut self, _: Area) -> io::Result<Image> {
        *self.looks.lock().unwrap() += 1;
        let rgb = if self.from.elapsed() < self.after { self.before } else { self.then };
        Ok(Image { width: 1, height: 1, rgb: rgb.to_vec() })
    }
}

/// Play `m` into a recorder, its eyes turning red `after` in, until `stop`.
fn play_seeing(
    m: &str,
    after: Duration,
    stop: &StopFlag,
) -> (Result<(), MacroError>, Vec<Sent>, Vec<String>) {
    let (sent, closed) = (Rc::default(), Rc::default());
    let reports = RefCell::new(Vec::new());
    let looks = Arc::new(Mutex::new(0));
    let from = Instant::now();
    let open = |_: &Path| -> io::Result<Box<dyn Eyes + Send>> {
        let looks = Arc::clone(&looks);
        Ok(Box::new(Turns { from, after, before: [0, 0, 0], then: [255, 0, 0], looks }))
    };
    let player = Player {
        display: Path::new("/run/user/1000/rbxmgr/rbxmgr-7.wayland"),
        running: &|| true,
        connect: &recording_into(&sent, &closed, None),
        report: &|line| reports.borrow_mut().push(line),
        pick: &top,
        now: &Local::now,
        start: Instant::now(),
        sight: &Sight { open: &open, image: &no_image },
    };
    let got = player.play(&parse(m).unwrap(), stop);
    let sent = sent.borrow().clone();
    (got, sent, reports.into_inner())
}

#[test]
fn a_when_plays_the_moment_it_sees_in_the_middle_of_a_step() {
    let m = "hold w 1\nwhen color 0 0 #ff0000\ndo tap e\nloop 1\n";
    let (got, sent, reports) = play_seeing(m, Duration::from_millis(200), &StopFlag::default());
    got.unwrap();
    let (w, e) = (keys::key_code("w").unwrap(), keys::key_code("e").unwrap());
    assert_eq!(
        sent,
        [Sent::Key(w, true), Sent::Key(e, true), Sent::Key(e, false), Sent::Key(w, false)],
        "e tapped while w is held, not after it"
    );
    assert!(reports.iter().any(|r| r == "when 0, 0 turns #ff0000: pressing e"), "{reports:?}");
}

#[test]
fn a_macro_of_only_whens_plays_them_until_stopped() {
    let stop = StopFlag::default();
    let stopper = stop.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        stopper.set();
    });
    let m = "when color 0 0 #ff0000\ndo click 5 6\n";
    let (got, sent, _) = play_seeing(m, Duration::from_millis(100), &stop);
    timer.join().unwrap();
    got.unwrap();
    assert_eq!(sent[0], Sent::MoveTo(5, 6));
    assert_eq!(sent.iter().filter(|s| matches!(s, Sent::Button(_, true))).count(), 1, "once");
}

#[test]
fn a_when_naming_a_missing_image_stops_before_the_first_step() {
    let m = "tap e\nwhen image coin 1 1\ndo tap f\n";
    let (got, sent, _) = play_seeing(m, Duration::ZERO, &StopFlag::default());
    assert_eq!(got, Err(MacroError::Sight("no image named coin".into())));
    assert!(sent.is_empty());
}

#[test]
fn a_when_that_exits_cuts_the_round_short_from_the_middle_of_a_wait() {
    let started = Instant::now();
    let m =
        "press w\nwait 600\ntap f\nwhen color 0 0 #ff0000\ndo tap e\ndo exit\ndo tap g\nloop 1\n";
    let (got, sent, reports) = play_seeing(m, Duration::from_millis(200), &StopFlag::default());
    got.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "the wait is not waited out");
    let (w, e) = (keys::key_code("w").unwrap(), keys::key_code("e").unwrap());
    assert_eq!(
        sent,
        [Sent::Key(w, true), Sent::Key(e, true), Sent::Key(e, false), Sent::Key(w, false)],
        "neither f nor g, and w let go of"
    );
    assert!(
        reports.iter().any(|r| r == "when 0, 0 turns #ff0000: ending the round"),
        "{reports:?}"
    );
}

#[test]
fn a_when_leaves_a_key_the_steps_are_holding_down() {
    let m = "press w\nwait 1\nwhen color 0 0 #ff0000\ndo tap w\ndo press w\nloop 1\n";
    let (got, sent, _) = play_seeing(m, Duration::from_millis(200), &StopFlag::default());
    got.unwrap();
    let w = keys::key_code("w").unwrap();
    assert_eq!(sent, [Sent::Key(w, true), Sent::Key(w, false)], "down once, up at the round's end");
}

#[test]
fn a_repeat_caught_mid_tap_is_let_go_of_before_a_when_plays() {
    let m = "repeat e 1 0.5\nwait 1\nwhen color 0 0 #ff0000\ndo tap f\nloop 1\n";
    let (got, sent, _) = play_seeing(m, Duration::ZERO, &StopFlag::default());
    got.unwrap();
    let (e, f) = (keys::key_code("e").unwrap(), keys::key_code("f").unwrap());
    let f_down = sent.iter().position(|s| *s == Sent::Key(f, true)).unwrap();
    let e_ups = sent[..f_down].iter().filter(|s| **s == Sent::Key(e, false)).count();
    let e_downs = sent[..f_down].iter().filter(|s| **s == Sent::Key(e, true)).count();
    assert_eq!(e_downs, e_ups, "e is not held through the when: {sent:?}");
}

#[test]
fn a_when_leaves_a_key_a_hold_is_holding_down() {
    let m = "hold w 1\nwhen color 0 0 #ff0000\ndo tap w\ndo hold w 0.1\nloop 1\n";
    let (got, sent, _) = play_seeing(m, Duration::from_millis(200), &StopFlag::default());
    got.unwrap();
    let w = keys::key_code("w").unwrap();
    assert_eq!(sent, [Sent::Key(w, true), Sent::Key(w, false)], "down once, up at the hold's end");
}

#[test]
fn a_when_glides_from_where_the_steps_left_the_pointer_and_hands_it_back() {
    let m = "move to 100 100\nwait 1\nmove to 300 100 0.1\nwhen color 0 0 #ff0000\n\
             do move to 200 100 0.1\nloop 1\n";
    let (got, sent, _) = play_seeing(m, Duration::from_millis(200), &StopFlag::default());
    got.unwrap();
    let xs: Vec<i32> = sent
        .iter()
        .filter_map(|s| match s {
            Sent::MoveTo(x, _) => Some(*x),
            _ => None,
        })
        .collect();
    let at_200 = xs.iter().position(|&x| x == 200).unwrap();
    assert!(at_200 > 1, "the when glided rather than jumped: {xs:?}");
    assert!(xs[1..at_200].iter().all(|&x| x > 100 && x < 200), "{xs:?}");
    assert!(xs[at_200 + 1] > 200 && xs[at_200 + 1] < 300, "the steps glide on from 200: {xs:?}");
}
