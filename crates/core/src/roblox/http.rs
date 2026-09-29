//! Talking HTTP to Roblox: the transport seam, the CSRF dance every POST
//! needs, and turning a refusal into something a user can act on.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::RobloxError;
use crate::types::Cookie;

/// What the manager says it is. Roblox's web APIs answer a desktop client.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Roblox/WinInet";

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// One request to Roblox.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub cookie: Option<Cookie>,
    pub csrf: Option<String>,
    /// Send `Referer: https://www.roblox.com/`, which the login endpoint wants.
    pub referer: bool,
    pub json: Option<Value>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Request {
            method: Method::Get,
            url: url.into(),
            cookie: None,
            csrf: None,
            referer: false,
            json: None,
        }
    }

    pub fn post(url: impl Into<String>, json: Value) -> Self {
        Request { method: Method::Post, json: Some(json), ..Request::get(url) }
    }

    pub fn as_user(mut self, cookie: &Cookie) -> Self {
        self.cookie = Some(cookie.clone());
        self
    }

    pub fn with_referer(mut self) -> Self {
        self.referer = true;
        self
    }
}

/// Roblox's answer, whatever its status.
#[derive(Clone, Debug, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// The first header called `name`, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Every header called `name`, ignoring case (Set-Cookie comes several times).
    pub fn headers_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, RobloxError> {
        serde_json::from_slice(&self.body).map_err(|e| {
            RobloxError::BadResponse(format!(
                "HTTP {} with a body that is not the JSON expected ({e})",
                self.status
            ))
        })
    }
}

/// How requests reach Roblox. Adapters: [`UreqTransport`], and canned
/// responses in the self-check. A non-2xx answer is a [`Response`], not an
/// error; `Err` means Roblox was not reached at all.
pub trait Transport: Send + Sync {
    fn send(&self, req: &Request) -> Result<Response, RobloxError>;
}

/// HTTPS through ureq.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .user_agent(USER_AGENT)
            .build();
        UreqTransport { agent: ureq::Agent::new_with_config(config) }
    }
}

impl Transport for UreqTransport {
    fn send(&self, req: &Request) -> Result<Response, RobloxError> {
        let mut headers: Vec<(&str, String)> = Vec::new();
        if let Some(cookie) = &req.cookie {
            headers.push(("Cookie", format!(".ROBLOSECURITY={}", cookie.expose())));
        }
        if let Some(token) = &req.csrf {
            headers.push(("X-CSRF-TOKEN", token.clone()));
        }
        if req.referer {
            headers.push(("Referer", "https://www.roblox.com/".to_owned()));
        }
        let result = match req.method {
            Method::Get => {
                let mut r = self.agent.get(&req.url);
                for (k, v) in &headers {
                    r = r.header(*k, v);
                }
                r.call()
            }
            Method::Post => {
                let mut r = self.agent.post(&req.url);
                for (k, v) in &headers {
                    r = r.header(*k, v);
                }
                match &req.json {
                    Some(json) => r.send_json(json),
                    None => r.send_empty(),
                }
            }
        };
        let mut resp = result.map_err(|e| RobloxError::Offline(e.to_string()))?;
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let body =
            resp.body_mut().read_to_vec().map_err(|e| RobloxError::Offline(e.to_string()))?;
        Ok(Response { status: resp.status().as_u16(), headers, body })
    }
}

/// Send, and for a POST learn the CSRF token from the first 403 and retry
/// once with it: Roblox hands the token out on the rejection, not on request.
pub fn send(t: &dyn Transport, mut req: Request) -> Result<Response, RobloxError> {
    let first = t.send(&req)?;
    if req.method != Method::Post || first.status != 403 {
        return Ok(first);
    }
    let Some(token) = first.header("x-csrf-token") else {
        return Ok(first);
    };
    req.csrf = Some(token.to_owned());
    t.send(&req)
}

/// The response when it is a success; otherwise an error. A 401 on a request
/// made as a user is [`RobloxError::Expired`].
pub fn ok(resp: Response, as_user: bool) -> Result<Response, RobloxError> {
    match resp.status {
        200..=299 => Ok(resp),
        401 if as_user => Err(RobloxError::Expired),
        status => Err(RobloxError::Http { status, detail: detail(&resp) }),
    }
}

/// A refusal as something a user can act on: the status, the 2FA challenge
/// Roblox announces only in a header, and the start of Roblox's own reason.
pub fn detail(resp: &Response) -> String {
    let mut bits = vec![format!("HTTP {}", resp.status)];
    if resp.header("rblx-challenge-type").is_some() || resp.header("rbx-challenge-id").is_some() {
        bits.push(
            "Roblox wants a 2FA/security challenge for this account, which this flow cannot \
             answer -- approve the login on a device where that account is already verified"
                .to_owned(),
        );
    }
    let start = &resp.body[..resp.body.len().min(400)];
    let body = String::from_utf8_lossy(start);
    if !body.trim().is_empty() {
        bits.push(body.trim().to_owned());
    }
    bits.join(" -- ")
}

#[cfg(test)]
pub(crate) mod canned {
    //! A transport that answers from a script and remembers what it was asked.

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct Canned {
        answers: Mutex<VecDeque<Result<Response, RobloxError>>>,
        pub asked: Mutex<Vec<Request>>,
    }

    impl Canned {
        pub fn new() -> Self {
            Canned::default()
        }

        /// Queue an answer: a status and a JSON (or any) body.
        pub fn answer(self, status: u16, body: &str) -> Self {
            self.answer_with(status, &[], body)
        }

        pub fn answer_with(self, status: u16, headers: &[(&str, &str)], body: &str) -> Self {
            let resp = Response {
                status,
                headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                body: body.as_bytes().to_vec(),
            };
            self.answers.lock().unwrap().push_back(Ok(resp));
            self
        }

        pub fn asked(&self) -> Vec<Request> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Transport for Canned {
        fn send(&self, req: &Request) -> Result<Response, RobloxError> {
            self.asked.lock().unwrap().push(req.clone());
            self.answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("no canned answer left for {}", req.url))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::canned::Canned;
    use super::*;
    use serde_json::json;

    #[test]
    fn a_post_refused_for_its_csrf_token_is_retried_once_with_it() {
        let t = Canned::new().answer_with(403, &[("x-csrf-token", "tok")], "").answer(200, "{}");
        let resp = send(&t, Request::post("https://x/y", json!({}))).unwrap();
        assert_eq!(resp.status, 200);
        let asked = t.asked();
        assert_eq!((asked[0].csrf.as_deref(), asked[1].csrf.as_deref()), (None, Some("tok")));
    }

    #[test]
    fn a_403_without_a_token_is_not_retried() {
        let t = Canned::new().answer(403, "no");
        assert_eq!(send(&t, Request::post("https://x/y", json!({}))).unwrap().status, 403);
        assert_eq!(t.asked().len(), 1);
    }

    #[test]
    fn a_get_is_never_retried() {
        let t = Canned::new().answer_with(403, &[("x-csrf-token", "tok")], "");
        send(&t, Request::get("https://x/y")).unwrap();
        assert_eq!(t.asked().len(), 1);
    }

    #[test]
    fn a_401_as_a_user_is_an_expired_session() {
        let resp = Response { status: 401, ..Response::default() };
        assert!(matches!(ok(resp, true), Err(RobloxError::Expired)));
    }

    #[test]
    fn an_outage_page_is_an_http_error_with_its_start_in_the_detail() {
        let resp = Response {
            status: 503,
            body: b"<html>Service Unavailable</html>".to_vec(),
            ..Response::default()
        };
        match ok(resp, false) {
            Err(RobloxError::Http { status: 503, detail }) => {
                assert!(
                    detail.starts_with("HTTP 503") && detail.contains("Service Unavailable"),
                    "{detail}"
                )
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_2fa_challenge_is_named_in_the_detail() {
        let resp = Response {
            status: 403,
            headers: vec![("Rblx-Challenge-Type".into(), "twostepverification".into())],
            ..Response::default()
        };
        assert!(detail(&resp).contains("2FA"), "{}", detail(&resp));
    }

    #[test]
    fn the_detail_keeps_at_most_400_bytes_of_body() {
        let resp = Response { status: 500, body: vec![b'x'; 1000], ..Response::default() };
        assert!(detail(&resp).len() < 420);
    }
}
