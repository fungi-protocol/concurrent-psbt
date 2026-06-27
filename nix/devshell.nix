{
  perSystem =
    {
      config,
      pkgs,
      toolchains,
      ...
    }:
    let
      mkDevShell =
        craneLib:
        craneLib.devShell {
          packages = with pkgs; [
            cargo-llvm-cov
            cargo-nextest
            cargo-sort
            config.packages.scrub-commit-history
            config.treefmt.build.wrapper
            just
            rust-analyzer
          ];
        };
    in
    {
      devShells = builtins.mapAttrs (_: mkDevShell) toolchains // {
        default = mkDevShell toolchains.nightly;
      };
    };
}
