# Rust rewrite, part 2: the UI

Date: 2026-09-29
Status: design (the user delegated approval: "do what you think is right until it's finished")

## Purpose

The `rbxmgr` binary: the Roblox Manager window, in gtk4-rs + libadwaita, on top of `rbxmgr-core` (part 1). It has feature and look parity with the Python window, whose design (the v2 handoff) is kept:

- warm dark surfaces with one amber accent;
- Source Sans 3 and JetBrains Mono;
- Material Symbols Rounded as a ligature icon font.

Part 3 (packaging, removing Python) follows.

## Requirements

- Parity with every window feature of `roblox-manager.py`:
  - **Title bar:** a status pill, Update Roblox, Add account, refresh-all, close.
  - **Game bar:** favourites tiles, the games browser, Join a friend.
  - **Accounts:** the leader card with its auto-join list; group cards with drag-and-drop, select, launch and settings; account rows with a settings panel (leader, group, auto-join, label, note, macro, macro-ready window, low-power client, session, remove).
  - **Macros:** cards with a switch, hotkey, steps and Run; the macro editor dialog; the help page.
  - **Activity** log.
  - **Action bar:** Stop all, Launch selected, Launch as group.
  - **Dialogs:** Add account / Sign in again (Quick Login), Join a friend.
  - **Hotkeys:** in-window shortcuts, and the `run-macro` GApplication action for compositor bindings.
  - **Background work:** a running-clients poll every 2 s, the busy count, and toasts.
- Same CSS classes and stylesheet as the Python app. The stylesheet moves verbatim into `crates/app/resources/style.css`.
- The project rules hold: no file over 600 lines, and no GTK in the core.
- The UI thread never blocks. Keyring, HTTP, processes and macro playback run on worker threads, and their results come back to the main loop.

## Architecture

```
crates/app/
  resources/style.css           the design's stylesheet (from the Python CSS)
  src/
    main.rs                     adw::Application, CSS, the run-macro action
    services.rs                 the core wired up: keyring, Roblox, profiles, launcher, paths
    worker.rs                   run on a thread, deliver to the main loop (async-channel)
    state.rs                    AppState: the stores + runtime state keyed by UserId
    ui/
      widgets.rs                label/box/icon/button/switch/chip/thumb builders
      modal.rs                  the design's modal (icon tile, title, close, footer)
      activity.rs               log kinds and the activity list
      window/mod.rs             Window: layout, refresh, busy, toast, poll
      window/launching.rs       launch selected / chain / group / row, sessions, update
      window/accounts.rs        account and group actions (layout, rename, remove, add)
      window/macros.rs          macro actions (run, stop, save, delete, hotkeys)
      accounts/row.rs           AccountRow
      accounts/settings.rs      the row's settings panel
      accounts/leader.rs        the leader card and FollowerRow
      accounts/group.rs         GroupCard and its editor
      games.rs                  GameBar
      friends.rs                FriendsDialog
      login.rs                  AddAccountDialog (Quick Login)
      macros/card.rs            MacroCard
      macros/editor.rs          MacroDialog
```

### State

- `AppState` holds:
  - `AccountStore` and `MacroLibrary`;
  - runtime sets keyed by **UserId**: `running`, `launching`, `joining`, `open_accounts`;
  - `macro_runs: HashMap<UserId, (StopFlag, String)>`;
  - `open_macros`, `edit_group`, `ungrouped_open`, `friend` (target), `game_list`, `activity`, `busy`.
- Because runtime state is keyed by user id, renaming an account strands nothing. This fixes the Python rename bug.
- `Window` is an `Rc` handle: `Rc<RefCell<AppState>>`, the widgets it redraws, and `Services`. Widgets keep a `Window` clone, as the Python widgets kept `self.window`.
- Redraws rebuild a section from state, as the Python app did. It is simple, and fast enough for tens of accounts.

### Workers

`worker::run(work: impl FnOnce() -> T + Send, done: impl FnOnce(T) + 'static)` does three things:

1. runs `work` on a thread;
2. sends its result over an `async_channel`;
3. a `glib::spawn_future_local` task awaits the result and calls `done` on the main loop.

Log lines from workers go through a cloned `async_channel::Sender<String>` drained by one main-loop task. That replaces the Python `once()`.

### Services

Built once at startup:

- `Paths::from_env()`;
- `Keyring(DbusSecrets)`. If the session bus is unavailable, a Keyring whose every call reports that error;
- `HttpRoblox(UreqTransport)`;
- `CordialProfiles(SystemRunner)`;
- `Launcher`, with `roblox_build` and the default `Pacing`;
- `IconCache(paths.icons())`.

## Behaviour notes (differences from the Python window, deliberate)

- **Launch reports:** expired and failed accounts come back from `LaunchReport` and are applied when the launch ends. Failures still stream to the activity log as they happen.
- **Macros survive renames:** a macro running on an account survives the account's rename, since runs are keyed by user id.
- **No unlock deadlocks:** account removal clears the keyring on a worker, and any failure is logged. The keyring prompt no longer needs the main loop, so nothing can deadlock on it.

## Testing

GTK cannot be driven headlessly in the unit tests. So:

- The pure UI pieces get unit tests: `log_kind`, status-pill text, the state→chip mapping, the target label, the friend status text, the hotkey clash message.
- A **smoke run**:
  - the built app runs inside a headless wlroots compositor (`cage` with `WLR_BACKENDS=headless`), against a temp `XDG_DATA_HOME` seeded with fixture accounts and macros;
  - screenshots are taken with `grim` and compared by eye with the Python window, run the same way.
- This never touches the user's real data or screen.
