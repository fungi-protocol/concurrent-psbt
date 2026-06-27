{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem =
    { pkgs, config, ... }:
    {
      formatter = pkgs.writeShellScriptBin "fmt" ''
        ${pkgs.cargo-sort}/bin/cargo-sort --workspace
        exec ${config.treefmt.build.wrapper}/bin/treefmt "$@"
      '';

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
          settings.formatting.reorder_keys = false; # already the default, but making it explicit because true would interfere with cargo-sort
        };
        programs.yamlfmt.enable = true;

        settings.global.excludes = [ ".envrc" ];
      };
    };
}
