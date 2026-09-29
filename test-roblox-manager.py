#!/usr/bin/env python3
"""Self-check for roblox-manager.py. Offline: no network, no flatpak,
no keyring, no browser, no GTK -- gi is stubbed so this runs under a bare python3.

    nix shell nixpkgs#python3 -c python3 test-roblox-manager.py
"""

import email.message
import importlib.util
import io
import json
import os
import sys
import tempfile
import types
import urllib.error
import urllib.parse
from datetime import datetime, timedelta, timezone

# Stub gi before importing the app: everything checked here is string building,
# the Cordial glue, the login flow and the launch state machines -- no GTK.
for _name in ("gi", "gi.repository"):
    sys.modules[_name] = types.ModuleType(_name)
sys.modules["gi"].require_version = lambda *a, **k: None


# Every GTK symbol must work both as a callable and as a base class, since the
# app subclasses Adw.ExpanderRow, Adw.ApplicationWindow and Adw.Application at
# import time.
class _Meta(type):
    def __getattr__(cls, _name):
        return _Meta("_leaf", (), {})


class _AnyBase(metaclass=_Meta):
    def __init__(self, *a, **k):
        pass

    def __getattr__(self, _name):
        return lambda *a, **k: None


class _Ns:
    def __getattr__(self, name):
        return _Meta(name, (_AnyBase,), {})


for sym in ("Adw", "Gtk", "GLib", "GObject", "Gio", "Gdk", "Pango"):
    setattr(sys.modules["gi.repository"], sym, _Ns())

_here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location(
    "rbxmgr", os.path.join(_here, "roblox-manager.py"))
rbx = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rbx)

failures = []


def check(label, cond, detail=""):
    if cond:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}" + (f" -- {detail}" if detail else ""))
        failures.append(label)


# ==========================================================================
# join link (the engine's own roblox://experiences/start form)
# ==========================================================================
check("a game is joined by place",
      rbx.join_url("606849621") == "roblox://experiences/start?placeId=606849621")
check("a server is joined by gameInstanceId",
      rbx.join_url("1818", "abc-def-123")
      == "roblox://experiences/start?placeId=1818&gameInstanceId=abc-def-123")
check("no place is Roblox's home screen, even with a server id",
      rbx.join_url("") is None and rbx.join_url(None, "abc") is None)
for _bad in (("1818&x=1", None), ("1818", "abc&accessCode=x")):
    try:
        rbx.join_url(*_bad)
        check(f"a value that would add a parameter is refused: {_bad}", False)
    except ValueError:
        check(f"a value that would add a parameter is refused: {_bad}", True)
check("a join link carries no ticket", "ticket" not in rbx.join_url("1", "a-b")
      and "gameinfo" not in rbx.join_url("1", "a-b"))

# ==========================================================================
# CSRF retry: Roblox hands the token back on the rejection, not on request.
# ==========================================================================
class _Resp(io.BytesIO):
    def __init__(self, body=b"{}", headers=None):
        super().__init__(body)
        self.headers = headers or email.message.Message()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


def _http_error(code, headers):
    h = email.message.Message()
    for k, v in headers.items():
        h[k] = v
    return urllib.error.HTTPError("https://x", code, "no", h, None)


calls = []
_real_urlopen = rbx.urllib.request.urlopen


def fake_urlopen(req, timeout=None):
    calls.append(dict(req.headers))
    if len(calls) == 1:
        raise _http_error(403, {"x-csrf-token": "TOK-1"})
    return _Resp(b'{"ok": true}')


rbx.urllib.request.urlopen = fake_urlopen
with rbx.post_csrf("https://apis.roblox.com/x", body={}) as r:
    json.load(r)
check("403 without a token is retried once", len(calls) == 2, str(len(calls)))
check("first attempt sends no CSRF token",
      not any(k.lower() == "x-csrf-token" for k in calls[0]), str(calls[0]))
check("retry sends the token from the 403",
      calls[1].get("X-csrf-token") == "TOK-1", str(calls[1]))

# A non-403 error must propagate rather than be retried into a second failure.
calls.clear()


def fake_500(req, timeout=None):
    calls.append(1)
    raise _http_error(500, {})


rbx.urllib.request.urlopen = fake_500
try:
    rbx.post_csrf("https://apis.roblox.com/x", body={})
    check("a 500 propagates", False, "no exception raised")
except urllib.error.HTTPError as e:
    check("a 500 propagates", e.code == 500 and len(calls) == 1, f"{e.code}/{len(calls)}")

# An expired cookie is a 401; say what to do about it, not "HTTP 401".
rbx.urllib.request.urlopen = lambda req, timeout=None: (_ for _ in ()).throw(
    _http_error(401, {}))
try:
    rbx.authenticated_user("stale")
    check("an expired session names the fix", False, "no exception")
except RuntimeError as e:
    check("an expired session names the fix", "Sign in again" in str(e), str(e))
rbx.urllib.request.urlopen = _real_urlopen

# A 403 with no token must not be retried forever either.
calls.clear()


def fake_403_notoken(req, timeout=None):
    calls.append(1)
    raise _http_error(403, {})


rbx.urllib.request.urlopen = fake_403_notoken
try:
    rbx.post_csrf("https://apis.roblox.com/x", body={})
    check("403 without a token is not retried", False, "no exception raised")
except urllib.error.HTTPError:
    check("403 without a token is not retried", len(calls) == 1, str(len(calls)))

rbx.urllib.request.urlopen = _real_urlopen

# ==========================================================================
# Quick Login (browser flow)
# ==========================================================================
_app_src = open(os.path.join(_here, "roblox-manager.py")).read()

# The whole point of the chosen flow: the code is typed by a human at Roblox and
# is never handed to a browser in a URL, so nothing can be auto-navigated.
check("the confirm page is Roblox's own cross-device page",
      rbx.QL_CONFIRM_URL == "https://www.roblox.com/crossdevicelogin/confirmcode",
      rbx.QL_CONFIRM_URL)
check("no url is ever built with the code in it",
      "?code=" not in _app_src and "quick_login_url" not in _app_src,
      "found a code-bearing url builder")
# Call syntax, not prose: once()'s docstring names this function as the cause of
# the runaway-tab bug.
check("the browser is only ever sent to the bare confirm page",
      _app_src.count("launch_default_for_uri(") == 1
      and "launch_default_for_uri(QL_CONFIRM_URL, None)" in _app_src,
      f"{_app_src.count('launch_default_for_uri(')} call site(s)")

hdrs = ["foo=bar; Path=/",
        ".ROBLOSECURITY=_|WARNING|_abc123; Domain=.roblox.com; HttpOnly",
        "other=1"]
check("cookie is picked out of Set-Cookie",
      rbx.cookie_from_set_cookie(hdrs) == "_|WARNING|_abc123",
      str(rbx.cookie_from_set_cookie(hdrs)))
check("no cookie -> None", rbx.cookie_from_set_cookie(["a=b"]) is None)
check("a similarly named cookie is not mistaken for it",
      rbx.cookie_from_set_cookie(["ROBLOSECURITY=nope; Path=/"]) is None)


def drive_quick_login(statuses, cancel_after=None, timeout=9):
    """Run the flow with injected create/status/redeem.

    Returns (result, codes_shown, ticks, logs).
    """
    codes, ticks, logs, seq = [], [], [], list(statuses)
    state = {"polls": 0}
    rbx.quick_login_create = lambda: ("CODE9", "KEY9")
    rbx.quick_login_redeem = lambda c, k: "CK"
    rbx.authenticated_user = lambda ck: {"id": 42, "name": "n", "displayName": "D"}

    def status(c, k):
        state["polls"] += 1
        return seq.pop(0) if seq else "Created"

    rbx.quick_login_status = status

    def cancelled():
        return cancel_after is not None and state["polls"] >= cancel_after

    try:
        out = rbx.quick_login(
            log=logs.append, on_code=codes.append, on_tick=ticks.append,
            cancelled=cancelled, poll=3, timeout=timeout, sleep=lambda _s: None)
    except Exception as e:
        out = e
    return out, codes, ticks, logs


_save = (rbx.quick_login_create, rbx.quick_login_status,
         rbx.quick_login_redeem, rbx.authenticated_user)

out, codes, ticks, logs = drive_quick_login(["Validated"])
check("validated login returns cookie and user",
      out == ("CK", {"id": 42, "name": "n", "displayName": "D"}), str(out))
check("the code is shown in the app exactly once", codes == ["CODE9"], str(codes))
check("the log tells the user where to enter it",
      any("CODE9" in m and rbx.QL_CONFIRM_URL in m for m in logs), str(logs))

out, codes, ticks, logs = drive_quick_login(
    ["Created", "UserLinked", "Validated"], timeout=90)
check("login waits through Created/UserLinked", out[0] == "CK", str(out))
check("UserLinked is surfaced to the user",
      any("now confirm it" in m for m in logs), str(logs))
check("the countdown is handed out once per poll", len(ticks) == 3, str(ticks))
check("the countdown counts down", ticks == sorted(ticks, reverse=True), str(ticks))
check("the countdown starts at the timeout", ticks[0] == 90, str(ticks))

out, _, _, _ = drive_quick_login(["Cancelled"])
check("a rejected login raises", isinstance(out, RuntimeError), str(out))
check("rejection says it was rejected", "rejected" in str(out), str(out))

out, _, _, _ = drive_quick_login(["Created"] * 5)
check("an unapproved login times out", isinstance(out, RuntimeError), str(out))
check("an expired code is told apart, so the dialog can fetch a new one",
      isinstance(out, rbx.CodeExpired), repr(out))
check("timeout does not redeem anything", "approval" in str(out), str(out))

# Closing the dialog must stop the poll rather than leave a thread running out
# the full timeout against Roblox.
out, _, ticks, _ = drive_quick_login(["Created"] * 5, cancel_after=1, timeout=900)
check("cancelling stops the poll", isinstance(out, RuntimeError), str(out))
check("cancelling is reported as cancelled", str(out) == "cancelled", str(out))
check("cancelling stops it early, not at the timeout", len(ticks) <= 2, str(ticks))

(rbx.quick_login_create, rbx.quick_login_status,
 rbx.quick_login_redeem, rbx.authenticated_user) = _save

# ==========================================================================
# Failure reporting. A failure after approval used to close the dialog exactly
# like a success did, so "it detected the code, closed, and added nothing" was
# indistinguishable from working.
# ==========================================================================
def _err(code, headers=None, body=b""):
    h = email.message.Message()
    for k, v in (headers or {}).items():
        h[k] = v
    return urllib.error.HTTPError("https://x", code, "no", h, io.BytesIO(body))


d = rbx.http_detail(_err(403))
check("http detail names the status code", d == "HTTP 403", d)

d = rbx.http_detail(_err(429, body=b'{"errors":[{"message":"Too many requests"}]}'))
check("http detail includes the response body", "Too many requests" in d, d)
check("http detail still names the code with a body", d.startswith("HTTP 429"), d)

d = rbx.http_detail(_err(403, {"rblx-challenge-type": "twostepverification"}))
check("a 2FA challenge is explained, not shown as a bare 403",
      "2FA" in d and "challenge" in d, d)
d = rbx.http_detail(_err(403, {"rbx-challenge-id": "abc"}))
check("the older challenge header is recognised too", "2FA" in d, d)

# An unreadable body must not turn a reportable error into a crash.
class _Unreadable(io.BytesIO):
    def read(self, *a):
        raise OSError("connection reset")


e = urllib.error.HTTPError("https://x", 500, "no", email.message.Message(),
                           _Unreadable())
check("an unreadable body still yields a detail", rbx.http_detail(e) == "HTTP 500",
      rbx.http_detail(e))

# Each Quick Login step must say which step failed, not just the HTTP code.
_saved_post = rbx.post_csrf


def _raising_post(*a, **k):
    raise _err(403, body=b'{"errors":[{"message":"nope"}]}')


rbx.post_csrf = _raising_post
for fn, args, word in (
    (rbx.quick_login_create, (), "code"),
    (rbx.quick_login_status, ("C", "K"), "check"),
    (rbx.quick_login_redeem, ("C", "K"), "redeem"),
):
    try:
        fn(*args)
        check(f"{fn.__name__} reports failure", False, "no exception")
    except RuntimeError as exc:
        check(f"{fn.__name__} names the step that failed",
              word in str(exc) and "403" in str(exc), str(exc))
    except Exception as exc:
        check(f"{fn.__name__} raises RuntimeError, not a raw HTTPError",
              False, f"{type(exc).__name__}: {exc}")
rbx.post_csrf = _saved_post

# A redeem that returns 200 but no cookie is a failure, not a silent success.
class _NoCookie:
    headers = email.message.Message()

    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


rbx.post_csrf = lambda *a, **k: _NoCookie()
try:
    rbx.quick_login_redeem("C", "K")
    check("a cookie-less 200 is an error", False, "no exception")
except RuntimeError as exc:
    check("a cookie-less 200 is an error", "no .ROBLOSECURITY" in str(exc), str(exc))
rbx.post_csrf = _saved_post

_dlg_src = _app_src.split("    def _work(self, gen):", 1)[1]
check("a non-cancelled failure shows the error instead of closing",
      "once(self.show_error, str(e))" in _dlg_src, "failure path does not show_error")
check("a non-cancelled failure is also written to stderr",
      "file=sys.stderr" in _dlg_src, "failure is not logged to stderr")
check("only a user-initiated cancel closes the dialog on failure",
      _dlg_src.split("except Exception as e:")[1].split("return")[0]
      .count("once(self.close)") == 1,
      "failure path still closes the dialog unconditionally")

# ==========================================================================
# launch modes
# ==========================================================================
def run_each(names=("a", "b", "c"), bad=()):
    spawns, logs = [], []

    def do_spawn(n, u):
        if n in bad:
            raise RuntimeError("sign-in refused")
        spawns.append((n, u))

    got = rbx.launch_each(list(names), "1818", do_spawn, logs.append,
                          stagger=0, sleep=lambda _s: None)
    return spawns, logs, got


spawns, logs, got = run_each()
check("launch-each launches every account",
      [s[0] for s in spawns] == ["a", "b", "c"], str(spawns))
check("launch-each returns what launched", got == ["a", "b", "c"], str(got))
check("launch-each gives everyone the game, not a server", all(
      s[1] == rbx.join_url("1818") for s in spawns), str(spawns))

spawns, logs, got = run_each(bad=("b",))
check("launch-each survives one bad account",
      [s[0] for s in spawns] == ["a", "c"], str(spawns))

# The stagger spaces sign-ins: a skipped account costs no wait.
_sleeps = []
rbx.launch_each(["up", "a", "b"], "1818",
                lambda n, u: None, lambda _m: None, stagger=8,
                sleep=_sleeps.append, is_running=lambda n: n == "up")
check("launch-each waits only between real launches", _sleeps == [8],
      str(_sleeps))
_urls = []
rbx.launch_each(["a", "b"], "1818", lambda n, u: _urls.append(u), lambda _m: None,
                stagger=0, sleep=lambda _s: None, job_id="JOB-F")
check("launch-each can send everyone into one server (joining a friend)",
      _urls == [rbx.join_url("1818", "JOB-F")] * 2, str(_urls))
check("launch-each reports the bad account",
      any("b: FAILED" in m for m in logs), str(logs))
check("launch-each omits the failure from its result", got == ["a", "c"], str(got))


def run_follow(job_after_polls, names=("main", "a1", "a2"), timeout=90,
               leader_fails=False):
    spawns, logs, polls = [], [], {"n": 0}

    def get_job_id(_name):
        polls["n"] += 1
        return "JOB-XYZ" if polls["n"] >= job_after_polls else None

    def do_spawn(n, u):
        if leader_fails and n == names[0]:
            raise RuntimeError("leader exploded")
        spawns.append((n, u))

    jid = rbx.follow_leader(
        list(names), "1818",
        get_job_id=get_job_id, do_spawn=do_spawn, log=logs.append,
        stagger=0, sleep=lambda _s: None, timeout=timeout, poll=3,
    )
    return spawns, jid, logs, polls["n"]


spawns, jid, _, _ = run_follow(job_after_polls=2)
check("leader goes first", spawns[0][0] == "main", str(spawns))
check("group launch launches everyone",
      [s[0] for s in spawns] == ["main", "a1", "a2"], str(spawns))
check("leader joins the game, any server",
      spawns[0][1] == rbx.join_url("1818"), spawns[0][1])
check("followers join the leader's server", all(
      s[1] == rbx.join_url("1818", "JOB-XYZ") for s in spawns[1:]), str(spawns[1:]))
check("returns the shared jobId", jid == "JOB-XYZ", str(jid))

# The failure that matters most: the leader never surfaces. Followers must still
# launch (into their own servers) rather than be silently dropped.
spawns, jid, logs, _ = run_follow(job_after_polls=10**6, timeout=9)
check("leader timeout still launches followers",
      [s[0] for s in spawns] == ["main", "a1", "a2"], str(spawns))
check("leader timeout returns no jobId", jid is None, str(jid))
check("leader timeout falls back to the game, any server", all(
      s[1] == rbx.join_url("1818") for s in spawns[1:]), str(spawns))
check("leader timeout is reported", any("no server after" in m for m in logs), str(logs))

# If the leader itself cannot launch there is no server to follow, and polling
# presence for a client that was never started would just burn the timeout.
spawns, jid, logs, polled = run_follow(job_after_polls=1, leader_fails=True)
check("a failed leader aborts the group", spawns == [], str(spawns))
check("a failed leader does not poll", polled == 0, f"{polled} polls")
check("a failed leader is reported", any("leader FAILED" in m for m in logs), str(logs))

# A single account is just a launch. Polling for a jobId nobody will use would
# stall the caller for `timeout` seconds and hit the API for nothing.
spawns, jid, _, polled = run_follow(job_after_polls=1, names=("solo",))
check("single account launches once", len(spawns) == 1 and jid is None, str(spawns))
check("single account never polls presence", polled == 0, f"{polled} polls")

check("empty list is a no-op", rbx.follow_leader(
    [], "1", lambda n: None, lambda n, u: None, lambda m: None) is None)

# ==========================================================================
# presence parsing: leader_presence extracts (job_id, place_id)
# ==========================================================================
_real_post = rbx.post_csrf
rbx.post_csrf = lambda url, cookie=None, body=None: _Resp(json.dumps({
    "userPresences": [{
        "userPresenceType": 2, "lastLocation": "Subplace",
        "placeId": 9999, "rootPlaceId": 1818, "gameId": "JOB-SUB",
        "universeId": 555, "userId": 123,
    }]
}).encode())

sub_jid, sub_pid = rbx.leader_presence("CK", 123)
check("leader_presence returns subplace job_id", sub_jid == "JOB-SUB", sub_jid)
check("leader_presence returns subplace place_id", sub_pid == "9999", sub_pid)
check("job_id_of still extracts the job_id", rbx.job_id_of("CK", 123) == "JOB-SUB")

# When user is not in game (gameId is null)
rbx.post_csrf = lambda url, cookie=None, body=None: _Resp(json.dumps({
    "userPresences": [{"userPresenceType": 1, "lastLocation": "Website", "userId": 123}]
}).encode())
no_jid, no_pid = rbx.leader_presence("CK", 123)
check("leader_presence handles no-game presence", no_jid is None and no_pid is None, f"{no_jid}/{no_pid}")

# Empty userPresences list
rbx.post_csrf = lambda url, cookie=None, body=None: _Resp(b'{"userPresences": []}')
empty_jid, empty_pid = rbx.leader_presence("CK", 123)
check("leader_presence handles empty presences", empty_jid is None and empty_pid is None, f"{empty_jid}/{empty_pid}")

check("in-game friends are those in a game whose place shows; a hidden server is None",
      rbx.parse_in_game([
          {"userPresenceType": 2, "placeId": 9, "gameId": "J", "userId": 1,
           "lastLocation": "Obby"},
          {"userPresenceType": 2, "placeId": 8, "gameId": None, "userId": 2},
          {"userPresenceType": 2, "placeId": None, "userId": 3},
          {"userPresenceType": 1, "userId": 4}])
      == [{"user_id": 1, "place_id": "9", "job_id": "J", "game": "Obby"},
          {"user_id": 2, "place_id": "8", "job_id": None, "game": "Place 8"}])

_asked = []


def _friends_post(url, cookie=None, body=None):
    _asked.append((url, body))
    if "presence" in url:
        return _Resp(json.dumps({"userPresences": [
            {"userPresenceType": 2, "placeId": 9, "gameId": "J1", "userId": 11,
             "lastLocation": "Obby"},
            {"userPresenceType": 2, "placeId": 9, "gameId": "J2", "userId": 12},
            {"userPresenceType": 0, "userId": 13}]}).encode())
    return _Resp(json.dumps({"data": [
        {"id": 11, "name": "zed", "displayName": "Zed"},
        {"id": 12, "name": "amy", "displayName": "amy"}]}).encode())


_real_open = rbx._open
rbx._open = lambda url, cookie=None, **k: _Resp(json.dumps({"data": [
    {"id": 11, "name": ""}, {"id": 12, "name": ""}, {"id": 13, "name": ""}]}).encode())
rbx.post_csrf = _friends_post
_fr = rbx.friends_status("CK", 5)
check("friends come back named: in a game first, with their server, then the rest",
      [(f["display"], f["state"], f["job_id"]) for f in _fr]
      == [("amy", "game", "J2"), ("Zed", "game", "J1"),
          ("user 13", "offline", None)], str(_fr))
check("every friend's name is looked up, in one batch",
      _asked[-1][1] == {"userIds": [11, 12, 13]}, str(_asked))
_asked.clear()
rbx.presences("CK", list(range(120)))
check("presence is asked in batches", [len(b["userIds"]) for _u, b in _asked] == [50, 50, 20])
rbx._open = _real_open
rbx.post_csrf = _real_post

# Leader enters a subplace: followers must join the subplace, not the root place.
spawns_sub = []
rbx.follow_leader(
    ["main", "f1"], "1818",
    get_job_id=lambda n: ("JOB-SUB", "9999"),
    do_spawn=lambda n, u: spawns_sub.append((n, u)),
    log=lambda m: None,
    stagger=0, sleep=lambda _s: None, timeout=9, poll=3,
    is_running=lambda _n: False,
)
check("follower joins the leader subplace",
      "placeId=9999" in urllib.parse.unquote(spawns_sub[1][1]), spawns_sub[1][1])
check("follower carries the subplace jobId",
      "gameInstanceId=JOB-SUB" in urllib.parse.unquote(spawns_sub[1][1]), spawns_sub[1][1])

# Leader is already running: leader is NOT re-spawned, followers join its server.
spawns_lead_running = []
jid_lead_running = rbx.follow_leader(
    ["main", "f1"], "1818",
    get_job_id=lambda n: ("JOB-ALREADY", "1818"),
    do_spawn=lambda n, u: spawns_lead_running.append((n, u)),
    log=lambda m: None,
    stagger=0, sleep=lambda _s: None, timeout=9, poll=3,
    is_running=lambda n: (n == "main"),
)
check("already-running leader is not spawned",
      [s[0] for s in spawns_lead_running] == ["f1"], str(spawns_lead_running))
check("follower still joins running leader server",
      "JOB-ALREADY" in urllib.parse.unquote(spawns_lead_running[0][1]), str(spawns_lead_running))
check("already-running leader returns jobId", jid_lead_running == "JOB-ALREADY")

# Already-running followers are skipped
spawns_fol_running = []
rbx.follow_leader(
    ["main", "f1", "f2"], "1818",
    get_job_id=lambda n: "JOB-1",
    do_spawn=lambda n, u: spawns_fol_running.append((n, u)),
    log=lambda m: None,
    stagger=0, sleep=lambda _s: None, timeout=9, poll=3,
    is_running=lambda n: (n == "f1"),
)
check("already-running follower is skipped in group launch",
      [s[0] for s in spawns_fol_running] == ["main", "f2"], str(spawns_fol_running))

spawns_each_running = rbx.launch_each(
    ["a", "b", "c"], "1818",
    do_spawn=lambda n, u: None,
    log=lambda m: None,
    stagger=0, sleep=lambda _s: None,
    is_running=lambda n: (n == "b"),
)
check("already-running account is skipped in launch_each",
      spawns_each_running == ["a", "c"], str(spawns_each_running))

# Leader, auto-join and groups (the pure layout helpers the window calls)
def _accts():
    return [{"name": n, "user_id": i, "selected": s}
            for i, (n, s) in enumerate([("alt1", True), ("alt2", True),
                                        ("alt3", False), ("alt4", True)], 1)]


_a = _accts()
rbx.migrate_layout(_a)
check("the old layout carries over: first selected leads, the rest selected follow",
      rbx.leader_of(_a)["name"] == "alt1"
      and [a["name"] for a in rbx.followers_of(_a)] == ["alt2", "alt4"],
      str(_a))
rbx.migrate_layout(_a)
check("migrating twice changes nothing",
      [a["name"] for a in rbx.followers_of(_a)] == ["alt2", "alt4"])

rbx.make_leader(_a, "alt3")
check("a new leader is selected, leaves the auto-join list, and is the only one",
      rbx.leader_of(_a)["name"] == "alt3" and _a[2]["selected"]
      and sum(1 for a in _a if a.get("leader")) == 1
      and [a["name"] for a in rbx.followers_of(_a)] == ["alt2", "alt4"])
rbx.make_leader(_a, "alt2")
check("a follower made leader stops following; the rest renumber from 1",
      [(a["name"], a["follow"]) for a in rbx.followers_of(_a)] == [("alt4", 1)],
      str(_a))
rbx.set_follow(_a, "alt1", True)
rbx.set_follow(_a, "alt2", True)
check("the leader cannot follow itself; others join at the end",
      [a["name"] for a in rbx.followers_of(_a)] == ["alt4", "alt1"])
rbx.move_follower(_a, "alt1", -1)
check("a follower moves up the join order",
      [a["name"] for a in rbx.followers_of(_a)] == ["alt1", "alt4"])
rbx.move_follower(_a, "alt1", -1)
check("the first follower cannot move further up",
      [a["name"] for a in rbx.followers_of(_a)] == ["alt1", "alt4"])
rbx.set_follow(_a, "alt1", False)
check("unlinking renumbers", [(a["name"], a["follow"])
                              for a in rbx.followers_of(_a)] == [("alt4", 1)])

_a = _accts()
_g = [{"id": "g1", "name": "Farm"}, {"id": "g2", "name": "Event"}]
_a[1]["group"] = "g2"
_a[2]["group"] = "g1"
_a[3]["group"] = "gone"
rbx.make_leader(_a, "alt1")
check("drawn order is group by group, then the ungrouped; a deleted group's "
      "accounts are ungrouped; the leader is drawn apart",
      [a["name"] for a in rbx.visual_order(_a, _g)] == ["alt3", "alt2", "alt4"])
rbx.drop_account(_a, _g, "alt4", "alt3")
check("dropping onto a row takes its group and its place (dragged up: before it)",
      [a["name"] for a in rbx.visual_order(_a, _g)] == ["alt4", "alt3", "alt2"]
      and _a[[a["name"] for a in _a].index("alt4")]["group"] == "g1", str(_a))
rbx.drop_account(_a, _g, "alt4", "alt2")
check("dragged down, it lands after the row",
      [a["name"] for a in rbx.visual_order(_a, _g)] == ["alt3", "alt2", "alt4"])
_before = [dict(a) for a in _a]
rbx.drop_account(_a, _g, "alt1", "alt2")
check("the leader is not dragged anywhere", _a == _before)

with tempfile.TemporaryDirectory() as _tmp:
    rbx.GROUPS = os.path.join(_tmp, "groups.json")
    check("no groups file means migrate first", rbx.load_groups() is None)
    rbx.save_groups(_g)
    check("groups round-trip", rbx.load_groups() == _g)

# ==========================================================================
# favourite games (the tile bar that replaced the Place ID entry)
# ==========================================================================
FAVS = {"data": [
    {"id": 111, "name": "Blox Fruits", "rootPlace": {"id": 2753915549}},
    {"id": 222, "name": "No root place here"},
    {"id": 333, "name": "", "rootPlace": {"id": 606849621}},
]}

favs = rbx.parse_favorites(FAVS)
check("a favourite becomes a launchable place",
      favs[0] == {"universe_id": "111", "place_id": "2753915549",
                  "name": "Blox Fruits"}, str(favs[0]))
check("a universe with no root place is dropped", len(favs) == 2, str(favs))
check("a nameless game still gets a label",
      favs[1]["name"] == "Place 606849621", str(favs[1]))
check("ids are strings, so they compare with a tile's place",
      all(isinstance(g["place_id"], str) for g in favs), str(favs))
check("nothing to parse is no games, not a crash",
      rbx.parse_favorites({}) == [] and rbx.parse_favorites({"data": None}) == [])

seen = []


def fake_get(req, timeout=None):
    seen.append(req.full_url)
    return _Resp(json.dumps(FAVS).encode())


rbx.urllib.request.urlopen = fake_get
got = rbx.favorite_games("CK", 7, limit=1)
check("favourites come from the v2 favourites endpoint",
      seen[0].startswith("https://games.roblox.com/v2/users/7/favorite/games"),
      seen[0])
check("the limit is one Roblox accepts",
      "limit=50" in seen[0] and "limit=1&" not in seen[0], seen[0])
check("the limit is applied here instead", len(got) == 1, str(got))

# The bar is every account's favourites, not the leader's.


def _fav(place, name):
    return {"universe_id": place, "place_id": place, "name": name}


ACCTS = [
    {"name": "a", "favorites": [_fav("1", "One"), _fav("2", "Two")],
     "plays": {"2": 3}},
    {"name": "b", "favorites": [_fav("3", "Three"), _fav("2", "Two")]},
    {"name": "c", "favorites": [_fav("3", "Three")]},
]
merged = rbx.merge_favorites(ACCTS)
check("every account's favourites reach the bar",
      [g["place_id"] for g in merged] == ["2", "3", "1"], str(merged))
check("a merged game is not duplicated", len(merged) == 3, str(merged))
check("merging survives an account with no favourites",
      rbx.merge_favorites([{"name": "a"}]) == [], "empty account")
check("the merge is limited", len(rbx.merge_favorites(ACCTS, limit=2)) == 2)

icons = rbx.parse_icon_urls({"data": [
    {"targetId": 111, "state": "Completed", "imageUrl": "https://cdn/a.png"},
    {"targetId": 222, "state": "Pending", "imageUrl": ""},
    {"targetId": 333, "state": "Completed"},
]})
check("only finished icons are offered", icons == {"111": "https://cdn/a.png"},
      str(icons))

seen.clear()
rbx.urllib.request.urlopen = lambda req, timeout=None: (
    seen.append(req.full_url), _Resp(b'{"data": []}'))[1]
rbx.game_icon_urls(["111", "222"])
check("icons are fetched in one batched request", len(seen) == 1, str(seen))
check("both universes ride in that request",
      "universeIds=111,222" in seen[0] and "size=150x150" in seen[0], seen[0])
seen.clear()
check("no ids -> no request",
      rbx.game_icon_urls([]) == {} and not seen, str(seen))

with tempfile.TemporaryDirectory() as tmp:
    rbx.ICONS = os.path.join(tmp, "_icons")
    seen.clear()
    rbx.urllib.request.urlopen = lambda req, timeout=None: (
        seen.append(req.full_url), _Resp(b"PNGDATA"))[1]
    path = rbx.cached_icon("111", "https://cdn/a.png")
    check("an icon lands in the cache", path and open(path, "rb").read() == b"PNGDATA",
          str(path))
    check("nothing is left half-written",
          os.listdir(rbx.ICONS) == ["111.png"], str(os.listdir(rbx.ICONS)))
    again = rbx.cached_icon("111", "https://cdn/a.png")
    check("a cached icon is not downloaded twice",
          again == path and len(seen) == 1, str(seen))
    check("cache-only asks the network for nothing",
          rbx.cached_icon("999") is None and len(seen) == 1, str(seen))

    # The whole point of caching by id: the bar draws from disk at startup, so
    # opening the app neither hits Roblox nor wakes the keyring.
    seen.clear()
    rbx.urllib.request.urlopen = lambda req, timeout=None: (_ for _ in ()).throw(
        AssertionError("startup must not hit the network"))
    check("a known icon still resolves with no network",
          rbx.cached_icon("111") == path, str(path))

seen.clear()


def fail_icons(req, timeout=None):
    raise urllib.error.URLError("no network")


rbx.urllib.request.urlopen = fail_icons
noicons = rbx.with_icons([{"universe_id": "111", "place_id": "1", "name": "n"}])
check("a thumbnail outage still yields tiles",
      noicons[0]["icon"] is None and noicons[0]["place_id"] == "1", str(noicons))

rbx.urllib.request.urlopen = _real_urlopen


# The Place ID entry is gone: a place can now only come from a tile Roblox
# itself handed over, which is what makes the launch path need no validation.
# Filling the strip activates a tile, and that handler calls back into the
# window -- which during GameBar's own construction has no `.games` yet. The
# app crashed in exactly that order on startup, so keep the constructor empty.
_bar_init = _app_src.partition("class GameBar")[2].partition("def place_id")[0]
check("the bar does not fill itself while the window is half-built",
      "self.set_games(" not in _bar_init, "GameBar.__init__ builds tiles")

# Widget syntax, not prose: GameBar's docstring names the entry it replaced.
check("no place id is typed in any more",
      'EntryRow(title="Place ID' not in _app_src
      and "place.get_text" not in _app_src,
      "a Place ID entry is still wired up")


# ==========================================================================
# last-launch display
# ==========================================================================
now = datetime(2026, 9, 22, 12, 0, tzinfo=timezone.utc)
check("no timestamp reads as never", rbx.relative_time(None) == "never launched")
check("junk timestamp reads as never",
      rbx.relative_time("not-a-date") == "never launched")
check("seconds ago", rbx.relative_time(
    (now - timedelta(seconds=5)).isoformat(), now) == "5s ago")
check("minutes ago", rbx.relative_time(
    (now - timedelta(minutes=7)).isoformat(), now) == "7m ago")
check("hours ago", rbx.relative_time(
    (now - timedelta(hours=5)).isoformat(), now) == "5h ago")
check("days ago", rbx.relative_time(
    (now - timedelta(days=3)).isoformat(), now) == "3d ago")
check("old timestamps fall back to a date", rbx.relative_time(
    (now - timedelta(days=400)).isoformat(), now) == "2025-08-18")
# A naive timestamp must not raise on subtraction against an aware "now".
check("naive timestamps are treated as UTC",
      rbx.relative_time("2026-09-22T11:00:00", now) == "1h ago",
      rbx.relative_time("2026-09-22T11:00:00", now))

# ==========================================================================
# renaming a label: the label is also a keyring key and two directory names,
# so a rename that only edited accounts.json would orphan all three.
# ==========================================================================
check("an empty label is rejected", not rbx.valid_name(""))
check("a label with a slash is rejected", not rbx.valid_name("a/b"))
check("a dot-relative label is rejected",
      not rbx.valid_name("..") and not rbx.valid_name("."))
check("an ordinary label is accepted", rbx.valid_name("alt 1"))
check("a label that would share the icon cache is rejected",
      not rbx.valid_name("_icons"))
check("a label with a control character is rejected",
      not rbx.valid_name("alt\n1"))

_keep = (rbx.secret_store, rbx.secret_lookup, rbx.secret_clear)
keyring = {"old": "cookie-old"}
rbx.secret_store = lambda n, c: keyring.__setitem__(n, c)
rbx.secret_lookup = lambda n: keyring[n]
rbx.secret_clear = lambda n: keyring.pop(n, None)
rbx.move_account_data("old", "new")
check("rename re-keys the cookie and drops the old key",
      keyring == {"new": "cookie-old"}, str(keyring))
rbx.secret_store, rbx.secret_lookup, rbx.secret_clear = _keep


# ==========================================================================
# accounts.json holds no secrets
# ==========================================================================
with tempfile.TemporaryDirectory() as tmp:
    rbx.STATE = tmp
    rbx.ACCOUNTS = os.path.join(tmp, "accounts.json")
    rbx.save_accounts([{
        "name": "alt1", "user_id": 7, "display": "D", "note": "farm acct",
        "last_launch": "2026-09-22T05:00:00+00:00", "selected": True,
    }])
    with open(rbx.ACCOUNTS) as f:
        raw = f.read()
    check("index round-trips", rbx.load_accounts()[0]["name"] == "alt1", raw)
    check("index stores no cookie field",
          "ROBLOSECURITY" not in raw and "cookie" not in raw.lower(), raw)
    check("index stores no private key",
          "privateKey" not in raw and "password" not in raw.lower(), raw)

    rbx.ACCOUNTS = os.path.join(tmp, "missing.json")
    check("a missing index is an empty list, not a crash", rbx.load_accounts() == [])
    with open(rbx.ACCOUNTS, "w") as f:
        f.write("{not json")
    check("a corrupt index is an empty list, not a crash", rbx.load_accounts() == [])
    with open(rbx.ACCOUNTS, "w") as f:
        f.write('{"accounts": []}')
    check("a non-list index is rejected", rbx.load_accounts() == [])

# ==========================================================================
# Keyring. The login collection is locked on a machine that autologins (PAM
# never sees a password), and libsecret neither prompts nor explains -- it fails
# with "Cannot create an item in a locked collection", which reached the user as
# a bare "returned non-zero exit status 1".
# ==========================================================================
import inspect  # noqa: E402

check("storing a secret unlocks the keyring first",
      "ensure_keyring_unlocked()" in inspect.getsource(rbx.secret_store))
check("reading a secret unlocks the keyring first",
      "ensure_keyring_unlocked()" in inspect.getsource(rbx.secret_lookup))
# The whole reason for the D-Bus layer: secret-tool inside the Flatpak writes
# to a private keyring Cordial cannot read.
check("nothing goes through secret-tool any more",
      "secret-tool\"" not in open(os.path.join(_here, "roblox-manager.py")).read()
      and not hasattr(rbx, "SECRET_TOOL"))

_real_ensure = rbx.ensure_keyring_unlocked          # before anything stubs it
_kr = (rbx.keyring_store, rbx.keyring_lookup, rbx.keyring_clear)
_ring = {}
rbx.keyring_store = lambda attrs, label, secret, bus=None: _ring.__setitem__(
    tuple(sorted(attrs.items())), (label, secret))
rbx.keyring_lookup = lambda attrs, bus=None: (_ring.get(tuple(sorted(attrs.items())))
                                              or (None, None))[1]
rbx.keyring_clear = lambda attrs, bus=None: _ring.pop(tuple(sorted(attrs.items())), None)
rbx.ensure_keyring_unlocked = lambda *a, **k: None
rbx.secret_store("x", "  cookie-value \n")
check("an account's cookie is filed under app=rbxmgr, account=<label>",
      list(_ring) == [(("account", "x"), ("app", "rbxmgr"))]
      and _ring[(("account", "x"), ("app", "rbxmgr"))][0] == "rbxmgr x", str(_ring))
check("a stored cookie round-trips stripped",
      rbx.secret_lookup("x") == "cookie-value", repr(rbx.secret_lookup("x")))
rbx.secret_clear("x")
try:
    rbx.secret_lookup("x")
    check("a missing cookie is an error that names the fix", False, "no exception")
except RuntimeError as exc:
    check("a missing cookie is an error that names the fix",
          "Sign in again" in str(exc), str(exc))
rbx.keyring_store, rbx.keyring_lookup, rbx.keyring_clear = _kr

# ensure_keyring_unlocked hands the unlock to the main thread and waits on it.
rbx.ensure_keyring_unlocked = _real_ensure

immediate = lambda fn, *a: fn(*a)  # noqa: E731  stands in for once()
check("an unlocked keyring returns quietly",
      rbx.ensure_keyring_unlocked(
          schedule=immediate, unlock=lambda done: done(None)) is None)

try:
    rbx.ensure_keyring_unlocked(
        schedule=immediate, unlock=lambda done: done("prompt was dismissed"))
    check("a refused unlock raises", False, "no exception")
except RuntimeError as exc:
    check("a refused unlock raises with the reason",
          "dismissed" in str(exc), str(exc))

# A prompt nobody ever answers must not hang the launch thread forever.
try:
    rbx.ensure_keyring_unlocked(
        timeout=0.05, schedule=immediate, unlock=lambda done: None)
    check("an unanswered prompt times out", False, "no exception")
except RuntimeError as exc:
    check("an unanswered prompt times out", "did not unlock" in str(exc), str(exc))

# The unlock must be handed to the scheduler, not run on the calling thread:
# the Completed signal only arrives on a thread with a running main context.
handed = []
rbx.ensure_keyring_unlocked(schedule=lambda fn, *a: (handed.append(fn), fn(*a))[1],
                            unlock=lambda done: done(None))
check("the unlock is scheduled onto the main thread", len(handed) == 1, str(handed))

# ==========================================================================
# GTK main-loop hand-off. GLib re-runs an idle callback for as long as it returns
# something truthy; handing it Gio.AppInfo.launch_default_for_uri (which returns
# True) reopened the browser hundreds of times. once() must swallow the return
# value, and nothing may call GLib.idle_add around it.
# ==========================================================================
class _FakeGLib:
    SOURCE_REMOVE = False

    def __init__(self):
        self.returned = []

    def idle_add(self, cb):
        self.returned.append(cb())


_real_glib = rbx.GLib
fake = _FakeGLib()
rbx.GLib = fake

ran = []
rbx.once(ran.append, "payload")
check("once runs the callback exactly once", ran == ["payload"], str(ran))
check("once tells GLib not to repeat", fake.returned == [False], str(fake.returned))

# The exact shape of the bug: a callback whose own return value is truthy.
fake.returned.clear()
truthy = []
rbx.once(lambda: (truthy.append(1), True)[1])
check("a truthy callback still does not repeat",
      fake.returned == [False], str(fake.returned))
check("the truthy callback ran once", truthy == [1], str(truthy))

# once() passes several arguments through, which is how launch_default_for_uri
# gets its (url, None).
fake.returned.clear()
args = []
rbx.once(lambda *a: args.append(a), "url", None)
check("once forwards every argument", args == [("url", None)], str(args))

rbx.GLib = _real_glib

_src = open(os.path.join(_here, "roblox-manager.py")).read()
_before, _, _after = _src.partition("GLib.idle_add(run)")
# Match call syntax, not prose: the helper's own docstring mentions the name.
check("only once() touches GLib.idle_add",
      _before.count("GLib.idle_add(") == 0
      and _after.count("GLib.idle_add(") == 0,
      f"{_before.count('GLib.idle_add(')} before / "
      f"{_after.count('GLib.idle_add(')} after the helper")
check("once() returns GLib.SOURCE_REMOVE",
      "return GLib.SOURCE_REMOVE" in _src, "helper does not return SOURCE_REMOVE")

# ==========================================================================
# Cordial. Nothing here runs flatpak or pgrep: `run` is injected throughout.
# ==========================================================================
_pgrep = (
    "4101 /app/bin/cordial-run --lib-dir /x/lib --apk /x/base.apk --host-libc "
    "--game-activity --run 0 --profile main\n"
    "4102 /app/bin/cordial-run --lib-dir /x/lib --apk /x/base.apk --run 0 "
    "--profile alt 2 --join-url roblox-player:1+launchmode:play\n"
    # The manager's own `flatpak-spawn --host pgrep ...` matches too, and so
    # does any shell that mentions a client's command line.
    "4200 flatpak-spawn --host pgrep -a -f cordial-run\n"
    "4300 bash -c /app/bin/cordial-run --profile main & sleep 1\n"
    # This app starts engines with --command, which leaves argv[0] bare.
    "4400 cordial-run --lib-dir /l --apk /a --run 0 --profile rbxmgr-7\n"
    "4401 bwrap --args 74 -- cordial-run --profile rbxmgr-7\n"
    "junk line\n")
check("clients are read with their profiles, spaces and all",
      rbx.parse_clients(_pgrep) == {4101: "main", 4102: "alt 2", 4400: "rbxmgr-7"},
      str(rbx.parse_clients(_pgrep)))
check("no clients is an empty answer", rbx.parse_clients("") == {})

class _R:
    def __init__(self, rc=0, out=b"", err=b""):
        self.returncode, self.stdout, self.stderr = rc, out, err


_FETCHED = (b'{"apk": "/c/build/x86_64/base.apk", "engine": "/c/lib/x86_64", '
            b'"version": "2.734.0.917"}\n')
_calls, _logs = [], []
_got = rbx.roblox_build(_logs.append, run=lambda a, timeout=0: (
    _calls.append(a), _R(0, _FETCHED))[1])
check("an installed Roblox build is used as it is, with no fetch",
      _got == ("/c/lib/x86_64", "/c/build/x86_64/base.apk")
      and _calls == [["cordial-fetch", "--status"]] and _logs == [], str(_calls))
_calls.clear()
_got = rbx.roblox_build(_logs.append, run=lambda a, timeout=0: (
    _calls.append(a), _R(1) if "--status" in a else _R(0, _FETCHED))[1])
check("no build yet installs one, and says so",
      _calls == [["cordial-fetch", "--status"], ["cordial-fetch"]]
      and _got[0] == "/c/lib/x86_64" and len(_logs) == 1, str(_calls))
_calls.clear()
rbx.roblox_build(lambda m: None, newest=True, run=lambda a, timeout=0: (
    _calls.append(a), _R(0, _FETCHED))[1])
check("updating asks for the newest build and skips the status check",
      _calls == [["cordial-fetch", "--newest"]], str(_calls))
try:
    rbx.roblox_build(lambda m: None, run=lambda a, timeout=0: _R(
        1, err=b"asking local\ncordial-fetch: no source has a build\n"))
    check("a failed install says why", False, "no exception")
except RuntimeError as exc:
    check("a failed install says why", "no source has a build" in str(exc), str(exc))

with tempfile.TemporaryDirectory() as _tmp:
    _old_flatpak, _old_profiles, _old_json = (
        rbx.FLATPAK_CORDIAL, rbx.CORDIAL_PROFILES, rbx.CORDIAL_SHELL_JSON)
    rbx.FLATPAK_CORDIAL = os.path.join(_tmp, "flatpak")
    rbx.CORDIAL_PROFILES = os.path.join(_tmp, "native/profiles")
    rbx.CORDIAL_SHELL_JSON = os.path.join(_tmp, "native/config/shell.json")
    for _n in ("rbxmgr-1", "rbxmgr-2", "rbxmgr-3", "someone-else"):
        os.makedirs(os.path.join(rbx.FLATPAK_CORDIAL, "data/cordial/profiles", _n, "data"))
    os.makedirs(os.path.join(rbx.CORDIAL_PROFILES, "rbxmgr-2"))
    os.makedirs(os.path.join(rbx.FLATPAK_CORDIAL, "config/cordial"))
    with open(os.path.join(rbx.FLATPAK_CORDIAL, "config/cordial/shell.json"), "w") as _f:
        _f.write('{"gamemode": false}')
    _cleared = []
    rbx.migrate_flatpak_cordial(lambda m: None, clear=_cleared.append,
                                clients=lambda: {9: "rbxmgr-3"})
    _left = sorted(os.listdir(os.path.join(rbx.FLATPAK_CORDIAL, "data/cordial/profiles")))
    check("the manager's profiles move out of the Flatpak, never over one already "
          "here and never while a client has one open",
          _left == ["rbxmgr-2", "rbxmgr-3", "someone-else"]
          and os.path.isdir(os.path.join(rbx.CORDIAL_PROFILES, "rbxmgr-1", "data")), str(_left))
    check("a moved profile's old keyring sessions are dropped",
          sorted(a["store"] for a in _cleared) == ["cookies", "identity"]
          and all(a["profile"].startswith(rbx.FLATPAK_CORDIAL) for a in _cleared),
          str(_cleared))
    check("Cordial's settings are carried over",
          rbx.cordial_settings(lambda a, timeout=0: _R(0, open(a[-1], "rb").read()))
          == {"gamemode": False})
    rbx.FLATPAK_CORDIAL, rbx.CORDIAL_PROFILES, rbx.CORDIAL_SHELL_JSON = (
        _old_flatpak, _old_profiles, _old_json)

# Starting an account's engine directly, the way Cordial's window does.
_files = {
    rbx.CORDIAL_SHELL_JSON: json.dumps({
        "roblox": {"apk": None, "lib_dir": None}, "gamemode": False,
        "throttle": "visible", "pointer_acceleration": "unlockedcursor",
        "graphics": "automatic", "present_mode": "mailbox", "gamepad": True,
        "close_on_leave": False, "graphics_optimization_mode": "more-cores",
        "audio_output": "", "title_bar": "default"}),
}


_writes = []


def _host(argv, timeout=0, input=None):
    if input is not None:
        _writes.append((argv, input))
        return _R(0)
    body = _files.get(argv[-1]) if argv[0] == "cat" else None
    return _R(0 if body is not None else 1, (body or "").encode())


_cfg = rbx.cordial_settings(_host)
_env = rbx.cordial_engine_env(_cfg)
check("Cordial's settings reach the engine as its window passes them",
      _env == {"CORDIAL_SECRET_STORE": "keyring", "CORDIAL_GAMEMODE": "0",
               "CORDIAL_THROTTLE": "visible", "CORDIAL_POINTER_ACCEL": "unlocked",
               "CORDIAL_PRESENT_MODE": "mailbox",
               "CORDIAL_PERFORMANCE": "throughput"}, str(_env))
check("a missing or corrupt shell.json is Cordial's defaults",
      rbx.cordial_settings(lambda a, timeout=0: _R(0, b"{nope")) == {}
      and rbx.cordial_engine_env({}) == {"CORDIAL_SECRET_STORE": "keyring"})
_argv = rbx.client_argv("rbxmgr-7", "roblox://experiences/start?placeId=1",
                        ("/l", "/a.apk"))
check("the engine is the fork's cordial-run, for the account's profile",
      _argv[:5] == ["cordial-run", "--lib-dir", "/l", "--apk", "/a.apk"]
      and _argv[_argv.index("--profile") + 1] == "rbxmgr-7"
      and _argv[_argv.index("--run") + 1] == "0"
      and _argv[-2:] == ["--join-url", "roblox://experiences/start?placeId=1"], str(_argv))
check("no game means no join link",
      "--join-url" not in rbx.client_argv("p", None, ("/l", "/a")))


class _P:
    def __init__(self, rc):
        self.returncode = rc

    def poll(self):
        return self.returncode


with tempfile.TemporaryDirectory() as _tmp:
    rbx.LOGS = _tmp
    _started = []
    _p = rbx.launch_client("rbxmgr-7", None, ("/l", "/a.apk"), run=_host, sleep=lambda _s: None,
                           start=lambda a, out, env=None: (_started.append((a, out.name)), _P(None))[1])
    check("a client that stays up is a launch, logged per profile",
          _started and _started[0][1] == os.path.join(_tmp, "rbxmgr-7.log"), str(_started))

    def _dies(a, out, env=None):
        out.write(b"profiles: rbxmgr-7 is already in use\n")
        out.flush()
        return _P(1)

    try:
        rbx.launch_client("rbxmgr-7", None, ("/l", "/a.apk"), run=_host, sleep=lambda _s: None, start=_dies)
        check("a client that dies at once is a failure, and says why", False)
    except RuntimeError as exc:
        check("a client that dies at once is a failure, and says why",
              "already in use" in str(exc), str(exc))
    check("the previous launch's log is kept",
          os.path.exists(os.path.join(_tmp, "rbxmgr-7.log.1")))
    _started.clear()
    rbx.launch_client("rbxmgr-7", None, ("/l", "/a.apk"), run=_host, sleep=lambda _s: None, nested=True,
                      start=lambda a, out, env=None: (_started.append(a), _P(None))[1])
    check("a macro-ready client runs inside its own cage",
          _started[0][:2] == ["cage", "--"] and _started[0][-1] == "rbxmgr-7"
          and "cordial-run" in " ".join(_started[0]), str(_started))
    check("a normal client writes no flags", _writes == [], str(_writes))
    _started.clear()
    rbx.launch_client("rbxmgr-7", None, ("/l", "/a.apk"), run=_host, sleep=lambda _s: None,
                      low_power=True,
                      start=lambda a, out, env=None: (_started.append((a, env)), _P(None))[1])
    _a, _e = _started[0]
    check("a low-power client runs niced, throttled when unfocused, FIFO-paced",
          _a[:4] == ["nice", "-n", "10", "cordial-run"]
          and _e["CORDIAL_THROTTLE"] == "unfocused" and _e["CORDIAL_PRESENT_MODE"] == "fifo"
          and _e["CORDIAL_SECRET_STORE"] == "keyring", str(_started))
    check("a low-power client gets its frame and thread caps in its own flags.json",
          len(_writes) == 1
          and _writes[0][0][-1] == os.path.join(rbx.CORDIAL_PROFILES, "rbxmgr-7", "flags.json")
          and json.loads(_writes[0][1]) == rbx.LOW_POWER_FLAGS, str(_writes))

_mine = {"DFIntTaskSchedulerTargetFps": 144, "FFlagX": True}
check("low power adds its flags beside yours and never overrides your value",
      rbx.low_power_flags(_mine, True) == {"DFIntTaskSchedulerTargetFps": 144,
                                            "FFlagX": True,
                                            "FIntTaskSchedulerAutoThreadLimit": 2})
check("turning low power off takes back only its own values",
      rbx.low_power_flags(dict(_mine, FIntTaskSchedulerAutoThreadLimit=2), False)
      == _mine and rbx.low_power_flags(rbx.LOW_POWER_FLAGS, False) == {})
_writes.clear()
_files[os.path.join(rbx.CORDIAL_PROFILES, "p", "flags.json")] = json.dumps(
    rbx.LOW_POWER_FLAGS)
rbx.apply_low_power("p", True, run=_host)
rbx.apply_low_power("q", False, run=_host)
check("flags.json is left alone when nothing changes", _writes == [], str(_writes))
_files[os.path.join(rbx.CORDIAL_PROFILES, "p", "flags.json")] = "{broken"
rbx.apply_low_power("p", True, run=_host)
check("a flags.json that does not parse is not overwritten", _writes == [])

# ==========================================================================
# macros: parsing, the commands they become, and the player's guards
# ==========================================================================
_TAP = rbx.TAP_PRESS
_loops, _steps = rbx.parse_macro(
    "# farm\nloop 3\ntap E\ntap f 0.2-0.3\nhold shift+w 1-2\n\ntype -Gg\nclick right 10 20\n"
    "move -5 5\nwait 0.5\ntap F5\ntap /\n")
check("a macro parses into the evdev codes a US keyboard sends",
      _loops == 3 and _steps == [
          ("hold", [18], *_TAP), ("hold", [33], 0.2, 0.3), ("hold", [42, 17], 1.0, 2.0),
          ("type", [(12, False), (34, True), (34, False)]),
          ("click", 0x111, (10, 20)), ("move", -5, 5), ("wait", 0.5, 0.5),
          ("hold", [63], *_TAP), ("hold", [53], *_TAP)], str(_steps))
# The bug this replaced: wtype numbered keys in the order it met them, so the
# first key of every run went out as evdev 1 -- Escape, Roblox's menu.
check("a tap is its own key's code, never Escape",
      rbx.parse_macro("tap j")[1] == [("hold", [36], *_TAP)]
      and rbx.parse_macro("tap space")[1] == [("hold", [57], *_TAP)]
      and rbx.KEY_CODES["esc"] == 1
      and [k for k, v in rbx.KEY_CODES.items() if v == 1] == ["esc", "escape"])
for _bad, _why in (("tap -k", "unknown key"), ("tap e\nwait 3-1", "line 2"),
                   ("click 5", "click"), ("jump", "don't understand"),
                   ("type héllo", "cannot type"), ("# nothing", "no steps")):
    try:
        rbx.parse_macro(_bad)
        check(f"a bad macro is refused: {_bad!r}", False)
    except ValueError as exc:
        check(f"a bad macro is refused: {_bad!r}", _why in str(exc), str(exc))


class _Rng:
    def uniform(self, lo, hi):
        return hi


class _Input:
    def __init__(self, fail_on=None):
        self.sent, self.closed, self.path, self.fail_on = [], False, None, fail_on

    def key(self, code, down):
        if down and code == self.fail_on:
            raise OSError(32, "Broken pipe")
        self.sent.append(("key", code, down))

    def button(self, code, down):
        self.sent.append(("button", code, down))

    def motion(self, dx, dy):
        self.sent.append(("motion", dx, dy))

    def close(self):
        self.closed = True


class _Stop:
    def __init__(self):
        self.waits = []

    def wait(self, t=None):
        self.waits.append(t)
        return False

    def is_set(self):
        return False


_in, _st = _Input(), _Stop()
rbx.play_step(_in, ("hold", [42, 17], 1.0, 1.5), _st, _Rng())
check("a combo is pressed in order, held, and released in reverse",
      _in.sent == [("key", 42, True), ("key", 17, True), ("key", 17, False),
                   ("key", 42, False)] and _st.waits == [1.5], str(_in.sent))
_in = _Input()
rbx.play_step(_in, ("type", [(12, False), (34, True)]), _Stop(), _Rng())
check("typed text shifts only the characters that need it",
      _in.sent == [("key", 12, True), ("key", 12, False), ("key", 42, True),
                   ("key", 34, True), ("key", 34, False), ("key", 42, False)],
      str(_in.sent))
_in = _Input()
rbx.play_step(_in, ("click", 0x110, (10, 20)), _Stop(), _Rng())
check("an absolute click homes the pointer to the corner first",
      _in.sent == [("motion", -100000, -100000), ("motion", 10, 20),
                   ("button", 0x110, True), ("button", 0x110, False)], str(_in.sent))
_in = _Input(fail_on=17)
try:
    rbx.play_step(_in, ("hold", [42, 17], 1, 1), _Stop(), _Rng())
except OSError:
    pass
check("a key already down is let go when the next press fails",
      _in.sent == [("key", 42, True), ("key", 42, False)], str(_in.sent))

with tempfile.TemporaryDirectory() as _tmp:
    open(os.path.join(_tmp, "wayland-9"), "w").close()
    _script = rbx.nested_argv("rbxmgr-7", [
        "sh", "-c", '[ "$XDG_RUNTIME_DIR/rbxmgr/rbxmgr-7.wayland" -ef '
                    '"$XDG_RUNTIME_DIR/wayland-9" ] && echo linked'])
    _r = __import__("subprocess").run(
        _script[2:], capture_output=True,
        env=dict(os.environ, XDG_RUNTIME_DIR=_tmp, WAYLAND_DISPLAY="wayland-9"))
    check("the nested client's display is linked while it runs, then removed",
          _r.stdout == b"linked\n" and not os.path.exists(
              os.path.join(_tmp, "rbxmgr", "rbxmgr-7.wayland")), str(_r))


# VirtualInput against a stand-in compositor: the real wire bytes, over a real
# socket, with the keymap passed as a file descriptor.
def _compositor(path, seen):
    import socket
    import struct
    import threading
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(path)
    srv.listen(1)

    def serve():
        conn, _ = srv.accept()
        buf = b""
        while True:
            data, fds, _f, _a = socket.recv_fds(conn, 65536, 4)
            if not data:
                break
            buf += data
            for fd in fds:
                os.lseek(fd, 0, 0)
                seen["keymap"] = os.read(fd, 65536)
                os.close(fd)
            while len(buf) >= 8:
                obj, so = struct.unpack_from("=II", buf)
                if len(buf) < so >> 16:
                    break
                body, buf = buf[8:so >> 16], buf[so >> 16:]
                seen["msgs"].append((obj, so & 0xFFFF, body))
                if (obj, so & 0xFFFF) == (1, 1):        # get_registry
                    reg, = struct.unpack("=I", body)
                    for n, iface in enumerate(seen["offer"], 1):
                        b = rbx._u(n) + rbx._wl_str(iface) + rbx._u(1)
                        conn.sendall(rbx._u(reg, (8 + len(b)) << 16) + b)
                elif (obj, so & 0xFFFF) == (1, 0):      # sync
                    cb, = struct.unpack("=I", body)
                    conn.sendall(rbx._u(cb, 12 << 16, 0))
        conn.close()
        srv.close()

    t = threading.Thread(target=serve, daemon=True)
    t.start()
    return t


with tempfile.TemporaryDirectory() as _tmp:
    _sock = os.path.join(_tmp, "w")
    _seen = {"msgs": [], "offer": ("wl_seat", "zwp_virtual_keyboard_manager_v1",
                                   "zwlr_virtual_pointer_manager_v1")}
    _t = _compositor(_sock, _seen)
    _vi = rbx.VirtualInput(_sock)
    _vi.key(36, True)
    _vi.key(36, False)
    _vi.key(42, True)
    _vi.key(42, False)
    _vi.button(0x110, True)
    _vi.close()
    _t.join(5)
    import struct as _struct
    _binds = [_m for _m in _seen["msgs"] if _m[0] == 2 and _m[1] == 0]
    _keys = [_struct.unpack("=III", _b)[1:] for _o, _op, _b in _seen["msgs"]
             if _o == _vi.kbd and _op == 1]
    check("VirtualInput binds the seat and both virtual-input managers",
          [rbx._wl_unstr(_b, 4) for _o, _op, _b in _binds] == list(_seen["offer"]),
          str(_binds))
    check("VirtualInput uploads a US evdev keymap as a file descriptor",
          b"pc+us" in _seen.get("keymap", b"") and _seen["keymap"].endswith(b"\0"))
    check("VirtualInput sends the key's real evdev code, down then up",
          _keys == [(36, 1), (36, 0), (42, 1), (42, 0)], str(_keys))
    _mods = [_struct.unpack("=4I", _b)[0] for _o, _op, _b in _seen["msgs"]
             if _o == _vi.kbd and _op == 2]
    check("VirtualInput reports Shift as a modifier itself -- wlroots does not",
          _mods == [1, 0], str(_mods))
    check("VirtualInput clicks with the button code and ends each pointer event",
          [(_o, _op) for _o, _op, _b in _seen["msgs"] if _o == _vi.ptr][:2]
          == [(_vi.ptr, 2), (_vi.ptr, 4)])

with tempfile.TemporaryDirectory() as _tmp:
    _sock = os.path.join(_tmp, "w")
    _compositor(_sock, {"msgs": [], "offer": ("wl_seat",)})
    try:
        rbx.VirtualInput(_sock)
        check("a display without virtual input is refused by name", False)
    except RuntimeError as exc:
        check("a display without virtual input is refused by name",
              "zwp_virtual_keyboard_manager_v1" in str(exc), str(exc))

_old = {"script": "\nwait range(60,70)\nkey j\nwait range(340,341)\nkey j\nloop",
        "start_delay": 45, "hidden": False, "place_id": "", "place_name": ""}
_text = rbx.migrate_macro(_old)
check("the earlier manager's macros carry over with the same timing",
      _text == "start 45\nwait 60-70\ntap j\nwait 340-341\ntap j\n"
      and rbx.parse_macro(_text)[1][:2] == [("start", 45.0, 45.0), ("wait", 60.0, 70.0)],
      repr(_text))
with tempfile.TemporaryDirectory() as _tmp:
    rbx.MACROS = os.path.join(_tmp, "macros.json")
    with open(rbx.MACROS, "w") as _f:
        json.dump({"haki": _old, "new": "tap e\n", "junk": 5}, _f)
    check("loading converts old entries and drops what cannot be text",
          rbx.load_macros() == {"haki": _text, "new": "tap e\n"})
    rbx.save_macros({"a": "tap e\n", "b": "tap f\n"}, disabled={"b"})
    check("a switched-off macro keeps its text and its switch",
          rbx.load_macros() == {"a": "tap e\n", "b": "tap f\n"}
          and rbx.disabled_macros() == {"b"}, str(rbx.disabled_macros()))

    rbx.save_macros({"a": "tap e\n", "b": "tap f\n"}, disabled={"b"},
                    hotkeys={"a": "F6", "b": "<Control>q"})
    check("a hotkey is kept with its macro, switched off or not",
          rbx.load_macros() == {"a": "tap e\n", "b": "tap f\n"}
          and rbx.disabled_macros() == {"b"}
          and rbx.macro_hotkeys() == {"a": "F6", "b": "<Control>q"})

_src = "# note\nstart 5\ntap j\nhold shift+w 2\nwait 60-70\nfrob 1\nloop 3\n"
check("the editor's rows: types named, notes kept, the loop apart",
      rbx.macro_rows(_src)
      == ([("Note", "note"), ("Start", "5"), ("Key", "j"), ("Hold", "shift+w 2"),
           ("Wait", "60-70"), ("Frob", "1")], 3))
check("rows go back to the very text they came from, unknown commands included",
      rbx.macro_text(*rbx.macro_rows(_src)) == _src)
check("a macro with no loop line runs until stopped",
      rbx.macro_rows("tap e")[1] == 0 and rbx.loop_label(0) == "Until stopped"
      and rbx.loop_label(1) == "Once" and rbx.loop_label(3) == "3 rounds")
check("an editor-made macro is one the engine runs",
      rbx.parse_macro(rbx.macro_text([("Key", "e"), ("Wait", "0.5"),
                                      ("Click", "960 540")], 1))[0] == 1)

check("a new account is labelled after its username",
      rbx.unique_label("givemegoodname24", {"alt"}) == "givemegoodname24")
check("a taken label is numbered",
      rbx.unique_label("alt", {"alt", "alt 2"}) == "alt 3")
check("a username that is no valid label still gets one",
      rbx.valid_name(rbx.unique_label("_x", set()))
      and rbx.valid_name(rbx.unique_label("", set())))

import xml.dom.minidom
try:
    xml.dom.minidom.parseString("<markup>" + rbx.MACRO_HELP + "</markup>")
    check("the macro help is well-formed markup", True)
except Exception as exc:
    check("the macro help is well-formed markup", False, str(exc))

_stop = _Stop()
os.environ["XDG_RUNTIME_DIR"] = "/run/user/1000"
_in = _Input()


def _connect(path):
    _in.path = path
    return _in


_reports = []
rbx.run_macro("rbxmgr-7", 2, [("start", 7, 7), ("hold", [18], 0.05, 0.05), ("wait", 0, 0)],
              _stop, run=None, clients=lambda _r: {5: "rbxmgr-7"}, connect=_connect,
              report=_reports.append, now=lambda: 0)
check("a playing macro says what it is doing, and when a wait ends",
      _reports[:3] == [f"round 1: waiting 7s, until {__import__('time').strftime('%H:%M:%S', __import__('time').localtime(7))}",
                       "round 1: pressing e", _reports[2]]
      and _reports[2].startswith("round 1: waiting 0s") and _reports[3] == "round 2: pressing e"
      and rbx.describe_step(("hold", [42, 17], 1, 1)) == "pressing shift+w"
      and rbx.describe_step(("hold", [57], 0, 0)) == "pressing space", str(_reports))
check("a macro plays its loops into its own display only",
      _in.sent == [("key", 18, True), ("key", 18, False)] * 2
      and _in.path == "/run/user/1000/rbxmgr/rbxmgr-7.wayland" and _in.closed,
      str(_in.sent))
check("the start delay is waited once, not every pass",
      _stop.waits == [7, 0.05, 0, 0.05, 0], str(_stop.waits))


def _no_display(_path):
    raise FileNotFoundError(2, "No such file")


for _label, _conn, _clients in (
        ("a macro refuses a client that is not running", _connect, {}),
        ("a macro refuses a client outside a macro-ready window", _no_display,
         {5: "rbxmgr-7"})):
    _in = _Input()
    try:
        rbx.run_macro("rbxmgr-7", 0, [("hold", [18], 0, 0)], _Stop(), run=None,
                      clients=lambda _r, c=_clients: c, connect=_conn)
        check(_label, False)
    except RuntimeError:
        check(_label, _in.sent == [], str(_in.sent))
_in = _Input(fail_on=18)
try:
    rbx.run_macro("rbxmgr-7", 0, [("hold", [18], 0, 0)], _Stop(), run=None,
                  clients=lambda _r: {5: "rbxmgr-7"}, connect=_connect)
    check("a display that goes away ends the macro with a reason", False)
except RuntimeError as exc:
    check("a display that goes away ends the macro with a reason",
          "went away" in str(exc) and _in.closed, str(exc))
_stopped = __import__("threading").Event()
_stopped.set()
_in = _Input()
rbx.run_macro("rbxmgr-7", 0, [("hold", [18], 0, 0)], _stopped, run=None,
              clients=lambda _r: {5: "rbxmgr-7"}, connect=_connect)
check("a stopped macro sends nothing more", _in.sent == [])

_killed = []
_n = rbx.stop_profiles({"main"}, clients={1: "main", 2: "mine-from-cordial", 3: "main"},
                       run=lambda a, timeout=0: _killed.append(a))
check("stopping signals only the accounts' own clients",
      _n == 2 and _killed == [["kill", "1", "3"]], str(_killed))
_killed.clear()
check("stopping nothing runs nothing",
      rbx.stop_profiles({"main"}, clients={2: "other"},
                        run=lambda a, timeout=0: _killed.append(a)) == 0
      and _killed == [])

# Seeding: what Cordial's routing reads back must be exactly what it expects,
# parsed the way crates/cordial-shell/src/browser_account/profile.rs does.
check("a profile is keyed by user id, inside Cordial's name rule",
      rbx.cordial_profile(123) == "rbxmgr-123")
_cookie = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing|_ABC.def-123"
_store = rbx.cordial_cookie_store(_cookie)
_lines = [l for l in _store.splitlines() if l and not l.startswith("#")]
_found = set()
for _l in _lines:
    _host, _jar = _l.split("\t", 1)
    for _pair in _jar.split(";"):
        _n, _, _v = _pair.strip().partition("=")
        if _n == ".ROBLOSECURITY":
            _found.add(_v)
check("the seeded jar carries exactly the account's session",
      _found == {_cookie} and len(_lines) == 2, repr(_store))
check("the jar is in the settable name=value form Cordial keeps, not Netscape",
      all("=" in l.split("\t", 1)[1] and "\t" not in l.split("\t", 1)[1]
          for l in _lines))
_id = json.loads(rbx.cordial_identity({"id": 123, "name": "bob"}))
check("the identity is schema 1 with the id and username routing needs",
      _id["schema"] == 1 and _id["userId"] == 123 and _id["username"] == "bob")
_enc = rbx.cordial_encode("a\tb\n")
check("keyring bodies use Cordial's hex encoding",
      _enc == "cordial-secret-hex-v1:" + "a\tb\n".encode().hex())

_ran, _stored = [], []
_prof = rbx.seed_cordial_profile(
    {"id": 123, "name": "bob"}, _cookie,
    run=lambda a, timeout=0: (_ran.append(a), _R(0))[1],
    store=lambda attrs, label, secret: _stored.append((attrs, secret)))
_dir = os.path.join(rbx.CORDIAL_PROFILES, "rbxmgr-123")
check("seeding creates the profile directory routing scans",
      _prof == "rbxmgr-123" and _ran == [["mkdir", "-p", _dir]], str(_ran))
check("seeding writes identity and cookies under Cordial's attributes",
      [a["store"] for a, _ in _stored] == ["identity", "cookies"]
      and all(a == {"xdg:schema": "org.cordial.Session", "application": "cordial",
                    "profile": _dir, "store": a["store"]} for a, _ in _stored),
      str([a for a, _ in _stored]))
check("the cookie store is written byte-exact, hex-encoded as Cordial does",
      bytes.fromhex(_stored[1][1].split(":", 1)[1]).decode() == _store)
_cleared = []
rbx.clear_cordial_profile(123, clear=_cleared.append)
check("removing an account clears what was given to its profile",
      sorted(a["store"] for a in _cleared) == ["cookies", "identity"]
      and all(a["profile"] == _dir for a in _cleared), str(_cleared))

# A launch took the busy count twice (itself, then run_task) and gave it back
# once, so after the first launch every later one was refused as "wait for the
# current launch" for good. Launches take it only through run_task now, and
# nothing refuses a launch for being busy.
_ln = inspect.getsource(rbx.Window.launch_names)
check("a launch takes the busy count exactly once, through run_task",
      "set_busy(True)" not in _ln and "run_task(" in _ln, _ln[:200])
check("no launch is refused for another one running",
      "self.busy" not in _ln
      and "Wait for the current launch" not in open(
          os.path.join(_here, "roblox-manager.py")).read())

print()
if failures:
    print(f"FAILED ({len(failures)}): {', '.join(failures)}")
    sys.exit(1)
print("All roblox-manager checks passed.")
