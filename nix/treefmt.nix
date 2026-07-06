{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { pkgs, ... }:
    let
      cargo-sort = pkgs.writeShellApplication {
        name = "treefmt-cargo-sort";
        runtimeInputs = [ pkgs.cargo-sort ];
        text = ''
          for manifest in "$@"; do
            cargo-sort "$(dirname "$manifest")"
          done
        '';
      };
    in
    {
      treefmt = {
        projectRootFile = "flake.nix";

        programs.nixfmt.enable = true;
        programs.rustfmt.enable = true;
        programs.shellcheck.enable = true;
        programs.shfmt.enable = true;
        programs.just.enable = true;
        programs.mdformat.enable = true;
        programs.taplo = {
          enable = true;
          priority = 1;
        };
        programs.yamlfmt.enable = true;

        settings.formatter.cargo-sort = {
          command = cargo-sort;
          priority = 0;
          includes = [
            "Cargo.toml"
            "**/Cargo.toml"
          ];
        };

        settings.global.excludes = [ ".envrc" ];
      };
    };
}
