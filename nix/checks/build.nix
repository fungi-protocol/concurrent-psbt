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
        checkArgs
        // {
          cargoArtifacts = cargoArtifactsRelease;
          # The tests-* checks run the test suite under nextest.
          doCheck = false;
        }
      );
    };
}
