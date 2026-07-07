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
            config.packages.validate-commits
            config.treefmt.build.wrapper
            just
            nodejs
            rust-analyzer
            typescript
          ];
        };
    in
    {
      devShells = builtins.mapAttrs (_: mkDevShell) toolchains // {
        default = mkDevShell toolchains.nightly;
      };
    };
}
