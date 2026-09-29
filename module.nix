# The manager and its desktop entry. Host-specific pieces (impermanence
# paths, DNS pins for a filtering resolver) belong to the importing config.
#
# Note: Roblox's own rules treat simultaneous clients as a policy violation,
# and community reports tie them to anti-cheat flags. Installing this is
# inert; the exposure starts when the second client is launched, which is
# always an explicit click.
{pkgs, ...}: {
  environment.systemPackages = [
    (import ./package.nix {inherit pkgs;})
    (pkgs.makeDesktopItem {
      # Named after the app id, which is the window's Wayland app_id: docks and
      # task bars find a window's icon through the entry of that name.
      name = "io.github.mujo.RobloxManager";
      desktopName = "Roblox Manager";
      genericName = "Roblox Account Manager";
      comment = "Launch several Roblox accounts into the same server";
      icon = "io.github.mujo.RobloxManager";
      exec = "roblox-manager";
      terminal = false;
      # X11's match, for the same reason (GTK sets WM_CLASS to the app id).
      startupWMClass = "io.github.mujo.RobloxManager";
      # Game alone: listing a second main category makes the entry show up twice
      # in menus (desktop-file-validate warns about exactly this).
      categories = ["Game"];
      keywords = ["roblox" "cordial" "account" "alt" "multi" "instance"];
    })
  ];
}
