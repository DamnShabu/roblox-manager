//! A group: the leader first, then the rest into its server.

use std::time::Duration;

use super::{LaunchAccount, Run, url};
use crate::cordial::JoiningWatch;
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
                leaderless(run, leader, followers, &why);
                return None;
            }
        },
        Err(why) => {
            leaderless(run, leader, followers, &why);
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
    // A leader just sent to the home screen is in no server and will not
    // be: waiting would cost the whole timeout before the rest start.
    let (server, leader_place) = if place.is_none() && !already_running {
        run.log(format!("{}: launched to the home screen; nobody to join", leader.label));
        (None, None)
    } else {
        if !already_running {
            run.log(format!("{}: launched, waiting for its server", leader.label));
        }
        wait_for_server(run, leader, already_running)
    };
    if run.stopped() {
        run.give_up(followers);
        return server;
    }
    let joining = run.split_running(followers);
    let launched =
        server.as_ref().map_or_else(|| "launched".into(), |s| format!("launched into {s}"));
    let target = leader_place.as_ref().or(place);
    run.start_all(&joining, url(target, server.as_ref()).as_deref(), &launched);
    server
}

/// The leader did not start, so there is no server to join and the rest
/// never launch. Each of them is reported as failed with that reason, so a
/// launch report never leaves out an account it was asked to start.
fn leaderless(run: &Run<'_>, leader: &LaunchAccount, followers: &[LaunchAccount], why: &str) {
    run.log(format!("{}: leader FAILED -- {why}; nobody has a server to join", leader.label));
    for a in followers {
        run.fail(a, format!("not started: the leader, {}, did not start", leader.label));
    }
}

/// Watch for the leader's server until one is known, time runs out, or the
/// launch is stopped. A leader started here names its server in its own log
/// the moment it starts joining, which is glanced at often; presence is asked
/// as well, less often, for a client whose Stacked does not print that line.
/// A leader that was already running is asked at once, and only presence
/// answers for it: its log predates this launch.
fn wait_for_server(
    run: &Run<'_>,
    leader: &LaunchAccount,
    already_running: bool,
) -> (Option<ServerId>, Option<PlaceId>) {
    let pacing = run.pacing();
    let secs = pacing.leader_timeout.as_secs();
    // A zero poll would never advance `waited`.
    let poll = pacing.poll.max(Duration::from_millis(1));
    let glance = pacing.glance.clamp(Duration::from_millis(1), poll);
    let mut log = (!already_running).then(|| run.joining_watch(leader));
    let mut waited = Duration::ZERO;
    let mut since_asked = if already_running { poll } else { Duration::ZERO };
    loop {
        if let Some((server, place)) = log.as_mut().and_then(JoiningWatch::server) {
            run.log(format!("{}: joining server {server}", leader.label));
            return (Some(server), Some(place));
        }
        if since_asked >= poll {
            since_asked = Duration::ZERO;
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
        if waited >= pacing.leader_timeout {
            break;
        }
        if !run.pause(glance) {
            return (None, None);
        }
        waited += glance;
        since_asked += glance;
    }
    run.log(format!("{}: no server after {secs}s -- followers get their own", leader.label));
    (None, None)
}
