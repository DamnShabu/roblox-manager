//! Quick Login: Roblox's own cross-device sign-in. The app asks Roblox for a
//! code, the user enters it at Roblox on a device they trust, and the
//! approved code is redeemed for a session. No password reaches this app,
//! and it never opens a browser.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::http::{self, Request, Response, Transport};
use super::{Roblox, RobloxError};
use crate::types::{Cookie, User};

/// Where the user enters the code.
pub const CONFIRM_URL: &str = "https://www.roblox.com/crossdevicelogin/confirmcode";
/// The code is short-lived, so poll briskly and give up rather than leave a
/// stale code sitting approved.
pub const POLL: Duration = Duration::from_secs(3);
pub const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickLoginCode {
    pub code: String,
    pub private_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickLoginStatus {
    Created,
    /// Entered at Roblox, not yet confirmed.
    UserLinked,
    Validated,
    Cancelled,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuickLoginError {
    /// The code went unapproved for its whole life; ask for a new one.
    #[error("no approval within {}s", TIMEOUT.as_secs())]
    CodeExpired,
    #[error("cancelled")]
    Cancelled,
    #[error("login was rejected at Roblox")]
    Rejected,
    #[error("could not get a code: {0}")]
    Create(RobloxError),
    #[error("could not check the code: {0}")]
    Status(RobloxError),
    #[error("redeeming the code failed: {0}")]
    Redeem(RobloxError),
    #[error("{0}")]
    Roblox(RobloxError),
}

/// What the flow tells whoever shows it.
pub trait QuickLoginEvents {
    /// The code to show the user.
    fn code(&mut self, code: &str);
    /// Seconds left before the code expires.
    fn tick(&mut self, secs_left: u64);
    /// Each poll's answer.
    fn status(&mut self, status: &QuickLoginStatus);
    fn log(&mut self, line: String);
}

/// The whole add-an-account flow: a code, polled until approved, redeemed for
/// a session, and the user it belongs to.
pub fn quick_login(
    roblox: &dyn Roblox,
    events: &mut dyn QuickLoginEvents,
    cancelled: &dyn Fn() -> bool,
    sleep: &dyn Fn(Duration),
) -> Result<(Cookie, User), QuickLoginError> {
    let code = roblox.quick_login_create().map_err(QuickLoginError::Create)?;
    events.code(&code.code);
    events.log(format!("Enter code {} at {CONFIRM_URL}", code.code));
    let mut waited = Duration::ZERO;
    while waited < TIMEOUT {
        if cancelled() {
            return Err(QuickLoginError::Cancelled);
        }
        events.tick((TIMEOUT - waited).as_secs());
        sleep(POLL);
        waited += POLL;
        let status = roblox.quick_login_status(&code).map_err(QuickLoginError::Status)?;
        events.status(&status);
        match status {
            QuickLoginStatus::Validated => {
                let cookie = roblox.quick_login_redeem(&code).map_err(QuickLoginError::Redeem)?;
                let user = roblox.whoami(&cookie).map_err(QuickLoginError::Roblox)?;
                return Ok((cookie, user));
            }
            QuickLoginStatus::Cancelled => return Err(QuickLoginError::Rejected),
            QuickLoginStatus::UserLinked => {
                events.log(format!("Code {} accepted -- now confirm it at Roblox", code.code));
            }
            QuickLoginStatus::Created | QuickLoginStatus::Other(_) => {}
        }
    }
    Err(QuickLoginError::CodeExpired)
}

pub(super) fn create(t: &dyn Transport) -> Result<QuickLoginCode, RobloxError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Created {
        code: Option<String>,
        private_key: Option<String>,
    }
    let req =
        Request::post("https://apis.roblox.com/auth-token-service/v1/login/create", json!({}));
    let got: Created = http::ok(http::send(t, req)?, false)?.json()?;
    match (got.code, got.private_key) {
        (Some(code), Some(private_key)) if !code.is_empty() && !private_key.is_empty() => {
            Ok(QuickLoginCode { code, private_key })
        }
        _ => Err(RobloxError::BadResponse("Quick Login create returned no code".into())),
    }
}

pub(super) fn status(
    t: &dyn Transport,
    code: &QuickLoginCode,
) -> Result<QuickLoginStatus, RobloxError> {
    #[derive(Deserialize)]
    struct Status {
        status: Option<String>,
    }
    let req = Request::post(
        "https://apis.roblox.com/auth-token-service/v1/login/status",
        json!({ "code": code.code, "privateKey": code.private_key }),
    );
    let got: Status = http::ok(http::send(t, req)?, false)?.json()?;
    Ok(match got.status.as_deref() {
        Some("Created") => QuickLoginStatus::Created,
        Some("UserLinked") => QuickLoginStatus::UserLinked,
        Some("Validated") => QuickLoginStatus::Validated,
        Some("Cancelled") => QuickLoginStatus::Cancelled,
        other => QuickLoginStatus::Other(other.unwrap_or("Unknown").to_owned()),
    })
}

pub(super) fn redeem(t: &dyn Transport, code: &QuickLoginCode) -> Result<Cookie, RobloxError> {
    let req = Request::post(
        "https://auth.roblox.com/v2/login",
        json!({ "ctype": "AuthToken", "cvalue": code.code, "password": code.private_key }),
    )
    .with_referer();
    let resp = http::ok(http::send(t, req)?, false)?;
    session_cookie(&resp).ok_or_else(|| {
        RobloxError::BadResponse(
            "Roblox accepted the code but sent no .ROBLOSECURITY cookie back".into(),
        )
    })
}

/// `.ROBLOSECURITY` out of a response's Set-Cookie headers.
fn session_cookie(resp: &Response) -> Option<Cookie> {
    resp.headers_named("set-cookie").flat_map(|raw| raw.split(';')).find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == ".ROBLOSECURITY" && !v.is_empty()).then(|| Cookie::new(v))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roblox::HttpRoblox;
    use crate::roblox::http::canned::Canned;
    use crate::types::UserId;
    use std::cell::Cell;
    use std::sync::Arc;

    #[derive(Default)]
    struct Seen {
        code: Option<String>,
        statuses: Vec<QuickLoginStatus>,
        ticks: Vec<u64>,
    }

    impl QuickLoginEvents for Seen {
        fn code(&mut self, code: &str) {
            self.code = Some(code.to_owned());
        }
        fn tick(&mut self, secs_left: u64) {
            self.ticks.push(secs_left);
        }
        fn status(&mut self, status: &QuickLoginStatus) {
            self.statuses.push(status.clone());
        }
        fn log(&mut self, _line: String) {}
    }

    const CREATED: &str = r#"{"code": "ABC123", "privateKey": "k"}"#;

    fn run(
        t: Canned,
        cancel_after: Option<usize>,
    ) -> (Result<(Cookie, User), QuickLoginError>, Seen) {
        let roblox = HttpRoblox::new(Arc::new(t));
        let mut seen = Seen::default();
        let polls = Cell::new(0);
        let cancelled = || cancel_after.is_some_and(|n| polls.get() >= n);
        let sleep = |_d: Duration| polls.set(polls.get() + 1);
        let got = quick_login(&roblox, &mut seen, &cancelled, &sleep);
        (got, seen)
    }

    #[test]
    fn an_approved_code_is_redeemed_for_a_session_and_its_user() {
        let t = Canned::new()
            .answer(200, CREATED)
            .answer(200, r#"{"status": "UserLinked"}"#)
            .answer(200, r#"{"status": "Validated"}"#)
            .answer_with(
                200,
                &[("set-cookie", ".ROBLOSECURITY=sess; domain=.roblox.com; HttpOnly")],
                "{}",
            )
            .answer(200, r#"{"id": 5, "name": "alt"}"#);
        let (got, seen) = run(t, None);
        let (cookie, user) = got.unwrap();
        assert_eq!((cookie.expose(), user.id), ("sess", UserId(5)));
        assert_eq!(seen.code.as_deref(), Some("ABC123"));
        assert_eq!(seen.statuses, vec![QuickLoginStatus::UserLinked, QuickLoginStatus::Validated]);
        assert_eq!(seen.ticks, vec![180, 177]);
    }

    #[test]
    fn the_redeem_sends_the_code_the_key_and_a_referer() {
        let t = Arc::new(Canned::new().answer_with(
            200,
            &[("Set-Cookie", "a=b; .ROBLOSECURITY=s")],
            "{}",
        ));
        let code = QuickLoginCode { code: "C".into(), private_key: "K".into() };
        assert_eq!(redeem(&*t, &code).unwrap().expose(), "s");
        let asked = &t.asked()[0];
        assert!(asked.referer);
        assert_eq!(asked.json.as_ref().unwrap()["cvalue"], "C");
        assert_eq!(asked.json.as_ref().unwrap()["password"], "K");
    }

    #[test]
    fn a_code_rejected_at_roblox_ends_the_flow() {
        let t = Canned::new().answer(200, CREATED).answer(200, r#"{"status": "Cancelled"}"#);
        assert_eq!(run(t, None).0.unwrap_err(), QuickLoginError::Rejected);
    }

    #[test]
    fn a_code_nobody_approves_expires() {
        let mut t = Canned::new().answer(200, CREATED);
        for _ in 0..60 {
            t = t.answer(200, r#"{"status": "Created"}"#);
        }
        assert_eq!(run(t, None).0.unwrap_err(), QuickLoginError::CodeExpired);
    }

    #[test]
    fn the_dialog_can_stop_the_poll() {
        let t = Canned::new().answer(200, CREATED).answer(200, r#"{"status": "Created"}"#);
        assert_eq!(run(t, Some(1)).0.unwrap_err(), QuickLoginError::Cancelled);
    }

    #[test]
    fn an_approval_that_brings_no_session_is_an_error() {
        let t = Canned::new()
            .answer(200, CREATED)
            .answer(200, r#"{"status": "Validated"}"#)
            .answer(200, "{}");
        assert!(matches!(
            run(t, None).0,
            Err(QuickLoginError::Redeem(RobloxError::BadResponse(_)))
        ));
    }

    #[test]
    fn a_create_without_a_code_is_an_error() {
        let t = Canned::new().answer(200, "{}");
        assert!(matches!(run(t, None).0, Err(QuickLoginError::Create(_))));
    }
}
