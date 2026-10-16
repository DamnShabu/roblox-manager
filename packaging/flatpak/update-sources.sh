#!/usr/bin/env bash
# Regenerate the Flatpak's offline Cargo sources from the lock files. Run it
# after Cargo.lock or cordial/Cargo.lock changes; CI fails until it has been.
#
#   packaging/flatpak/update-sources.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

generate() {
    nix run .#flatpak-cargo-generator -- "$1" -o "$2"
}
generate Cargo.lock packaging/flatpak/cargo-sources.json
generate cordial/Cargo.lock packaging/flatpak/cordial-cargo-sources.json
