#!/bin/sh
# The manager and Cordial keep their data in the XDG homes. A Flatpak gets
# private ones under ~/.var/app; point them back at the host's, so the
# Flatpak and a native install share accounts, macros and profiles.
# Flatpak passes the host's own values on as HOST_XDG_*_HOME when set.
export XDG_DATA_HOME="${HOST_XDG_DATA_HOME:-$HOME/.local/share}"
export XDG_CONFIG_HOME="${HOST_XDG_CONFIG_HOME:-$HOME/.config}"
export XDG_CACHE_HOME="${HOST_XDG_CACHE_HOME:-$HOME/.cache}"
exec /app/libexec/roblox-manager "$@"
