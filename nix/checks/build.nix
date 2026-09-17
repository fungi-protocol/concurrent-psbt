{
  perSystem =
    {
      checkArgs,
      cargoArtifactsRelease,
      toolchains,
      ...
    }:
    {
      checkTags.build = [ "nightly" ];

      checks.build = toolchains.nightly.buildPackage (
        checkArgs // { cargoArtifacts = cargoArtifactsRelease; }
      );
    };
}
