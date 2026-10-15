# Product

<!-- impeccable:product-schema 1 -->

## Platform

linux-desktop

A native Linux desktop app: Rust, GTK4 + libadwaita (`crates/app`), x86_64,
Wayland for macros. Not web, iOS or Android; none of the web or mobile
platform guidance applies. Desktop conventions to honour: the header bar as
title bar, keyboard shortcuts for every main action, a resizable window with
a side pane, the desktop's keyring and password prompts.

## Users

Linux players who grind Roblox games with a main account plus several alts
(Grand Piece Online, Blox Fruits and the like). They launch the alts, leave
them farming or staying online, often under a macro, and use the rest of
their desktop for other things while the clients run. They come back to the
window to check on clients, fix a signed-out account, start or stop a macro,
or swap games.

## Product Purpose

Run many Roblox accounts at once on Linux and keep them working unattended.
Sign each account in once, launch any set of them into a game with one
click, run key-and-click macros on them while the desktop is used for
something else, and keep a dozen clients from eating the machine. Success is
a farm the user can start in seconds and stop thinking about.

Roblox itself runs in Stacked (a fork of Cordial, a runtime for Roblox's
official Android build), which the app installs and updates on its own.

## Positioning

What a neighbouring launcher (Sober, Vinegar, Bloxstrap-likes) cannot
truthfully claim:

- **Macros while you work.** Macro-ready windows run each client inside its
  own nested compositor, so emulated input reaches it without focus, while
  hidden, on another workspace, and without touching the real keyboard or
  mouse. Macros can be recorded (F8) and can react to what shows on screen
  (When image / colour).
- **Linux-native, safe sessions.** Built for Linux first. Sign-in is Roblox
  Quick Login only: no password is ever typed into the app. Sessions live in
  the system keyring and nowhere else.
- **Lightweight many-clients.** Per-account performance levels (Low 10 FPS
  through Max) and hidden windows, so many clients run at once on one machine.

## Operating Context

- Long-running sessions: the window is a control room checked now and then,
  not a place the user lives in. Clients and macros keep running when the
  window is hidden or closed.
- Entry points besides the main window: the Play button on roblox.com opens a
  small Join popup (link handling); macros can be bound to desktop hotkeys
  via `gapplication action ... run-macro`.
- Distribution: .deb, .rpm, Arch package, Flatpak, AppImage, NixOS flake.
  Self-update per install type, checked against `SHA256SUMS`.
- Requires a Secret Service keyring (GNOME Keyring or KWallet); macros need a
  Wayland session.

## Capabilities and Constraints

- Accounts: add via Quick Login code, label, note, performance level, session
  check (ok / expired / checking), sign in again, remove, search.
- **Performance is always within reach:** an account's performance level can
  be changed from where the account is shown, and for a whole selection at
  once, not only from its settings.
- **Main account:** one account can be pinned as the main. It stays at the
  top of the list and is the default target for account-scoped actions (for
  example its friends for Join a friend, its favourites).
- **Leader / auto-join is deprecated.** The leader-launches-first-then-
  followers-join-its-server mechanism is being removed; do not design new
  surfaces around it. Pinning a main account (above) is what survives of it.
- Groups: named sets of accounts with a game of their own and a Launch/Stop
  on the group.
- Launch targets: favourited games, Roblox's own games browser, a friend's
  server. Launch selected, per-row start/stop, Stop All.
- Hidden windows (off screen and taskbar, game still running at a capped
  frame rate), per account and all at once.
- Macros: step editor and Text mode, rounds, ranges, hotkeys, recording,
  When steps, run on every ticked account with per-account progress.
- Activity log; updates for the app, Stacked and the Roblox build.
- Code: `crates/core` holds every behaviour with no GTK; `crates/app` is the
  window. No source file over 600 lines.
- Data files (`~/.local/share/rbxmgr/*.json`), keyring attributes and the app
  id `io.github.mujo.RobloxManager` must stay compatible with earlier
  versions, rename or not.
- Not supported yet: private-server links and "follow a user" links.

## Brand Commitments

- **Function first.** The UI is made primarily for functionality, not for
  beauty: no borrowed visual worlds, metaphors or decorative themes. The look
  stays the neutral dark system in DESIGN.md, refined; design effort goes
  into structure, density, how readable state is at a glance, and how few
  clicks the common actions take. The craft bar is Roblox Account Manager
  (every action one click from the account list) and Mission Center / GNOME
  System Monitor (live state and resource use at a glance, native GTK4).
- **Name: Stacked.** The product is being renamed from "Roblox Manager" to
  **Stacked**, one name for the app and the runtime it ships. The rename is
  decided but not yet carried through the code, packaging or README, which
  still say Roblox Manager. New surfaces use Stacked.
- Mark: `packaging/icons/roblox-manager.svg` is the app's mark today.
- Voice: plain, direct, second person, short sentences; says what a control
  does and what happens next, in the product's own words (account, session,
  launch, macro-ready window). See README.md and CONTEXT.md for terminology;
  use those words, don't drift to synonyms.
- **Risk honesty is binding.** Roblox's terms don't allow multiple clients and
  people report anti-cheat flags. Any surface that introduces launching
  multiple clients states that risk plainly and prominently, never downplayed:
  installing does nothing on its own, the risk starts at the second client,
  use alts you can afford to lose.

## Evidence on Hand

- `README.md`: full user guide, install table, shortcuts, troubleshooting.
- `CONTEXT.md`: domain glossary. `docs/macros-when.md`: When steps in depth.
- `docs/screenshots/*.png`: hero, add account, group settings, join link,
  account settings, macro editor, macro text, row menu. These predate the
  v0.10 window redesign and still show leader/auto-join; treat them as stale.
- No user counts, testimonials, reviews or press exist; never invent them.

## Product Principles

1. **Set it and leave it.** Every design choice favours a farm that runs
   unattended: state visible at a glance, problems surfaced with their fix,
   nothing that needs babysitting.
2. **Never touch what isn't ours.** No passwords, no real keyboard or mouse
   outside a recording, nothing read from the client; sessions only in the
   keyring. Say so where it matters.
3. **Many clients, one machine.** Running more accounts should cost the user
   as little machine and attention as possible.
4. **Honest about risk.** Multi-client use is against Roblox's terms; the
   product says so plainly and never pretends otherwise.
5. **Linux-native.** Follow the desktop's conventions (keyring, portals,
   shortcuts, packaging) rather than imitating a Windows launcher.
