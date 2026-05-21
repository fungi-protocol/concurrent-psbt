{ inputs, ... }:
{
  perSystem =
    { toolchains, ... }:
    let
      craneLib = toolchains.nightly;
      src = craneLib.cleanCargoSource inputs.self;

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      cargoArtifactsRelease = craneLib.buildDepsOnly commonArgs;
      cargoArtifactsDev = craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = "dev"; });

      concurrent-psbt = craneLib.buildPackage (commonArgs // { cargoArtifacts = cargoArtifactsRelease; });
    in
    {
      _module.args = {
        inherit commonArgs cargoArtifactsRelease cargoArtifactsDev;
      };

      packages.default = concurrent-psbt;
    };
}
