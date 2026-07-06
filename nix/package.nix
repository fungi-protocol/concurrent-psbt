{ inputs, ... }:
{
  perSystem =
    { pkgs, toolchains, ... }:
    let
      craneLib = toolchains.nightly;
      sourceRoot = inputs.self;
      src = pkgs.lib.cleanSourceWith {
        src = sourceRoot;
        filter =
          path: _type:
          let
            rel = pkgs.lib.removePrefix "${toString sourceRoot}/" (toString path);
          in
          rel == "Cargo.lock"
          || rel == "Cargo.toml"
          || rel == "crates"
          || pkgs.lib.hasPrefix "crates/" rel
          || rel == "contrib"
          || rel == "contrib/demo-gui"
          || pkgs.lib.hasPrefix "contrib/demo-gui/" rel;
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      cargoArtifactsDev = craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = "dev"; });
      cargoArtifactsRelease = cargoArtifactsDev;

      concurrent-psbt = craneLib.buildPackage (
        commonArgs
        // {
          CARGO_PROFILE = "dev";
          cargoArtifacts = cargoArtifactsDev;
        }
      );
    in
    {
      _module.args = {
        inherit commonArgs cargoArtifactsRelease cargoArtifactsDev;
      };

      packages.default = concurrent-psbt;
    };
}
