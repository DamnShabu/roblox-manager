# Rust rewrite, part 1: the core

Date: 2026-09-29
Status: design, awaiting review

## Purpose

roblox-manager is one 4,900-line Python file (`roblox-manager.py`). It is being rewritten in Rust, so that it is faster and stays easy to build on. The rewrite comes in three parts, each with its own spec and plan:

1. **The core** (this spec): a Cargo workspace plus the `rbxmgr-core` library, which holds every behaviour except drawing. It is tested without GTK.
2. **The UI**: the `rbxmgr` binary, gtk4-rs + libadwaita, on top of the core.
3. **Packaging and the switch-over**: `package.nix`, the AppImage and the NixOS module move from Python to Rust, and the Python file is removed.

The Python app remains the shipped app until part 3.

## Requirements

Stated by the user:

- Rust.
- A directory per area (accounts, macros, …) instead of one file.
- No source file over 600 lines.
- No bad design, no bad code, no "if it works it works".

Agreed during design:

- Full feature parity with the Python app.
- Existing user data keeps working unchanged: JSON files, keyring entries, Cordial profiles.
- The `cordial/` fork is untouched. The core drives `cordial-run` and `cordial-fetch` as programs.
- Concurrency: a blocking core. The UI runs core calls on worker threads and receives results over a channel. No async runtime.
- UI toolkit (part 2): gtk4-rs + libadwaita.

## Success criteria

- Every behaviour listed under "Behaviour to preserve" is covered by a passing `cargo test`.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- No `.rs` file exceeds 600 lines. This is enforced by a test.
- The core has no dependency on GTK, GLib or any UI crate.
- JSON files written by the Python app load, and save back with no data lost.

## Workspace layout

```
Cargo.toml                    workspace: members crates/core, crates/app
crates/
  core/                       rbxmgr-core (lib)
    src/
      lib.rs                  re-exports; the crate's map
      paths.rs                XDG resolution, one rule for every path
      json_file.rs            atomic write-then-rename; lenient read
      accounts/
        mod.rs                Account, Group, AccountStore
        layout.rs             leader, auto-join order, groups, drag-and-drop order
        labels.rs             label rule, unique labels
      keyring/
        mod.rs                Keyring, trait Secrets, Attrs
        dbus.rs               DbusSecrets: Secret Service over zbus (blocking)
        memory.rs             MemorySecrets (tests, and exported for the app's tests)
      roblox/
        mod.rs                trait Roblox, HttpRoblox, RobloxError, User, Presence
        http.rs               trait Transport, UreqTransport, CSRF retry, error detail
        users.rs              whoami
        presence.rs           presence, leader server
        friends.rs            friends with status, ranked
        favorites.rs          favourites, merge across accounts
        icons.rs              icon URLs, on-disk icon cache
        quick_login.rs        create / poll / redeem, and the flow
        join_url.rs           roblox://experiences/start links
      cordial/
        mod.rs                CordialProfiles, Profile, Build, ClientOpts, CordialError
        session.rs            identity, cookie store, hex encoding
        engine.rs             shell.json → env, client argv, low-power env and flags
        clients.rs            pgrep output parsing
        build.rs              cordial-fetch: status, install, update; one at a time
        migrate.rs            the old Flatpak Cordial
        process.rs            trait Runner, SystemRunner
      launch/
        mod.rs                Launcher, LaunchRequest, LaunchReport, Pacing
        each.rs               every account into the target
        group.rs              leader first, followers into its server
      macros/
        mod.rs                MacroLibrary, MacroError
        grammar.rs            one tokenizer: text ⇄ editor rows → steps
        keys.rs               US-layout evdev tables
        player.rs             play loop, step playback, release guarantees
        wayland.rs            VirtualInput: virtual keyboard + pointer wire protocol
        nested.rs             display path and cage argv, from one rule
    tests/
      size_limit.rs           no .rs file in the workspace over 600 lines
      python_compat.rs        fixtures written by the Python app
      fixtures/               accounts.json, groups.json, macros.json samples
  app/                        rbxmgr (bin); part 2. A stub main in part 1.
```

Two crates rather than one per area: the core/app split is the seam that must be enforced by the compiler (the core never imports GTK). Areas are directories inside the core.

## Dependencies (core)

`serde` + `serde_json`, `ureq` (with rustls), `zbus` (blocking API), `thiserror`, `rand`, `chrono`. Dev-only: `tempfile`. Nothing else without a reason recorded in the plan.

## Module interfaces

Signatures are indicative; the plan fixes them exactly. Every public item has a doc comment saying what a caller must know: invariants, errors, which thread.

### Newtypes

`Label`, `UserId`, `PlaceId`, `ServerId`, `Profile`, `Cookie`. Each is constructed by a validating parse:

- `PlaceId`: digits only.
- `ServerId`: alphanumeric and `-`.
- `Label`: the Python `valid_name` rule. Non-empty, no `/`, no leading `.` or `_`, printable.
- `Profile`: `rbxmgr-<userId>`, from `Profile::of(UserId)`.
- `Cookie`: no `Display` or `Debug` output of its value, so it cannot be logged by accident.

### keyring

```rust
pub trait Secrets: Send + Sync {
    fn lookup(&self, attrs: &Attrs) -> Result<Option<String>, KeyringError>;
    fn store(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError>;
    fn clear(&self, attrs: &Attrs) -> Result<(), KeyringError>;
    fn unlock(&self) -> Result<(), KeyringError>;
}
pub struct Keyring { secrets: Box<dyn Secrets> }
impl Keyring {
    pub fn cookie(&self, label: &Label) -> Result<Cookie, KeyringError>;   // missing → NoCookie, names "Sign in again"
    pub fn set_cookie(&self, label: &Label, cookie: &Cookie) -> Result<(), KeyringError>;
    pub fn drop_cookie(&self, label: &Label) -> Result<(), KeyringError>;
    pub fn move_cookie(&self, old: &Label, new: &Label) -> Result<(), KeyringError>;
    pub fn put(&self, attrs: &Attrs, label: &str, secret: &str) -> Result<(), KeyringError>;
    pub fn delete(&self, attrs: &Attrs) -> Result<(), KeyringError>;
    pub fn forget(&self, attrs: &Attrs);   // best effort, never unlocks, never errors
}
```

- Every method except `forget` calls `unlock` first.
- `DbusSecrets::unlock` blocks the calling thread until the prompt's `Completed` signal arrives, with a 180 s timeout. Unlike Python, it does not need the main thread.
- `store` deletes existing matches before `CreateItem`, because an entry `secret-tool` wrote carries an extra `xdg:schema` attribute.
- Sessions use the "plain" algorithm.
- An account cookie is filed under `{app: rbxmgr, account: <label>}` with the item label `rbxmgr <label>`. Lookups strip surrounding whitespace.

### roblox

```rust
pub trait Roblox: Send + Sync {
    fn whoami(&self, cookie: &Cookie) -> Result<User, RobloxError>;
    fn presence(&self, cookie: &Cookie, user: UserId) -> Result<Presence, RobloxError>;
    fn friends(&self, cookie: &Cookie, user: UserId) -> Result<Vec<Friend>, RobloxError>;
    fn favorites(&self, cookie: &Cookie, user: UserId, limit: usize) -> Result<Vec<Game>, RobloxError>;
    fn icon_urls(&self, universes: &[u64]) -> Result<HashMap<u64, String>, RobloxError>;
    fn quick_login_create(&self) -> Result<QuickLoginCode, RobloxError>;
    fn quick_login_status(&self, code: &QuickLoginCode) -> Result<QuickLoginStatus, RobloxError>;
    fn quick_login_redeem(&self, code: &QuickLoginCode) -> Result<Cookie, RobloxError>;
}
pub fn quick_login(roblox: &dyn Roblox, events: &mut dyn QuickLoginEvents,
                   cancelled: &dyn Fn() -> bool, clock: &dyn Clock)
    -> Result<(Cookie, User), QuickLoginError>;   // CodeExpired is its own variant
pub fn join_url(place: &PlaceId, server: Option<&ServerId>) -> String;
```

- `HttpRoblox` talks through `trait Transport`. The production adapter is `UreqTransport`; tests use canned responses.
- Every POST retries once with the `x-csrf-token` from the 403 rejection.
- HTTP 401 on an authenticated call is `RobloxError::Expired`.
- Error detail is taken from Roblox's JSON `errors[0].message` when present.
- Batching and ranking match the Python app: presences in batches of 50, friends in batches of 100, and friends ranked game → online → offline.

### cordial

```rust
pub trait Runner: Send + Sync {
    fn run(&self, argv: &[String], timeout: Duration) -> Result<Output, CordialError>;
    fn spawn(&self, argv: &[String], log: File, env: &[(String, String)]) -> Result<Box<dyn Child>, CordialError>;
}
pub struct CordialProfiles { keyring, root, logs, runner, sleep }
impl CordialProfiles {
    pub fn seed(&self, user: &User, cookie: &Cookie) -> Result<Profile, CordialError>;
    pub fn clear(&self, user: UserId) -> Result<(), CordialError>;
    pub fn set_low_power(&self, profile: &Profile, on: bool) -> Result<(), CordialError>;
    pub fn launch(&self, profile: &Profile, url: Option<&str>, build: &Build, opts: ClientOpts) -> Result<(), CordialError>;
    pub fn running(&self) -> Result<HashSet<Profile>, CordialError>;
    pub fn stop(&self, which: &HashSet<Profile>) -> Result<usize, CordialError>;
    pub fn migrate_flatpak(&self, log: &dyn Fn(String)) -> Result<(), CordialError>;
}
pub fn roblox_build(runner: &dyn Runner, log: &dyn Fn(String), newest: bool) -> Result<Build, CordialError>;
```

- `seed` creates the profile directory, then stores identity and cookies under `{xdg:schema: org.cordial.Session, application: cordial, profile: <full path>, store: identity|cookies}`. Bodies are hex-encoded as `cordial-secret-hex-v1:<hex>`.
- `launch` covers:
  - applying low power;
  - building the env from `~/.config/cordial/shell.json`;
  - rotating `<profile>.log` to `.log.1`;
  - wrapping in `nice -n 10` (low power) and in cage (nested);
  - a failure if the client exits within `STARTUP_CHECK` (5 s), carrying the log's last line.
- `running` parses `pgrep -a -f cordial-run`. Only processes whose argv[0] basename is `cordial-run` count.
- `roblox_build` holds a process-wide lock, so two launches never download twice.

### launch

```rust
pub struct Launcher { keyring: Arc<Keyring>, roblox: Arc<dyn Roblox>, profiles: Arc<CordialProfiles>, pacing: Pacing }
pub struct LaunchRequest { accounts: Vec<LaunchAccount>, mode: Mode /* Each | Group */, place: Option<PlaceId>, server: Option<ServerId> }
pub struct LaunchReport { launched: Vec<(UserId, User)>, expired: Vec<UserId>, failed: Vec<(UserId, String)> }
impl Launcher { pub fn launch(&self, req: LaunchRequest, log: &dyn Fn(String)) -> Result<LaunchReport, LaunchError>; }
```

- `Pacing` holds the stagger (8 s), the leader timeout (90 s), the poll interval (3 s) and an injected sleep.
- `Err` only when no build could be installed. Every per-account outcome is in the report.

### accounts

```rust
pub struct AccountStore { accounts: Vec<Account>, groups: Vec<Group>, paths }
impl AccountStore {
    pub fn load(paths) -> Self;                       // runs the old-layout migration when groups.json is absent
    pub fn add_or_refresh(&mut self, user: &User) -> (Label, bool);
    pub fn rename(&mut self, id: UserId, new: Label) -> Result<Label /*old*/, AccountError>;
    pub fn remove(&mut self, id: UserId) -> Option<Account>;
    pub fn record_launch(&mut self, id: UserId, user: &User, place: Option<&PlaceId>);
    pub fn set_session(&mut self, id: UserId, state: SessionState);
    pub fn set_macro(&mut self, id: UserId, macro_: Option<String>);   // turns nested on
    pub fn rename_macro(&mut self, old: &str, new: &str);
    pub fn drop_macro(&mut self, name: &str);
    pub fn layout(&mut self) -> Layout<'_>;          // make_leader, set_follow, move_follower, set_group, drop_on
    pub fn save(&self) -> Result<(), AccountError>;
}
```

- Accounts are keyed by `UserId`. The label is a display name, and runtime state never keys on it.
- `SessionState::Checking` is held in memory only and never written to disk.
- `Account` keeps unknown JSON fields (`#[serde(flatten)] extra`), so nothing another version wrote is lost.

### macros

```rust
pub struct MacroLibrary { .. }   // load, iter (sorted), text, enabled, hotkey, hotkeys,
                                 // save(old, new, text, hotkey) -> Result<(), MacroError>, set_enabled, delete
pub fn rows(text: &str) -> (Vec<Row>, u32);          // editor view
pub fn to_text(rows: &[Row], loops: u32) -> String;
pub fn parse(text: &str) -> Result<Macro, ParseError>;   // Macro { loops, steps }; ParseError { line, message }
pub fn play(profile: &Profile, m: &Macro, stop: &StopFlag, running: &dyn Fn() -> bool,
            connect: &dyn Fn(&Path) -> io::Result<Box<dyn Input>>, report: &dyn Fn(String),
            rng: &mut dyn RngCore) -> Result<(), MacroError>;
```

- `rows`, `to_text` and `parse` share one tokenizer.
- `nested::display_file(profile)` is the single source of the display path. `nested::cage_argv` passes it to the shell as `$0`.
- `VirtualInput` implements `Input` over the Wayland wire protocol:
  - binds `wl_seat`, `zwp_virtual_keyboard_manager_v1` and `zwlr_virtual_pointer_manager_v1`;
  - uploads a US xkb keymap through a memfd;
  - sends modifier state explicitly.

## Data flow

The UI (part 2) owns the `AccountStore` and `MacroLibrary` on the main thread. For every slow operation (launch, session check, favourites, friends, Quick Login, stop, macro playback), it:

1. copies what the worker needs;
2. runs the core call on a worker thread;
3. receives the result over a channel;
4. applies it to the store on the main thread, then saves.

Core types are `Send`, and nothing in the core holds UI state.

## Compatibility

- Paths are unchanged: `~/.local/share/rbxmgr/{accounts,groups,macros}.json`, `~/.cache/rbxmgr/{_icons,logs}`, `~/.local/share/cordial/profiles`, `~/.config/cordial/shell.json`, and `$XDG_RUNTIME_DIR/rbxmgr/<profile>.wayland`.
- XDG resolution, one rule: an unset or empty `XDG_*_HOME` falls back to the default.
- JSON schemas are unchanged:
  - A macro entry is a bare string, or `{text, enabled, hotkey}`.
  - Legacy `{script, start_delay, …}` entries are migrated on read.
  - Accounts keep leader/follow/group/selected/nested/low_power/macro/plays/favorites/last_place.
- Keyring attributes and item labels are unchanged.

## Error handling

- The core has one `thiserror` enum per area. Every message says what to do next where there is a fix (for example "use Sign in again", or "launch it again with Macro-ready window on").
- No `unwrap`/`expect` outside tests, and no swallowed errors except `Keyring::forget`.

## Testing

Tests go through each module's interface, using the second adapter at every seam. The behaviours they must cover are the checks in the Python self-check at `git show HEAD:test-roblox-manager.py`, used as a checklist. That file is not restored to the tree. Areas:

- **keyring:** unlock before every read/write; a refused unlock is an error on every path except `forget`; cookie round-trip and move.
- **roblox:** CSRF retry; 401 → Expired; error detail extraction; presence batching; friends ranking; favourites merge; Quick Login create → poll → redeem, expiry, cancellation; join URL validation.
- **cordial:**
  - seeded identity, cookie jar and hex encoding are byte-exact;
  - low-power flags are added and removed without touching the user's own values, and the file is untouched when nothing changes;
  - client argv, env from shell.json, log rotation, early-exit failure;
  - pgrep parsing edge cases; stop signals only the accounts' own clients;
  - build install/update/failure; Flatpak migration in a temp dir.
- **launch:** each mode; group mode waits for the leader and followers join its server; followers get their own servers on timeout; running clients are skipped; stagger only between real sign-ins; expired and failed accounts are reported.
- **accounts:**
  - JSON round trip, and no secret ever in accounts.json;
  - old-layout migration; leader/follow/group/drag operations;
  - label uniqueness; rename and remove; `Checking` never persisted.
- **macros:**
  - grammar round trip; parse errors name the line; evdev codes for a US layout;
  - play releases every pressed key on stop or failure;
  - `VirtualInput` against a fake compositor on a real Unix socket;
  - nested display path agrees with and without `XDG_RUNTIME_DIR`.
- **Compatibility:** Python-written fixtures load and save back losslessly.
- **Size:** `tests/size_limit.rs` fails if any `.rs` file exceeds 600 lines.

## Out of scope for part 1

The GTK UI, CSS and icons (part 2). Nix/AppImage packaging and removing the Python app (part 3). Any change to `cordial/`.
