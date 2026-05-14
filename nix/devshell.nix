{
  perSystem =
    { pkgs, craneLibNightly, ... }:
    {
      devShells.default = craneLibNightly.devShell {
        packages = with pkgs; [
          cargo-nextest
          rust-analyzer
        ];
      };
    };
}
