# Home-manager module for Thoth — privacy-first, offline-capable voice
# transcription.
#
# Per-user configuration: installs Thoth and (by default) runs it as a
# systemd user service so it starts with your graphical session. The package
# is already wrapped by the flake with the runtime dependencies it needs
# (wl-clipboard, wtype, CUDA/Vulkan libraries, GTK resources).
#
# Note: the flake only builds Thoth for x86_64-linux (`meta.platforms`), so
# on other Linux architectures you must set `services.thoth.package` to your
# own build. If you import this file directly rather than through the
# flake's `homeManagerModules.default`, the `package` option has no default
# — set it yourself (the flake wrapper wires it to `self.packages`).
#
# Usage:
#   inputs = {
#     thoth.url = "github:poodle64/thoth";
#   };
#
#   homeConfigurations.myhost = home-manager.lib.homeManagerConfiguration {
#     modules = [
#       inputs.thoth.homeManagerModules.default
#       {
#         services.thoth.enable = true;
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
      defaultText = lib.literalExpression "self.packages.${pkgs.stdenv.hostPlatform.system}.thoth";
      description = "The Thoth package to use.";
    };

    autostart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Whether to start Thoth automatically with your graphical session.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Install into the user profile as well: registers the .desktop entry
    # for application launchers and puts `thoth` on PATH for
    # troubleshooting, even when autostart is disabled.
    home.packages = [ cfg.package ];

    systemd.user.services.thoth = {
      Unit = {
        Description = "Thoth voice transcription";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };

      Service = {
        ExecStart = "${cfg.package}/bin/thoth";
        Restart = "on-failure";
        RestartSec = 5;
      };

      Install.WantedBy = lib.mkIf cfg.autostart [ "graphical-session.target" ];
    };
  };
}
