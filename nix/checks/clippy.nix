{
  perSystem =
    {
      checkArgs,
      cargoArtifactsDev,
      toolchains,
      ...
    }:
    {
      checkTags.clippy = [
        "lint"
        "nightly"
        "quick"
      ];

      checks.clippy = toolchains.nightly.cargoClippy (
        checkArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
        }
      );
    };
}
