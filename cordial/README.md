# Cordial

The runtime the manager's clients run in. `source.json` pins which source
is built -- today upstream (`luohoa97/cordial`) at an audited commit plus the
patches beside it. `package.nix` builds it for Nix; the Flatpak manifest
(`packaging/flatpak/io.github.mujo.RobloxManager.yml`) builds the same, and
`crates/core/tests/cordial_pin.rs` fails `cargo test` when the two disagree.

## What the manager relies on

Any source this points at must keep these, or `crates/core/src/cordial`
changes with it. Each item names the code that depends on it.

- **Two programs on PATH**, `cordial-run` and `cordial-fetch`.
- **`cordial-run --lib-dir DIR --apk FILE --host-libc --game-activity --run 0
  --profile NAME [--join-url URL]`** (`engine.rs`). The URL is the engine's
  own deep link, `roblox://experiences/start?placeId=P[&gameInstanceId=S]`
  (`roblox/join_url.rs`). The running process's argv[0] ends in
  `cordial-run` and carries `--profile NAME` (found with `pgrep -a -f`,
  `clients.rs`); it survives its first 5 s when the launch is good
  (`STARTUP_CHECK`) and ends its session cleanly on SIGTERM.
- **`cordial-fetch [--newest | --status]`** (`build.rs`): the last stdout
  line is `{"version", "apk", "engine"}`, the `--apk` and `--lib-dir` above;
  a failure exits non-zero with its reason as the last stderr line.
- **Profiles** at `$XDG_DATA_HOME/cordial/profiles/<name>`, each with its
  FastFlags in `flags.json` (`profiles.rs`).
- **Settings** in `$XDG_CONFIG_HOME/cordial/shell.json`, passed to the
  engine as the `CORDIAL_*` variables `engine::env` sets, plus
  `CORDIAL_SECRET_STORE=keyring`.
- **Sessions in the Secret Service** (`session.rs`, `profiles.rs`): filed
  under `xdg:schema=org.cordial.Session`, `application=cordial`,
  `profile=<profile directory>`, `store=identity|cookies`; the secret is
  `cordial-secret-hex-v1:<hex>` of the identity JSON (schema 1) or the
  cookie store (v1). Existing users' keyrings hold these, so they stay.

## Moving to another source (our fork)

1. Push the fork with the patches committed, or leave them to be applied.
2. In `source.json` set `owner`, `repo`, `rev`, `version`, and `hash` (build
   once with `lib.fakeHash` and copy the hash Nix prints). `patches` lists
   only what the fork does not already carry -- `[]` once both are in it.
3. Copy the fork's `Cargo.lock` over `Cargo.lock` here, and regenerate
   `packaging/flatpak/cordial-cargo-sources.json` from it
   (`flatpak-cargo-generator.py cordial/Cargo.lock -o ...`).
4. In the manifest's `cordial` module: the git `url` and `commit`,
   `CORDIAL_GIT_SHA` (the commit's first 7 characters, then `-mujo`), the
   `type: patch` entries, and the libjnivm / mcpelauncher-linker commits if
   the fork moved its submodules.
5. `cargo test` (the pin check), `nix build .#cordial-mujo`, and
   `packaging/flatpak/build.sh`. Then launch one account and play a macro
   in a macro-ready window: the checks above cannot see the engine.
