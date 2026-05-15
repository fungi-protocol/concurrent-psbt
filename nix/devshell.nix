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
            config.treefmt.build.wrapper
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
