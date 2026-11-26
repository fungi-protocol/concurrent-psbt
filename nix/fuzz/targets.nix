{
  perSystem =
    { fuzzArgs, toolchains, ... }:
    {
      checkTags.fuzz-targets-clippy = [
        "lint"
        "nightly"
        "quick"
      ];

      # The root clippy check does not reach this workspace. Lint it with
      # crash_test's panic compiled in, which is the larger of its two forms.
      checks.fuzz-targets-clippy = toolchains.nightly.cargoClippy (
        fuzzArgs "targets"
        // {
          RUSTFLAGS = "--cfg fuzzer_verify_crash";
          pnameSuffix = "-fuzz-targets-clippy";
          cargoClippyExtraArgs = "--all-targets -- -D warnings -D unused -D clippy::allow_attributes";
        }
      );
    };
}
