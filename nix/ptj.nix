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
          cargoArtifacts = cargoArtifactsRelease;
          cargoExtraArgs = "-p ptj";
          pname = "ptj";
        }
      );
    };
}
