# roblox-manager: domain glossary

The words the code and its docs use. When a name here fits, use it; don't
drift to synonyms.

**Account** -- one Roblox user this app manages. Identified by its Roblox
**user id**; its **label** is only the name shown and the key its cookie is
filed under. Runtime state is keyed by user id, never by label.

**Label** -- the account's name in this app. A single ordinary path component
(no `/`, no leading `.` or `_`). Renaming moves its keyring entry.

**Session** -- an account's `.ROBLOSECURITY` cookie, kept only in the keyring.
Its state is *ok* (Roblox last accepted it), *expired* (Roblox refused it --
"Sign in again" is the fix) or *checking* (never saved).

**Keyring** -- the desktop Secret Service. Every read or write unlocks it first;
`forget` is the one best-effort path that never prompts.

**Leader** -- the account that launches first in a group launch. **Followers**
(the auto-join list) launch after it, into its **server**.

**Group** -- a named set of accounts drawn together, with a game to launch
them into.

**Place** -- a Roblox game's start place (digits). **Server** -- one running
instance of a place (Roblox's gameId / gameInstanceId).

**Target** -- where a launch sends accounts: the place picked in the game bar,
or a friend's server.

**Cordial** -- the Roblox runtime this repo ships a fork of, Stacked
(`cordial-run`, `cordial-fetch`). **Cordial profile** -- one per account, named
`rbxmgr-<user id>`; the manager seeds its session before every launch.
**Build** -- the installed Roblox engine and APK the clients run.

**Launch** -- signing each account's session in with Roblox, seeding its
Cordial profile and starting its **client**; its outcome per account is
launched, expired or failed (a **launch report**).

**Low-power client** -- a client throttled, FIFO-paced, niced and capped, for
an account along for the ride.

**Hidden window** -- a client's window unmapped at the manager's request
(SIGUSR1; SIGUSR2 shows it), the game still running behind it at a capped
frame rate. A macro-ready client's is its cage's window, which its **window
relay** unmaps instead, leaving the client's own window up inside the cage
for macros. Not minimised: that is the compositor's, and niri has none.

**Macro** -- a named sequence of steps (tap, hold, press, release, repeat, type,
click, move, path, turn, scroll, wait, start, timeline) played into one client,
a set number of **rounds** or until stopped.
**Macro-ready window** -- a client run inside its own nested compositor
(cage), where a macro's emulated input can reach it.

**Recording** -- what a macro-ready window received between two presses of
the **record key** (F8) in it, made into a timeline and added to the macro
being edited. **Relay** -- the pass-through a macro-ready client runs behind, which
reports its window's input to a recording, and nothing at all otherwise.

**Timeline** -- steps that each start at a time of their own (`at 0.5 hold w 1`),
playing over one another against one clock. **Path** -- the pointer gliding
through points on the window. **Turn** -- raw (unaccelerated, relative) mouse
movement, what a game turns its camera by; the pointer follows it.
