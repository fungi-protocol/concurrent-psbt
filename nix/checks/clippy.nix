{
  perSystem =
    {
      checkArgs,
      cargoArtifactsDev,
      pkgs,
      toolchains,
      ...
    }:
    {
      checkTags.clippy = [
        "lint"
        "nightly"
        "quick"
      ];

      # Lint every feature configuration: code gated on one test feature (or on
      # neither) is otherwise compiled by nothing in CI, so its warnings pile up
      # unseen. cargo-hack's --each-feature runs clippy with no features, with
      # the default ones, with each feature alone and with all of them, so a new
      # feature is covered without editing this file. All runs share
      # cargoArtifactsDev.
      #
      # crane's cargoClippy cannot be given a different cargo subcommand, so this
      # is a custom cargo derivation (crane docs: "Custom cargo commands"). It
      # calls cargo directly rather than cargoWithProfile, which would put
      # `--profile` between `hack` and `clippy`.
      checks.clippy = toolchains.nightly.mkCargoDerivation (
        checkArgs
        // {
          cargoArtifacts = cargoArtifactsDev;
          pnameSuffix = "-clippy";
          nativeBuildInputs = [ pkgs.cargo-hack ];
          buildPhaseCargoCommand = ''
            cargo hack clippy --locked --workspace --each-feature --all-targets \
              -- -D warnings -D unused -D clippy::allow_attributes
          '';
        }
      );
    };
}
