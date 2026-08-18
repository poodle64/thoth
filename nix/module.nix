# NixOS module for Thoth — privacy-first, offline-capable voice transcription.
#
# System-level configuration: installs the packaged Thoth binary. Runtime
# dependencies (wl-clipboard, wtype, the CUDA/Vulkan libraries and the GTK
# resources the binary dlopen()s) are already baked into the wrapped package
# by the flake's postFixup, so adding it to systemPackages is sufficient.
#
# Unlike Handy, Thoth's text insertion uses wtype/enigo/portal-based input
# (see src-tauri/src/text_insert.rs) rather than rdev/raw uinput, so no
# /dev/uinput udev rule is required.
#
# Note: the flake only builds Thoth for x86_64-linux (`meta.platforms`), so
# on other Linux architectures you must set `programs.thoth.package` to your
# own build. If you import this file directly rather than through the
# flake's `nixosModules.default`, the `package` option has no default — set
# it yourself (the flake wrapper wires it to `self.packages`).
#
# Usage:
#   inputs = {
#     thoth.url = "github:poodle64/thoth";
#   };
#
#   nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
#     modules = [
#       inputs.thoth.nixosModules.default
#       {
#         programs.thoth.enable = true;
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
  cfg = config.programs.thoth;
in
{
  options.programs.thoth = {
    enable = lib.mkEnableOption "Thoth voice transcription";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "self.packages.${pkgs.stdenv.hostPlatform.system}.thoth";
      description = "The Thoth package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
