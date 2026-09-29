#!/usr/bin/env python3
"""mujō Roblox manager: several Roblox accounts, launched together.

Roblox runs in Cordial -- mujō's fork of it, as the cordial-run and
cordial-fetch command-line tools (./cordial). Cordial runs several
accounts side by side natively -- one Cordial profile per account -- so this
app gives each account's profile its session and starts that profile's client
with a link to the chosen game. See the Cordial section below.

Nothing is injected into the Roblox client -- no LD_PRELOAD, no memory reads, no
hooks. Every capability here is built from Roblox's own web APIs, plus a
`roblox://experiences/start` link to the game -- the engine's own deep link,
the one Roblox's mobile app is opened with.

Accounts are added through Roblox's Quick Login (cross-device) flow: this app
asks Roblox for a code, opens your browser at Roblox's own confirmation page,
and redeems the approved code for a session cookie. The password is typed into
roblox.com and is never seen by this process.

Sensitive data: the session cookie lives only in the Secret Service (gnome
keyring, encrypted at rest under your login password). accounts.json holds names,
numeric user ids, notes, the leader's
favourited games and timestamps -- no credentials; game icons are cached in
~/.cache/rbxmgr/_icons. Each account's Cordial profile gets a copy of its
cookie, also in the keyring (seed_cordial_profile), and Cordial is started with
CORDIAL_SECRET_STORE=keyring so it never writes one to a plaintext file.

Self-check: test-roblox-manager.py
"""

import json
import os
import random
import re
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, GObject, Gtk, Pango  # noqa: E402

SCHEMA = "io.github.mujo.RobloxManager"

STATE = os.path.join(
    os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share")), "rbxmgr"
)
CACHE = os.path.join(
    os.environ.get("XDG_CACHE_HOME", os.path.expanduser("~/.cache")), "rbxmgr"
)
ACCOUNTS = os.path.join(STATE, "accounts.json")

UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Roblox/WinInet"

# How long to wait for Roblox to report the leader's server, and how far apart to
# space launches. The stagger is not politeness -- several accounts signing in
# from one IP in the same second is what gets Roblox to start refusing.
LEADER_TIMEOUT = 90
LEADER_POLL = 3
DEFAULT_STAGGER = 8

# Quick Login: the code is short-lived, so poll briskly and give up rather than
# leave a stale code sitting approved.
QL_POLL = 3
QL_TIMEOUT = 180
QL_CONFIRM_URL = "https://www.roblox.com/crossdevicelogin/confirmcode"


# --------------------------------------------------------------------------
# accounts.json is an index, not a secret store.
# --------------------------------------------------------------------------
def load_accounts():
    try:
        with open(ACCOUNTS) as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError):
        return []
    return data if isinstance(data, list) else []


def _save_json(path, data):
    """Write-then-rename, so a crash mid-write never leaves half a file."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path + ".tmp", "w") as f:
        json.dump(data, f, indent=2)
    os.replace(path + ".tmp", path)


def save_accounts(accounts):
    _save_json(ACCOUNTS, accounts)


# --------------------------------------------------------------------------
# Layout: one leader, the accounts that auto-join its server, and named
# groups. Kept on the account dicts -- "leader": True, "follow": its place in
# the auto-join order, "group": a group id -- so a rename carries them along.
# The groups themselves (id, name, place, whether open) are groups.json.
# --------------------------------------------------------------------------
GROUPS = os.path.join(STATE, "groups.json")


def load_groups():
    """The groups, or None before the first save -- when the old layout
    (first selected account leads, the other selected follow) is carried
    over by migrate_layout."""
    try:
        with open(GROUPS) as f:
            data = json.load(f)
    except FileNotFoundError:
        return None
    except (OSError, json.JSONDecodeError):
        return []
    return [g for g in data if isinstance(g, dict) and g.get("id")] \
        if isinstance(data, list) else []


def save_groups(groups):
    _save_json(GROUPS, groups)


def migrate_layout(accounts):
    """The earlier layout as leader and auto-join: the first selected account
    led "Launch as group" and the rest selected followed it."""
    if any(a.get("leader") or a.get("follow") for a in accounts):
        return
    chosen = [a for a in accounts if a.get("selected", True)]
    for i, a in enumerate(chosen):
        if i == 0:
            a["leader"] = True
        else:
            a["follow"] = i


def leader_of(accounts):
    return next((a for a in accounts if a.get("leader")), None)


def followers_of(accounts):
    """The auto-join order. The leader never follows itself."""
    return sorted((a for a in accounts if a.get("follow") and not a.get("leader")),
                  key=lambda a: a["follow"])


def _renumber(followers):
    for i, a in enumerate(followers, 1):
        a["follow"] = i


def make_leader(accounts, name):
    """name leads, selected; it leaves the auto-join list. Its group stays on
    the account, so it goes back there when another account takes the lead."""
    for a in accounts:
        a.pop("leader", None)
        if a["name"] == name:
            a["leader"] = True
            a["selected"] = True
            a.pop("follow", None)
    _renumber(followers_of(accounts))


def set_follow(accounts, name, on):
    rest = [a for a in followers_of(accounts) if a["name"] != name]
    for a in accounts:
        if a["name"] == name:
            a.pop("follow", None)
            if on and not a.get("leader"):
                rest.append(a)
    _renumber(rest)


def move_follower(accounts, name, delta):
    fs = followers_of(accounts)
    i = next((k for k, a in enumerate(fs) if a["name"] == name), -1)
    j = i + delta
    if i >= 0 and 0 <= j < len(fs):
        fs[i], fs[j] = fs[j], fs[i]
        _renumber(fs)


def group_of(acct, groups):
    gid = acct.get("group")
    return gid if any(g["id"] == gid for g in groups) else None


def visual_order(accounts, groups):
    """Everyone but the leader, as drawn: group by group, then the ungrouped,
    each in list order."""
    rest = [a for a in accounts if not a.get("leader")]
    order = [g["id"] for g in groups] + [None]
    return [a for gid in order for a in rest if group_of(a, groups) == gid]


def drop_account(accounts, groups, name, onto):
    """Dragging name onto the row of onto: it takes onto's group and its
    place in the drawn order -- after it when dragged down, before when up.
    The list is rewritten in drawn order, so that is what it keeps."""
    by = {a["name"]: a for a in accounts}
    if name == onto or name not in by or onto not in by or by[name].get("leader"):
        return
    vis = visual_order(accounts, groups)
    src, dst = vis.index(by[name]), vis.index(by[onto])
    vis.pop(src)
    vis.insert(dst, by[name])
    by[name]["group"] = by[onto].get("group")
    accounts[:] = [a for a in accounts if a.get("leader")] + vis


def relative_time(iso, now=None):
    """'3m ago' / 'never'. Display only -- never used for ordering."""
    if not iso:
        return "never launched"
    try:
        then = datetime.fromisoformat(iso)
    except ValueError:
        return "never launched"
    if then.tzinfo is None:
        then = then.replace(tzinfo=timezone.utc)
    secs = ((now or datetime.now(timezone.utc)) - then).total_seconds()
    if secs < 0:
        return "just now"
    for limit, div, unit in (
        (60, 1, "s"), (3600, 60, "m"), (86400, 3600, "h"), (2592000, 86400, "d"),
    ):
        if secs < limit:
            return f"{int(secs // div)}{unit} ago"
    return then.strftime("%Y-%m-%d")


# --------------------------------------------------------------------------
# Secrets, in the Secret Service. Cookies travel over D-Bus (keyring_store),
# never in an argv that would show up in ps.
# --------------------------------------------------------------------------
# --- Secret Service, over D-Bus -------------------------------------------
# The login keyring is not necessarily unlocked. On a machine that autologins,
# PAM never sees a password, so gnome-keyring has no key to unlock the login
# collection with and every write fails with "Cannot create an item in a locked
# collection". libsecret does not prompt on its own here -- not on store and not
# on lookup -- so the unlock has to be asked for explicitly, which is what routes
# it through the desktop's keyring prompter.
SECRETS_BUS = "org.freedesktop.secrets"
SECRETS_PATH = "/org/freedesktop/secrets"
SVC_IFACE = "org.freedesktop.Secret.Service"
COLL_IFACE = "org.freedesktop.Secret.Collection"
ITEM_IFACE = "org.freedesktop.Secret.Item"
PROMPT_IFACE = "org.freedesktop.Secret.Prompt"


def _secrets_call(bus, path, iface, method, args=None, reply=None):
    return bus.call_sync(SECRETS_BUS, path, iface, method, args, reply,
                         Gio.DBusCallFlags.NONE, 10000, None)


def default_collection(bus):
    r = _secrets_call(bus, SECRETS_PATH, SVC_IFACE, "ReadAlias",
                      GLib.Variant("(s)", ("default",)),
                      GLib.VariantType("(o)"))
    return r.unpack()[0]


def collection_locked(bus, path):
    r = _secrets_call(bus, path, "org.freedesktop.DBus.Properties", "Get",
                      GLib.Variant("(ss)", (COLL_IFACE, "Locked")),
                      GLib.VariantType("(v)"))
    return bool(r.unpack()[0])


def unlock_default_collection(on_done):
    """Unlock the default keyring, prompting if needed. Main thread only.

    Calls on_done(None) when usable, or on_done(message) when not. The prompt is
    asynchronous, so the Completed signal has to be subscribed before Prompt() is
    called -- and the subscription only delivers on a thread with a running main
    context, which is why this must not be called from a worker thread.
    """
    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        path = default_collection(bus)
        if path == "/":
            on_done("there is no default keyring collection")
            return
        if not collection_locked(bus, path):
            on_done(None)
            return

        r = _secrets_call(bus, SECRETS_PATH, SVC_IFACE, "Unlock",
                          GLib.Variant("(ao)", ([path],)),
                          GLib.VariantType("(aoo)"))
        unlocked, prompt = r.unpack()
        if unlocked:
            on_done(None)
            return
        if prompt == "/":
            on_done("the keyring stayed locked and offered no prompt")
            return

        sub = {}

        def completed(_conn, _sender, _obj, _iface, _signal, params):
            dismissed = params.unpack()[0]
            bus.signal_unsubscribe(sub["id"])
            on_done("the keyring unlock prompt was dismissed" if dismissed
                    else None)

        sub["id"] = bus.signal_subscribe(
            SECRETS_BUS, PROMPT_IFACE, "Completed", prompt, None,
            Gio.DBusSignalFlags.NONE, completed)
        _secrets_call(bus, prompt, PROMPT_IFACE, "Prompt",
                      GLib.Variant("(s)", ("",)), GLib.VariantType("()"))
    except Exception as e:
        on_done(f"could not unlock the keyring: {e}")


def ensure_keyring_unlocked(timeout=180, schedule=None, unlock=None):
    """Block the calling worker thread until the keyring is usable.

    The unlock itself runs on the main thread (it needs the main loop for the
    prompt), so this hands it over and waits on an Event.
    """
    schedule = schedule or once
    unlock = unlock or unlock_default_collection
    box = {}
    done = threading.Event()

    def finish(err):
        box["err"] = err
        done.set()

    schedule(unlock, finish)
    if not done.wait(timeout):
        raise RuntimeError(f"keyring did not unlock within {timeout}s")
    if box.get("err"):
        raise RuntimeError(box["err"])


# --------------------------------------------------------------------------
# The keyring, spoken to directly over D-Bus rather than through secret-tool.
#
# Not a preference: libsecret, and so secret-tool, ignores the desktop keyring
# inside a Flatpak whenever the Secret portal exists -- finding /.flatpak-info
# overrides even SECRET_BACKEND=service -- and keeps a private keyring file of
# the app's own instead. Cordial reads the sessions this app gives it from the
# desktop keyring, so from inside the manager's Flatpak every seeded session
# vanished and every client started signed out. The D-Bus interface below is
# the same one libsecret uses outside a sandbox, and the one Cordial uses.
#
# "plain" session: the secret travels unencrypted over the session bus, which
# only this user's processes can reach -- the same trade Cordial makes.
# --------------------------------------------------------------------------
def _bus():
    return Gio.bus_get_sync(Gio.BusType.SESSION, None)


def _keyring_session(bus):
    r = _secrets_call(bus, SECRETS_PATH, SVC_IFACE, "OpenSession",
                      GLib.Variant("(sv)", ("plain", GLib.Variant("s", ""))),
                      GLib.VariantType("(vo)"))
    return r.unpack()[1]


def _keyring_search(bus, attrs):
    """Unlocked items carrying at least `attrs` (the service matches a subset,
    so an item secret-tool wrote with an extra xdg:schema is found too)."""
    r = _secrets_call(bus, SECRETS_PATH, SVC_IFACE, "SearchItems",
                      GLib.Variant("(a{ss})", (attrs,)),
                      GLib.VariantType("(aoao)"))
    return list(r.unpack()[0])


def _keyring_delete(bus, items):
    for item in items:
        _secrets_call(bus, item, ITEM_IFACE, "Delete", None,
                      GLib.VariantType("(o)"))


def keyring_lookup(attrs, bus=None):
    """The secret stored under `attrs`, as text, or None."""
    bus = bus or _bus()
    items = _keyring_search(bus, attrs)
    if not items:
        return None
    r = _secrets_call(bus, SECRETS_PATH, SVC_IFACE, "GetSecrets",
                      GLib.Variant("(aoo)", (items[:1], _keyring_session(bus))),
                      GLib.VariantType("(a{o(oayays)})"))
    secrets = r.unpack()[0]
    return bytes(secrets[items[0]][2]).decode() if items[0] in secrets else None


def keyring_store(attrs, label, secret, bus=None):
    """Store `secret` (text) under exactly `attrs`, replacing what was there.

    Existing matches are deleted first rather than left to CreateItem's
    replace: that only replaces an item with the *same* attribute set, and an
    entry secret-tool wrote earlier carries an extra xdg:schema, so it would
    survive beside the new one as a stale second answer to every lookup.
    """
    bus = bus or _bus()
    _keyring_delete(bus, _keyring_search(bus, attrs))
    collection = default_collection(bus)
    props = {
        "org.freedesktop.Secret.Item.Label": GLib.Variant("s", label),
        "org.freedesktop.Secret.Item.Attributes": GLib.Variant("a{ss}", attrs),
    }
    value = (_keyring_session(bus), b"", secret.encode(), "text/plain; charset=utf8")
    r = _secrets_call(bus, collection, COLL_IFACE, "CreateItem",
                      GLib.Variant("(a{sv}(oayays)b)", (props, value, True)),
                      GLib.VariantType("(oo)"))
    if r.unpack()[1] != "/":
        raise RuntimeError("the keyring is locked -- unlock it and try again")


def keyring_clear(attrs, bus=None):
    bus = bus or _bus()
    _keyring_delete(bus, _keyring_search(bus, attrs))


def _account_attrs(name):
    return {"app": "rbxmgr", "account": name}


def secret_store(name, cookie):
    ensure_keyring_unlocked()
    keyring_store(_account_attrs(name), f"rbxmgr {name}", cookie)


def secret_lookup(name):
    ensure_keyring_unlocked()
    out = (keyring_lookup(_account_attrs(name)) or "").strip()
    if not out:
        raise RuntimeError(f"no cookie in keyring for '{name}' -- use "
                           "'Sign in again' on the account")
    return out


def secret_clear(name):
    try:
        keyring_clear(_account_attrs(name))
    except Exception:
        pass    # a locked or absent keyring has nothing of ours to clear


# --------------------------------------------------------------------------
# Roblox web API. stdlib only -- this is a handful of requests, not a client
# library. Roblox hands out its CSRF token on the rejection rather than on
# request, so every POST retries once with the token it was just given.
# --------------------------------------------------------------------------
def _open(url, cookie=None, method="GET", body=None, csrf=None, referer=False):
    headers = {"User-Agent": UA}
    if cookie:
        headers["Cookie"] = f".ROBLOSECURITY={cookie}"
    if csrf:
        headers["X-CSRF-TOKEN"] = csrf
    if referer:
        headers["Referer"] = "https://www.roblox.com/"
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    return urllib.request.urlopen(req, timeout=20)


def post_csrf(url, cookie=None, body=None, referer=False):
    """POST, learning the CSRF token from the first 403 and retrying once."""
    try:
        return _open(url, cookie, "POST", body, referer=referer)
    except urllib.error.HTTPError as first:
        token = first.headers.get("x-csrf-token")
        if first.code != 403 or not token:
            raise
        return _open(url, cookie, "POST", body, csrf=token, referer=referer)


def http_detail(e):
    """Turn an HTTPError into something a user can act on.

    Roblox answers most refusals with a JSON body naming the reason, and gates
    some logins behind a 2FA challenge announced only in a header. Without both
    of those surfaced, every failure reads as an indistinguishable "HTTP 403".
    """
    bits = [f"HTTP {e.code}"]
    if e.headers.get("rblx-challenge-type") or e.headers.get("rbx-challenge-id"):
        bits.append("Roblox wants a 2FA/security challenge for this account, "
                    "which this flow cannot answer -- approve the login on a "
                    "device where that account is already verified")
    try:
        body = e.read(400).decode(errors="replace").strip()
    except Exception:
        body = ""
    if body:
        bits.append(body)
    return " -- ".join(bits)


class SessionExpired(RuntimeError):
    """The stored cookie was refused: the account's row says Expired."""


def authenticated_user(cookie):
    try:
        with _open("https://users.roblox.com/v1/users/authenticated", cookie) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 401:
            raise SessionExpired("the stored session has expired or was signed "
                                 "out -- use 'Sign in again' on the account") from e
        raise


def presences(cookie, user_ids, batch=50):
    """Presence API entries for user_ids, asked as `cookie`'s account -- the
    server (gameId) is only filled in where that account may join."""
    out = []
    for i in range(0, len(user_ids), batch):
        with post_csrf(
            "https://presence.roblox.com/v1/presence/users",
            cookie, body={"userIds": list(user_ids[i:i + batch])},
        ) as r:
            out += json.load(r).get("userPresences") or []
    return out


def leader_presence(cookie, user_id):
    """The leader's server instance and place from the presence API.

    Returns (job_id, place_id) or (None, None).
    """
    found = presences(cookie, [user_id])
    if not found:
        return None, None
    p = found[0]
    job_id = p.get("gameId") or None
    place_id = str(p["placeId"]) if p.get("placeId") else None
    return job_id, place_id


def parse_in_game(entries):
    """Presence entries -> [{user_id, place_id, job_id, game}] for those in a
    game (type 2) whose place is visible. job_id is None when their privacy
    hides the server from the asking account."""
    return [{"user_id": int(p["userId"]), "place_id": str(p["placeId"]),
             "job_id": p.get("gameId") or None,
             "game": p.get("lastLocation") or f"Place {p['placeId']}"}
            for p in entries
            if p.get("userPresenceType") == 2 and p.get("placeId") and p.get("userId")]


FRIEND_RANK = {"game": 0, "online": 1, "offline": 2}


def friends_status(cookie, user_id, batch=100):
    """Every friend of the account and where they are now: state "game",
    "online" or "offline"; place_id/job_id/game only for those in a game
    whose place shows. In a game first, then online, then by name.

    Three public web APIs: the friend list (ids only -- Roblox blanks the
    names there), their presence, then their names.
    """
    with _open(f"https://friends.roblox.com/v1/users/{int(user_id)}/friends",
               cookie) as r:
        ids = [int(f["id"]) for f in json.load(r).get("data") or [] if f.get("id")]
    entries = presences(cookie, ids)
    playing = {p["user_id"]: p for p in parse_in_game(entries)}
    # Studio (3), and a game whose place is hidden, show as plain online.
    online = {int(p["userId"]) for p in entries
              if p.get("userPresenceType") in (1, 2, 3) and p.get("userId")}
    names = {}
    for i in range(0, len(ids), batch):
        with post_csrf("https://users.roblox.com/v1/users",
                       body={"userIds": ids[i:i + batch]}) as r:
            names.update({u["id"]: u for u in json.load(r).get("data") or []})
    out = []
    for uid in ids:
        f = playing.get(uid) or {"user_id": uid, "place_id": None,
                                 "job_id": None, "game": None}
        f["state"] = ("game" if uid in playing else
                      "online" if uid in online else "offline")
        u = names.get(uid) or {}
        f["name"] = u.get("name") or f"user {uid}"
        f["display"] = u.get("displayName") or f["name"]
        out.append(f)
    return sorted(out, key=lambda f: (FRIEND_RANK[f["state"]], f["display"].lower()))


def job_id_of(cookie, user_id):
    """The leader's server instance, from the presence API -- not the client."""
    job_id, _ = leader_presence(cookie, user_id)
    return job_id


# --------------------------------------------------------------------------
# Favourite games. Read-only catalogue data -- the account's own favourites and
# the icons the Roblox site shows for them. Nothing here can launch anything.
# --------------------------------------------------------------------------
ICONS = os.path.join(CACHE, "_icons")   # leading _ so no account label can claim it
FAVORITES_SHOWN = 24


def parse_favorites(payload):
    """v2 favourites -> [{universe_id, place_id, name}].

    Entries with no rootPlace are dropped: the launcher joins a *place*, so a
    universe this app cannot open would only ever be a tile that fails.
    """
    games = []
    for g in payload.get("data") or []:
        root = (g.get("rootPlace") or {}).get("id")
        if not root:
            continue
        games.append({
            "universe_id": str(g.get("id") or ""),
            "place_id": str(root),
            "name": g.get("name") or f"Place {root}",
        })
    return games


def favorite_games(cookie, user_id, limit=FAVORITES_SHOWN):
    """The account's favourited games, most recently favourited first.

    Roblox's page size only takes 10/25/50/100, so the page is asked for whole
    and cut down here.
    """
    url = (f"https://games.roblox.com/v2/users/{user_id}/favorite/games"
           "?limit=50&sortOrder=Desc")
    with _open(url, cookie) as r:
        return parse_favorites(json.load(r))[:limit]


def merge_favorites(accounts, limit=FAVORITES_SHOWN):
    """Every account's favourites in one strip, best games first.

    Ranked by how often the game was actually launched from here, then by how
    many accounts favourited it, then by how near the top of a favourites list
    it sat. One account's list is no longer the whole bar.
    """
    plays = {}
    for acct in accounts:
        for place, n in (acct.get("plays") or {}).items():
            plays[place] = plays.get(place, 0) + int(n or 0)
    merged = {}
    for acct in accounts:
        for pos, g in enumerate(acct.get("favorites") or []):
            e = merged.get(g["place_id"])
            if e is None:
                merged[g["place_id"]] = dict(g, accounts=1, pos=pos)
            else:
                e["accounts"] += 1
                e["pos"] = min(e["pos"], pos)
    ranked = sorted(merged.values(),
                    key=lambda e: (-plays.get(e["place_id"], 0),
                                   -e["accounts"], e["pos"], e["name"]))
    return [{k: e[k] for k in ("universe_id", "place_id", "name")}
            for e in ranked[:limit]]


def parse_icon_urls(payload):
    """thumbnails v1 -> {universe_id: url}, skipping the ones still rendering."""
    return {
        str(d.get("targetId")): d["imageUrl"]
        for d in payload.get("data") or []
        if d.get("state") == "Completed" and d.get("imageUrl")
    }


def game_icon_urls(universe_ids):
    """One batched thumbnail request. No cookie: game icons are public."""
    ids = [str(i) for i in universe_ids if i]
    if not ids:
        return {}
    url = ("https://thumbnails.roblox.com/v1/games/icons?universeIds="
           + ",".join(ids) + "&size=150x150&format=Png&isCircular=false")
    with _open(url) as r:
        return parse_icon_urls(json.load(r))


def cached_icon(universe_id, url=None):
    """Path to a game's icon, downloaded at most once. None when there is none.

    Icons live in ~/.cache, not /persist: they are regenerable. Called with no
    url it asks the cache only.
    """
    if not universe_id:
        return None
    path = os.path.join(ICONS, f"{universe_id}.png")
    if os.path.exists(path):
        return path
    if not url:
        return None
    try:
        with _open(url) as r:
            data = r.read()
        os.makedirs(ICONS, exist_ok=True)
        with open(path + ".tmp", "wb") as f:
            f.write(data)
        os.replace(path + ".tmp", path)
    except Exception:
        return None           # a missing icon is a placeholder, not a failure
    return path


def with_icons(games):
    """Fill in each game's cached icon path. Network; worker thread only."""
    try:
        urls = game_icon_urls([g["universe_id"] for g in games])
    except Exception:
        urls = {}
    for g in games:
        g["icon"] = cached_icon(g["universe_id"], urls.get(g["universe_id"]))
    return games


# --------------------------------------------------------------------------
# Quick Login (Roblox's own cross-device flow). No password reaches this app.
# --------------------------------------------------------------------------
def quick_login_create():
    try:
        with post_csrf(
            "https://apis.roblox.com/auth-token-service/v1/login/create", body={}
        ) as r:
            d = json.load(r)
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"could not get a code: {http_detail(e)}") from e
    if not d.get("code") or not d.get("privateKey"):
        raise RuntimeError("Quick Login create returned no code")
    return d["code"], d["privateKey"]


def quick_login_status(code, private_key):
    try:
        with post_csrf(
            "https://apis.roblox.com/auth-token-service/v1/login/status",
            body={"code": code, "privateKey": private_key},
        ) as r:
            return json.load(r).get("status", "Unknown")
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"could not check the code: {http_detail(e)}") from e


def cookie_from_set_cookie(headers):
    """Pull .ROBLOSECURITY out of a Set-Cookie header list."""
    for raw in headers:
        for part in raw.split(";"):
            k, _, v = part.strip().partition("=")
            if k == ".ROBLOSECURITY" and v:
                return v
    return None


def quick_login_redeem(code, private_key):
    """Exchange an approved code for a session cookie."""
    try:
        with post_csrf(
            "https://auth.roblox.com/v2/login",
            body={"ctype": "AuthToken", "cvalue": code, "password": private_key},
            referer=True,
        ) as r:
            cookie = cookie_from_set_cookie(r.headers.get_all("Set-Cookie") or [])
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"redeeming the code failed: {http_detail(e)}") from e
    if not cookie:
        raise RuntimeError("Roblox accepted the code but sent no .ROBLOSECURITY "
                           "cookie back")
    return cookie


class CodeExpired(RuntimeError):
    """The code went unapproved for its whole life; the dialog asks for a new one."""


def quick_login(log, on_code, on_tick=None, cancelled=None, on_status=None,
                poll=QL_POLL, timeout=QL_TIMEOUT, sleep=time.sleep):
    """Full add-an-account flow. Returns (cookie, user) or raises.

    This never opens a browser. It asks Roblox for a code, hands it to on_code
    for display, and polls until you have entered that code at Roblox yourself.
    Putting the code in a URL for the browser to consume is deliberately not an
    option here: typing it on the device you trust is what the cross-device flow
    is for, and it keeps the app from launching anything.

    on_tick(seconds_remaining) drives the countdown; cancelled() lets the dialog
    stop the poll; on_status(status) hears each poll's answer. Callables are injected so the self-check can drive the whole
    flow with no network, no browser and no waiting.
    """
    code, key = quick_login_create()
    on_code(code)
    log(f"Enter code {code} at {QL_CONFIRM_URL}")

    waited = 0
    while waited < timeout:
        if cancelled is not None and cancelled():
            raise RuntimeError("cancelled")
        if on_tick is not None:
            on_tick(timeout - waited)
        sleep(poll)
        waited += poll
        status = quick_login_status(code, key)
        if on_status is not None:
            on_status(status)
        if status == "Validated":
            cookie = quick_login_redeem(code, key)
            return cookie, authenticated_user(cookie)
        if status == "Cancelled":
            raise RuntimeError("login was rejected at Roblox")
        if status == "UserLinked":
            log(f"Code {code} accepted -- now confirm it at Roblox")
    raise CodeExpired(f"no approval within {timeout}s")


# --------------------------------------------------------------------------
# Launch URI. Pure string building, which is why the self-check covers it.
# --------------------------------------------------------------------------
def join_url(place_id, job_id=None):
    """The link handed to the engine, or None for Roblox's own home screen.

    The engine's own form (roblox://experiences/start), not the website's
    roblox-player: one. Cordial 0.18 refuses a roblox-player: link that names
    a server, which is exactly what a follower joining its leader needs; this
    form it passes through untouched, and gameInstanceId is the engine's name
    for the server (v0.19 translates the website's gameId to it). There is no
    ticket in it: the engine signs in from the session seeded into the
    account's profile, so no ticket is ever redeemed -- see launch_client.
    Both values come from Roblox's own APIs; they are checked anyway, since
    anything else in them would add parameters to the link.
    """
    if not place_id:
        return None
    if not str(place_id).isdigit():
        raise ValueError(f"place id {place_id!r} is not a number")
    url = f"roblox://experiences/start?placeId={place_id}"
    if job_id:
        if not all(c.isalnum() or c == "-" for c in str(job_id)):
            raise ValueError(f"server id {job_id!r} is not a server id")
        url += f"&gameInstanceId={job_id}"
    return url


# --------------------------------------------------------------------------
# Labels.
# --------------------------------------------------------------------------
NAME_RULE = ("A label cannot be empty, start with '.' or '_', or contain '/' "
             "or control characters")


def valid_name(name):
    """A label is a keyring key, and older installs also used it as a directory
    name, so it stays a single, ordinary path component. A leading '_' is
    reserved for ~/.cache/rbxmgr/_icons."""
    return (bool(name) and "/" not in name and name[0] not in "._"
            and name.isprintable())


def unique_label(base, taken):
    """A label for a newly approved account: its Roblox username, numbered
    when another account already carries it."""
    base = (base or "").strip()
    base = base if valid_name(base) else f"acct {base}".strip()
    name, n = base, 2
    while name in taken:
        name, n = f"{base} {n}", n + 1
    return name


def move_account_data(old, new):
    """Re-key an account's keyring entry. Runs on a worker thread -- secret_*
    need the main loop free to drive the keyring prompt."""
    secret_store(new, secret_lookup(old))
    secret_clear(old)


# --------------------------------------------------------------------------
# Cordial, the Roblox runtime: mujō's fork, as two command-line tools on PATH
# (./cordial). `cordial-run` is one account's game client and
# `cordial-fetch` installs the Roblox build it runs; everything a launcher does
# is done here.
#
# Cordial runs one engine process per profile (`cordial-run --profile <name>`),
# so several accounts need no fake HOMEs, namespaces or lock workarounds. The
# manager starts each one itself, with the game's own deep link, which is how
# followers reach their leader's server (roblox-player: links naming a server
# are refused by the engine) and what Stop acts on.
#
# The engine signs in from the session saved in its profile, so before every
# launch the manager writes the account's profile itself -- rbxmgr-<userId>,
# with the cookie it already holds. It is written into the keyring, in the
# format Cordial 0.18 reads (crates/cordial-shell/src/secrets.rs, cookies.rs
# and identity.rs at the pinned build). An upstream bump that changes it needs
# this changed with it.
# --------------------------------------------------------------------------
# A client still up this long after starting got past the checks that end
# one at once (a profile already in use, a missing engine, a bad link).
STARTUP_CHECK = 5
_XDG = {k: os.environ.get(f"XDG_{k}_HOME") or os.path.expanduser(d)
        for k, d in (("DATA", "~/.local/share"), ("CONFIG", "~/.config"))}
CORDIAL_PROFILES = os.path.join(_XDG["DATA"], "cordial/profiles")
# Settings in the format upstream's window saved them; nothing writes it now,
# so it only exists when carried over (migrate_flatpak_cordial) or hand-written.
CORDIAL_SHELL_JSON = os.path.join(_XDG["CONFIG"], "cordial/shell.json")
LOGS = os.path.join(CACHE, "logs")

# Where the Flatpak build of Cordial, which the manager used before the fork,
# kept the same things.
FLATPAK_CORDIAL = os.path.expanduser("~/.var/app/io.github.luohoa97.Cordial")


def run_host(argv, timeout=60, input=None):
    feed = {"input": input} if input is not None else {"stdin": subprocess.DEVNULL}
    return subprocess.run(argv, timeout=timeout, capture_output=True, **feed)


def _last_line(raw):
    lines = raw.decode(errors="replace").strip().splitlines()
    return lines[-1] if lines else ""


def _detach(argv, out=subprocess.DEVNULL, env=None):
    """Start argv in its own session, reaped on a thread rather than left a
    zombie."""
    p = subprocess.Popen(argv, start_new_session=True, stdin=subprocess.DEVNULL,
                         stdout=out, stderr=subprocess.STDOUT,
                         env=None if env is None else {**os.environ, **env})
    threading.Thread(target=p.wait, daemon=True).start()
    return p


def _read_host(path, run=run_host):
    r = run(["cat", path], timeout=10)
    return r.stdout.decode(errors="replace") if r.returncode == 0 else None


def cordial_settings(run=run_host):
    try:
        cfg = json.loads(_read_host(CORDIAL_SHELL_JSON, run) or "{}")
    except json.JSONDecodeError:
        cfg = {}
    return cfg if isinstance(cfg, dict) else {}


def _fetch_result(r):
    """(lib_dir, apk) from cordial-fetch's JSON line, or None."""
    try:
        got = json.loads(_last_line(r.stdout))
    except json.JSONDecodeError:
        return None
    return (got["engine"], got["apk"]) if got.get("engine") and got.get("apk") else None


_fetch_lock = threading.Lock()


def roblox_build(log, run=run_host, newest=False):
    """(lib_dir, apk) of the installed Roblox build, installing one first when
    there is none -- or, with newest, whenever a newer one exists. Installing
    takes a copy already on this machine when there is one, and downloads
    otherwise; either way cordial-fetch only installs what Roblox signed.
    One at a time: the startup of two launches must not both download."""
    with _fetch_lock:
        if not newest:
            got = _fetch_result(run(["cordial-fetch", "--status"], timeout=30))
            if got:
                return got
        log("Getting the Roblox build (first run or update; this can take a "
            "few minutes)...")
        r = run(["cordial-fetch", *(["--newest"] if newest else [])], timeout=3600)
        got = _fetch_result(r) if r.returncode == 0 else None
        if not got:
            raise RuntimeError(f"could not install Roblox: "
                               f"{_last_line(r.stderr) or r.returncode}")
        return got


def migrate_flatpak_cordial(log, clear=None, clients=None):
    """Carry the manager's profiles and Cordial's settings over from the
    Flatpak Cordial it used before the fork, once: the profiles hold each
    account's Roblox storage and settings. Moved, not copied, and never over
    something already here. The sessions the Flatpak profiles were given are
    dropped from the keyring -- they are keyed by the old path, and the
    manager gives every profile a fresh one before each launch. A profile a
    client still has open (an old Flatpak one) waits for the next start."""
    clear = clear or keyring_clear
    in_use = set((clients or cordial_clients)().values())
    old = os.path.join(FLATPAK_CORDIAL, "data/cordial/profiles")
    try:
        names = [n for n in os.listdir(old) if n.startswith("rbxmgr-")]
    except OSError:
        names = []
    for name in names:
        dest = os.path.join(CORDIAL_PROFILES, name)
        if os.path.exists(dest) or name in in_use:
            continue
        os.makedirs(CORDIAL_PROFILES, exist_ok=True)
        # shutil, not os.replace: under impermanence the two sides are
        # separate bind mounts, and rename() refuses to cross those (EXDEV)
        # even on one disk. It copies then deletes there.
        shutil.move(os.path.join(old, name), dest)
        for kind in ("identity", "cookies"):
            try:
                clear(dict(cordial_secret_attrs(name, kind),
                           profile=os.path.join(old, name)))
            except Exception:
                pass    # a locked keyring keeps a stale copy; never fatal
        log(f"Moved Cordial profile {name} out of the old Flatpak")
    old_cfg = os.path.join(FLATPAK_CORDIAL, "config/cordial/shell.json")
    if os.path.exists(old_cfg) and not os.path.exists(CORDIAL_SHELL_JSON):
        os.makedirs(os.path.dirname(CORDIAL_SHELL_JSON), exist_ok=True)
        with open(old_cfg, "rb") as src, open(CORDIAL_SHELL_JSON, "wb") as dst:
            dst.write(src.read())


def cordial_engine_env(cfg):
    """Cordial's settings as the environment its window gives an engine
    (spawn() in crates/cordial-shell/src/launch.rs, 0.18). An absent key keeps
    the engine's own default, as it does there.
    # ponytail: MangoHud and unpacked plugins are not carried; add them from
    # the same function if either is ever turned on in Cordial.
    """
    env = {"CORDIAL_SECRET_STORE": "keyring"}
    if cfg.get("gamemode") is False:
        env["CORDIAL_GAMEMODE"] = "0"
    if cfg.get("title_bar") in ("compact", "hidden"):
        env["CORDIAL_TITLE_BAR"] = cfg["title_bar"]
    if cfg.get("throttle") in ("visible", "unfocused", "off"):
        env["CORDIAL_THROTTLE"] = cfg["throttle"]
    accel = {"unlockedcursor": "unlocked", "always": "always"}.get(
        cfg.get("pointer_acceleration"))
    if accel:
        env["CORDIAL_POINTER_ACCEL"] = accel
    if cfg.get("graphics") not in (None, "", "automatic"):
        env["CORDIAL_GRAPHICS"] = str(cfg["graphics"])
    if cfg.get("present_mode") in ("fifo", "mailbox", "immediate"):
        env["CORDIAL_PRESENT_MODE"] = cfg["present_mode"]
    if cfg.get("gamepad") is False:
        env["CORDIAL_GAMEPAD"] = "0"
    if cfg.get("close_on_leave") is True:
        env["CORDIAL_CLOSE_ON_LEAVE"] = "1"
    mode = cfg.get("graphics_optimization_mode")
    if mode in ("roblox-app", "mobile-tier"):
        env["CORDIAL_DEVICE_PROFILE"] = {"roblox-app": "roblox-app",
                                         "mobile-tier": "android-tablet"}[mode]
    if mode in ("more-cores", "fewer-cores"):
        env["CORDIAL_PERFORMANCE"] = {"more-cores": "throughput",
                                      "fewer-cores": "latency"}[mode]
    sink = str(cfg.get("audio_output") or "").strip()
    if sink:
        env["CORDIAL_AUDIO_SINK"] = sink
    return env


# A low-power client: for an account that is along for the ride. Cordial's own
# knobs -- throttle when unfocused (its window's "slow the game down in the
# background"), FIFO pacing (its help: "fifo saves the power"), no GameMode
# boost -- plus a lower CPU priority, so the client you play wins every tie.
LOW_POWER_ENV = {"CORDIAL_THROTTLE": "unfocused", "CORDIAL_PRESENT_MODE": "fifo",
                 "CORDIAL_GAMEMODE": "0"}
# And two FastFlags in the profile's own flags.json, the file Cordial reads for
# per-profile overrides (docs/fastflags.md): a frame-rate target and a cap on
# the engine's worker threads, which otherwise size themselves to every core
# in every client at once. Both names were checked against libroblox.so.
LOW_POWER_FLAGS = {"DFIntTaskSchedulerTargetFps": 20,
                   "FIntTaskSchedulerAutoThreadLimit": 2}


def low_power_flags(flags, on):
    """flags with the low-power values added, or taken back out. A value you
    set to something else yourself is never overwritten, nor removed."""
    flags = dict(flags)
    for k, v in LOW_POWER_FLAGS.items():
        if on:
            flags.setdefault(k, v)
        elif flags.get(k) == v:
            del flags[k]
    return flags


def apply_low_power(profile, on, run=run_host):
    """Bring the profile's flags.json in line with its low-power switch,
    leaving the file untouched when nothing changes."""
    path = os.path.join(CORDIAL_PROFILES, profile, "flags.json")
    text = _read_host(path, run)
    try:
        flags = json.loads(text) if text else {}
    except json.JSONDecodeError:
        return      # not ours to repair; Cordial reports it itself
    if not isinstance(flags, dict):
        return
    new = low_power_flags(flags, on)
    if new != flags:
        r = run(["sh", "-c", 'cat > "$0.tmp" && mv "$0.tmp" "$0"', path],
                input=json.dumps(new, indent=2).encode() + b"\n")
        if r.returncode != 0:
            raise RuntimeError(f"could not write {path}: {_last_line(r.stderr)}")


def client_argv(profile, url, build):
    """One account's engine, with the arguments upstream's window starts it
    with (spawn() in crates/cordial-shell/src/launch.rs)."""
    lib, apk = build
    argv = ["cordial-run", "--lib-dir", lib, "--apk", apk,
            "--host-libc", "--game-activity", "--run", "0", "--profile", profile]
    if url:
        argv += ["--join-url", url]
    return argv


def launch_client(profile, url, build, run=run_host, sleep=time.sleep, start=None,
                  nested=False, low_power=False):
    """Start one account's engine and check it survives its first seconds.

    Its output goes to ~/.cache/rbxmgr/logs/<profile>.log -- the only account
    of why a client ended -- with the previous launch kept as .log.1.
    """
    apply_low_power(profile, low_power, run)
    env = cordial_engine_env(cordial_settings(run))
    if low_power:
        env.update(LOW_POWER_ENV)
    os.makedirs(LOGS, exist_ok=True)
    log_path = os.path.join(LOGS, f"{profile}.log")
    if os.path.exists(log_path):
        os.replace(log_path, log_path + ".1")
    with open(log_path, "wb") as out:
        argv = client_argv(profile, url, build)
        if low_power:
            argv = ["nice", "-n", "10", *argv]
        p = (start or _detach)(nested_argv(profile, argv) if nested else argv, out, env)
    sleep(STARTUP_CHECK)
    if p.poll() is not None:
        try:
            with open(log_path, "rb") as f:
                tail = _last_line(f.read()[-4096:])
        except OSError:
            tail = ""
        raise RuntimeError(f"its client exited at once ({tail or p.returncode}); "
                           f"log: {log_path}")
    return p


def parse_clients(pgrep_output):
    """{pid: profile} from `pgrep -a`: one line per engine process,
    `<pid> /nix/store/.../bin/cordial-run --lib-dir ... --profile <name> ...`.

    Only processes whose argv[0] is the cordial-run binary count. `pgrep -f`
    matches anywhere in a command line, so a `nice cordial-run ...` on its way
    to exec, or a shell that merely mentions one, would otherwise pass for a
    client -- and be stopped by Stop.
    # ponytail: pgrep joins argv with spaces, so a profile name containing
    # " --" would be cut short there; read /proc/<pid>/cmdline if one does.
    """
    clients = {}
    for line in pgrep_output.splitlines():
        pid, _, cmd = line.strip().partition(" ")
        if (not pid.isdigit()
                or os.path.basename(cmd.split(" ", 1)[0]) != "cordial-run"
                or " --profile " not in cmd):
            continue
        profile = cmd.split(" --profile ", 1)[1].split(" --", 1)[0]
        if profile:
            clients[int(pid)] = profile
    return clients


def cordial_clients(run=run_host):
    """Every running Cordial game client, as {pid: profile}. pgrep exits 1
    when nothing matches, which is an answer, not an error."""
    r = run(["pgrep", "-a", "-f", "cordial-run"], timeout=10)
    return parse_clients(r.stdout.decode(errors="replace"))


def stop_profiles(profiles, clients=None, run=run_host):
    """SIGTERM every client running one of `profiles`; cordial-run ends its
    session cleanly on it. Returns how many were signalled. Clients of
    profiles no account owns -- somebody playing from Cordial directly --
    are left alone."""
    clients = cordial_clients(run) if clients is None else clients
    pids = [str(p) for p, prof in clients.items() if prof in profiles]
    if pids:
        run(["kill", *pids], timeout=10)
    return len(pids)


def cordial_profile(user_id):
    """The Cordial profile an account plays in. Keyed by the Roblox user id,
    so renaming the label never strands it, and within Cordial's name rule
    (letters, digits, - and _)."""
    return f"rbxmgr-{int(user_id)}"


def cordial_secret_attrs(profile, kind):
    """The keyring attributes Cordial files a profile's `kind` under, keyed
    by the profile's full path."""
    return {"xdg:schema": "org.cordial.Session", "application": "cordial",
            "profile": os.path.join(CORDIAL_PROFILES, profile), "store": kind}


def _escape_jar(jar):
    return (jar.replace("\\", "\\\\").replace("\t", "\\t")
            .replace("\n", "\\n").replace("\r", "\\r"))


def cordial_cookie_store(cookie):
    """Cordial's cookie-store body holding one session. Both hosts are the
    ones Cordial seeds itself; the routing check requires every copy of
    .ROBLOSECURITY in the store to agree, which these do."""
    jar = _escape_jar(f".ROBLOSECURITY={cookie}")
    return ("# cordial cookie store v1 -- a live Roblox session. "
            "Treat it as a password.\n"
            + "".join(f"{host}\t{jar}\n" for host in (".roblox.com", "roblox.com")))


def cordial_identity(user):
    """Cordial's saved identity (schema 1). Only userId and username are
    required; the client rewrites the rest from Roblox once it is up."""
    return json.dumps({
        "schema": 1,
        "userId": int(user["id"]),
        "username": user["name"],
        "displayName": user.get("displayName") or user["name"],
        "membershipType": 0,
        "isUnder13": False,
        "hasRobloxSubscription": False,
        "countryCode": "",
    }) + "\n"


def cordial_encode(body):
    """How Cordial stores a body in the keyring: hex behind a version prefix,
    because some services mangle tabs and newlines in a text secret."""
    return "cordial-secret-hex-v1:" + body.encode().hex()


def seed_cordial_profile(user, cookie, run=run_host, store=None):
    """Make the account's Cordial profile routable. Returns its name.

    Rewritten on every launch rather than once: the manager's cookie has just
    been checked with Roblox (authenticated_user), so it is known good. The
    keyring is the only place the session goes -- never a file in the
    profile, whatever Cordial's own fallback would do.
    """
    store = store or keyring_store
    profile = cordial_profile(user["id"])
    r = run(["mkdir", "-p", os.path.join(CORDIAL_PROFILES, profile)])
    if r.returncode != 0:
        raise RuntimeError(f"could not create Cordial profile {profile}: "
                           f"{_last_line(r.stderr)}")
    for kind, body in (("identity", cordial_identity(user)),
                       ("cookies", cordial_cookie_store(cookie))):
        store(cordial_secret_attrs(profile, kind),
              f'Cordial: Roblox {kind} for profile "{profile}"',
              cordial_encode(body))
    return profile


def clear_cordial_profile(user_id, clear=None):
    """Drop the session and identity the manager gave an account's profile.
    The profile directory stays; it holds no credential."""
    clear = clear or keyring_clear
    for kind in ("identity", "cookies"):
        clear(cordial_secret_attrs(cordial_profile(user_id), kind))


# --------------------------------------------------------------------------
# Launch orchestration. Callables are injected so the self-check can drive both
# modes with fakes: no network, no Cordial, no waiting.
# --------------------------------------------------------------------------
def launch_each(names, place_id, do_spawn, log,
                stagger=DEFAULT_STAGGER, sleep=time.sleep,
                is_running=None, job_id=None):
    """Every account into its own server -- or all into job_id's, when one is
    given. Returns the names that launched."""
    is_running = is_running or (lambda _n: False)
    launched, minted = [], False
    for name in names:
        try:
            if is_running(name):
                log(f"{name}: already running -- skipping launch")
                continue
            # The stagger spaces sign-ins, so an account that was skipped
            # costs no wait -- and neither does the first one.
            if minted:
                sleep(stagger)
            minted = True
            do_spawn(name, join_url(place_id, job_id))
            launched.append(name)
            log(f"{name}: launched")
        except Exception as e:
            log(f"{name}: FAILED -- {e}")
    return launched


def follow_leader(names, place_id, get_job_id, do_spawn, log,
                  stagger=DEFAULT_STAGGER, sleep=time.sleep,
                  timeout=LEADER_TIMEOUT, poll=LEADER_POLL,
                  is_running=None):
    """Launch names[0], wait for its server, then send the rest into it.

    Returns the jobId everyone joined, or None if the leader never reported one
    (in which case the followers still launch, into their own servers, rather
    than silently not launching at all).
    """
    if not names:
        return None
    leader, followers = names[0], names[1:]
    is_running = is_running or (lambda _n: False)

    already_running = False
    try:
        if is_running(leader):
            already_running = True
            log(f"{leader}: leader is already running, looking for its server")
        else:
            do_spawn(leader, join_url(place_id))
    except Exception as e:
        log(f"{leader}: leader FAILED -- {e}; nobody has a server to join")
        return None

    # Nobody to place means nothing to look up: polling presence here would burn
    # up to `timeout` seconds and a stack of API calls to answer a question with
    # no consumer.
    if not followers:
        if not already_running:
            log(f"{leader}: launched")
        return None
    if not already_running:
        log(f"{leader}: launched, waiting for its server")

    job_id = None
    leader_place = None
    waited = 0
    while waited < timeout:
        if not already_running or waited > 0:
            sleep(poll)
            waited += poll
        else:
            waited += poll
        try:
            res = get_job_id(leader)
            if isinstance(res, tuple) and len(res) == 2:
                job_id, leader_place = res
            elif isinstance(res, dict):
                job_id = res.get("gameId") or res.get("job_id")
                leader_place = res.get("placeId") or res.get("place_id")
            else:
                job_id, leader_place = res, None
        except Exception as e:  # a failed poll is not a failed launch
            log(f"{leader}: presence poll failed ({e})")
            job_id = None
        if job_id:
            log(f"{leader}: in server {job_id}")
            break
        if not job_id and waited < timeout:
            log(f"{leader}: waiting for server ({waited}s/{timeout}s)...")

    if not job_id:
        log(f"{leader}: no server after {timeout}s -- followers get their own")

    target_place = leader_place or place_id
    for name in followers:
        try:
            if is_running(name):
                log(f"{name}: already running -- skipping launch")
                continue
            sleep(stagger)
            do_spawn(name, join_url(target_place, job_id))
            log(f"{name}: launched{' into ' + job_id if job_id else ''}")
        except Exception as e:
            log(f"{name}: FAILED -- {e}")
    return job_id


# --------------------------------------------------------------------------
# Macros: emulated input, never your hardware and never the client.
#
# A "macro-ready" account's client runs inside its own nested compositor
# (cage) -- a normal window on your desktop, but with a display of its own, so
# the game keeps keyboard focus there whatever you are doing elsewhere. A macro
# types into that display through the Wayland virtual-keyboard and
# virtual-pointer protocols (VirtualInput): input the compositor emulates,
# delivered like any keyboard's. Your real devices are never read or grabbed,
# and nothing touches the client process.
# --------------------------------------------------------------------------
MACROS = os.path.join(STATE, "macros.json")
MACRO_HELP = """\
<b>What it does</b>
A macro presses keys and clicks for one account, on its own, for as long as you \
let it -- while you use other windows, or step away.

<b>Why a "macro-ready window"</b>
Roblox ignores the keyboard whenever its window is not the focused one. A \
macro-ready account's client runs on a small display of its own, inside a \
normal window on your desktop, where it always has focus. The macro types into \
that display with emulated input -- a virtual keyboard and mouse. Your real \
keyboard and mouse are never used or read, and nothing touches the game itself.

<b>Using it</b>
1. Make a macro: press New, add its steps, then Save.
2. Open the account's settings (the gear) and pick the macro. That turns on \
<i>Macro-ready window</i>.
3. Launch the account (relaunch it if it was already running).
4. Press Run here in its settings. Press Stop here to end it.

Or press Run on a macro card to play it on every selected account at once; \
that turns on their macro-ready windows too. A macro switched off cannot run.

<b>Hotkeys</b>
A macro's hotkey runs it on the selected accounts, or stops it, while this \
window has focus. To use it from anywhere, bind a key in your compositor to
<tt>gapplication action io.github.mujo.RobloxManager run-macro "'Macro 1'"</tt>

<b>Steps</b>
<tt>Key  KEY [SECONDS]</tt>   press and release a key
<tt>Hold KEY SECONDS</tt>     keep a key down
<tt>Type TEXT</tt>            type text, e.g. into chat
<tt>Click [left|right|middle] [X Y]</tt>   click, optionally at a point
<tt>Move DX DY</tt>           move the mouse by an amount
<tt>Wait SECONDS</tt>         pause
<tt>Start SECONDS</tt>        pause once, before the first round only
<tt>Note TEXT</tt>            a reminder; does nothing
Repeat runs the steps once, a set number of rounds, or until stopped.

Keys are letters, digits and punctuation, or names such as space, enter, esc, \
tab, shift, ctrl, alt, up, down, left, right, F1. Combine them with +: \
<tt>Hold shift+w 2</tt>. Keys and typed text follow a US keyboard layout.

<b>Randomness</b>
Any SECONDS can be a range, <tt>Wait 60-240</tt>, picked afresh every time. Every \
key press is held for a random moment too -- 0.04-0.12 s for a tap unless you \
give one -- and typed characters are 0.05-0.16 s apart.

<b>Example</b>
<tt>Start  45
Wait   60-70
Key    j
Wait   340-341</tt>  Repeat: until stopped

<b>Good to know</b>
Keep the window on a workspace you can see; on a hidden one the game can stall. \
Stop lets go of any held key at once. Roblox games have their own rules on \
macros -- AFK use can be against them.
"""
# Humans never press a key for the same few milliseconds twice.
TAP_PRESS = (0.04, 0.12)
TYPE_GAP = (0.05, 0.16)

# Linux evdev key codes, laid out as a US keyboard. These are what reach the
# game: Roblox's in-game key path (nativePassKeyEvent) takes the raw evdev code,
# not the keysym -- see VirtualInput.
_ROWS = {2: "1234567890-=", 16: "qwertyuiop[]", 30: "asdfghjkl;'`",
         43: "\\zxcvbnm,./"}
CHAR_CODES = {c: first + i for first, row in _ROWS.items() for i, c in enumerate(row)}
CHAR_CODES[" "] = 57
_SHIFTED = dict(zip("!@#$%^&*()_+{}:\"~|<>?", "1234567890-=[];'`\\,./"))
SHIFT = 42
KEY_CODES = {
    **CHAR_CODES,
    "esc": 1, "escape": 1, "backspace": 14, "tab": 15, "enter": 28, "return": 28,
    "ctrl": 29, "control_l": 29, "shift": SHIFT, "shift_l": SHIFT, "shift_r": 54,
    "alt": 56, "alt_l": 56, "space": 57, "capslock": 58, "caps_lock": 58,
    **{f"f{n}": 58 + n for n in range(1, 11)}, "f11": 87, "f12": 88,
    "control_r": 97, "alt_r": 100, "home": 102, "up": 103, "pageup": 104,
    "prior": 104, "left": 105, "right": 106, "end": 107, "down": 108,
    "pagedown": 109, "next": 109, "insert": 110, "delete": 111,
    "minus": 12, "equal": 13, "comma": 51, "period": 52, "slash": 53,
}
BUTTONS = {"left": 0x110, "right": 0x111, "middle": 0x112}
# Modifier keys -> their bit in the keymap's state: Shift, Control, Mod1 (Alt).
MOD_MASKS = {42: 1, 54: 1, 29: 4, 97: 4, 56: 8, 100: 8}


def _read_macros():
    try:
        with open(MACROS) as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError):
        return {}
    return data if isinstance(data, dict) else {}


def load_macros():
    """{name: text}. A switched-off macro is stored as {"text", "enabled"};
    any other dict is the earlier macro manager's entry."""
    macros = {}
    for name, m in _read_macros().items():
        if isinstance(m, dict):
            m = m.get("text") if "text" in m else migrate_macro(m)
        if isinstance(m, str):
            macros[str(name)] = m
    return macros


def migrate_macro(old):
    """The earlier macro manager's entry -- {"script", "start_delay", ...},
    with `key K`, `wait range(A,B)` and a closing bare `loop` -- as this one's
    text. Its other fields (hidden, place) have no counterpart and are dropped."""
    lines = []
    delay = old.get("start_delay")
    if isinstance(delay, (int, float)) and delay > 0:
        lines.append(f"start {delay}")
    for line in str(old.get("script") or "").splitlines():
        line = line.strip()
        if line in ("", "loop"):
            continue  # repeating until stopped is the default here
        line = re.sub(r"^key\s+", "tap ", line)
        line = re.sub(r"range\(\s*([\d.]+)\s*,\s*([\d.]+)\s*\)", r"\1-\2", line)
        lines.append(line)
    return "\n".join(lines).strip() + "\n"


def disabled_macros():
    return {str(n) for n, m in _read_macros().items()
            if isinstance(m, dict) and "text" in m and m.get("enabled") is False}


def macro_hotkeys():
    """{name: GTK accelerator} for the macros that have a hotkey."""
    return {str(n): m["hotkey"] for n, m in _read_macros().items()
            if isinstance(m, dict) and "text" in m and m.get("hotkey")}


def save_macros(macros, disabled=(), hotkeys=None):
    """A plain macro stays a bare string; one switched off or with a hotkey
    becomes {"text", "enabled", "hotkey"}."""
    hotkeys = hotkeys or {}

    def entry(name, text):
        if name not in disabled and not hotkeys.get(name):
            return text
        e = {"text": text, "enabled": name not in disabled}
        if hotkeys.get(name):
            e["hotkey"] = hotkeys[name]
        return e

    _save_json(MACROS, {n: entry(n, t) for n, t in macros.items()})


# The editor's step types, and the command each one is in a macro's text.
STEP_TYPES = {"Key": "tap", "Hold": "hold", "Type": "type", "Click": "click",
              "Move": "move", "Wait": "wait", "Start": "start", "Note": "#"}


def macro_rows(text):
    """([(type, value)], loops) -- a macro as the editor's rows; loops 0 is
    until stopped. A # line is a Note. A command the editor has no type for
    keeps its own name, so saving writes it back unchanged."""
    cmds = {v: k for k, v in STEP_TYPES.items()}
    rows, loops = [], 0
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        if line.startswith("#"):
            rows.append(("Note", line[1:].strip()))
            continue
        cmd, _, rest = line.partition(" ")
        if cmd.lower() == "loop" and rest.strip().isdigit():
            loops = int(rest)
            continue
        rows.append((cmds.get(cmd.lower(), cmd.capitalize()), rest.strip()))
    return rows, loops


def macro_text(rows, loops):
    """macro_rows backwards."""
    lines = [f"{STEP_TYPES.get(t, t.lower())} {v}".strip() for t, v in rows]
    if loops:
        lines.append(f"loop {loops}")
    return "\n".join(lines) + "\n"


def loop_label(loops):
    return "Until stopped" if not loops else "Once" if loops == 1 else f"{loops} rounds"


def _keys(token):
    codes = [KEY_CODES.get(k.lower()) for k in token.split("+")]
    if None in codes:
        raise ValueError(f"unknown key {token!r}")
    return codes


def _typed(text):
    """[(code, shifted)] for each character, as a US keyboard types it."""
    out = []
    for c in text:
        if c in CHAR_CODES:
            out.append((CHAR_CODES[c], False))
        elif c.isupper() and c.lower() in CHAR_CODES:
            out.append((CHAR_CODES[c.lower()], True))
        elif c in _SHIFTED:
            out.append((CHAR_CODES[_SHIFTED[c]], True))
        else:
            raise ValueError(f"cannot type {c!r} -- US keyboard characters only")
    return out


def _seconds(token):
    lo, _, hi = token.partition("-")
    try:
        lo, hi = float(lo), float(hi or lo)
    except ValueError:
        raise ValueError(f"not a duration: {token!r}") from None
    if not 0 <= lo <= hi:
        raise ValueError(f"not a duration: {token!r}")
    return lo, hi


def parse_macro(text):
    """(loops, steps) from a macro's text; loops 0 means until stopped.
    Raises ValueError naming the line at fault."""
    loops, steps = 0, []
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        cmd, _, rest = line.partition(" ")
        args = rest.split()
        try:
            cmd = cmd.lower()
            if cmd == "type" and rest.strip():
                steps.append(("type", _typed(rest.strip())))
            elif cmd == "tap" and len(args) in (1, 2):
                # A tap is a short hold: its press length is random as well.
                steps.append(("hold", _keys(args[0]),
                              *(_seconds(args[1]) if len(args) == 2 else TAP_PRESS)))
            elif cmd == "hold" and len(args) == 2:
                steps.append(("hold", _keys(args[0]), *_seconds(args[1])))
            elif cmd == "wait" and len(args) == 1:
                steps.append(("wait", *_seconds(args[0])))
            elif cmd == "click" and len(args) <= 3:
                button = args.pop(0) if args and args[0].isalpha() else "left"
                if button not in ("left", "right", "middle") or len(args) == 1:
                    raise ValueError("expected click [left|right|middle] [X Y]")
                steps.append(("click", BUTTONS[button],
                              tuple(int(a) for a in args) if args else None))
            elif cmd == "move" and len(args) == 2:
                steps.append(("move", int(args[0]), int(args[1])))
            elif cmd == "start" and len(args) == 1:
                steps.append(("start", *_seconds(args[0])))
            elif cmd == "loop" and len(args) == 1 and args[0].isdigit():
                loops = int(args[0])
            else:
                raise ValueError(f"don't understand {line!r}")
        except ValueError as e:
            raise ValueError(f"line {n}: {e}") from None
    if not steps:
        raise ValueError("the macro has no steps")
    return loops, steps


# Keymap sent with the virtual keyboard. The compositor compiles it, so the
# includes resolve against its own xkeyboard-config.
KEYMAP = """xkb_keymap {
  xkb_keycodes { include "evdev+aliases(qwerty)" };
  xkb_types { include "complete" };
  xkb_compat { include "complete" };
  xkb_symbols { include "pc+us+inet(evdev)" };
};
"""


def _u(*vals):
    return struct.pack(f"={len(vals)}I", *vals)


def _wl_str(s):
    b = s.encode() + b"\0"
    return _u(len(b)) + b + b"\0" * (-len(b) % 4)


def _wl_unstr(body, off):
    n, = struct.unpack_from("=I", body, off)
    return body[off + 4:off + 3 + n].decode(errors="replace")


class VirtualInput:
    """A virtual keyboard and pointer on one Wayland display: a nested cage.

    Not wtype and wlrctl, which this used to run. Roblox's in-game key path
    takes the raw evdev keycode, and wtype makes up a keymap per run that
    numbers keys in the order it meets them -- so the first key of every run
    went out as code 1, Escape, and `tap j` opened Roblox's menu instead of
    pressing J. This uploads a real US keymap once and presses the real code.

    It speaks the Wayland wire protocol itself: a handful of fixed messages,
    which is less than a binding library would be to ship in three builds.
    """

    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.buf = b""
        self.mods = set()
        self.last_id = 1                       # 1 is wl_display
        try:
            self.sock.connect(path)
            registry = self._new()
            self._send(1, 1, _u(registry))     # wl_display.get_registry
            offered = {_wl_unstr(body, 4): struct.unpack_from("=I", body)[0]
                       for obj, op, body in self._roundtrip()
                       if obj == registry and op == 0}

            def bind(iface):
                if iface not in offered:
                    raise RuntimeError(f"its display offers no {iface}")
                new = self._new()
                self._send(registry, 0,
                           _u(offered[iface]) + _wl_str(iface) + _u(1, new))
                return new

            seat = bind("wl_seat")
            kbd_mgr = bind("zwp_virtual_keyboard_manager_v1")
            ptr_mgr = bind("zwlr_virtual_pointer_manager_v1")
            self.kbd, self.ptr = self._new(), self._new()
            self._send(kbd_mgr, 0, _u(seat, self.kbd))
            self._send(ptr_mgr, 0, _u(seat, self.ptr))
            keymap = KEYMAP.encode() + b"\0"
            fd = os.memfd_create("rbxmgr-keymap")
            try:
                os.write(fd, keymap)
                self._send(self.kbd, 0, _u(1, len(keymap)), fds=[fd])  # xkb v1
            finally:
                os.close(fd)
            self._roundtrip()                  # a refusal shows up here, not mid-macro
        except BaseException:
            self.sock.close()
            raise

    def _new(self):
        self.last_id += 1
        return self.last_id

    def _send(self, obj, op, body=b"", fds=()):
        msg = _u(obj, (8 + len(body)) << 16 | op) + body
        if fds:
            socket.send_fds(self.sock, [msg], fds)
        else:
            self.sock.sendall(msg)

    def _pump(self, block):
        """The events that have arrived. A protocol error is raised."""
        try:
            data = self.sock.recv(65536, 0 if block else socket.MSG_DONTWAIT)
        except BlockingIOError:
            return []
        if not data:
            raise RuntimeError("its display closed")
        self.buf += data
        events = []
        while len(self.buf) >= 8:
            obj, size_op = struct.unpack_from("=II", self.buf)
            size, op = size_op >> 16, size_op & 0xFFFF
            if size < 8:
                raise RuntimeError("its display sent a malformed message")
            if len(self.buf) < size:
                break
            body, self.buf = self.buf[8:size], self.buf[size:]
            if obj == 1 and op == 0:           # wl_display.error
                raise RuntimeError(f"its display refused input: {_wl_unstr(body, 8)}")
            events.append((obj, op, body))
        return events

    def _roundtrip(self):
        """Every event before the display has handled all that was sent."""
        done = self._new()
        self._send(1, 0, _u(done))             # wl_display.sync
        events = []
        while True:
            for ev in self._pump(True):
                if ev[0] == done:
                    return events
                events.append(ev)

    def _stamp(self):
        self._pump(False)                      # surface an error; drop the rest
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def key(self, code, down):
        self._send(self.kbd, 1, _u(self._stamp(), code, int(down)))
        if code in MOD_MASKS:
            # wlroots does not work modifier state out from a virtual
            # keyboard's keys; without this, Shift+h typed "h".
            (self.mods.add if down else self.mods.discard)(code)
            mask = 0
            for c in self.mods:
                mask |= MOD_MASKS[c]
            self._send(self.kbd, 2, _u(mask, 0, 0, 0))

    def motion(self, dx, dy):
        self._send(self.ptr, 0, _u(self._stamp()) + struct.pack("=ii", dx * 256, dy * 256))
        self._send(self.ptr, 4)                # frame

    def button(self, code, down):
        self._send(self.ptr, 2, _u(self._stamp(), code, int(down)))
        self._send(self.ptr, 4)

    def close(self):
        try:
            self._send(self.kbd, 3)            # destroy: the keyboard, then the pointer
            self._send(self.ptr, 8)
            self._roundtrip()
        except (OSError, RuntimeError):
            pass
        finally:
            self.sock.close()


def play_step(inp, step, stop, rng=random):
    """One key, text, click or move step. Every key or button pressed is let
    go again, even when stop cuts a hold short or the display fails."""
    kind = step[0]

    def press(codes, lo, hi, send=inp.key):
        down = []
        try:
            for c in codes:
                send(c, True)
                down.append(c)
            stop.wait(rng.uniform(lo, hi))
        finally:
            for c in reversed(down):
                send(c, False)

    if kind == "hold":
        press(step[1], step[2], step[3])
    elif kind == "type":
        for code, shifted in step[1]:
            press([SHIFT, code] if shifted else [code], *TAP_PRESS)
            if stop.wait(rng.uniform(*TYPE_GAP)):
                return
    elif kind == "click":
        if step[2]:
            # Relative motion only; the far corner is the origin, since the
            # compositor clamps the pointer to its output.
            inp.motion(-100000, -100000)
            inp.motion(*step[2])
        press([step[1]], *TAP_PRESS, send=inp.button)
    elif kind == "move":
        inp.motion(step[1], step[2])
    else:
        raise ValueError(kind)


def display_file(profile):
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return os.path.join(runtime, "rbxmgr", f"{profile}.wayland")


def nested_argv(profile, argv):
    """argv in a cage of its own. The shell hard-links cage's socket to
    display_file while the client runs, where macros connect to it, and
    removes it when the client ends -- cage then exits with it. A link, not
    the display's name, so input can only ever reach this cage: once cage is
    gone the link refuses connections, whoever gets its name next. The
    profile goes in as $0, never into the script."""
    return ["cage", "--", "sh", "-c",
            'f="$XDG_RUNTIME_DIR/rbxmgr/$0.wayland"; mkdir -p "${f%/*}"; '
            'ln -f "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" "$f"; "$@"; rm -f "$f"',
            profile, *argv]


def describe_step(step):
    """What a key, text, click or move step does, for the status line."""
    kind = step[0]
    if kind == "hold":
        names = {**{v: k for k, v in reversed(KEY_CODES.items())}, 57: "space"}
        return "pressing " + "+".join(names.get(c, str(c)) for c in step[1])
    return {"type": "typing", "click": "clicking", "move": "moving the mouse"}[kind]


def run_macro(profile, loops, steps, stop, run=run_host, rng=random,
              clients=None, connect=None, report=None, now=time.time):
    """Play steps into profile's nested display until stop is set or loops
    run out. Before every pass it checks the client is still up.

    report(text) hears each step as it starts -- a wait with the clock time
    it ends at -- since a macro that opens on minutes of waiting otherwise
    looks exactly like one that does nothing."""
    report = report or (lambda _t: None)
    clients = clients or cordial_clients
    connect = connect or VirtualInput

    def check_running():
        if profile not in clients(run).values():
            raise RuntimeError("its client is not running -- launch it first")

    check_running()
    try:
        inp = connect(display_file(profile))
    except (FileNotFoundError, ConnectionRefusedError):
        raise RuntimeError("its client is in a normal window -- launch it again "
                           "with Macro-ready window on") from None
    done = 0
    try:
        while not stop.is_set() and (loops == 0 or done < loops):
            if done:
                check_running()
            for step in steps:
                if stop.is_set():
                    return
                if step[0] in ("wait", "start"):
                    if step[0] == "wait" or done == 0:
                        secs = rng.uniform(step[1], step[2])
                        until = time.strftime("%H:%M:%S", time.localtime(now() + secs))
                        report(f"round {done + 1}: waiting {secs:.0f}s, until {until}")
                        stop.wait(secs)
                    continue
                report(f"round {done + 1}: {describe_step(step)}")
                play_step(inp, step, stop, rng)
            done += 1
    except OSError as e:
        raise RuntimeError(f"its window went away ({e.strerror or e})") from None
    finally:
        inp.close()




# --------------------------------------------------------------------------
# GUI. The look is the Roblox Manager v2 design handoff: warm dark surfaces,
# one amber accent, Source Sans 3 and JetBrains Mono, and Material Symbols
# Rounded for icons -- a ligature font, so an icon is a Label whose text is
# the icon's name (the package ships all three). It is drawn with the app's
# own stylesheet over libadwaita's dark scheme.
# --------------------------------------------------------------------------
_STRIPES = ("repeating-linear-gradient(135deg, #2f281d 0px, #2f281d 6px, "
            "#29231a 6px, #29231a 12px)")
CSS = """
window.rbx { background: #211c15; color: #efe6d6; font-size: 14px;
  font-family: "Source Sans 3", "Source Sans Pro", system-ui, sans-serif; }
.mono { font-family: "JetBrains Mono", monospace; }
.ms { font-family: "Material Symbols Rounded"; font-weight: 400;
  font-variation-settings: "FILL" 0; }
.ms.fill { font-variation-settings: "FILL" 1; }
.ms.bold { font-weight: 700; }
button.b { background: none; box-shadow: none; border: none; padding: 0;
  min-height: 0; min-width: 0; color: inherit; font-weight: normal;
  border-radius: 8px; transition: none; }
button.b:disabled { opacity: .4; }
tooltip { background: #2e271c; border: 1px solid #4a3f2e; border-radius: 10px;
  color: #efe6d6; box-shadow: 0 12px 30px rgba(0,0,0,.5); }
tooltip label { font-size: 13px; }
.amber { color: #e9bd6a; }
.bullet { min-width: 3px; min-height: 3px; border-radius: 999px; background: #5f5646; }
.vdiv { min-width: 1px; min-height: 20px; background: #332b1f; margin: 0 2px; }
@keyframes rmspin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
@keyframes rmpulse { from { transform: scale(1); opacity: .6; }
                     to { transform: scale(2.8); opacity: 0; } }

/* dots: 7 px, a pulsing ring where something is live */
.dot { min-width: 7px; min-height: 7px; border-radius: 999px; background: #5f5646; }
.dot.d8 { min-width: 8px; min-height: 8px; }
.dot.ring { animation: rmpulse 1.6s ease-out infinite; }
.dot.running { background: #8fd18a; }
.dot.joining, .dot.starting, .dot.wait { background: #e9bd6a; }
.dot.expired, .dot.error { background: #ff9d8c; }
.dot.idlepill { background: #6b6150; }

/* title bar */
.titlebar { min-height: 52px; padding: 0 10px 0 16px; background: #1d1812;
  border-bottom: 1px solid #332b1f; }
.apptitle { font-size: 15px; font-weight: 700; }
.pill { min-height: 24px; padding: 0 10px; border-radius: 999px; background: #2b2419;
  color: #9c8f78; font-size: 12px; font-weight: 600; }
.pill.on { background: #1f2a1c; color: #8fd18a; }
button.tbtn { min-height: 32px; padding: 0 12px 0 10px; border: 1px solid #3a3124;
  background: #27211a; color: #efe6d6; font-size: 13px; font-weight: 600; }
button.tbtn:hover { background: #2e271c; border-color: #4a3f2e; }
button.tbtn .ms { color: #e9bd6a; }
button.tbtn.upd { min-width: 156px; }
button.tbtn.upd.busy { border-color: #5a4724; }
button.tbtn.upd.busy .ms { animation: rmspin 1s linear infinite; }
button.tbtn.upd.done { background: #1f2a1c; border-color: #3f5a39; color: #8fd18a; }
button.tbtn.upd.done .ms { color: #8fd18a; }
button.ibtn { min-width: 32px; min-height: 32px; color: #9c8f78; }
button.ibtn:hover { background: #2b2419; color: #efe6d6; }
button.ibtn.close:hover { background: #4a2420; color: #ffb3a6; }
window.busy button.reload .ms { animation: rmspin 1s linear infinite; }

/* columns and section heads */
.left { padding: 24px 28px 28px; border-right: 1px solid #332b1f; }
.right { padding: 24px; background: #1f1a13; }
.stile { min-width: 34px; min-height: 34px; border-radius: 10px; background: #2b2419;
  border: 1px solid #3a3124; color: #e9bd6a; }
.stitle { font-size: 16px; font-weight: 700; }
.ssub { font-size: 13px; color: #9c8f78; }
button.textbtn { min-height: 32px; padding: 0 10px 0 8px; color: #b9ab93;
  font-size: 13px; font-weight: 600; }
button.textbtn:hover { color: #efe6d6; background: #2b2419; }
button.tbtn.plain { padding: 0 12px 0 8px; }

/* game tiles */
button.gtile { color: #efe6d6; border-radius: 14px; }
.gthumb { min-width: 128px; min-height: 128px; border-radius: 14px;
  border: 1px solid #332b1f; background: #26201a; transition: transform 150ms ease; }
button.gtile:hover .gthumb { transform: translateY(-2px); }
.gthumb.dashed { border: 1.5px dashed #4a3f2e; background: #1b1611; color: #b9ab93; }
.gthumb.friend { border: 1px solid #3f5a39; background: #1c2118; color: #8fd18a; }
.gthumb.sel { box-shadow: 0 0 0 2px #211c15, 0 0 0 4px #e9bd6a; }
.stripes { background: """ + _STRIPES + """; }
.tilecheck { min-width: 24px; min-height: 24px; border-radius: 999px; background: #e9bd6a;
  color: #2a1f0c; box-shadow: 0 2px 8px rgba(0,0,0,.45); }
.gname { font-size: 13px; font-weight: 600; color: #b9ab93; }
.gname.sel { color: #efe6d6; }
.gmeta { font-size: 12px; color: #7d715d; }
.thumb { border-radius: 10px; }
.thumb.t28 { border-radius: 6px; }
.thumb.none { border: 1.5px dashed #4a3f2e; background: #1b1611; color: #7d715d; }
.thumb.game { border: 1px solid #3a3124; }

/* cards: the leader, each group, the ungrouped */
.card { border: 1px solid #332b1f; border-radius: 14px; background: #1f1a13; }
.card.lead { border-color: #5a4724; }
.card:drop(active) { border-color: #e9bd6a; box-shadow: none; }
.lstrip { padding: 9px 14px; background: #2a2217; border-bottom: 1px solid #3d3120; }
.ltitle { font-size: 12px; font-weight: 700; letter-spacing: .08em; color: #f0c779; }
.ldesc { font-size: 12px; color: #9c8f78; }
.ltarget { font-size: 12px; color: #e9c887; }
.noleader { padding: 18px 16px; font-size: 13px; color: #7d715d; }
.ffoot { border-top: 1px solid #2b2419; padding: 12px 14px 14px; background: #1b1611; }
.fhead { font-size: 12px; font-weight: 600; color: #9c8f78; margin-bottom: 2px; }
.fcount { font-weight: 400; color: #7d715d; }
.frow { padding: 6px 6px 6px 8px; border-radius: 9px; background: #221d16;
  border: 1px solid #2b2419; }
.fnum { min-width: 22px; min-height: 22px; border-radius: 6px; background: #2b2419;
  color: #e9c887; font-size: 11px; }
.fname { font-size: 14px; font-weight: 600; }
.fgroup { font-size: 12px; color: #7d715d; }
button.mini { min-width: 28px; min-height: 28px; border-radius: 7px; color: #9c8f78; }
button.mini:hover { background: #2b2419; color: #efe6d6; }
button.mini:disabled { opacity: .3; }
button.mini.unlink:hover { background: #3a1d19; color: #ff9d8c; }
.dashedbox { padding: 12px; border-radius: 9px; border: 1px dashed #3a3124;
  font-size: 13px; color: #7d715d; }
.ghead { padding: 10px 12px 10px 8px; background: #231e16; }
button.chev { min-width: 18px; min-height: 28px; color: #9c8f78; }
button.chev:hover { color: #efe6d6; }
.gtitle { font-size: 15px; font-weight: 700; }
.gcount { font-size: 12px; color: #7d715d; }
.rchip { min-height: 20px; padding: 0 8px; border-radius: 999px; background: #1f2a1c;
  color: #8fd18a; font-size: 11px; font-weight: 700; }
.ggame { font-size: 12px; color: #7d715d; }
.ggame.set { color: #e9c887; }
button.glaunch { min-height: 34px; padding: 0 12px 0 8px; border-radius: 9px;
  border: 1px solid #4a3f2e; background: #2b2419; font-size: 13px; font-weight: 600; }
button.glaunch:hover { background: #3a3124; }
.gempty { border-top: 1px solid #2b2419; padding: 18px 16px; font-size: 13px;
  color: #7d715d; }

/* account rows */
.arow { padding: 12px 12px 12px 8px; background: #1f1a13;
  border-top: 1px solid #2b2419; }
.arow.first { border-top: none; }
.arow.sel { background: #28221a; }
.arow.open { background: #2b2419; }
.arow.dragging { opacity: .4; }
.arow.above { box-shadow: inset 0 2px 0 #e9bd6a; }
.arow.below { box-shadow: inset 0 -2px 0 #e9bd6a; }
.handle { color: #5f5646; }
button.cbox { min-width: 20px; min-height: 20px; border-radius: 6px;
  border: 1.5px solid #4a3f2e; color: #2a1f0c; }
button.cbox.on { background: #e9bd6a; border-color: #e9bd6a; }
.aname { font-size: 15px; font-weight: 600; color: #9c8f78; }
.aname.on { color: #efe6d6; }
.ajbadge { min-height: 20px; padding: 0 8px 0 6px; border-radius: 999px;
  background: #3a2e1a; color: #f0c779; font-size: 11px; font-weight: 700; }
.noteic { min-width: 24px; min-height: 24px; border-radius: 6px; color: #b9ab93; }
.noteic:hover { background: #2e271c; color: #efe6d6; }
.eco { color: #7d715d; }
.sub { font-size: 12px; color: #8a7c64; }
.chip { min-height: 24px; padding: 0 10px; border-radius: 999px; font-size: 12px;
  font-weight: 600; background: #2b2419; color: #8a7c64; }
.chip.small { min-height: 22px; padding: 0 9px; font-size: 11px; }
.chip.running { background: #1f2a1c; color: #8fd18a; }
.chip.joining, .chip.starting { background: #3a2e1a; color: #f0c779; }
.chip.expired { background: #3a1d19; color: #ff9d8c; }
button.playb { min-width: 34px; min-height: 34px; border-radius: 9px;
  border: 1px solid #3a3124; background: #2b2419; color: #efe6d6; }
button.playb:hover { background: #3a3124; }
button.playb.stop { background: #3a1d19; border-color: #5a2b24; color: #ff9d8c; }
button.setb { min-width: 34px; min-height: 34px; border-radius: 9px; color: #9c8f78; }
button.setb:hover { background: #2b2419; color: #efe6d6; }
button.setb.open { background: #3a2e1a; color: #f0c779; }

/* settings panels */
.panel { background: #1b1611; border-top: 1px solid #2b2419;
  padding: 18px 20px 18px 70px; }
.panel.group { padding: 16px 20px 18px 70px; }
.plabel { font-size: 13px; font-weight: 600; color: #b9ab93; }
.phint { font-size: 12px; color: #9c8f78; }
.phint2 { font-size: 12px; color: #7d715d; }
.leadnote { min-height: 32px; font-size: 13px; font-weight: 600; color: #e9bd6a; }
button.obtn { min-height: 32px; padding: 0 12px 0 10px; border: 1px solid #3a3124;
  color: #efe6d6; font-size: 13px; font-weight: 600; }
button.obtn:hover { border-color: #e9bd6a; color: #e9bd6a; }
button.opt { min-height: 32px; padding: 0 12px; border: 1px solid #3a3124;
  color: #b9ab93; font-size: 13px; font-weight: 600; }
button.opt:hover { background: #2b2419; }
button.opt.on { background: #3a2e1a; border-color: #5a4724; color: #f0c779; }
button.gopt { min-height: 38px; padding: 0 12px 0 5px; border-radius: 9px; }
button.sbtn { min-height: 32px; padding: 0 12px 0 9px; border: 1px solid #3a3124;
  background: #2b2419; color: #efe6d6; font-size: 13px; font-weight: 600; }
button.sbtn:hover { background: #3a3124; }
button.sbtn.amber { background: #e9bd6a; border-color: #e9bd6a; color: #2a1f0c; }
button.sbtn.amber:hover { background: #f2cb80; }
button.dbtn { min-height: 34px; padding: 0 12px 0 9px; border: 1px solid #3a3124;
  color: #ff9d8c; font-size: 13px; font-weight: 600; }
button.dbtn:hover, button.dbtn.armed { background: #3a1d19; }
button.dbtn.armed { border-color: #5a2b24; }
.prm { padding-top: 14px; border-top: 1px solid #2b2419; }
entry.field, .notebox { min-height: 38px; border-radius: 9px; background: #211c15;
  color: #efe6d6; font-size: 14px; box-shadow: inset 0 0 0 1px #3a3124;
  border: none; outline: none; }
entry.field:focus-within, .notebox:focus-within { box-shadow: inset 0 0 0 1px #e9bd6a; }
.notebox textview, .notebox textview text { background: transparent; color: #efe6d6; }
.sess { font-size: 13px; color: #b9ab93; }
.sess.expired { color: #ff9d8c; }
.sess.checking { color: #e9c887; }
switch.sw { min-width: 38px; min-height: 22px; padding: 0; border: none;
  border-radius: 999px; background: #3a3124; box-shadow: none; }
switch.sw:checked { background: #e9bd6a; }
switch.sw slider { min-width: 18px; min-height: 18px; margin: 2px; padding: 0;
  border-radius: 999px; background: #9c8f78; box-shadow: 0 1px 3px rgba(0,0,0,.35); }
switch.sw:checked slider { background: #fff6e6; }
switch.sw image { color: transparent; }

/* macros */
button.hbtn { min-height: 32px; padding: 0 12px 0 8px; border: 1px solid #3a3124;
  background: #27211a; color: #efe6d6; font-size: 13px; font-weight: 600; }
button.hbtn:hover { background: #2e271c; }
button.hbtn .ms { color: #e9bd6a; }
.mcard { border: 1px solid #332b1f; border-radius: 14px; background: #27211a; }
.mcard.running { border-color: #6b5a3e; box-shadow: 0 0 0 3px rgba(233,189,106,.08); }
.mhead { padding: 14px 14px 12px 16px; }
.mname { font-size: 15px; font-weight: 700; }
.mrun { font-size: 12px; font-weight: 600; color: #8fd18a; }
.mmeta { font-size: 12px; color: #9c8f78; }
.hk { min-height: 22px; padding: 0 7px; border-radius: 6px; background: #1f1a13;
  border: 1px solid #3a3124; color: #d8ccb6; }
.hk .mono { font-size: 11px; font-weight: 500; }
.chevic { color: #7d715d; }
.mbody { padding: 0 14px 14px; }
.msteps { padding-top: 12px; border-top: 1px solid #332b1f; }
.mstep { padding: 5px 10px 5px 5px; border-radius: 9px; background: #1f1a13; }
.mstepic { min-width: 28px; min-height: 28px; border-radius: 7px; background: #2e2619;
  color: #e9bd6a; }
.msteptype { font-size: 13px; font-weight: 600; }
.mstepval { font-size: 12px; color: #d8ccb6; }
.merr { font-size: 12px; color: #ff9d8c; }
button.medit { min-height: 36px; border-radius: 9px; border: 1px solid #3a3124;
  color: #d8ccb6; font-size: 13px; font-weight: 600; }
button.medit:hover { background: #2b2419; }
button.mrunb { min-height: 36px; border-radius: 9px; border: 1px solid #e9bd6a;
  background: #e9bd6a; color: #2a1f0c; font-size: 13px; font-weight: 700; }
button.mrunb:hover { background: #f2cb80; }
button.mrunb.stop { background: #3a1d19; border-color: #5a2b24; color: #ff9d8c; }
.mempty { font-size: 13px; color: #9c8f78; }
.activity { border-radius: 14px; border: 1px solid #332b1f; background: #1b1611;
  padding: 12px 14px; }
.acthead { font-size: 12px; font-weight: 600; color: #9c8f78; }
.logline { font-size: 13px; color: #d8ccb6; }
.logtime { font-size: 11px; color: #5f5646; }
.k-launch, .k-macro, .k-friend, .k-join { color: #e9bd6a; }
.k-stop, .k-error { color: #ff9d8c; }
.k-info { color: #7d715d; }
.k-update { color: #8fd18a; }

/* action bar */
.actionbar { padding: 14px 16px 14px 20px; background: #1d1812;
  border-top: 1px solid #332b1f; }
.summary { font-size: 14px; font-weight: 600; }
.target { font-size: 12px; color: #9c8f78; }
button.big { min-height: 42px; padding: 0 16px 0 12px; border-radius: 10px;
  font-size: 14px; font-weight: 600; }
button.big.stopall { border: 1px solid #5a2b24; color: #ff9d8c; }
button.big.stopall:hover { background: #3a1d19; }
button.big.second { border: 1px solid #4a3f2e; background: #2b2419; }
button.big.second:hover { background: #3a3124; }
button.big.primary { padding: 0 20px 0 14px; background: #e9bd6a; color: #2a1f0c;
  font-weight: 700; box-shadow: 0 6px 18px rgba(233,189,106,.18); }
button.big.primary:hover { background: #f2cb80; }
button.big:disabled { opacity: .45; }

/* modals */
dialog.modal sheet { color: #efe6d6; border-radius: 16px;
  background: radial-gradient(ellipse 120% 45% at 50% 0%, #2c2418 0%, #231d15 70%);
  box-shadow: inset 0 0 0 1px #3d3326, inset 0 1px 0 rgba(255,236,200,.06),
              0 40px 90px rgba(0,0,0,.65); }
.mhdr { padding: 18px 18px 2px 22px; }
.micon { min-width: 42px; min-height: 42px; border-radius: 11px; background: #3a2e1a;
  border: 1px solid #5a4724; color: #f0c779; }
.mtitle { font-size: 18px; font-weight: 700; }
.msub { font-size: 14px; color: #9c8f78; }
button.mclose { min-width: 32px; min-height: 32px; border-radius: 8px; color: #9c8f78; }
button.mclose:hover { background: #2e271c; color: #efe6d6; }
.mfoot { padding: 12px 22px; border-top: 1px solid #332b1f; background: #1f1a13; }
button.cancel { min-height: 36px; padding: 0 16px; border-radius: 9px;
  border: 1px solid #4a3f2e; background: #2b2419; font-size: 14px; font-weight: 600; }
button.cancel:hover { background: #3a3124; }
button.save { min-height: 36px; padding: 0 16px 0 12px; border-radius: 9px;
  background: #e9bd6a; color: #2a1f0c; font-size: 14px; font-weight: 700; }
button.save:hover { background: #f2cb80; }
button.mdel { min-height: 36px; padding: 0 12px 0 8px; border-radius: 9px;
  color: #ff9d8c; font-size: 13px; font-weight: 600; }
button.mdel:hover { background: #3a1d19; }

/* add account */
.stepper { padding: 16px 22px 4px; }
.circle { min-width: 23px; min-height: 23px; border-radius: 999px;
  border: 1.5px solid #4a3f2e; background: #2b2419; color: #d8ccb6;
  font-size: 12px; font-weight: 700; }
.circle.done { background: #e9bd6a; border-color: #e9bd6a; color: #2a1f0c;
  font-size: 16px; }
.circle.active { background: transparent; border-color: #e9bd6a; color: #e9bd6a; }
.connector { min-width: 2px; background: #3a3124; margin: 4px 0; }
.stepbody { padding: 2px 0 18px; }
.steptitle { font-size: 15px; font-weight: 600; }
.stephelp { font-size: 13px; color: #9c8f78; }
button.open { min-height: 38px; padding: 0 14px 0 16px; border-radius: 9px;
  background: #e9bd6a; color: #2a1f0c; font-size: 14px; font-weight: 700;
  box-shadow: 0 6px 18px rgba(233,189,106,.18); }
button.open:hover { background: #f2cb80; }
button.url { font-size: 12px; color: #7d715d; padding: 2px 4px; border-radius: 5px; }
button.url:hover { color: #b9ab93; }
.codecard { border-radius: 12px; background: #1a1510; border: 1px solid #3a3124; }
.codetop { padding: 14px 12px 10px 18px; }
.code { font-size: 30px; font-weight: 500; letter-spacing: .14em; color: #f7ecd6; }
button.copy { min-height: 36px; padding: 0 14px 0 11px; border-radius: 8px;
  border: 1px solid #3a3124; background: #2b2419; font-size: 13px;
  font-weight: 600; color: #d8ccb6; }
button.copy:hover { background: #3a3124; }
button.copy.copied { background: #1f2a1c; border-color: #3f5a39; color: #8fd18a; }
.expiry { padding: 0 12px 10px 18px; font-size: 12px; color: #7d715d; }
button.newcode { font-size: 12px; color: #b9ab93; padding: 3px 6px; border-radius: 5px; }
button.newcode:hover { color: #efe6d6; background: #2b2419; }
progressbar.exp trough { min-height: 3px; border-radius: 0; background: #2b2419;
  box-shadow: none; }
progressbar.exp progress { min-height: 3px; border-radius: 0; background: #e9bd6a; }
progressbar.exp.low progress { background: #ff9d8c; }
.qlstatus { margin-top: 10px; padding: 10px 12px; border-radius: 10px;
  background: #2a2217; border: 1px solid #3d3120; font-size: 13px; color: #e9c887; }
.qlstatus.error { background: #3a1d19; border-color: #5a2b24; color: #ff9d8c; }

/* join a friend */
.fbody { padding: 16px 22px 0; }
.fof { font-size: 12px; color: #7d715d; margin-right: 4px; }
button.fchip { min-height: 28px; padding: 0 11px; border-radius: 999px;
  border: 1px solid #3a3124; color: #b9ab93; font-size: 12px; font-weight: 600; }
button.fchip.on { background: #3a2e1a; border-color: #5a4724; color: #f0c779; }
.search { min-height: 40px; padding: 0 12px; border-radius: 10px; background: #1a1510;
  border: 1px solid #3a3124; }
.search .ms { color: #7d715d; }
.search entry { background: none; box-shadow: none; border: none; outline: none;
  color: #efe6d6; min-height: 0; padding: 0; }
.flist { padding: 10px 14px 14px; }
.friend { min-height: 56px; padding: 8px 8px 8px 12px; border-radius: 10px; }
.friend.sel { background: #28221a; }
.fdot { min-width: 9px; min-height: 9px; border-radius: 999px; background: #5f5646; }
.fdot.game { background: #8fd18a; box-shadow: 0 0 0 3px rgba(143,209,138,.16); }
.fdot.online { background: #82bde6; box-shadow: 0 0 0 3px rgba(130,175,235,.16); }
.frname { font-size: 14px; font-weight: 600; }
.frname.offline { color: #9c8f78; }
.frstatus { font-size: 12px; color: #7d715d; }
.frstatus.game { color: #8fd18a; }
.frstatus.online { color: #82bde6; }
button.join { min-height: 32px; padding: 0 12px 0 9px; border-radius: 8px;
  background: #e9bd6a; color: #2a1f0c; font-size: 13px; font-weight: 700; }
button.join:hover { background: #f2cb80; }
button.join.chosen { background: #1f2a1c; color: #8fd18a; }
.nofriends { padding: 28px 8px; font-size: 13px; color: #7d715d; }
.fsum { font-size: 13px; color: #9c8f78; }

/* edit macro */
.ebody { padding: 18px 22px 0; }
.elabel { font-size: 12px; font-weight: 600; color: #9c8f78; }
entry.efield { min-height: 40px; border-radius: 10px; background: #1a1510;
  box-shadow: inset 0 0 0 1px #3a3124; color: #efe6d6; border: none; outline: none; }
entry.efield:focus-within { box-shadow: inset 0 0 0 1px #e9bd6a; }
button.hkcap { min-height: 40px; padding: 0 12px; border-radius: 10px;
  border: 1px solid #3a3124; background: #1a1510; color: #efe6d6; font-size: 13px; }
button.hkcap .ms { color: #7d715d; }
button.hkcap.capturing { border-color: #e9bd6a; color: #e9bd6a; }
.seg { padding: 3px; border-radius: 10px; background: #1a1510; border: 1px solid #3a3124; }
button.segopt { min-height: 32px; padding: 0 14px 0 10px; border-radius: 7px;
  color: #9c8f78; font-size: 13px; font-weight: 600; }
button.segopt.on { background: #3a2e1a; color: #f0c779; }
spinbutton.rounds { min-height: 32px; border-radius: 7px; background: #211c15;
  box-shadow: inset 0 0 0 1px #3a3124; border: none; }
.estep { padding: 6px; border-radius: 10px; background: #1f1a13; border: 1px solid #2e271c; }
.estepn { font-size: 11px; color: #7d715d; }
.stype { min-height: 32px; padding: 0 0 0 9px; border-radius: 7px;
  border: 1px solid #3a3124; background: #2b2419; color: #e9bd6a; }
.stype dropdown > button { background: none; box-shadow: none; border: none;
  min-height: 30px; padding: 0 6px 0 4px; color: #e9bd6a; font-size: 13px;
  font-weight: 600; }
entry.sval { min-height: 32px; border-radius: 7px; background: #1a1510;
  box-shadow: inset 0 0 0 1px #3a3124; border: none; outline: none; font-size: 12px; }
entry.sval:focus-within { box-shadow: inset 0 0 0 1px #e9bd6a; }
.noSteps { padding: 18px; border-radius: 10px; border: 1px dashed #3a3124;
  font-size: 13px; color: #7d715d; }
button.addstep { min-height: 32px; padding: 0 12px 0 8px; border-radius: 8px;
  border: 1px dashed #4a3f2e; color: #d8ccb6; font-size: 13px; font-weight: 600; }
button.addstep:hover { background: #2b2419; border-color: #6b5a3e; }
button.addstep .ms { color: #e9bd6a; }
""" + "".join(f".s{n} {{ font-size: {n}px; }}\n" for n in range(11, 37))


def once(fn, *args):
    """Run fn on the GTK main loop exactly once.

    GLib keeps re-running an idle callback for as long as it returns something
    truthy, so a callback whose return value is merely incidental turns into an
    infinite loop. That is not hypothetical: `Gio.AppInfo.launch_default_for_uri`
    returns True on success, and handing it straight to GLib.idle_add reopened
    the browser hundreds of times. Every cross-thread call goes through here so
    that no call site has to remember to return GLib.SOURCE_REMOVE.
    """

    def run():
        fn(*args)
        return GLib.SOURCE_REMOVE

    GLib.idle_add(run)


def copy_to_clipboard(widget, text):
    """Put text on the clipboard.

    GdkClipboard exposes only set/set_content through introspection here -- there
    is no set_text to call, so this goes straight to a content provider instead of
    probing for a convenience method that does not exist.
    """
    widget.get_clipboard().set_content(Gdk.ContentProvider.new_for_bytes(
        "text/plain;charset=utf-8", GLib.Bytes.new(text.encode())))


# Small builders: the design is a lot of labelled boxes, and GTK's keyword
# constructors spell each one out at length.
def lbl(text, css="", **kw):
    kw.setdefault("xalign", 0)
    return Gtk.Label(label=text, css_classes=css.split(), **kw)


def hbox(spacing=0, css="", *children, **kw):
    box = Gtk.Box(spacing=spacing, css_classes=css.split(), **kw)
    for c in children:
        box.append(c)
    return box


def vbox(spacing=0, css="", *children, **kw):
    return hbox(spacing, css, *children, orientation=Gtk.Orientation.VERTICAL, **kw)


def icon(name, size=18, css="", fill=False, **kw):
    """A Material Symbols glyph: the font turns the name into the icon."""
    kw.setdefault("valign", Gtk.Align.CENTER)
    kw.setdefault("halign", Gtk.Align.CENTER)
    return Gtk.Label(label=name, css_classes=["ms", f"s{size}", *css.split(),
                                              *(["fill"] if fill else [])], **kw)


def btn(text, css, on_click=None, tooltip=None, ic=None, size=18, fill=False,
        gap=6, **kw):
    """A button of an optional icon and optional text, as .icon and .text."""
    b = Gtk.Button(css_classes=["b", *css.split()], tooltip_text=tooltip, **kw)
    b.icon = icon(ic, size, fill=fill) if ic else None
    b.text = Gtk.Label(label=text) if text is not None else None
    b.set_child(hbox(gap, "", *[w for w in (b.icon, b.text) if w],
                     halign=Gtk.Align.CENTER))
    b.set_cursor_from_name("pointer")
    if on_click:
        b.connect("clicked", lambda _b: on_click())
    return b


def armed_btn(text, armed_text, ic, action, css="dbtn"):
    """A destructive button that asks for a second click within 3 s."""
    b = btn(text, css, ic=ic, size=17, gap=5, halign=Gtk.Align.START)
    timer = []

    def reset():
        timer.clear()
        b.text.set_label(text)
        b.remove_css_class("armed")
        return GLib.SOURCE_REMOVE

    def click(_b):
        if timer:
            GLib.source_remove(timer.pop())
            action()
            return
        b.text.set_label(armed_text)
        b.add_css_class("armed")
        timer.append(GLib.timeout_add(3000, reset))

    b.connect("clicked", click)
    return b


def switch(active, on_change, tooltip=None):
    sw = Gtk.Switch(active=active, valign=Gtk.Align.CENTER, css_classes=["sw"],
                    tooltip_text=tooltip)
    sw.set_cursor_from_name("pointer")
    sw.connect("notify::active", lambda s, _p: on_change(s.get_active()))
    return sw


def wrap(*children, spacing=6):
    box = Adw.WrapBox(child_spacing=spacing, line_spacing=spacing)
    for c in children:
        box.append(c)
    return box


def dot(kind, pulse=False, size=7):
    core = Gtk.Box(css_classes=["dot", f"d{size}", kind],
                   valign=Gtk.Align.CENTER, halign=Gtk.Align.CENTER)
    if not pulse:
        return core
    o = Gtk.Overlay(child=core, valign=Gtk.Align.CENTER)
    o.add_overlay(Gtk.Box(css_classes=["dot", f"d{size}", kind, "ring"]))
    return o


STATES = {"running": ("Running", True), "joining": ("Joining…", True),
          "starting": ("Starting…", True), "expired": ("Expired", False),
          "idle": ("Idle", False)}


def chip(state, small=False):
    text, pulse = STATES[state]
    return hbox(6, f"chip {state}{' small' if small else ''}", dot(state, pulse),
                lbl(text), valign=Gtk.Align.CENTER)


def section(ic, title, sub, *suffix):
    return hbox(12, "",
                vbox(0, "stile", icon(ic, 19), valign=Gtk.Align.CENTER),
                vbox(1, "", lbl(title, "stitle"),
                     lbl(sub, "ssub", ellipsize=Pango.EllipsizeMode.END),
                     hexpand=True, valign=Gtk.Align.CENTER),
                *suffix)


def setting(panel, title, content, top=7):
    """One labelled row of a settings panel: a 124 px label column, then
    the content."""
    content.set_hexpand(True)
    panel.append(hbox(18, "", lbl(title, "plabel", valign=Gtk.Align.START,
                                  margin_top=top, width_request=124, wrap=True,
                                  max_width_chars=16),
                      content))


def clear(box):
    while (child := box.get_first_child()) is not None:
        box.remove(child)


_textures = {}


def game_thumb(path, size, css="", ic=None, ic_size=20):
    """A game's icon, size px square, or the striped placeholder -- or, with
    ic, that symbol centred on the box's own background."""
    box = Gtk.Box(css_classes=["thumb", f"t{size}", *css.split()],
                  width_request=size, height_request=size,
                  valign=Gtk.Align.CENTER, halign=Gtk.Align.CENTER)
    box.set_overflow(Gtk.Overflow.HIDDEN)
    if ic:
        # Overlaid, not packed: centring a packed child takes hexpand, and
        # that spreads up and stretches every tile in the row.
        over = Gtk.Overlay(child=box, valign=Gtk.Align.CENTER,
                           halign=Gtk.Align.CENTER)
        over.add_overlay(icon(ic, ic_size))
        over.box = box
        return over
    tex = _textures.get(path)
    if path and tex is None:
        try:
            tex = _textures[path] = Gdk.Texture.new_from_filename(path)
        except Exception:
            tex = None          # a half-written cache entry is not fatal
    if tex is not None:
        # An Image, not a Picture: a Picture asks for the icon's own 150 px
        # and would widen whatever holds it past the design's.
        box.append(Gtk.Image(paintable=tex, pixel_size=size))
    else:
        box.add_css_class("stripes")
    return box


def hotkey_label(accel):
    ok, key, mods = Gtk.accelerator_parse(accel or "")
    return Gtk.accelerator_get_label(key, mods) if ok and key else (accel or "—")


# Activity lines carry an icon by kind. Worked out from the message, not
# passed by every caller: log() has dozens of callers, most in the backend.
# ponytail: keyword match; a new message style may land in "info".
LOG_KINDS = {"launch": "rocket_launch", "stop": "stop_circle", "macro": "bolt",
             "info": "info", "update": "download_done", "friend": "person_search",
             "error": "error", "join": "link"}


def log_kind(msg):
    m = msg.lower()
    for kind, words in (
            ("error", ("failed", "could not", "expired", "error", "cannot")),
            ("update", ("up to date",)),
            ("stop", ("stopped", "removed", "was not running")),
            ("join", ("joined", " into ", "in server")),
            ("friend", ("join ", "joining")),
            ("launch", ("launch",)),
            ("macro", ("macro", "playing", "round ", "saved"))):
        if any(w in m for w in words):
            return kind
    return "info"


class Modal(Adw.Dialog):
    """The design's modal: icon tile, title and subtitle, a close button,
    content, and a footer."""

    def __init__(self, ic, title, sub, width):
        super().__init__(content_width=width, title=title)
        self.add_css_class("modal")
        self.title_label = lbl(title, "mtitle")
        self.sub_label = lbl(sub, "msub", wrap=True)
        self.header = hbox(14, "mhdr",
                           vbox(0, "micon", icon(ic, 22), valign=Gtk.Align.START),
                           vbox(2, "", self.title_label, self.sub_label,
                                hexpand=True, margin_top=1),
                           btn(None, "mclose", self.close, "Close", ic="close",
                               size=20, valign=Gtk.Align.START))

    def build(self, body, footer):
        self.set_child(vbox(0, "", self.header, body, footer))


class AccountRow(Gtk.Box):
    """One account: select, launch/stop, and a gear that opens everything
    else about it. Every account but the leader can be dragged -- onto
    another row to reorder or regroup, onto a group, or onto the leader to
    auto-join it."""

    def __init__(self, window, acct, first=False):
        super().__init__(orientation=Gtk.Orientation.VERTICAL)
        self.window = w = window
        self.acct = acct
        name = acct["name"]
        lead = bool(acct.get("leader"))
        on = bool(acct.get("selected", True))
        opened = name in w.open_accounts
        fnum = acct.get("follow") if not lead else None

        kids = []
        if lead:
            star = icon("star", 18, "amber", fill=True)
            star.set_tooltip_text("Leader")
            kids.append(star)
        else:
            handle = icon("drag_indicator", 18, "handle")
            handle.set_tooltip_text("Drag to reorder, move to a group, or drop "
                                    "on the leader")
            handle.set_cursor_from_name("grab")
            kids.append(handle)
        kids.append(btn(None, "cbox" + (" on" if on else ""),
                        lambda: w.toggle_selected(name),
                        "Include in Launch selected", ic="check" if on else None,
                        size=15, valign=Gtk.Align.CENTER))

        title = hbox(8, "", lbl(name, "aname" + (" on" if on else ""),
                                ellipsize=Pango.EllipsizeMode.END))
        if acct.get("username") and acct["username"] != name:
            title.set_tooltip_text(f"@{acct['username']}")
        if fnum:
            title.append(hbox(4, "ajbadge", icon("link", 13),
                              lbl(f"Auto-join #{fnum}"), valign=Gtk.Align.CENTER))
        note = (acct.get("note") or "").strip()
        if note:
            n = vbox(0, "noteic", icon("sticky_note_2", 16), valign=Gtk.Align.CENTER)
            n.set_tooltip_markup(
                "<span size='small' weight='bold' foreground='#e9bd6a'>NOTE</span>\n"
                + GLib.markup_escape_text(note))
            n.set_cursor_from_name("help")
            title.append(n)
        if acct.get("low_power"):
            eco = icon("eco", 15, "eco")
            eco.set_tooltip_text("Low-power client")
            title.append(eco)
        sub = hbox(6, "sub", icon("schedule", 14),
                   lbl(relative_time(acct.get("last_launch"))))
        if acct.get("macro") in w.macros:
            sub.append(Gtk.Box(css_classes=["bullet"], valign=Gtk.Align.CENTER))
            sub.append(icon("keyboard", 14))
            sub.append(lbl(acct["macro"], ellipsize=Pango.EllipsizeMode.END))
        info = vbox(3, "", title, sub, hexpand=True, valign=Gtk.Align.CENTER)

        self.chipbox = hbox(0, "", valign=Gtk.Align.CENTER)
        self.play = btn(None, "playb", self.on_play_stop, ic="play_arrow", size=20,
                        fill=True, valign=Gtk.Align.CENTER)
        gear = btn(None, "setb" + (" open" if opened else ""),
                   lambda: w.toggle_account(name), "Account settings",
                   ic="settings", size=20, valign=Gtk.Align.CENTER)
        css = "arow" + (" first" if first else "") + (" sel" if on else "") \
            + (" open" if opened else "")
        self.line = hbox(12, css, *kids, info, self.chipbox, self.play, gear)
        self.append(self.line)
        if not lead:
            self._dnd()
        if opened:
            self.append(self._settings())
        self.set_state()

    # -- drag and drop ------------------------------------------------------
    def _dnd(self):
        name = self.acct["name"]
        src = Gtk.DragSource(actions=Gdk.DragAction.MOVE)
        src.connect("prepare", lambda _s, _x, _y: Gdk.ContentProvider.new_for_value(
            GObject.Value(GObject.TYPE_STRING, name)))
        src.connect("drag-begin", lambda s, _d: (
            s.set_icon(Gtk.WidgetPaintable.new(self.line), 20, 20),
            self.line.add_css_class("dragging")))
        src.connect("drag-end", lambda *_a: self.line.remove_css_class("dragging"))
        self.line.add_controller(src)

        drop = Gtk.DropTarget.new(GObject.TYPE_STRING, Gdk.DragAction.MOVE)
        drop.set_preload(True)
        drop.connect("motion", self._motion)
        drop.connect("leave", lambda *_a: self._mark(None))
        drop.connect("drop", lambda _t, v, _x, _y: (
            self._mark(None), self.window.drop_on_row(v, name), True)[-1])
        self.line.add_controller(drop)

    def _motion(self, target, _x, _y):
        """Mark where the dragged account would land: after this row when it
        comes from above, before it when from below (drop_account)."""
        value = target.get_value()
        if value and value != self.acct["name"]:
            order = [a["name"] for a in self.window.visual()]
            if value in order:
                down = order.index(value) < order.index(self.acct["name"])
                self._mark("below" if down else "above")
        return Gdk.DragAction.MOVE

    def _mark(self, where):
        for c in ("above", "below"):
            (self.line.add_css_class if c == where else self.line.remove_css_class)(c)

    # -- the settings panel -------------------------------------------------
    def _settings(self):
        w, acct = self.window, self.acct
        name = acct["name"]
        lead = bool(acct.get("leader"))
        panel = vbox(18, "panel")

        def row(title, content, top=7):
            setting(panel, title, content, top)

        if lead:
            row("Leader", hbox(6, "leadnote", icon("star", 18, fill=True),
                               lbl("This account is the leader")))
            row("Group", lbl("The leader can't be in a group. Make another account "
                             "leader from its settings to hand over the lead.",
                             "phint", wrap=True, max_width_chars=72), top=0)
        else:
            row("Leader", hbox(0, "", btn("Make leader", "obtn",
                                          lambda: w.set_leader(name), ic="star")))
            now = group_of(acct, w.groups)
            opts = [(None, "Ungrouped")] + [(g["id"], g["name"] or "Untitled group")
                                            for g in w.groups]
            row("Group", wrap(*[btn(t, "opt" + (" on" if gid == now else ""),
                                    lambda gid=gid: w.set_group(name, gid))
                                for gid, t in opts]))
            ld = w.leader()
            if ld:
                row("Auto-join", hbox(12, "",
                                      switch(bool(acct.get("follow")),
                                             lambda on: w.set_follow(name, on)),
                                      lbl(f"Joins {ld['name']}'s server right after "
                                          "it launches.", "phint", wrap=True,
                                          max_width_chars=72)), top=2)

        label = Gtk.Entry(text=name, hexpand=True, css_classes=["field"])
        label.connect("activate", lambda e: w.rename_account(acct, e.get_text().strip()))
        row("Label", vbox(6, "",
                          hbox(8, "", label,
                               btn("Rename", "sbtn", lambda: w.rename_account(
                                   acct, label.get_text().strip()),
                                   valign=Gtk.Align.CENTER)),
                          lbl("Your name for this account; renaming moves its "
                              "keyring entry too.", "phint2", wrap=True)), top=10)

        # accounts.json is not encrypted at rest on this host, so the note is
        # for labels, not secrets.
        # Grows with its text; a scroller's minimum height here made GTK
        # measure the panel smaller than its own minimum.
        note = Gtk.TextView(wrap_mode=Gtk.WrapMode.WORD_CHAR, top_margin=9,
                            bottom_margin=9, left_margin=12, right_margin=12,
                            accepts_tab=False, height_request=62)
        note.get_buffer().set_text(acct.get("note") or "")
        note.get_buffer().connect("changed", self.on_note_changed)
        box = hbox(0, "notebox", note)
        note.set_hexpand(True)
        box.set_overflow(Gtk.Overflow.HIDDEN)
        row("Note", vbox(6, "", box,
                         hbox(5, "phint2", icon("sticky_note_2", 14),
                              lbl("Hover the note icon next to the name to read it. "
                                  "Plain text -- not for passwords."))), top=10)

        mine = acct.get("macro") if acct.get("macro") in w.macros else None
        playing = name in w.macro_runs
        run = btn("Stop here" if playing else "Run here", "sbtn",
                  lambda: w.play_macro_here(acct),
                  "Stop the macro on this account" if playing
                  else "Play the macro into this account's client",
                  ic="stop" if playing else "play_arrow", fill=True, gap=4,
                  sensitive=bool(mine) or playing)
        row("Macro", wrap(*[btn(t, "opt" + (" on" if m == mine else ""),
                                lambda m=m: w.pick_macro(acct, m))
                            for m, t in [(None, "None")] + [(m, m) for m in
                                                            sorted(w.macros)]],
                          Gtk.Box(css_classes=["vdiv"], valign=Gtk.Align.CENTER,
                                  margin_start=4, margin_end=4),
                          run))

        def toggle_row(title, key, text):
            def set_on(on):
                acct[key] = on
                w.persist()
                if key == "low_power":
                    w.refresh_accounts()
            row(title, hbox(12, "", switch(bool(acct.get(key)), set_on),
                            lbl(text, "phint", wrap=True, max_width_chars=72,
                                hexpand=True)), top=2)

        toggle_row("Macro-ready window", "nested",
                   "Runs the client on a display of its own, so a macro can play "
                   "into it while you use other windows. Picking a macro turns "
                   "it on. Applies from the next launch.")
        toggle_row("Low-power client", "low_power",
                   "For an account you are not playing on: 20 FPS, fewer CPU "
                   "threads, lower priority, and slower still when its window is "
                   "not focused. Applies from the next launch.")

        state = acct.get("session") or "ok"
        if state == "checking":
            text, kind = "Checking session…", "checking"
        elif state == "expired":
            text, kind = "Session expired", "expired"
        else:
            when = acct.get("session_checked")
            text = f"Signed in · checked {relative_time(when)}" if when else "Signed in"
            kind = "ok"
        if state == "expired":
            action = btn("Sign in again", "sbtn amber",
                         lambda: AddAccountDialog(w, relogin=acct).present(w),
                         ic="login", size=17, gap=5)
        else:
            action = btn("Checking…" if state == "checking" else "Check session",
                         "sbtn", lambda: w.check_sessions([name]),
                         "Ask Roblox whether the stored session still works",
                         ic="sync", size=17, gap=5, sensitive=state != "checking")
        row("Session", vbox(6, "",
                            hbox(12, "",
                                 hbox(7, f"sess {kind}",
                                      dot({"ok": "running", "checking": "wait",
                                           "expired": "expired"}[kind],
                                          kind == "checking"), lbl(text)),
                                 action),
                            lbl("Sign in again when launches fail with HTTP 401. "
                                f"Cordial profile {cordial_profile(acct['user_id'])}.",
                                "phint2", wrap=True, selectable=True)), top=8)
        row("", hbox(0, "prm", armed_btn("Remove account", "Click again to remove",
                                         "person_remove",
                                         lambda: w.remove_account(name))), top=0)
        return panel

    # -- state --------------------------------------------------------------
    def set_state(self):
        state = self.window.state_of(self.acct)
        clear(self.chipbox)
        self.chipbox.append(chip(state))
        live = state == "running"
        self.play.icon.set_label("stop" if live else "play_arrow")
        (self.play.add_css_class if live else self.play.remove_css_class)("stop")
        self.play.set_sensitive(state not in ("starting", "joining"))
        self.play.set_tooltip_text(
            "Close this account's client" if live else
            "Launch the leader, then auto-join the linked accounts"
            if self.acct.get("leader") else "Launch")

    # -- handlers -----------------------------------------------------------
    def on_note_changed(self, buf):
        self.acct["note"] = buf.get_text(buf.get_start_iter(), buf.get_end_iter(),
                                         False)
        self.window.persist()

    def on_play_stop(self):
        if self.window.state_of(self.acct) == "running":
            self.window.stop_account(self.acct["name"])
        else:
            self.window.play_account(self.acct)


class FollowerRow(Gtk.Box):
    """One account in the leader's auto-join list."""

    def __init__(self, window, acct, n, count):
        super().__init__(spacing=10, css_classes=["frow"])
        self.window, self.acct = window, acct
        name = acct["name"]
        gid = group_of(acct, window.groups)
        gname = next((g["name"] or "Untitled group" for g in window.groups
                      if g["id"] == gid), "Ungrouped")
        self.chipbox = hbox(0, "", valign=Gtk.Align.CENTER)
        for w in (lbl(str(n), "fnum mono", xalign=.5, valign=Gtk.Align.CENTER),
                  hbox(8, "", lbl(name, "fname", ellipsize=Pango.EllipsizeMode.END),
                       lbl(gname, "fgroup"), hexpand=True, valign=Gtk.Align.CENTER),
                  self.chipbox,
                  hbox(2, "",
                       btn(None, "mini", lambda: window.move_follower(name, -1),
                           "Join earlier", ic="arrow_upward", sensitive=n > 1),
                       btn(None, "mini", lambda: window.move_follower(name, 1),
                           "Join later", ic="arrow_downward", sensitive=n < count),
                       btn(None, "mini unlink",
                           lambda: window.set_follow(name, False),
                           "Stop auto-joining", ic="link_off"))):
            self.append(w)
        self.set_state()

    def set_state(self):
        clear(self.chipbox)
        self.chipbox.append(chip(self.window.state_of(self.acct), small=True))


class GroupCard(Gtk.Box):
    """A named group -- or, with no group, the ungrouped accounts: a header
    that selects, launches and edits it, then its rows. A drop target: an
    account dragged here joins it."""

    def __init__(self, window, g, accts):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, css_classes=["card"])
        self.window, self.g, self.accts = window, g, accts
        w = window
        gid = g["id"] if g else None
        opened = g.get("open", True) if g else w.ungrouped_open
        self.set_overflow(Gtk.Overflow.HIDDEN)

        n_sel = sum(1 for a in accts if a.get("selected", True))
        every = bool(accts) and n_sel == len(accts)
        game = w.game_by_place(g.get("place_id")) if g else None
        if g and g.get("place_id"):
            thumb = game_thumb(game and game.get("icon"), 40, "game")
            gname = (game or {}).get("name") or g.get("game") or f"Place {g['place_id']}"
            gline = hbox(5, "ggame set", icon("videogame_asset", 14),
                         lbl(gname, ellipsize=Pango.EllipsizeMode.END))
        else:
            thumb = game_thumb(None, 40, "none",
                               ic="sports_esports" if g else "folder_open")
            gline = hbox(5, "ggame",
                         icon("link_off" if g else "folder_open", 14),
                         lbl("No game assigned" if g else "Not in a group"))
            gname = None
        self.name_label = lbl((g["name"] or "Untitled group") if g else "Ungrouped",
                              "gtitle", ellipsize=Pango.EllipsizeMode.END)
        self.runbox = hbox(0, "", valign=Gtk.Align.CENTER)
        head = hbox(12, "ghead",
                    btn(None, "chev", lambda: w.toggle_group_open(gid),
                        "Collapse" if opened else "Expand",
                        ic="expand_more" if opened else "chevron_right", size=20,
                        valign=Gtk.Align.CENTER),
                    btn(None, "cbox" + (" on" if n_sel else ""),
                        lambda: w.select_accounts(accts, not every),
                        "Select every account in this group",
                        ic=("check" if every else "remove") if n_sel else None,
                        size=15, valign=Gtk.Align.CENTER),
                    thumb,
                    vbox(2, "",
                         hbox(8, "", self.name_label,
                              lbl(f"{len(accts)} account{'s' * (len(accts) != 1)}",
                                  "gcount"),
                              self.runbox),
                         gline, hexpand=True, valign=Gtk.Align.CENTER))
        if g:
            editing = w.edit_group == gid
            head.append(hbox(4, "",
                             btn("Launch", "glaunch", lambda: w.launch_group(gid),
                                 f"Launch every account in this group into {gname}"
                                 if gname else "Assign a game to launch this group",
                                 ic="play_arrow", fill=True, gap=4,
                                 sensitive=bool(gname and accts),
                                 valign=Gtk.Align.CENTER),
                             btn(None, "setb" + (" open" if editing else ""),
                                 lambda: w.toggle_group_edit(gid), "Group settings",
                                 ic="settings", size=20, valign=Gtk.Align.CENTER)))
        self.append(head)
        if g and w.edit_group == gid:
            self.append(self._editor())
        self.rows = []
        if opened:
            for a in accts:
                row = AccountRow(w, a)
                self.rows.append(row)
                self.append(row)
            if not accts:
                self.append(hbox(8, "gempty", icon("drag_indicator", 16),
                                 lbl("Drag accounts here to add them to this group"),
                                 halign=Gtk.Align.FILL))

        drop = Gtk.DropTarget.new(GObject.TYPE_STRING, Gdk.DragAction.MOVE)
        drop.connect("drop", lambda _t, v, _x, _y: (w.set_group(v, gid), True)[-1])
        self.add_controller(drop)
        self.set_state()

    def _editor(self):
        w, g = self.window, self.g
        panel = vbox(16, "panel group")
        name = Gtk.Entry(text=g["name"], placeholder_text="Group name",
                         css_classes=["field"], hexpand=True, max_width_chars=40)

        def renamed(e):
            g["name"] = e.get_text()
            self.name_label.set_label(g["name"] or "Untitled group")
            w.save_groups()

        name.connect("changed", renamed)
        name.connect("activate", lambda _e: w.refresh_accounts())
        setting(panel, "Group name", hbox(0, "", name, halign=Gtk.Align.START,
                                          width_request=420), top=10)

        def opt(place, label, path):
            on = (g.get("place_id") or None) == place
            b = Gtk.Button(css_classes=["b", "opt", "gopt", *(["on"] if on else [])])
            b.set_cursor_from_name("pointer")
            if place:
                t = game_thumb(path, 28, "game")
            else:
                t = game_thumb(None, 28, "none", ic="block", ic_size=16)
            b.set_child(hbox(8, "", t, lbl(label, ellipsize=Pango.EllipsizeMode.END,
                                           max_width_chars=26)))
            b.connect("clicked", lambda _b: w.set_group_game(g["id"], place, label))
            return b

        games = wrap(opt(None, "No game", None),
                     *[opt(x["place_id"], x["name"], x.get("icon"))
                       for x in w.game_list])
        setting(panel, "Game", vbox(8, "", games,
                                    lbl("Marks which game these accounts play. Launch "
                                        "on the group header opens it directly.",
                                        "phint2", wrap=True)), top=9)
        setting(panel, "", hbox(0, "prm",
                                armed_btn("Delete group", "Click again to delete",
                                          "delete", lambda: w.delete_group(g["id"]))),
                top=0)
        return panel

    def set_state(self):
        for r in self.rows:
            r.set_state()
        n = sum(1 for a in self.accts if a["name"] in self.window.running)
        clear(self.runbox)
        if n:
            self.runbox.append(hbox(6, "rchip", dot("running", True),
                                    lbl(f"{n} running")))


class MacroCard(Gtk.Box):
    """One macro: switch, hotkey, steps -- and Run on the selected accounts."""

    def __init__(self, window, name):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, css_classes=["mcard"])
        self.window = w = window
        self.name = name
        self.set_overflow(Gtk.Overflow.HIDDEN)
        text = w.macros[name]
        rows, loops = macro_rows(text)
        steps = [r for r in rows if r[0] != "Note"]
        opened = name in w.open_macros
        self.switch = switch(name not in w.macros_off,
                             lambda on: w.enable_macro(name, on), "Enable macro")
        self.running = hbox(6, "mrun", dot("running", True), lbl("Running"))
        head = vbox(10, "mhead",
                    hbox(10, "", lbl(name, "mname", hexpand=True,
                                     ellipsize=Pango.EllipsizeMode.END),
                         self.running, self.switch),
                    hbox(8, "mmeta",
                         hbox(5, "hk", icon("keyboard", 14),
                              lbl(hotkey_label(w.hotkeys.get(name)), "mono")),
                         lbl(f"{len(steps)} step{'s' * (len(steps) != 1)}"),
                         Gtk.Box(css_classes=["bullet"], valign=Gtk.Align.CENTER),
                         hbox(4, "", icon("repeat", 14), lbl(loop_label(loops))),
                         Gtk.Box(hexpand=True),
                         icon("expand_less" if opened else "expand_more", 20,
                              "chevic")))
        head.set_cursor_from_name("pointer")
        click = Gtk.GestureClick()
        click.connect("released", self._clicked)
        head.add_controller(click)
        self.head = head
        self.append(head)

        self.run = self.meta = None
        if opened:
            body = vbox(12, "mbody")
            listing = vbox(6, "msteps")
            for kind, value in rows:
                if kind == "Note":
                    continue
                shown = value + (" s" if kind in ("Wait", "Start") and value else "")
                listing.append(hbox(10, "mstep",
                                    vbox(0, "mstepic", icon(STEP_ICONS.get(
                                        kind, "radio_button_checked"), 17)),
                                    lbl(kind, "msteptype", hexpand=True),
                                    lbl(shown or "—", "mstepval mono",
                                        ellipsize=Pango.EllipsizeMode.END)))
            body.append(listing)
            try:
                parse_macro(text)
                broken = None
            except ValueError as e:
                broken = str(e)
                body.append(lbl(broken, "merr", wrap=True))
            self.meta = lbl("", "")
            body.append(hbox(6, "mmeta", icon("group", 15), self.meta))
            self.run = btn("Run", "mrunb", lambda: w.run_macro_card(name),
                           ic="play_arrow", fill=True, hexpand=True,
                           sensitive=broken is None)
            body.append(hbox(8, "",
                             btn("Edit steps", "medit",
                                 lambda: MacroDialog(w, name).present(w),
                                 ic="edit", size=17, hexpand=True),
                             self.run, homogeneous=True))
            self.append(body)
            self.update_meta(len(w.selected_accounts()))
        self.set_running()

    def _clicked(self, gesture, _n, x, y):
        picked = self.head.pick(x, y, Gtk.PickFlags.DEFAULT)
        if picked is None or not (picked == self.switch
                                  or picked.is_ancestor(self.switch)):
            self.window.toggle_macro(self.name)

    def update_meta(self, n):
        if self.meta is not None:
            self.meta.set_label(f"Runs on {n} selected client{'s' * (n != 1)}")

    def set_running(self):
        running = self.name in self.window.macros_running()
        (self.add_css_class if running else self.remove_css_class)("running")
        self.running.set_visible(running)
        if self.run is not None:
            self.run.icon.set_label("stop" if running else "play_arrow")
            self.run.text.set_label("Stop" if running else "Run")
            (self.run.add_css_class if running else self.run.remove_css_class)("stop")
            self.run.set_sensitive(running or self.name not in self.window.macros_off)


STEP_ICONS = {"Key": "keyboard", "Hold": "keyboard_keys", "Type": "text_fields",
              "Click": "mouse", "Move": "open_with", "Wait": "timer",
              "Start": "hourglass_top", "Note": "notes"}
STEP_HINTS = {"Key": "e  ·  shift+w  ·  space", "Hold": "w 2  ·  shift+w 0.5-1",
              "Type": "text to type", "Click": "960 540  ·  right  ·  left 10 20",
              "Move": "40 0", "Wait": "0.5  ·  60-240", "Start": "45",
              "Note": "what this part does"}


class MacroDialog(Modal):
    """Edit one macro as steps. Saving writes the same text the engine has
    always read (macro_text), and parse_macro checks it first."""

    def __init__(self, window, name=None):
        new = name is None
        super().__init__("keyboard", "New macro" if new else "Edit macro",
                         "Changes apply when you save", 560)
        self.window = w = window
        self.old = name
        if new:
            n = 1
            while f"Macro {n}" in w.macros:
                n += 1
            # Roblox kicks after 20 idle minutes; the starter keeps a client in.
            text, title = "# anti-AFK\ntap space\nwait 60-240\n", f"Macro {n}"
        else:
            text, title = w.macros[name], name
        rows, self.loops = macro_rows(text)
        self.rows = [list(r) for r in rows]
        self.hotkey = w.hotkeys.get(name) if name else None
        self.capturing = False

        self.name = Gtk.Entry(text=title, placeholder_text="Macro name",
                              css_classes=["efield"], hexpand=True)
        self.cap = btn(hotkey_label(self.hotkey) if self.hotkey else "Set hotkey",
                       "hkcap mono", self.on_capture,
                       "Press to set. While it waits: Esc keeps the old one, "
                       "Backspace clears it.", ic="keyboard", size=17, gap=8)
        self.cap.get_child().set_halign(Gtk.Align.START)
        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", self.on_key)
        self.add_controller(keys)

        self.seg = hbox(2, "seg", halign=Gtk.Align.START)
        self.spin = Gtk.SpinButton.new_with_range(2, 9999, 1)
        self.spin.add_css_class("rounds")
        self.spin.set_value(self.loops if self.loops > 1 else 10)
        self.spin.connect("value-changed", lambda s: self._set_loops(s.get_value_as_int()))

        self.count = lbl("", "phint2")
        self.list = vbox(6, "")
        steps = Gtk.ScrolledWindow(child=self.list, max_content_height=300,
                                   propagate_natural_height=True,
                                   hscrollbar_policy=Gtk.PolicyType.NEVER)
        adds = wrap(*[btn(t, "addstep", lambda t=t: self._add(t), ic="add", size=17,
                          gap=5) for t in ("Key", "Wait", "Click", "Type", "Hold",
                                           "Note")], spacing=8)
        self.err = lbl("", "merr", wrap=True, visible=False)

        body = vbox(18, "ebody",
                    hbox(12, "",
                         vbox(6, "", lbl("Name", "elabel"), self.name, hexpand=True),
                         vbox(6, "", lbl("Hotkey", "elabel"), self.cap,
                              width_request=160)),
                    vbox(6, "", lbl("Repeat", "elabel"),
                         hbox(10, "", self.seg, self.spin)),
                    vbox(8, "", hbox(8, "", lbl("Steps", "elabel"), self.count),
                         steps, adds, self.err))
        footer = hbox(8, "mfoot",
                      btn("Discard" if new else "Delete macro", "mdel",
                          self.close if new else self.on_delete, ic="delete", gap=5),
                      Gtk.Box(hexpand=True),
                      btn("Cancel", "cancel", self.close),
                      btn("Save", "save", self.on_save, ic="check"),
                      margin_top=20)
        self.header.insert_child_after(
            btn(None, "mclose", w.show_macro_help, "How macros work", ic="help",
                size=20, valign=Gtk.Align.START), self.header.get_first_child().get_next_sibling())
        self.build(body, footer)
        self._draw_seg()
        self._draw_steps()

    # -- repeat -------------------------------------------------------------
    def _set_loops(self, n):
        self.loops = n
        self._draw_seg()

    def _draw_seg(self):
        clear(self.seg)
        mode = "once" if self.loops == 1 else "until" if not self.loops else "rounds"
        for key, label, ic, loops in (
                ("once", "Once", "looks_one", 1),
                ("rounds", "Rounds", "pin", self.spin.get_value_as_int()),
                ("until", "Until stopped", "repeat", 0)):
            self.seg.append(btn(label, "segopt" + (" on" if key == mode else ""),
                                lambda n=loops: self._set_loops(n), ic=ic, size=16))
        self.spin.set_visible(mode == "rounds")

    # -- steps --------------------------------------------------------------
    def _draw_steps(self):
        clear(self.list)
        real = sum(1 for t, _v in self.rows if t != "Note")
        self.count.set_label(f"{real} step{'s' * (real != 1)}")
        if not self.rows:
            self.list.append(lbl("No steps yet. Add one below.", "noSteps", xalign=.5))
        types = list(STEP_TYPES)
        for i, r in enumerate(self.rows):
            opts = types if r[0] in types else types + [r[0]]
            kind = Gtk.DropDown.new_from_strings(opts)
            kind.set_selected(opts.index(r[0]))
            kind.connect("notify::selected",
                         lambda d, _p, r=r, opts=opts: self._retype(r, opts[d.get_selected()]))
            val = Gtk.Entry(text=r[1], placeholder_text=STEP_HINTS.get(r[0], ""),
                            css_classes=["sval", "mono"], hexpand=True)
            val.connect("changed", lambda e, r=r: r.__setitem__(1, e.get_text()))
            self.list.append(hbox(8, "estep",
                                  lbl(f"{i + 1:02d}", "estepn mono", xalign=.5,
                                      width_request=22),
                                  hbox(0, "stype", icon(STEP_ICONS.get(r[0], "code"), 16),
                                       kind, valign=Gtk.Align.CENTER),
                                  val,
                                  hbox(2, "",
                                       btn(None, "mini", lambda i=i: self._move(i, -1),
                                           "Move up", ic="arrow_upward", sensitive=i > 0),
                                       btn(None, "mini", lambda i=i: self._move(i, 1),
                                           "Move down", ic="arrow_downward",
                                           sensitive=i < len(self.rows) - 1),
                                       btn(None, "mini unlink", lambda i=i: self._del(i),
                                           "Remove step", ic="delete"),
                                       valign=Gtk.Align.CENTER)))

    def _retype(self, r, kind):
        if r[0] != kind:
            r[0] = kind
            # A deferred redraw: the dropdown whose popover just closed is
            # one of the widgets being replaced.
            once(self._draw_steps)

    def _add(self, kind):
        self.rows.append([kind, ""])
        self._draw_steps()

    def _move(self, i, d):
        j = i + d
        if 0 <= j < len(self.rows):
            self.rows[i], self.rows[j] = self.rows[j], self.rows[i]
            self._draw_steps()

    def _del(self, i):
        del self.rows[i]
        self._draw_steps()

    # -- hotkey -------------------------------------------------------------
    def on_capture(self):
        self.capturing = True
        self.cap.text.set_label("Press a key…")
        self.cap.add_css_class("capturing")

    def on_key(self, _c, keyval, _code, state):
        if not self.capturing:
            return False
        name = Gdk.keyval_name(keyval) or ""
        if name.startswith(("Shift", "Control", "Alt", "Super", "Meta", "ISO_")):
            return True               # a modifier alone is not a hotkey yet
        self.capturing = False
        self.cap.remove_css_class("capturing")
        if name == "BackSpace":
            self.hotkey = None
        elif name != "Escape":
            mods = state & Gtk.accelerator_get_default_mod_mask()
            self.hotkey = Gtk.accelerator_name(keyval, mods)
        self.cap.text.set_label(hotkey_label(self.hotkey) if self.hotkey
                                else "Set hotkey")
        return True

    # -- save ---------------------------------------------------------------
    def on_save(self):
        text = macro_text([tuple(r) for r in self.rows], self.loops)
        try:
            parse_macro(text)
        except ValueError as e:
            self.err.set_label(str(e))
            self.err.set_visible(True)
            return
        err = self.window.save_macro(self.old, self.name.get_text().strip(), text,
                                     self.hotkey)
        if err:
            self.err.set_label(err)
            self.err.set_visible(True)
        else:
            self.close()

    def on_delete(self):
        self.window.delete_macro(self.old)
        self.close()


class AddAccountDialog(Modal):
    """Roblox Quick Login as three steps: open the page, enter the code,
    approve. The code is asked for as soon as the dialog opens, renewed when
    it expires, and the account is labelled after its Roblox username.

    The app does not open a browser on its own. There is a button for it, but it
    goes to the bare confirmation page -- the code is never put in the URL, so
    nothing can be launched or navigated without a deliberate click.
    """

    def __init__(self, parent, relogin=None):
        """relogin: an existing account whose cookie is replaced in place --
        same label and keyring entry -- once Roblox approves the same user."""
        super().__init__("sync" if relogin else "person_add",
                         "Sign in again" if relogin else "Add account",
                         f"Approve as {relogin['name']}'s Roblox user"
                         if relogin else "Sign in with Roblox Quick Login", 480)
        self.parent = parent
        self.relogin = relogin
        self.holding = False
        self.closed = False
        self.gen = 0            # which code is current; older workers go quiet
        self.code = ""
        self.expires = 0
        self._copied_timer = None

        # 1 -- open the page
        self.c1 = self._circle("1")
        openit = btn("Open in browser", "open", self.on_open_page,
                     "Opens the page with no code in the address -- you type the code")
        openit.get_child().append(icon("open_in_new", 18))
        url = btn("roblox.com/crossdevicelogin", "url mono",
                  lambda: self._copy(QL_CONFIRM_URL, "Address copied"),
                  "Copy the full address")
        step1 = self._step(self.c1, "Open Quick Login in your browser",
                           "Use a device where you're already signed in.",
                           hbox(12, "", openit, url, margin_top=10))

        # 2 -- the code
        self.c2 = self._circle("2")
        self.code_a = lbl("···", "code mono", selectable=True)
        self.code_b = lbl("···", "code mono", selectable=True)
        self.copy = btn("Copy", "copy", self.on_copy, ic="content_copy", size=17,
                        valign=Gtk.Align.CENTER)
        self.expiry = lbl("Expires in –:––", "mono", hexpand=True)
        self.bar = Gtk.ProgressBar(css_classes=["exp"], fraction=1)
        card = vbox(0, "codecard",
                    hbox(14, "codetop",
                         hbox(14, "", self.code_a, self.code_b, hexpand=True),
                         self.copy),
                    hbox(8, "expiry", self.expiry,
                         btn("New code", "newcode", self.request_code, ic="refresh",
                             size=15, gap=4)),
                    self.bar)
        card.set_overflow(Gtk.Overflow.HIDDEN)
        step2 = self._step(self.c2, "Enter this code", None, card)

        # 3 -- approval
        c3 = self._circle("3")
        c3.add_css_class("active")
        self.pulse = dot("wait", True, 8)
        self.status = lbl("Requesting a code…", wrap=True, hexpand=True)
        self.status_box = hbox(10, "qlstatus", self.pulse, self.status)
        step3 = self._step(c3, "Approve the sign-in",
                           "The account shows up in your list automatically.",
                           self.status_box, last=True)

        self.build(vbox(0, "stepper", step1, step2, step3),
                   hbox(0, "mfoot", Gtk.Box(hexpand=True),
                        btn("Cancel", "cancel", self.close)))
        self.connect("closed", self.on_closed)
        GLib.timeout_add_seconds(1, self._tick)
        self.request_code()

    # -- pieces -------------------------------------------------------------
    @staticmethod
    def _circle(n):
        return Gtk.Label(label=n, css_classes=["circle"], valign=Gtk.Align.START)

    @staticmethod
    def _step(circle, title, helper, content, last=False):
        rail = vbox(0, "", circle, halign=Gtk.Align.CENTER, width_request=26)
        if not last:
            rail.append(Gtk.Box(css_classes=["connector"], vexpand=True,
                                halign=Gtk.Align.CENTER))
        body = vbox(4, "stepbody", lbl(title, "steptitle"), hexpand=True)
        if helper:
            body.append(lbl(helper, "stephelp", wrap=True))
        else:
            body.set_spacing(12)
        body.append(content)
        return hbox(14, "", rail, body)

    @staticmethod
    def _done(circle):
        circle.set_label("check")
        circle.add_css_class("ms")
        circle.add_css_class("bold")
        circle.add_css_class("done")

    def _copy(self, text, toast):
        copy_to_clipboard(self, text)
        self.parent.toast(toast)

    def _set_status(self, text, error=False):
        self.status.set_label(text)
        self.pulse.set_visible(not error)
        (self.status_box.add_css_class if error
         else self.status_box.remove_css_class)("error")

    # -- handlers -----------------------------------------------------------
    def on_open_page(self):
        # The bare confirmation page: no code in the query, nothing prefilled.
        Gio.AppInfo.launch_default_for_uri(QL_CONFIRM_URL, None)
        self._done(self.c1)

    def on_copy(self):
        if not self.code:
            return
        copy_to_clipboard(self, self.code)
        self._done(self.c2)
        self.copy.icon.set_label("check")
        self.copy.text.set_label("Copied")
        self.copy.add_css_class("copied")
        if self._copied_timer:
            GLib.source_remove(self._copied_timer)

        def reset():
            self._copied_timer = None
            self.copy.icon.set_label("content_copy")
            self.copy.text.set_label("Copy")
            self.copy.remove_css_class("copied")
            return GLib.SOURCE_REMOVE

        self._copied_timer = GLib.timeout_add(1600, reset)

    def on_closed(self, *_a):
        self.closed = True
        self._hold(False)

    def _tick(self):
        if self.closed:
            return GLib.SOURCE_REMOVE
        if self.expires:
            left = max(0, int(self.expires - time.monotonic()))
            self.expiry.set_label(f"Expires in {left // 60}:{left % 60:02d}")
            self.bar.set_fraction(left / QL_TIMEOUT)
            (self.bar.add_css_class if left < 60 else self.bar.remove_css_class)("low")
        return GLib.SOURCE_CONTINUE

    def show_code(self, gen, code):
        if gen != self.gen:
            return
        self.code = code
        half = (len(code) + 1) // 2
        self.code_a.set_label(code[:half])
        self.code_b.set_label(code[half:])
        self.expires = time.monotonic() + QL_TIMEOUT
        self._tick()
        self._set_status("Waiting for approval…")

    def show_status(self, gen, status):
        if gen == self.gen and status == "UserLinked":
            self._set_status("Code entered -- now approve the sign-in at Roblox")

    def show_error(self, msg):
        """Keep the dialog open and say what went wrong, on the dialog itself.

        A failure after approval used to close the dialog exactly like a success
        did, leaving the reason in a dim caption on the main window -- which read
        as "it worked but no account appeared". The dialog stays up now, and
        "New code" starts over without reopening anything.
        """
        self.expires = 0
        self._set_status(msg, error=True)

    # -- the flow -----------------------------------------------------------
    def request_code(self):
        """Ask Roblox for a code -- on opening, on "New code", and when the
        last one expired. A worker still polling an older code sees it is
        stale at its next poll and ends without a word."""
        self.gen += 1
        self.code = ""
        self.expires = 0
        self.code_a.set_label("···")
        self.code_b.set_label("···")
        self.expiry.set_label("Expires in –:––")
        self.bar.set_fraction(1)
        self._set_status("Requesting a code…")
        self._hold(True)
        threading.Thread(target=self._work, args=(self.gen,), daemon=True).start()

    def _stale(self, gen):
        return self.closed or gen != self.gen

    def _hold(self, on):
        """Hold the window's busy count at most once. The worker finishing and
        the dialog closing both release it, and only the first may count."""
        if on != self.holding:
            self.holding = on
            self.parent.set_busy(on)

    def _worker_done(self, gen):
        if not self._stale(gen) or self.closed:
            self._hold(False)

    def _work(self, gen):
        name = self.relogin["name"] if self.relogin else "new account"
        try:
            cookie, user = quick_login(
                log=self.parent.log,
                on_code=lambda code: once(self.show_code, gen, code),
                on_status=lambda status: once(self.show_status, gen, status),
                cancelled=lambda: self._stale(gen),
            )
            want = self.relogin and self.relogin["user_id"]
            if want and user.get("id") != want:
                # Storing it would silently turn this label into someone else.
                raise RuntimeError(
                    f"that code was approved by {user.get('name')}, not the "
                    f"account '{name}' belongs to -- sign in as that user")
            # The same Roblox user added twice would share one Cordial profile,
            # so a second approval refreshes the account that is already here.
            existing = self.relogin or next(
                (a for a in self.parent.accounts if a.get("user_id") == user.get("id")),
                None)
            name = existing["name"] if existing else unique_label(
                user.get("name"), {a["name"] for a in self.parent.accounts})
            secret_store(name, cookie)
        except CodeExpired:
            if not self._stale(gen):
                once(self.request_code)
            return
        except Exception as e:
            if self.closed:
                once(self.close)          # the user closed it; stay quiet
            elif not self._stale(gen):
                # Report on the dialog, to stderr and as a toast. Closing here is
                # what made a failure indistinguishable from a success.
                print(f"rbxmgr: add '{name}' failed: {e}", file=sys.stderr)
                self.parent.log(f"Could not add '{name}': {e}")
                self.parent.toast(f"Could not add '{name}'")
                once(self.show_error, str(e))
            return
        finally:
            once(self._worker_done, gen)
        if existing:
            once(self.parent.relogged, existing, user)
        else:
            once(self.parent.add_account, name, user)
        once(self.close)


class GameBar(Gtk.Box):
    """The accounts' favourite games as tiles, after the games browser and
    Join a friend.

    This is what the Place ID entry used to be: the only places on offer are
    ones an account already favourited, so there is nothing to mistype and no
    numeric id to look up. The games browser is the empty place id the
    launcher already understood.
    """

    def __init__(self, window):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.window = window
        self.place = ""
        self.tiles = wrap(spacing=18)
        self.append(self.tiles)
        self.hint = lbl("", "phint2", wrap=True)
        self.append(self.hint)
        # Deliberately left empty: the window is still part way through
        # building this widget and has no games yet. It fills it from
        # show_games() once it is done.

    def place_id(self):
        """"" for the games browser, otherwise the selected place."""
        return self.place

    def set_games(self, games, selected=""):
        """Redraw the tiles. Main thread only."""
        w = self.window
        if selected is not None:
            self.place = selected if any(g["place_id"] == selected
                                         for g in games) else ""
        while (c := self.tiles.get_first_child()) is not None:
            self.tiles.remove(c)
        friend = w.friend
        uses = {}
        for g in w.groups:
            if g.get("place_id"):
                uses.setdefault(g["place_id"], []).append(g["name"] or "Untitled group")
        self.tiles.append(self._tile("Games browser", "Opens Roblox's list",
                                     "travel_explore", None, "dashed",
                                     not friend and not self.place,
                                     lambda: self.select(""),
                                     "Open Roblox's own games browser"))
        f_name = friend["display"] if friend else "Join a friend"
        self.tiles.append(self._tile(
            f_name, f"In {friend['game']}" if friend else "Pick from a friends list",
            "person_search", None, "friend" if friend else "dashed", bool(friend),
            w.on_friends, "Send accounts into a friend's server"))
        for g in games:
            self.tiles.append(self._tile(
                g["name"], ", ".join(uses.get(g["place_id"], [])) or "Favourite",
                None, g.get("icon"), "", not friend and self.place == g["place_id"],
                lambda p=g["place_id"]: self.select(p), g["name"]))
        self.hint.set_visible(not games)
        self.hint.set_label("No favourites yet -- favourite a game on roblox.com, "
                            "then refresh.")

    def _tile(self, name, meta, ic, icon_path, css, selected, on_click, tooltip):
        thumb = game_thumb(icon_path, 128, f"gthumb {css}", ic=ic, ic_size=36)
        if selected:
            (thumb.box if ic else thumb).add_css_class("sel")
        over = Gtk.Overlay(child=thumb)
        if selected:
            # Packed top-right inside a box that fills the overlay: placed by
            # the overlay itself, the badge got the tile-wide slot to paint.
            badge = icon("check", 16, "bold tilecheck", width_request=24,
                         height_request=24)
            over.add_overlay(vbox(0, "", hbox(0, "", badge, halign=Gtk.Align.END,
                                              margin_top=8, margin_end=8)))
        b = Gtk.Button(css_classes=["b", "gtile"], tooltip_text=tooltip,
                       width_request=128)
        b.set_cursor_from_name("pointer")
        b.set_child(vbox(10, "", over,
                         vbox(1, "",
                              lbl(name, "gname" + (" sel" if selected else ""),
                                  ellipsize=Pango.EllipsizeMode.END,
                                  max_width_chars=1),
                              lbl(meta, "gmeta", ellipsize=Pango.EllipsizeMode.END,
                                  max_width_chars=1))))
        b.connect("clicked", lambda _b: on_click())
        return b

    def select(self, place):
        self.place = place or ""
        self.window.pick_game(self.place)


class FriendsDialog(Modal):
    """Every friend of one account, with where they are. Join makes a friend's
    server the launch target: the launch buttons then send accounts there."""

    def __init__(self, window, of):
        super().__init__("person_search", "Join a friend",
                         "Selected accounts launch into their server", 520)
        self.window = window
        self.cache = {}
        self.of = of
        self.query = ""
        self.chips = wrap(spacing=6)
        search = Gtk.Entry(placeholder_text="Search friends", hexpand=True)
        search.connect("changed", lambda e: self._search(e.get_text()))
        self.list = vbox(2, "flist")
        self.summary = lbl("", "fsum", hexpand=True)
        self.build(
            vbox(0, "",
                 vbox(12, "fbody", self.chips,
                      hbox(8, "search", icon("search", 18), search)),
                 # A floor, not only a ceiling: the dialog is sized while the
                 # list is still a spinner and does not grow when it fills.
                 Gtk.ScrolledWindow(child=self.list, min_content_height=280,
                                    max_content_height=340,
                                    propagate_natural_height=True,
                                    hscrollbar_policy=Gtk.PolicyType.NEVER)),
            hbox(10, "mfoot", self.summary, btn("Close", "cancel", self.close)))
        self.load(of)

    def load(self, acct):
        self.of = acct
        while (c := self.chips.get_first_child()) is not None:
            self.chips.remove(c)
        self.chips.append(lbl("Friends of", "fof", valign=Gtk.Align.CENTER))
        for a in self.window.accounts:
            self.chips.append(btn(a["name"], "fchip" + (" on" if a is acct else ""),
                                  lambda a=a: self.load(a)))
        key = acct["user_id"]
        if key in self.cache:
            self._show()
            return
        clear(self.list)
        self.list.append(Gtk.Spinner(spinning=True, halign=Gtk.Align.CENTER,
                                     margin_top=24, margin_bottom=24))
        self.summary.set_label("Loading…")

        def work():
            try:
                got = friends_status(secret_lookup(acct["name"]), acct["user_id"])
            except Exception as e:
                got = str(e)
            once(self._loaded, key, got)

        threading.Thread(target=work, daemon=True).start()

    def _loaded(self, key, got):
        self.cache[key] = got
        if self.of["user_id"] == key:
            self._show()

    def _search(self, text):
        self.query = text.strip().lower()
        self._show()

    def _show(self):
        got = self.cache.get(self.of["user_id"])
        clear(self.list)
        if isinstance(got, str):
            self.summary.set_label("")
            self.list.append(lbl(f"Could not load friends: {got}", "nofriends",
                                 wrap=True, xalign=.5))
            return
        n_game = sum(1 for f in got if f["state"] == "game")
        n_on = sum(1 for f in got if f["state"] == "online")
        self.summary.set_label(f"{n_game} in game · {n_on} online")
        shown = [f for f in got if not self.query
                 or self.query in f["name"].lower() or self.query in f["display"].lower()]
        chosen = self.window.friend
        for f in shown:
            state = f["state"]
            picked = bool(chosen) and chosen["user_id"] == f["user_id"]
            status = (f"In {f['game']}" + ("" if f["job_id"] else " · server hidden")
                      if state == "game" else state.capitalize())
            row = hbox(12, "friend" + (" sel" if picked else ""),
                       Gtk.Box(css_classes=["fdot", state], valign=Gtk.Align.CENTER),
                       vbox(2, "",
                            lbl(f["display"], "frname " + state,
                                ellipsize=Pango.EllipsizeMode.END),
                            lbl(status, "frstatus " + state,
                                ellipsize=Pango.EllipsizeMode.END),
                            hexpand=True, valign=Gtk.Align.CENTER))
            row.set_tooltip_text(f"@{f['name']}")
            if state == "game" and f["job_id"]:
                row.append(btn("Selected" if picked else "Join",
                               "join" + (" chosen" if picked else ""),
                               lambda f=f: self._join(f), ic="check" if picked else "login",
                               size=17, gap=5, valign=Gtk.Align.CENTER))
            self.list.append(row)
        if not shown:
            self.list.append(lbl("No friends match that search" if self.query
                                 else "No friends yet", "nofriends", xalign=.5))

    def _join(self, friend):
        self.close()
        self.window.join_friend(friend)


class Window(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app, title="Roblox Manager",
                         default_width=1320, default_height=860)
        self.add_css_class("rbx")
        self.accounts = load_accounts()
        self.groups = load_groups()
        if self.groups is None:
            migrate_layout(self.accounts)
            self.groups = []
            self.persist()
            self.save_groups()
        self.macros = load_macros()
        self.macros_off = disabled_macros() & set(self.macros)
        self.hotkeys = {n: k for n, k in macro_hotkeys().items() if n in self.macros}
        self.macro_runs = {}        # account name -> (Event that stops it, macro)
        self.busy = False
        self._busy_count = 0
        self.running = set()        # account names with a live client, polled
        self.launching = set()      # account names a launch is starting now
        self.joining = set()        # ...of those, the ones following a leader
        self._polling = False
        self.activity = []          # [(time, message)], newest first
        self.open_accounts = set()  # rows showing their settings
        self.open_macros = set()    # cards showing their steps
        self.edit_group = None      # the group showing its settings
        self.ungrouped_open = True
        self.friend = None          # a friend's server, the launch target
        self.game_list = []         # favourites, as the tiles show them
        self._rows = []
        self._cards = {}
        GLib.timeout_add_seconds(2, self._poll_running)

        # -- title bar ------------------------------------------------------
        self.pill_dot = hbox(0, "", valign=Gtk.Align.CENTER)
        self.pill_label = Gtk.Label(label="All idle")
        self.pill = hbox(7, "pill", self.pill_dot, self.pill_label,
                         valign=Gtk.Align.CENTER)
        self.upd = btn("Update Roblox", "tbtn upd", self.on_update_roblox,
                       "Pull the latest Roblox client", ic="download",
                       valign=Gtk.Align.CENTER)
        bar = hbox(12, "titlebar",
                   Gtk.Image(icon_name="roblox-manager-mark", pixel_size=28),
                   lbl("Roblox Manager", "apptitle"), self.pill,
                   Gtk.Box(hexpand=True),
                   self.upd,
                   btn("Add account", "tbtn", self.on_add,
                       "Add an account with Roblox Quick Login", ic="person_add",
                       valign=Gtk.Align.CENTER),
                   Gtk.Box(css_classes=["vdiv"], valign=Gtk.Align.CENTER),
                   btn(None, "ibtn reload", self.refresh_all,
                       "Check every session and reload favourites", ic="refresh",
                       size=20, valign=Gtk.Align.CENTER),
                   btn(None, "ibtn close", self.close, "Close", ic="close", size=20,
                       valign=Gtk.Align.CENTER))

        # -- left: game and accounts -----------------------------------------
        self.games = GameBar(self)
        self.select_all = btn("Select all", "textbtn", self.on_select_all,
                              ic="done_all")
        self.accounts_box = vbox(12, "")
        left = vbox(32, "left",
                    vbox(18, "",
                         section("videogame_asset", "Game",
                                 "Leader's favourites, or open the games browser",
                                 btn(None, "ibtn reload", self.reload_games,
                                     "Refresh favourites", ic="refresh", size=20,
                                     valign=Gtk.Align.CENTER)),
                         self.games),
                    vbox(18, "",
                         section("group", "Accounts",
                                 "Make one the leader in its settings · drag "
                                 "accounts onto it to auto-join after it",
                                 hbox(6, "", self.select_all,
                                      btn("New group", "tbtn plain", self.add_group,
                                          ic="create_new_folder"),
                                      valign=Gtk.Align.CENTER)),
                         self.accounts_box),
                    hexpand=True)

        # -- right: macros and activity --------------------------------------
        self.cards = vbox(16, "")
        self.log_box = vbox(8, "")
        right = vbox(16, "right",
                     section("keyboard", "Macros", "Hotkey-triggered input sequences",
                             btn("New", "hbtn", lambda: MacroDialog(self).present(self),
                                 ic="add", gap=4, valign=Gtk.Align.CENTER)),
                     self.cards, Gtk.Box(vexpand=True),
                     vbox(10, "activity",
                          hbox(6, "acthead", icon("history", 16), lbl("Activity")),
                          self.log_box),
                     width_request=380)
        # A row of boxes sizes each column's width for the height on offer,
        # and wrapped text in a column taller than the window then asks for
        # more width. A vertical scroller measures it at no fixed height and
        # reports its minimum, so it holds the design's width; the page itself
        # scrolls, so this one never has to.
        right = Gtk.ScrolledWindow(child=right, propagate_natural_height=True,
                                   hscrollbar_policy=Gtk.PolicyType.NEVER,
                                   hexpand=False)
        self.body = hbox(0, "", left, right)

        # -- action bar -----------------------------------------------------
        self.summary = lbl("", "summary", ellipsize=Pango.EllipsizeMode.END)
        self.target_text = lbl("", "", ellipsize=Pango.EllipsizeMode.END)
        self.btn_each = btn("Launch selected", "big second", self.launch_selected,
                            "Each selected account into the target", ic="play_arrow",
                            size=20, fill=True, gap=7)
        self.btn_group = btn("Launch as group", "big primary", self.launch_chain,
                             "The leader first; auto-join accounts follow into "
                             "its server", ic="groups", size=20, fill=True, gap=8)
        actions = hbox(12, "actionbar",
                       vbox(1, "", self.summary,
                            hbox(4, "target", icon("arrow_forward", 14),
                                 self.target_text),
                            hexpand=True, valign=Gtk.Align.CENTER),
                       hbox(8, "",
                            btn("Stop all", "big stopall", self.on_stop_all,
                                "Close every account's client and stop every macro",
                                ic="stop_circle", size=20, gap=7),
                            self.btn_each, self.btn_group))

        self.toasts = Adw.ToastOverlay(child=Gtk.ScrolledWindow(
            child=self.body, vexpand=True, hscrollbar_policy=Gtk.PolicyType.NEVER))
        toolbar = Adw.ToolbarView(content=self.toasts)
        toolbar.add_top_bar(Gtk.WindowHandle(child=bar))
        toolbar.add_bottom_bar(actions)
        self.set_content(toolbar)

        # Narrow window: the macros column wraps below the accounts.
        narrow = Adw.Breakpoint.new(Adw.BreakpointCondition.parse("max-width: 960sp"))
        narrow.add_setter(self.body, "orientation",
                          GObject.Value(Gtk.Orientation, Gtk.Orientation.VERTICAL))
        self.add_breakpoint(narrow)

        self.shortcuts = Gtk.ShortcutController()
        self.add_controller(self.shortcuts)

        self.refresh()
        self._log(time.strftime("%H:%M"), "Ready")
        self.run_task(lambda: migrate_flatpak_cordial(self.log),
                      failure="Could not move the old Cordial profiles")

    # -- state ------------------------------------------------------------
    def _last_place(self):
        for a in self.accounts:
            if a.get("last_place"):
                return str(a["last_place"])
        return ""

    def persist(self):
        save_accounts(self.accounts)

    def save_groups(self):
        save_groups(self.groups)

    def log(self, msg):
        once(self._log, time.strftime("%H:%M"), msg)

    def _log(self, stamp, msg):
        self.activity = [(stamp, msg)] + self.activity[:3]
        clear(self.log_box)
        for t, m in self.activity:
            kind = log_kind(m)
            line = hbox(8, "",
                        icon(LOG_KINDS[kind], 16, f"k-{kind}"),
                        lbl(m, "logline", hexpand=True,
                            ellipsize=Pango.EllipsizeMode.END, max_width_chars=1),
                        lbl(t, "logtime mono"))
            line.set_tooltip_text(m)
            self.log_box.append(line)

    def toast(self, msg):
        # Plain text: messages carry labels the user typed and error text such as
        # "<urlopen error ...>", which Adw.Toast would otherwise parse as markup.
        once(self.toasts.add_toast, Adw.Toast(title=msg, use_markup=False))

    def state_of(self, acct):
        name = acct["name"]
        if name in self.running:
            return "running"
        if name in self.joining:
            return "joining"
        if name in self.launching:
            return "starting"
        return "expired" if acct.get("session") == "expired" else "idle"

    def leader(self):
        return leader_of(self.accounts)

    def visual(self):
        return visual_order(self.accounts, self.groups)

    def selected_accounts(self):
        """Leader first, then in the drawn order."""
        ld = self.leader()
        return [a for a in ([ld] if ld else []) + self.visual()
                if a.get("selected", True)]

    def selected_names(self):
        return [a["name"] for a in self.selected_accounts()]

    def _acct(self, name):
        return next((a for a in self.accounts if a["name"] == name), None)

    def _changed(self):
        """Layout or selection changed: save, redraw the accounts."""
        self.persist()
        self.refresh_accounts()

    def set_leader(self, name):
        make_leader(self.accounts, name)
        self._changed()
        self.log(f"Leader set to {name}")

    def set_follow(self, name, on):
        set_follow(self.accounts, name, on)
        self._changed()

    def move_follower(self, name, delta):
        move_follower(self.accounts, name, delta)
        self._changed()

    def set_group(self, name, gid):
        acct = self._acct(name)
        if acct is None:
            return
        if acct.get("leader"):
            self.toast("The leader can't be in a group")
            return
        acct["group"] = gid
        # Dropped at the end of its new group.
        self.accounts.remove(acct)
        self.accounts.append(acct)
        self._changed()

    def drop_on_row(self, name, onto):
        drop_account(self.accounts, self.groups, name, onto)
        self._changed()

    def toggle_selected(self, name):
        acct = self._acct(name)
        acct["selected"] = not acct.get("selected", True)
        self._changed()

    def select_accounts(self, accts, on):
        for a in accts:
            a["selected"] = on
        self._changed()

    def on_select_all(self):
        self.select_accounts(self.accounts,
                             not all(a.get("selected", True) for a in self.accounts))

    def toggle_account(self, name):
        self.open_accounts ^= {name}
        self.refresh_accounts()

    # -- groups -----------------------------------------------------------
    def add_group(self):
        gid = f"g{int(time.time() * 1000)}"
        self.groups.append({"id": gid, "name": "New group", "place_id": None,
                            "open": True})
        self.edit_group = gid
        self.save_groups()
        self.refresh_accounts()

    def delete_group(self, gid):
        g = next((g for g in self.groups if g["id"] == gid), None)
        self.groups = [x for x in self.groups if x["id"] != gid]
        for a in self.accounts:
            if a.get("group") == gid:
                a.pop("group")
        self.edit_group = None
        self.save_groups()
        self._changed()
        self.show_games()
        self.log(f"Deleted group {(g and g['name']) or 'Untitled group'}")

    def toggle_group_open(self, gid):
        if gid is None:
            self.ungrouped_open = not self.ungrouped_open
        else:
            for g in self.groups:
                if g["id"] == gid:
                    g["open"] = not g.get("open", True)
            self.save_groups()
        self.refresh_accounts()

    def toggle_group_edit(self, gid):
        self.edit_group = None if self.edit_group == gid else gid
        self.refresh_accounts()

    def set_group_game(self, gid, place, name):
        for g in self.groups:
            if g["id"] == gid:
                g["place_id"], g["game"] = place, name if place else None
        self.save_groups()
        self.refresh_accounts()
        self.show_games()

    def game_by_place(self, place):
        return next((g for g in self.game_list if g["place_id"] == place), None)

    # -- drawing ----------------------------------------------------------
    def refresh(self):
        self.show_games()
        self.refresh_accounts()
        self.refresh_macros()

    def refresh_accounts(self):
        clear(self.accounts_box)
        self._rows = []
        if not self.accounts:
            self.accounts_box.append(
                hbox(8, "dashedbox", icon("person_add", 16),
                     lbl("No accounts yet -- add one with Add account; it signs in "
                         "with Roblox Quick Login.", wrap=True),
                     halign=Gtk.Align.FILL))
            self.refresh_launch_state()
            return
        ld = self.leader()
        card = vbox(0, "card lead")
        card.set_overflow(Gtk.Overflow.HIDDEN)
        self.target_strip = lbl("", ellipsize=Pango.EllipsizeMode.END,
                                max_width_chars=34)
        card.append(hbox(8, "lstrip",
                         icon("star", 16, "amber", fill=True),
                         lbl("LEADER", "ltitle"),
                         lbl("Launches first. Linked accounts auto-join its server.",
                             "ldesc", hexpand=True, ellipsize=Pango.EllipsizeMode.END),
                         hbox(5, "ltarget", icon("arrow_forward", 14),
                              self.target_strip)))
        if ld:
            row = AccountRow(self, ld, first=True)
            self._rows.append(row)
            card.append(row)
        else:
            card.append(hbox(8, "noleader", icon("star", 16),
                             lbl("No leader yet. Make an account the leader from "
                                 "its settings."), halign=Gtk.Align.CENTER))
        fs = followers_of(self.accounts)
        foot = vbox(6, "ffoot",
                    hbox(6, "fhead", icon("link", 15), lbl("Auto-join after leader"),
                         lbl(f"· {len(fs)} account{'s' * (len(fs) != 1)}" if fs
                             else "", "fcount")))
        for i, a in enumerate(fs, 1):
            fr = FollowerRow(self, a, i, len(fs))
            self._rows.append(fr)
            foot.append(fr)
        if not fs:
            foot.append(hbox(8, "dashedbox", icon("add_link", 16),
                             lbl("Drag accounts here, or turn on Auto-join in an "
                                 "account's settings", wrap=True),
                             halign=Gtk.Align.FILL))
        card.append(foot)
        drop = Gtk.DropTarget.new(GObject.TYPE_STRING, Gdk.DragAction.MOVE)
        drop.connect("drop", lambda _t, v, _x, _y: (
            ld is not None and self.set_follow(v, True), True)[-1])
        card.add_controller(drop)
        self.accounts_box.append(card)

        vis = self.visual()
        for g in self.groups + [None]:
            gid = g["id"] if g else None
            members = [a for a in vis if group_of(a, self.groups) == gid]
            if g is None and not members:
                continue
            gc = GroupCard(self, g, members)
            self._rows.append(gc)
            self.accounts_box.append(gc)
        self.refresh_launch_state()

    def show_games(self):
        """Draw the bar from what is already on disk: no network, and no keyring
        prompt just for opening the app. Refresh is the deliberate click."""
        self.game_list = [dict(g, icon=cached_icon(g.get("universe_id")))
                          for g in merge_favorites(self.accounts)]
        # A redraw (an account added, a launch finished) must not move the
        # user's pick out from under them, so the current tile wins.
        self.games.set_games(self.game_list, self.games.place_id() or self._last_place())

    def pick_game(self, place):
        self.friend = None
        self.games.set_games(self.game_list, place)
        self.refresh_launch_state()

    def reload_games(self):
        if not self.accounts:
            self.log("Add an account first -- favourites come from your accounts")
            return
        accounts = [dict(a) for a in self.accounts]   # the worker's own copy

        def apply(fresh, games, ok):
            # Back on the main loop, where persist() also runs.
            for a in self.accounts:
                if a["name"] in fresh:
                    a["favorites"] = fresh[a["name"]]
            self.persist()
            self.game_list = games
            self.games.set_games(games, self.games.place_id())
            self.refresh_accounts()
            self.log(f"{len(games)} game(s) from {ok}/{len(accounts)} account(s)")

        def work():
            fresh = {}
            for acct in accounts:
                name = acct["name"]
                try:
                    acct["favorites"] = fresh[name] = favorite_games(
                        secret_lookup(name), acct["user_id"])
                except Exception as e:
                    # One account failing must not blank the bar: its
                    # last-known favourites stay in the merge.
                    self.log(f"Could not load {name}'s favourites: {e}")
            games = with_icons(merge_favorites(accounts))
            once(apply, fresh, games, len(fresh))

        self.run_task(work, failure="Could not reload favourites")

    def target(self):
        """(place_id, job_id): a friend's server, or the picked game."""
        if self.friend:
            return self.friend["place_id"], self.friend["job_id"]
        return self.games.place_id(), None

    def target_label(self):
        if self.friend:
            return f"Join {self.friend['display']} · {self.friend['game']}"
        g = self.game_by_place(self.games.place_id())
        return g["name"] if g else "Games browser"

    def refresh_launch_state(self):
        chosen = self.selected_accounts()
        n = len(chosen)
        ld = self.leader()
        fs = followers_of(self.accounts)
        self.btn_each.set_sensitive(n >= 1)
        self.btn_each.text.set_label(f"Launch selected ({n})")
        self.btn_group.set_sensitive(ld is not None)
        self.summary.set_label(f"Leader {ld['name']} · {len(fs)} auto-join" if ld
                               else "No leader set")
        self.target_text.set_label(self.target_label())
        if getattr(self, "target_strip", None) is not None:
            self.target_strip.set_label(self.target_label())
        every = bool(self.accounts) and n == len(self.accounts)
        self.select_all.text.set_label("Deselect all" if every else "Select all")
        self.select_all.icon.set_label("remove_done" if every else "done_all")
        for card in self._cards.values():
            card.update_meta(n)
        self._update_pill()

    def _update_pill(self):
        n = len(self.running)
        self.pill_label.set_label(f"{n} running" if n else "All idle")
        (self.pill.add_css_class if n else self.pill.remove_css_class)("on")
        clear(self.pill_dot)
        self.pill_dot.append(dot("running" if n else "idlepill", bool(n)))

    def set_busy(self, busy):
        """Counted, not a flag: a launch, a reload and a login can overlap, and
        the first to finish must not end the spin under the others."""
        self._busy_count = max(0, self._busy_count + (1 if busy else -1))
        self.busy = self._busy_count > 0
        (self.add_css_class if self.busy else self.remove_css_class)("busy")

    # -- launching --------------------------------------------------------
    def _remember_place(self, pid):
        for a in self.accounts:
            a["last_place"] = pid
        self.persist()

    def launch_selected(self):
        names = self.selected_names()
        if not names:
            self.log("No accounts selected")
            return
        self.launch_names(names, "each", self.target())

    def launch_chain(self):
        """Launch as group: the leader, then its auto-join list into its
        server. With a friend as the target everyone goes to the friend's
        server, which is the same one."""
        ld = self.leader()
        if ld is None:
            self.log("No leader set -- make an account the leader first")
            return
        names = [ld["name"]] + [a["name"] for a in followers_of(self.accounts)]
        if self.friend:
            self.launch_names(names, "each", self.target())
        else:
            self.launch_names(names, "group")

    def launch_group(self, gid):
        g = next((g for g in self.groups if g["id"] == gid), None)
        if not g or not g.get("place_id"):
            return
        names = [a["name"] for a in self.visual() if group_of(a, self.groups) == gid]
        if names:
            self.log(f"Launching {g['name'] or 'group'} · {g.get('game') or g['place_id']}")
            self.launch_names(names, "each", (g["place_id"], None))

    def play_account(self, acct):
        """A row's play: the leader starts its chain; an auto-join account
        joins the leader when that is already running; anyone else goes to
        the target on their own."""
        ld = self.leader()
        if acct is ld:
            self.launch_chain()
        elif acct.get("follow") and ld and ld["name"] in self.running \
                and not self.friend:
            self.launch_names([ld["name"], acct["name"]], "group")
        else:
            self.launch_names([acct["name"]], "each", self.target())

    def launch_names(self, names, mode, target=None):
        """Start a launch at once, whatever else is running. Only an account
        another launch is still starting is left out -- starting it twice
        would have its second client refused by Cordial's profile lock.

        target: (place_id, job_id) to send everyone to -- a friend's server
        or a group's game -- instead of the game picked in the bar."""
        busy = [n for n in names if n in self.launching]
        names = [n for n in names if n not in self.launching]
        for n in busy:
            self.log(f"{n}: already launching -- skipped")
        if not names:
            return
        self.launching.update(names)
        joining = set(names[1:]) if mode == "group" else set()
        self.joining.update(joining)
        self._states()
        pid, job = target or (self.games.place_id(), None)
        if not target:
            self._remember_place(pid)
        by_name = {a["name"]: a for a in self.accounts}

        def launched(name, user):
            # On the main loop: persist() may be json-dumping these very dicts
            # there, and a new key mid-dump raises.
            acct = by_name.get(name)
            if acct is None:
                return
            now = datetime.now(timezone.utc).isoformat(timespec="seconds")
            acct["last_launch"] = acct["session_checked"] = now
            acct.pop("session", None)
            acct["username"] = user.get("name") or acct.get("username")
            if pid:
                plays = acct.setdefault("plays", {})
                plays[pid] = plays.get(pid, 0) + 1

        def is_running(name):
            return (cordial_profile(by_name[name]["user_id"])
                    in cordial_clients().values())

        build = {}

        def do_spawn(name, url):
            cookie = secret_lookup(name)
            try:
                user = authenticated_user(cookie)
            except SessionExpired:
                once(self.mark_session, name, "expired")
                raise
            profile = seed_cordial_profile(user, cookie)
            acct = by_name[name]
            launch_client(profile, url, build["got"],
                          nested=bool(acct.get("nested")),
                          low_power=bool(acct.get("low_power")))
            once(launched, name, user)

        def work():
            build["got"] = roblox_build(self.log)
            if mode == "group":
                follow_leader(
                    names, pid,
                    get_job_id=lambda n: leader_presence(
                        secret_lookup(n), by_name[n]["user_id"]),
                    do_spawn=do_spawn, log=self.log, is_running=is_running,
                )
            else:
                launch_each(
                    names, pid,
                    do_spawn=do_spawn, log=self.log, is_running=is_running,
                    job_id=job,
                )

        def done():
            self.launching.difference_update(names)
            self.joining.difference_update(joining)
            self.persist()
            self.refresh_accounts()

        self.run_task(work, done, "Launch failed")

    def on_friends(self):
        of = self.leader() or next(iter(self.selected_accounts()), None) \
            or (self.accounts[0] if self.accounts else None)
        if of is None:
            self.log("Add an account first -- friends come from your accounts")
            return
        FriendsDialog(self, of).present(self)

    def join_friend(self, friend):
        self.friend = friend
        self.games.set_games(self.game_list, None)
        self.refresh_launch_state()
        self.log(f"Target: join {friend['display']}")

    def run_task(self, work, done=None, failure="Failed"):
        """work() on a thread under the busy count; done() back on the main
        loop whatever happened."""
        self.set_busy(True)

        def run():
            try:
                work()
            except Exception as e:
                self.log(f"{failure}: {e}")
            finally:
                if done:
                    once(done)
                once(self.set_busy, False)

        threading.Thread(target=run, daemon=True).start()

    def _set_update(self, state):
        ic, text = {"idle": ("download", "Update Roblox"),
                    "busy": ("sync", "Updating…"),
                    "done": ("check_circle", "Up to date")}[state]
        self.upd.icon.set_label(ic)
        self.upd.text.set_label(text)
        for s in ("busy", "done"):
            (self.upd.add_css_class if s == state else self.upd.remove_css_class)(s)
        self.upd.set_sensitive(state != "busy")

    def on_update_roblox(self):
        """Install the newest Roblox build any source has. Launches install
        one when there is none, but only this moves to a newer one: Roblox
        turns old clients away, so this is the button for "the game says
        update". Running clients keep the build they started with."""
        self._set_update("busy")
        ok = []

        def work():
            roblox_build(self.log, newest=True)
            ok.append(True)
            self.log("Roblox is up to date")

        def done():
            if not ok:
                self._set_update("idle")
                return
            self._set_update("done")
            GLib.timeout_add(2600, lambda: self._set_update("idle") or GLib.SOURCE_REMOVE)

        self.run_task(work, done, failure="Could not update Roblox")

    def _poll_running(self):
        """Keeps each row's status and play/stop toggle honest, off the main
        loop: the pgrep is a subprocess."""
        if not self._polling:
            self._polling = True
            profiles = {cordial_profile(a["user_id"]): a["name"]
                        for a in self.accounts}

            def work():
                found = None
                try:
                    live = set(cordial_clients().values())
                    found = {n for p, n in profiles.items() if p in live}
                except Exception:
                    pass
                finally:
                    once(self._apply_running, found)

            threading.Thread(target=work, daemon=True).start()
        return GLib.SOURCE_CONTINUE

    def _apply_running(self, found):
        self._polling = False
        if found is None or found == self.running:
            return
        self.running = found
        self._states()

    def _states(self):
        for row in self._rows:
            row.set_state()
        self._update_pill()

    def _profiles(self, names):
        return {cordial_profile(a["user_id"]) for a in self.accounts
                if a["name"] in names}

    def stop_account(self, name):
        profiles = self._profiles({name})

        def work():
            n = stop_profiles(profiles)
            self.log(f"{name}: stopped" if n else f"{name}: was not running")

        threading.Thread(target=work, daemon=True).start()

    def on_stop_all(self):
        profiles = self._profiles({a["name"] for a in self.accounts})
        for stop, _macro in self.macro_runs.values():
            stop.set()

        def work():
            self.log(f"Stopped {stop_profiles(profiles)} client(s)")

        threading.Thread(target=work, daemon=True).start()

    # -- sessions ---------------------------------------------------------
    def mark_session(self, name, state):
        """state: "ok" (Roblox just took it), "expired", "checking", or None
        -- nothing known against it, and no new check time either."""
        acct = self._acct(name)
        if acct is None:
            return
        if state in ("expired", "checking"):
            acct["session"] = state
        else:
            acct.pop("session", None)
        if state == "ok":
            acct["session_checked"] = datetime.now(timezone.utc).isoformat(
                timespec="seconds")
        if state != "checking":
            self.persist()
        self.refresh_accounts()

    def check_sessions(self, names):
        """Ask Roblox whether each stored session still works. Only asks:
        a refused one says so on its row, and Sign in again is the fix."""
        before = {}
        for n in names:
            before[n] = (self._acct(n) or {}).get("session")
            self.mark_session(n, "checking")

        def work():
            for n in names:
                try:
                    authenticated_user(secret_lookup(n))
                    once(self.mark_session, n, "ok")
                except SessionExpired:
                    once(self.mark_session, n, "expired")
                    self.log(f"{n}: session expired -- sign in again")
                except Exception as e:
                    # Offline is not expired: the last verdict stands.
                    once(self.mark_session, n, before[n])
                    self.log(f"{n}: could not check the session: {e}")

        self.run_task(work, failure="Could not check sessions")

    def refresh_all(self):
        if self.accounts:
            self.check_sessions([a["name"] for a in self.accounts])
        self.reload_games()

    # -- macros -----------------------------------------------------------
    def show_macro_help(self):
        page = Adw.ToolbarView()
        page.add_top_bar(Adw.HeaderBar())
        text = Gtk.Label(label=MACRO_HELP, use_markup=True, wrap=True, xalign=0,
                         selectable=True, margin_top=6, margin_bottom=18,
                         margin_start=18, margin_end=18)
        page.set_content(Gtk.ScrolledWindow(child=text, propagate_natural_height=True,
                                            hscrollbar_policy=Gtk.PolicyType.NEVER))
        Adw.Dialog(title="How macros work", child=page, content_width=520,
                   content_height=640).present(self)

    def refresh_macros(self):
        clear(self.cards)
        self._cards = {n: MacroCard(self, n) for n in sorted(self.macros)}
        for card in self._cards.values():
            self.cards.append(card)
        if not self._cards:
            self.cards.append(lbl("No macros yet. A macro presses keys and clicks "
                                  "for an account on its own -- press New.",
                                  "mempty", wrap=True))
        self._bind_hotkeys()
        self.refresh_launch_state()

    def _bind_hotkeys(self):
        """In-app hotkeys: each toggles its macro on the selected accounts
        while this window has focus. From anywhere, bind a compositor key to
        `gapplication action io.github.mujo.RobloxManager run-macro "'NAME'"`."""
        while (s := self.shortcuts.get_item(0)) is not None:
            self.shortcuts.remove_shortcut(s)
        for name, accel in self.hotkeys.items():
            trigger = Gtk.ShortcutTrigger.parse_string(accel)
            if trigger is None:
                continue
            self.shortcuts.add_shortcut(Gtk.Shortcut.new(
                trigger, Gtk.CallbackAction.new(
                    lambda *_a, n=name: (self.run_macro_card(n), True)[-1])))

    def _save_macros(self):
        save_macros(self.macros, self.macros_off, self.hotkeys)

    def macros_running(self):
        return {macro for _stop, macro in self.macro_runs.values()}

    def toggle_macro(self, name):
        self.open_macros ^= {name}
        self.refresh_macros()

    def enable_macro(self, name, on):
        """A switched-off macro cannot run; switching one off stops it."""
        (self.macros_off.discard if on else self.macros_off.add)(name)
        self._save_macros()
        if not on:
            self._stop_macro(name)
        card = self._cards.get(name)
        if card:
            card.set_running()

    def _stop_macro(self, name):
        for stop, macro in list(self.macro_runs.values()):
            if macro == name:
                stop.set()

    def run_macro_card(self, name):
        """Run: the macro on every selected account. Stop: wherever it plays."""
        if name in self.macros_running():
            self._stop_macro(name)
            return
        if name in self.macros_off:
            self.toast(f"{name} is switched off")
            return
        chosen = self.selected_accounts()
        if not chosen:
            self.log("No accounts selected")
            return
        for acct in chosen:
            # A macro cannot reach a normal window once you look away.
            acct["nested"] = True
            self.start_macro(acct, name)
        self.persist()

    def pick_macro(self, acct, macro):
        acct["macro"] = macro
        if macro:
            # A macro cannot reach a normal window once you look away, so
            # wanting one means wanting this.
            acct["nested"] = True
        else:
            acct.pop("macro", None)
        self._changed()

    def play_macro_here(self, acct):
        name = acct["name"]
        if name in self.macro_runs:
            self.macro_runs[name][0].set()
        else:
            acct["nested"] = True
            self.persist()
            self.start_macro(acct, acct.get("macro"))

    def save_macro(self, old, new, text, hotkey):
        """The editor's Save. Returns what is wrong, or None once saved."""
        if not new:
            return "A macro needs a name"
        if new != old and new in self.macros:
            return f"A macro called {new} already exists"
        clash = next((n for n, k in self.hotkeys.items()
                      if hotkey and k == hotkey and n != old), None)
        if clash:
            return f"{hotkey_label(hotkey)} already runs {clash}"
        if old is not None and new != old:
            self.macros.pop(old)
            self.hotkeys.pop(old, None)
            if old in self.macros_off:
                self.macros_off.discard(old)
                self.macros_off.add(new)
            for a in self.accounts:
                if a.get("macro") == old:
                    a["macro"] = new
            self.open_macros.discard(old)
            self.persist()
        self.macros[new] = text
        if hotkey:
            self.hotkeys[new] = hotkey
        else:
            self.hotkeys.pop(new, None)
        if old is None:
            self.open_macros = {new}
        self._save_macros()
        self.refresh_macros()
        self.refresh_accounts()
        self.log(f"Saved {new}")
        return None

    def delete_macro(self, name):
        self._stop_macro(name)
        self.macros.pop(name, None)
        self.macros_off.discard(name)
        self.hotkeys.pop(name, None)
        for a in self.accounts:
            if a.get("macro") == name:
                a.pop("macro")
        self._save_macros()
        self.persist()
        self.refresh_macros()
        self.refresh_accounts()
        self.log(f"Deleted {name}")

    def _show_macro_state(self):
        for card in self._cards.values():
            card.set_running()
        self.refresh_accounts()

    def start_macro(self, acct, macro):
        name = acct["name"]
        if macro not in self.macros:
            self.toast(f"{name}: pick a macro first")
            return
        if macro in self.macros_off:
            self.toast(f"{macro} is switched off")
            return
        try:
            loops, steps = parse_macro(self.macros[macro])
        except ValueError as e:
            self.toast(f"{macro}: {e}")
            return
        # One macro per client: two typing into one display would interleave.
        if name in self.macro_runs:
            self.macro_runs[name][0].set()
        stop = threading.Event()
        entry = (stop, macro)
        self.macro_runs[name] = entry
        self._show_macro_state()
        profile = cordial_profile(acct["user_id"])
        self.log(f"{name}: playing {macro}")

        def work():
            try:
                run_macro(profile, loops, steps, stop,
                          report=lambda text: self.log(f"{name}: {macro} -- {text}"))
                self.log(f"{name}: {macro} {'stopped' if stop.is_set() else 'finished'}")
            except Exception as e:
                self.log(f"{name}: {macro} stopped -- {e}")
                self.toast(f"{name}: {macro} stopped -- {e}")
            finally:
                once(ended)

        def ended():
            if self.macro_runs.get(name) is entry:
                del self.macro_runs[name]
            self._show_macro_state()

        threading.Thread(target=work, daemon=True).start()

    # -- adding accounts --------------------------------------------------
    def on_add(self):
        AddAccountDialog(self).present(self)

    def add_account(self, name, user):
        """Called by the add dialog once Roblox has approved the code."""
        self.accounts.append({
            "name": name,
            "user_id": user["id"],
            "username": user.get("name"),
            "display": user.get("displayName") or user.get("name"),
            "note": "",
            "last_launch": None,
            "selected": True,
            "session_checked": datetime.now(timezone.utc).isoformat(timespec="seconds"),
        })
        if self.leader() is None:
            make_leader(self.accounts, name)
        self.persist()
        self.refresh()
        self.toast(f"Added {name}")
        self.log(f"Added {name} (@{user.get('name')})")

    def relogged(self, acct, user):
        """The add dialog's re-login: the keyring holds a fresh cookie now."""
        acct["display"] = user.get("displayName") or user.get("name")
        acct["username"] = user.get("name")
        self.mark_session(acct["name"], "ok")
        self.toast(f"Signed {acct['name']} in again")
        self.log(f"{acct['name']}: new session stored in the keyring")

    def rename_account(self, acct, new):
        """Rename the label, and everything keyed by it, on a worker thread."""
        old = acct["name"]
        if new == old:
            return
        if not valid_name(new):
            self.log(NAME_RULE)
            return
        if any(a["name"] == new for a in self.accounts):
            self.log(f"'{new}' already exists")
            return
        if self.busy:
            self.log("Wait for the current task to finish")
            self.refresh_accounts()
            return
        self.set_busy(True)

        def work():
            try:
                move_account_data(old, new)
            except Exception as e:
                self.log(f"Could not rename '{old}': {e}")
                once(self.refresh_accounts)   # put the old label back
                return
            finally:
                once(self.set_busy, False)
            once(self._renamed, acct, old, new)

        threading.Thread(target=work, daemon=True).start()

    def _renamed(self, acct, old, new):
        acct["name"] = new
        if old in self.open_accounts:
            self.open_accounts = (self.open_accounts - {old}) | {new}
        self.persist()
        self.refresh()
        self.toast(f"Renamed {old} → {new}")
        self.log(f"Renamed {old} → {new}")

    def remove_account(self, name):
        if self.busy:
            self.log("Wait for the current task to finish")
            return
        # Its client goes too: once the account is gone, Stop all no longer
        # knows its profile, so a client left running could not be stopped.
        # The session the manager gave its Cordial profile goes as well; the
        # empty profile directory stays.
        profiles = self._profiles({name})
        user_id = next(a["user_id"] for a in self.accounts if a["name"] == name)
        if name in self.macro_runs:
            self.macro_runs[name][0].set()

        def work():
            stop_profiles(profiles)
            clear_cordial_profile(user_id)

        threading.Thread(target=work, daemon=True).start()
        secret_clear(name)
        self.accounts = [a for a in self.accounts if a["name"] != name]
        set_follow(self.accounts, name, False)      # renumber the rest
        self.open_accounts.discard(name)
        self.persist()
        self.refresh()
        self.toast(f"Removed {name}")
        self.log(f"Removed {name}; cookie cleared from the keyring")


class App(Adw.Application):
    def __init__(self):
        super().__init__(application_id=SCHEMA)

    def do_startup(self):
        Adw.Application.do_startup(self)
        # The design is dark only; the stock widgets it leaves alone follow.
        Adw.StyleManager.get_default().set_color_scheme(Adw.ColorScheme.FORCE_DARK)
        Gtk.Window.set_default_icon_name("roblox-manager")
        css = Gtk.CssProvider()
        css.load_from_string(CSS)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        # A macro's hotkey from anywhere: bind a compositor key to
        #   gapplication action io.github.mujo.RobloxManager run-macro "'NAME'"
        act = Gio.SimpleAction.new("run-macro", GLib.VariantType.new("s"))
        act.connect("activate", lambda _a, p: self.props.active_window
                    and self.props.active_window.run_macro_card(p.get_string()))
        self.add_action(act)

    def do_activate(self):
        (self.props.active_window or Window(self)).present()


if __name__ == "__main__":
    sys.exit(App().run(sys.argv))
