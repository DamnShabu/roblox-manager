# roblox-manager as one file for other distros (Arch, ...): its GTK
# and the Cordial fork (cordial-run, cordial-fetch) come from the bundled
# closure; pgrep and a Secret Service (gnome-keyring, KWallet) come from the
# host. Needs unprivileged user namespaces. It also runs on NixOS, though the
# module is the better fit there.
#
#   nix build .#roblox-manager-appimage
{
  pkgs,
  lib,
  nix-appimage,
  roblox-manager,
}: let
  inherit (pkgs.stdenv.hostPlatform) system;

  # Mesa is bundled and pointed at explicitly: nix-built GTK cannot load the
  # host's GL drivers (NixOS finds them under /run/opengl-driver, which no
  # other distro has), and a host driver loaded into a nix glibc is a crash.
  # The game clients inherit it: cordial-run is nix-built too.
  # ponytail: Mesa only; NVIDIA-proprietary hosts get nouveau/software, now in
  # the game as well as the manager's window. Bundle nvidia's userspace
  # matched to the host driver if that ever matters.
  mesa = pkgs.mesa;

  # Host fonts first; DejaVu so a host without fontconfig still has text,
  # then the manager's own typefaces and icon font.
  fontsConf = pkgs.writeText "fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
      <dir>${pkgs.dejavu_fonts}/share/fonts</dir>
      ${lib.concatMapStrings (d: "<dir>${d}</dir>\n") roblox-manager.fontDirs}
      <cachedir prefix="xdg">fontconfig</cachedir>
    </fontconfig>
  '';

  portable = pkgs.writeShellScriptBin "roblox-manager" ''
    # appimage-run -- which NixOS routes every AppImage through -- runs it
    # under no_new_privs, and there flatpak's bwrap cannot map ids for its user
    # namespace: Cordial fails to start. Start the extracted
    # copy again as a transient user service, outside that wrapper, carrying
    # this environment across by name. A no_new_privs launcher elsewhere
    # (firejail, a hardened unit) gets the same treatment.
    no_new_privs=0
    while read -r key value; do
      if [ "$key" = NoNewPrivs: ]; then no_new_privs=$value; fi
    done < /proc/self/status
    if [ "$no_new_privs" = 1 ] && [ -z "''${RBXMGR_UNWRAPPED:-}" ] \
      && [ -x "''${APPDIR:-}/AppRun" ] && command -v systemd-run >/dev/null; then
      args=()
      while IFS= read -r -d "" assignment; do
        name=''${assignment%%=*}
        if [[ $name =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then args+=(-E "$name"); fi
      done < <(${pkgs.coreutils}/bin/env -0)
      exec systemd-run --user --quiet --collect --same-dir \
        "''${args[@]}" -E RBXMGR_UNWRAPPED=1 "$APPDIR/AppRun" "$@"
    fi

    export __EGL_VENDOR_LIBRARY_FILENAMES=${mesa}/share/glvnd/egl_vendor.d/50_mesa.json
    export LIBGL_DRIVERS_PATH=${mesa}/lib/dri
    export GBM_BACKENDS_PATH=${mesa}/lib/gbm
    export VK_DRIVER_FILES=${mesa}/share/vulkan/icd.d
    export FONTCONFIG_FILE=${fontsConf}
    # nix glibc cannot read another distro's locale archive.
    export LOCALE_ARCHIVE=${pkgs.glibcLocalesUtf8}/lib/locale/locale-archive
    # Cordial's own downloads look for CAs under their store path, and fail
    # verification without them. Prefer the host's bundle (it carries any CAs
    # the user added); nixpkgs' cacert when there is none.
    if [ -z "''${SSL_CERT_FILE:-}" ]; then
      SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt
      for f in /etc/ssl/certs/ca-certificates.crt /etc/pki/tls/certs/ca-bundle.crt /etc/ssl/ca-bundle.pem; do
        if [ -r "$f" ]; then SSL_CERT_FILE=$f; break; fi
      done
      export SSL_CERT_FILE
    fi
    exec ${lib.getExe roblox-manager} "$@"
  '';

  # nix-appimage's AppRun, patched (apprun.patch):
  # - pivot_root in place of chroot. The kernel refuses unshare(CLONE_NEWUSER)
  #   to a chrooted process, and flatpak's bwrap (non-setuid on Arch) needs
  #   it; under the stock AppRun no client could start.
  # - a host /nix/store is kept instead of replaced: /nix/store becomes a
  #   directory of symlinks into the bundle's store and then the host's. On
  #   NixOS flatpak, icon themes and /etc/resolv.conf all live in the host
  #   store; hiding it left clients failing with "No such file or directory"
  #   and the icons blank. (Not an overlay: appimage-run mounts inside
  #   /nix/store, which the kernel refuses as an overlay layer.)
  # - root symlinks are recreated as symlinks, so a dangling one (/lib32 under
  #   appimage-run) is no longer reported.
  apprun = pkgs.pkgsStatic.runCommandCC "AppRun" {} ''
    cp ${nix-appimage}/appruns/userns-chroot/main.c main.c
    chmod +w main.c
    patch -p1 < ${./apprun.patch}
    mkdir -p $out/mountroot
    $CC main.c -o $out/AppRun
  '';
in
  (nix-appimage.lib.${system}.mkAppImage.override {mkappimage-apprun = apprun;}) {
    program = lib.getExe portable;
    name = "roblox-manager-x86_64.AppImage";
    squashfsArgs = ["-comp" "zstd" "-Xcompression-level" "19"];
  }
