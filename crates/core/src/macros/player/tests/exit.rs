//! An Exit among a macro's own steps: the round ends there.

use super::*;

#[test]
fn an_exit_ends_the_round_lets_go_and_starts_the_next() {
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, reports) = run(
        "press w\ntap e\nexit\ntap f\nloop 2\n",
        &StopFlag::default(),
        &|| true,
        &recording_into(&sent, &closed, None),
    );
    got.unwrap();
    let (w, e) = (keys::key_code("w").unwrap(), keys::key_code("e").unwrap());
    let round = [Sent::Key(w, true), Sent::Key(e, true), Sent::Key(e, false), Sent::Key(w, false)];
    assert_eq!(*sent.borrow(), [round.clone(), round].concat(), "f is never tapped");
    assert_eq!(reports[2], "round 1, step 3/4: ending the round");
    assert_eq!(reports[3], "round 2, step 1/4: holding down w");
}

#[test]
fn an_exit_stops_a_repeat_rather_than_waiting_it_out() {
    let started = Instant::now();
    let (sent, closed) = (Rc::default(), Rc::default());
    let (got, _) = run(
        "repeat e 600 10\nexit\nloop 1\n",
        &StopFlag::default(),
        &|| true,
        &recording_into(&sent, &closed, None),
    );
    got.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(*sent.borrow(), [Sent::Key(18, true), Sent::Key(18, false)]);
}
