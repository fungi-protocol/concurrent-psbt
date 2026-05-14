{
  perSystem =
    { pkgs, ... }:
    {
      _module.args = {
        cargo-afl = import ./cargo-afl.nix { inherit pkgs; };
      };
    };
}
