//! Every account into the target, one after another.

use super::{LaunchAccount, Run, url};
use crate::types::{PlaceId, ServerId};

/// Each account into `place` (into `server` when given). An account already
/// running is skipped, and costs no wait; so does the first sign-in. A stop
/// ends it before the next sign-in.
pub(super) fn launch(
    run: &Run<'_>,
    accounts: &[LaunchAccount],
    place: Option<&PlaceId>,
    server: Option<&ServerId>,
) {
    let mut signed_in = false;
    for (i, a) in accounts.iter().enumerate() {
        if run.stopped() {
            return run.give_up(&accounts[i..]);
        }
        match run.running(a) {
            Ok(true) => {
                run.log(format!("{}: already running -- skipping launch", a.label));
                continue;
            }
            Ok(false) => {}
            Err(why) => {
                run.log(format!("{}: FAILED -- {why}", a.label));
                continue;
            }
        }
        if signed_in && !run.pause(run.pacing().stagger) {
            return run.give_up(&accounts[i..]);
        }
        signed_in = true;
        match run.start(a, url(place, server)) {
            Ok(()) => run.log(format!("{}: launched", a.label)),
            Err(why) => run.log(format!("{}: FAILED -- {why}", a.label)),
        }
    }
}
