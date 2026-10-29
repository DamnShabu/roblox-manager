# flatpak-builder-tools' cargo/flatpak-cargo-generator.py at a pinned commit,
# with the Python it needs: turns a Cargo.lock into the offline sources the
# Flatpak manifest vendors (packaging/flatpak/update-sources.sh runs it, and
# CI checks its output is what is committed).
{pkgs}: let
  rev = "74697c75b630d7330e77250fc13cb5ea688d9479";
  script = pkgs.fetchurl {
    url = "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/${rev}/cargo/flatpak-cargo-generator.py";
    hash = "sha256-Ci22vofXWRD6zvKKtG1NZGCALoQZq4UNDKpqNk0ms4A=";
  };
  python = pkgs.python3.withPackages (p: [p.aiohttp p.tomlkit]);
in
  pkgs.writeShellScriptBin "flatpak-cargo-generator" ''
    exec ${python}/bin/python3 ${script} "$@"
  ''
