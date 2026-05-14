{ inputs, ... }:
{
  perSystem = { system, ... }:
    let
      pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ inputs.rust-overlay.overlays.default ];
      };

      rustToolchain = pkgs.rust-bin.selectLatestNightlyWith (t: t.default);

      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
    in
    {
      _module.args = {
        inherit pkgs craneLib;
      };
    };
}
