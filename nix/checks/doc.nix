{
  perSystem =
    {
      commonArgs,
      cargoArtifactsDev,
      toolchains,
      ...
    }:
    {
      checkTags.doc = [
        "lint"
        "nightly"
      ];

      checks.doc = toolchains.nightly.cargoDoc (
        commonArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          CARGO_PROFILE = "dev";
          cargoDocExtraArgs = "--no-deps --all-features";
          RUSTDOCFLAGS = "-D warnings";
        }
      );
    };
}
