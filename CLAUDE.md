## Agent skills

### Issue tracker

Issues live in GitHub Issues for DamnShabu/roblox-manager (via `gh`). See `docs/agents/issue-tracker.md`.

### Domain docs

Single-context: one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Conventions

- Rust workspace: `crates/core` holds every behaviour (no GTK), `crates/app` the
  window. Inside each, one directory per area (accounts, macros, keyring, …).
- No source file over 600 lines -- split along a real seam, not arbitrarily.
  `crates/core/tests/size_limit.rs` enforces it.
- No "if it works it works": deep modules with small interfaces, no
  `unwrap`/`expect` outside tests, no swallowed errors (the one exception is
  `Keyring::forget`, which is documented as best-effort).
- Every seam into the outside world is a trait with two adapters: the real one
  and the one the tests use.
- Before committing: `nix develop -c cargo test`, `cargo clippy --all-targets
  -- -D warnings`, `cargo fmt --check`.
- Data files (`~/.local/share/rbxmgr/*.json`) and keyring attributes stay
  compatible with what earlier versions wrote.
