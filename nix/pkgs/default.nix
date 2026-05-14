{
  perSystem =
    { pkgs, rustStable, ... }:
    let
      cargo-afl = import ./cargo-afl.nix { inherit pkgs rustStable; };
    in
    {
      _module.args = {
        inherit cargo-afl;
        cargo-hfuzz = import ./cargo-hfuzz.nix { inherit pkgs; };
      };

      packages = {
        inherit cargo-afl;
        cargo-hfuzz = import ./cargo-hfuzz.nix { inherit pkgs; };
      };
    };
}
