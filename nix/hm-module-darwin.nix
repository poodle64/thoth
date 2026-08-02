# Home-manager module for Thoth on macOS (issue #117).
#
# Per-user configuration: installs Thoth and (by default) runs it as a
# launchd user agent in the Aqua (GUI) session. macOS home-manager manages
# per-user services through `launchd.agents` rather than systemd user units,
# and there is no `graphical-session.target`, so autostart is driven by
# `RunAtLoad` instead.
#
# Note: this flake currently only builds Thoth for x86_64-linux (see
# `meta.platforms`), so on darwin there is no package default to wire — you
# must set `services.thoth.package` to a darwin-capable build yourself.
#
# Usage:
#   inputs = {
#     thoth.url = "github:poodle64/thoth";
#   };
#
#   homeConfigurations.myhost = home-manager.lib.homeManagerConfiguration {
#     modules = [
#       inputs.thoth.homeManagerModules.darwin
#       {
#         services.thoth.enable = true;
#         services.thoth.package = pkgs.callPackage ...; # your darwin build
#       }
#     ];
#   };

{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.thoth;
in
{
  options.services.thoth = {
    enable = lib.mkEnableOption "Thoth voice transcription user service";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The Thoth package to use.";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to start Thoth automatically at login.";
    };
  };

  config = lib.mkIf cfg.enable {
    launchd.agents.thoth = {
      enable = true;
      # The GUI domain gives the agent a user Aqua session (window server
      # access), which a tray application needs.
      domain = "gui";
      config = {
        ProgramArguments = [ "${cfg.package}/bin/thoth" ];
        # Restart if Thoth crashes — `Crashed` is launchd's closest
        # equivalent to systemd's Restart=on-failure and only fires on
        # crash signals, so it does not interfere with a configurable
        # RunAtLoad and a clean quit is not restarted.
        RunAtLoad = cfg.autostart;
        KeepAlive = {
          Crashed = true;
        };
        ProcessType = "Interactive";
      };
    };
  };
}
