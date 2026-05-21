{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem = {
    treefmt = {
      projectRootFile = "flake.nix";

      programs.nixfmt.enable = true;
      programs.rustfmt.enable = true;
      programs.shellcheck.enable = true;
      programs.shfmt.enable = true;

      settings.global.excludes = [ ".envrc" ];
    };
  };
}
