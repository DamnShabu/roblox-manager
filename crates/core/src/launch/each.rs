//! Every account into the target, all at once.

use super::{LaunchAccount, Run, url};
use crate::types::{PlaceId, ServerId};

/// Each account into `place` (into `server` when given), every one at the
/// same time. An account already running is skipped. A stop before it
/// begins starts nobody.
pub(super) fn launch(
    run: &Run<'_>,
    accounts: &[LaunchAccount],
    place: Option<&PlaceId>,
    server: Option<&ServerId>,
) {
    if run.stopped() {
        return run.give_up(accounts);
    }
    let mut starting = Vec::new();
    for a in accounts {
        match run.running(a) {
            Ok(true) => run.log(format!("{}: already running -- skipping launch", a.label)),
            Ok(false) => starting.push(a.clone()),
            Err(why) => run.log(format!("{}: FAILED -- {why}", a.label)),
        }
    }
    run.start_all(&starting, url(place, server).as_deref(), "launched");
}
