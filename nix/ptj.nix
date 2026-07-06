{ ... }:
{
  perSystem =
    {
      commonArgs,
      cargoArtifactsRelease,
      toolchains,
      ...
    }:
    {
      packages.ptj = toolchains.nightly.buildPackage (
        commonArgs
        // {
          CARGO_PROFILE = "dev";
          cargoArtifacts = cargoArtifactsRelease;
          cargoExtraArgs = "-p ptj";
          pname = "ptj";
        }
      );
    };
}
