# mujō's fork of Cordial, the runtime that runs Roblox's official Android
# build natively: upstream source at the audited commit plus the patches
# beside this file, built as two command-line tools and nothing else.
#
#   cordial-run    one account's game client (upstream's engine binary)
#   cordial-fetch  installs the Roblox build (0002: upstream's "Download
#                  Roblox" button, without the window)
#
# Everything a launcher does -- accounts, sessions, joining servers, updates --
# is the Roblox manager's job (roblox-manager.py), so upstream's own launcher
# window (cordial-shell's binary), its Deno plugin runtime and the WebKit web
# views are left out, as is the Flatpak. The engine's window is still GTK4: it
# lives in the cordial-shell crate, and cordial-run links it as a library.
#
# Patches:
#   0001  keep the cursor inside a fullscreen window (zwp_confined_pointer_v1);
#         CORDIAL_NO_FULLSCREEN_CONFINE=1 turns it off. Verified by protocol
#         trace against niri 26.04, sway 1.12, KWin 6.7 (plain and with
#         XDG_CURRENT_DESKTOP=KDE) and mutter 50: confined, gives way to the
#         right-drag camera lock, confined again. Also keeps the toplevel's
#         input region non-empty in fullscreen (cordial-shell host_window.rs),
#         without which KWin -- and its camera lock -- cannot hold the cursor.
#         Hyprland is left unconfined on purpose: its confinement moves
#         pointer focus off the game canvas (see wayland.rs).
#   0002  cordial-fetch
#
# To move to a newer upstream: audit the new commit, bump `rev` and `hash`,
# and re-apply the patches (they touch two files).
{pkgs}: let
  inherit (pkgs) lib;
  version = "0.18.0";
in
  pkgs.rustPlatform.buildRustPackage {
    pname = "cordial-mujo";
    inherit version;

    # main@beff8f3, "Cordial 0.18.0" -- the same source the Flatpak build this
    # replaces was made from (see mujo's nixos/apps/roblox-manager.nix history).
    src = pkgs.fetchFromGitHub {
      owner = "luohoa97";
      repo = "cordial";
      rev = "beff8f316ec878c42e8cae8cc4757374f168a63f";
      # mcpelauncher-linker and libjnivm are submodules the build needs.
      fetchSubmodules = true;
      hash = "sha256-+EdhuDUj1TPTYtUklxO9TV3ai9G9lS6SUvgV7uhRkhs=";
    };
    patches = [
      ./0001-confine-pointer-while-fullscreen.patch
      ./0002-cordial-fetch-cli.patch
    ];
    cargoLock.lockFile = ./Cargo.lock;

    stdenv = pkgs.clangStdenv;
    nativeBuildInputs = with pkgs; [clang cmake pkg-config wrapGAppsHook4];
    # cmake is for the build scripts' native parts, not a project to configure.
    dontUseCmakeConfigure = true;
    buildInputs = with pkgs; [
      gtk4
      libadwaita
      glib
      gdk-pixbuf
      cairo
      pango
      graphene
      wayland
      libxkbcommon
      gsettings-desktop-schemas
      # The audio backends are compiled in only when these are found.
      pipewire
      alsa-lib
      libpulseaudio
      zlib
    ];

    cargoBuildFlags = ["-p" "cordial-runtime" "--bin" "cordial-run" "-p" "cordial-update" "--bin" "cordial-fetch"];
    # Upstream's suite drives GTK and a compositor; 0001's own test is run by
    # hand (cargo test -p cordial-runtime mujo_).
    doCheck = false;

    CORDIAL_GIT_SHA = "beff8f3-mujo";

    # Loaded with dlopen at run time, so not found through the ELF's own
    # dependencies: Vulkan for the engine, and whichever audio backend answers.
    preFixup = ''
      gappsWrapperArgs+=(
        --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath (with pkgs; [vulkan-loader libGL pipewire libpulseaudio alsa-lib])}"
      )
    '';

    meta = {
      description = "Cordial, trimmed to its command-line runtime, with mujō's patches";
      homepage = "https://github.com/luohoa97/cordial";
      license = lib.licenses.gpl3Plus;
      platforms = ["x86_64-linux"];
      mainProgram = "cordial-run";
    };
  }
