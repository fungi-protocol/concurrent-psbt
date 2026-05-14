{
  perSystem =
    { pkgs, craneLib, ... }:
    {
      devShells.default = craneLib.devShell {
        packages = with pkgs; [
          cargo-nextest
          rust-analyzer
        ];
      };
    };
}
