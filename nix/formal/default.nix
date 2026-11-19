# Formal-methods checks. Each verifier lives in its own sibling module in this
# directory and is picked up automatically, so verifiers can be added or removed
# independently without touching shared files.
#
# A verifier module contributes:
#   - `formal.packages`: tools added to `nix develop .#formal`
#   - `checks.formal-<tool>*`: flake checks running its proofs/models
{ lib, ... }:
let
  modules = lib.filterAttrs (
    name: type: type == "regular" && name != "default.nix" && lib.hasSuffix ".nix" name
  ) (builtins.readDir ./.);
in
{
  imports = map (name: ./${name}) (builtins.attrNames modules);

  perSystem =
    { config, pkgs, ... }:
    {
      options.formal.packages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
        description = "Tools provided by the formal-methods dev shell.";
      };

      config.devShells.formal = pkgs.mkShell {
        # just, as in the default shell, so the Justfile's recipes work here too.
        packages = config.formal.packages ++ [ pkgs.just ];
      };
    };
}
