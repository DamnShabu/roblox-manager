"""Roblox Manager's advanced macros.

A script in the advanced macros folder runs with this module already
imported, once for every account it is started on. Everything here goes
through the manager: keys and clicks reach the account's macro-ready window
the way a plain macro's do, and what the script sees of that window is a copy
of its frame the compositor hands over. Nothing reaches into the client.

    import rbxmgr as rb

    while True:
        coin = rb.find_center("coin")     # an image picked in the Macros tab
        if coin:
            rb.click(*coin)
        rb.tap("e")
        rb.wait(0.5)

print() goes to the manager's activity log. Stop in the manager ends the
script wherever it is; every key it left down is let go of.
"""

import json
import os
import struct
import sys
import zlib

__all__ = [
    "account", "user_id", "MacroError", "Frame",
    "tap", "hold", "key_down", "key_up", "release_all", "type_text",
    "click", "move_to", "move", "scroll", "wait",
    "find", "find_center", "wait_for", "pixel", "color_is", "frame", "images", "running",
]

# The manager talks over this process's stdin and stdout. They are taken
# here, before the script runs, and fd 1 becomes stderr: a print(), or a
# program the script starts, can never write into the conversation.
_requests = os.fdopen(os.dup(1), "wb", buffering=0)
_replies = os.fdopen(os.dup(0), "rb")
os.dup2(2, 1)
_null = os.open(os.devnull, os.O_RDONLY)
os.dup2(_null, 0)
os.close(_null)

#: The account this run plays into: its name in the manager, and its Roblox
#: user id.
account = os.environ.get("RBXMGR_ACCOUNT", "")
user_id = int(os.environ.get("RBXMGR_USER_ID", "0") or 0)


class MacroError(Exception):
    """The manager refused a request: a key it does not know, an image that
    was never picked, a window it cannot see."""


def _call(op, **args):
    args["op"] = op
    _requests.write((json.dumps(args) + "\n").encode())
    line = _replies.readline()
    if not line:
        # The manager stopped this run.
        raise SystemExit(0)
    reply = json.loads(line)
    if "error" in reply:
        raise MacroError(reply["error"])
    size = reply.get("bytes")
    if size is not None:
        data = _replies.read(size)
        if len(data) != size:
            raise SystemExit(0)
        return reply.get("ok"), data
    return reply.get("ok")


def _keys(keys):
    # tap("ctrl+w") and tap("ctrl", "w") are the same.
    names = []
    for k in map(str, keys):
        names.extend([k] if k == "+" else [p for p in k.split("+") if p])
    return names


def tap(*keys, secs=None):
    """Press the keys in order, hold them `secs` (a short tap by default),
    and let go in reverse. Keys are named as a plain macro names them:
    "e", "space", "shift", "f5", "mouse1"."""
    _call("tap", keys=_keys(keys), secs=secs)


def hold(*keys, secs):
    """Hold the keys down for `secs`, then let go."""
    _call("tap", keys=_keys(keys), secs=secs)


def key_down(*keys):
    """Press the keys and leave them down, until key_up or the run ends."""
    _call("press", keys=_keys(keys))


def key_up(*keys):
    _call("release", keys=_keys(keys))


def release_all():
    """Let go of every key and button still down."""
    _call("release_all")


def type_text(text):
    """Type `text` as a US keyboard would, a key at a time."""
    _call("type", text=str(text))


def click(x=None, y=None, button="mouse1"):
    """Click, at (x, y) from the window's top-left corner if given."""
    _call("click", button=button, x=x, y=y)


def move_to(x, y):
    """Put the pointer at (x, y)."""
    _call("move_to", x=int(x), y=int(y))


def move(dx, dy):
    """Raw mouse movement, as a mouse sends it: what turns a game's camera."""
    _call("move", dx=int(dx), dy=int(dy))


def scroll(notches, horizontal=False):
    """Turn the wheel: down (or right) is positive."""
    _call("scroll", notches=int(notches), horizontal=bool(horizontal))


def wait(secs):
    """Wait `secs`. Stop ends it at once."""
    _call("wait", secs=float(secs))


def _area(area):
    if area is None:
        return None
    x, y, w, h = area
    return [int(x), int(y), int(w), int(h)]


def find(image, area=None, least=0.9):
    """Where the picked image `image` is in the window now -- the (x, y) of
    its top-left corner -- or None. `area` is (x, y, w, h) to look only
    there; `least` is how alike it must be, 0 to 1."""
    at = _call("find", image=image, area=_area(area), least=float(least))
    return (at[0], at[1]) if at else None


def find_center(image, area=None, least=0.9):
    """find(), but the middle of the image where it is: the place to click."""
    at = _call("find", image=image, area=_area(area), least=float(least))
    return (at[0] + at[2] // 2, at[1] + at[3] // 2) if at else None


def wait_for(image, timeout=None, area=None, least=0.9, every=0.1):
    """find(), again every `every` seconds until the image shows; None if
    `timeout` seconds pass first."""
    waited = 0.0
    while True:
        at = find(image, area, least)
        if at or (timeout is not None and waited >= timeout):
            return at
        wait(every)
        waited += every


def pixel(x, y):
    """The (r, g, b) the window shows at (x, y)."""
    return tuple(_call("pixel", x=int(x), y=int(y)))


def color_is(x, y, rgb, within=24):
    """Whether the window shows `rgb` at (x, y), each channel within
    `within`."""
    return all(abs(a - b) <= within for a, b in zip(pixel(x, y), rgb))


class Frame:
    """A copy of the window, or of an area of it: `width` by `height` pixels
    whose top-left is (`x`, `y`) on the window, as red, green, blue bytes."""

    def __init__(self, x, y, width, height, rgb):
        self.x, self.y, self.width, self.height, self.rgb = x, y, width, height, rgb

    def pixel(self, x, y):
        """The (r, g, b) at (x, y) on the window, or None outside this
        frame."""
        x, y = x - self.x, y - self.y
        if not (0 <= x < self.width and 0 <= y < self.height):
            return None
        i = (y * self.width + x) * 3
        return tuple(self.rgb[i:i + 3])

    def save(self, path):
        """Write it as a PNG."""
        rows = b"".join(
            b"\x00" + self.rgb[r * self.width * 3:(r + 1) * self.width * 3]
            for r in range(self.height)
        )

        def chunk(kind, data):
            body = kind + data
            return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

        header = struct.pack(">IIBBBBB", self.width, self.height, 8, 2, 0, 0, 0)
        with open(path, "wb") as f:
            f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                    + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def frame(area=None):
    """A copy of the window as it is now, or of `area` (x, y, w, h) of it."""
    shape, rgb = _call("frame", area=_area(area))
    return Frame(shape["x"], shape["y"], shape["width"], shape["height"], rgb)


def images():
    """The names of the images picked in the Macros tab, which find() takes."""
    return list(_call("images"))


def running():
    """Whether the account's client is still up."""
    return bool(_call("running"))
