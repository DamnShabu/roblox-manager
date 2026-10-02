//! A group: the leader first, then the rest into its server.

use super::{LaunchAccount, Run, url};
use crate::types::{PlaceId, ServerId};

/// Launch the leader, wait for its server, then send the rest into it.
/// Returns the server everyone joined, or None when the leader never
/// reported one -- the followers then still launch, into their own servers,
/// rather than silently not launching at all.
pub(super) fn launch(
    run: &Run<'_>,
    leader: &LaunchAccount,
    followers: &[LaunchAccount],
    place: Option<&PlaceId>,
) -> Option<ServerId> {
    if run.stopped() {
        let everyone: Vec<LaunchAccount> =
            std::iter::once(leader).chain(followers).cloned().collect();
        run.give_up(&everyone);
        return None;
    }
    let already_running = match run.running(leader) {
        Ok(true) => {
            run.log(format!("{}: leader is already running, looking for its server", leader.label));
            true
        }
        Ok(false) => match run.start(leader, url(place, None)) {
            Ok(()) => false,
            Err(why) => {
                run.log(format!(
                    "{}: leader FAILED -- {why}; nobody has a server to join",
                    leader.label
                ));
                return None;
            }
        },
        Err(why) => {
            run.log(format!(
                "{}: leader FAILED -- {why}; nobody has a server to join",
                leader.label
            ));
            return None;
        }
    };
    // Nobody to place means nothing to look up: polling here would burn the
    // whole timeout answering a question with no consumer.
    if followers.is_empty() {
        if !already_running {
            run.log(format!("{}: launched", leader.label));
        }
        return None;
    }
    if !already_running {
        run.log(format!("{}: launched, waiting for its server", leader.label));
    }
    let (server, leader_place) = wait_for_server(run, leader, already_running);
    if run.stopped() {
        run.give_up(followers);
        return server;
    }
    let mut joining = Vec::new();
    for a in followers {
        match run.running(a) {
            Ok(true) => run.log(format!("{}: already running -- skipping launch", a.label)),
            Ok(false) => joining.push(a.clone()),
            Err(why) => run.log(format!("{}: FAILED -- {why}", a.label)),
        }
    }
    let launched =
        server.as_ref().map_or_else(|| "launched".into(), |s| format!("launched into {s}"));
    let target = leader_place.as_ref().or(place);
    run.start_all(&joining, url(target, server.as_ref()).as_deref(), &launched);
    server
}

/// Poll the leader's presence until it reports a server, time runs out, or
/// the launch is stopped. A leader that was already running is asked at
/// once; a new one gets a poll's time to arrive first.
fn wait_for_server(
    run: &Run<'_>,
    leader: &LaunchAccount,
    already_running: bool,
) -> (Option<ServerId>, Option<PlaceId>) {
    let pacing = run.pacing();
    let secs = pacing.leader_timeout.as_secs();
    let mut waited = std::time::Duration::ZERO;
    while waited < pacing.leader_timeout {
        if (!already_running || !waited.is_zero()) && !run.pause(pacing.poll) {
            return (None, None);
        }
        waited += pacing.poll;
        match run.presence(leader) {
            Ok(p) => {
                if let Some(server) = p.server {
                    run.log(format!("{}: in server {server}", leader.label));
                    return (Some(server), p.place);
                }
            }
            // A failed poll is not a failed launch.
            Err(why) => run.log(format!("{}: presence poll failed ({why})", leader.label)),
        }
        if waited < pacing.leader_timeout {
            run.log(format!(
                "{}: waiting for server ({}s/{secs}s)...",
                leader.label,
                waited.as_secs()
            ));
        }
    }
    run.log(format!("{}: no server after {secs}s -- followers get their own", leader.label));
    (None, None)
}
