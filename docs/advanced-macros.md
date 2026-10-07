# Advanced macros

An advanced macro is a Python script. Every `.py` file in
`~/.local/share/rbxmgr/advanced-macros/` shows in the Advanced tab of the
macros pane, with its first comment or docstring line under its name. Files
whose names start with `_` are not listed, so a module the scripts share can
sit beside them and be imported.

Tick accounts and press ▶ on a script: it runs once for each of them, as a
separate `python3` process, into that account's macro-ready window. Press it
again to stop every run. Stop ends a script wherever it is, a `time.sleep`
included (it gets SIGTERM, then SIGKILL a second later), and the manager lets
go of every key the script left down.

Scripts run from their own folder with the system's `python3`, so anything
you installed for it (numpy, Pillow) can be imported. Only keys, clicks and
copies of the window go through the manager. A script can't reach into the
Roblox client.

## The `rbxmgr` module

It is imported before your script runs. Coordinates count from the window's
top-left corner, the same as a plain macro's.

| Call | What it does |
|---|---|
| `tap("e")`, `tap("ctrl+w")`, `tap("shift", "w")` | Press the keys, hold them for a short tap (`secs=` for longer), let go |
| `hold("w", secs=2)` | Hold for `secs` |
| `key_down("shift")`, `key_up("shift")` | Leave a key down, and let it go |
| `release_all()` | Let go of everything still down |
| `type_text("gg")` | Type text a key at a time, US layout |
| `click()`, `click(400, 300)`, `click(button="mouse2")` | Click, at a point if given |
| `move_to(x, y)` | Put the pointer there |
| `move(dx, dy)` | Raw mouse movement: what turns a game's camera |
| `scroll(3)`, `scroll(-1, horizontal=True)` | Turn the wheel. Positive is down or right |
| `wait(secs)` | Wait. Stop ends it at once |
| `find("image1")` | The `(x, y)` of a picked image's top-left corner, or `None` |
| `find_center("image1")` | The middle of it: the place to click |
| `wait_for("image1", timeout=10)` | `find()` until it shows, or `None` after `timeout` |
| `pixel(x, y)` | The `(r, g, b)` shown there |
| `color_is(x, y, (255, 48, 48), within=24)` | Whether that pixel is about that colour |
| `frame()`, `frame((x, y, w, h))` | A copy of the window or an area of it. `.pixel(x, y)`, `.save("shot.png")`, `.rgb` bytes |
| `images()` | The names of the picked images |
| `running()` | Whether the account's client is still up |
| `account`, `user_id` | Which account this run is for |

Key names are the ones a plain macro uses: letters and digits, `space`,
`enter`, `shift`, `ctrl`, `alt`, `f1` to `f12`, `mouse1` to `mouse5`.

Images are the ones picked for a plain macro's **When** step. Pick one there
and it is saved as `image1`, `image2` and so on. `find(..., area=(x, y, w, h))`
looks only in that rectangle, and `least=0.95` asks for more of the image's
pixels to match (0.9 by default).

A request the manager can't do, such as a key it doesn't know, an image that
was never picked, or a window it can't see yet, raises `rbxmgr.MacroError`.
Catch it to carry on. An uncaught exception ends the run, and its last line
shows as the reason.

`print()` goes to the activity log, and the latest line shows on the
account's row while the script runs.

## How it talks to the manager

`rbxmgr` sends one JSON request per line on the process's stdout and reads one
reply per line on its stdin. Before your script runs it moves `print()` and
anything else on stdout over to stderr, so nothing the script prints can
corrupt the conversation. Any language could speak it, but only the Python
module is provided.
