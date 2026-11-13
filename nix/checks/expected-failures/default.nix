# Expected-failure tests verify that our checks catch mistakes. We save
# deliberately bad changes as patches in nix/checks/expected-failures/. Nix
# applies each patch to a separate copy of the source and runs the check against
# that copy. The test passes when the check catches the mistake and fails if the
# check misses it or fails for the wrong reason.
#
# For example, cargo-shear checks for unused dependencies. The saved patch adds
# fastrand as a dependency without using it in the code. The
# cargo-shear-rejects-unused-dependency test passes only when cargo-shear
# returns the expected exit code and reports that dependency as unused.

{
  perSystem =
    {
      commonArgs,
      config,
      pkgs,
      ...
    }:
    let
      unusedDependencySource = pkgs.applyPatches {
        name = "cargo-shear-unused-dependency-source";
        src = commonArgs.src;
        patches = [ ./cargo-shear-unused-dependency.patch ];
      };
    in
    {
      checkTags.cargo-shear-rejects-unused-dependency = [
        "lint"
        "nightly"
      ];

      checks.cargo-shear-rejects-unused-dependency = pkgs.testers.testBuildFailure' {
        name = "cargo-shear-rejects-unused-dependency";
        drv = config.checks.cargo-shear.overrideAttrs (_: {
          src = unusedDependencySource;
        });
        expectedBuilderExitCode = 1;
        expectedBuilderLogEntries = [
          "unused dependency `fastrand`"
          "crates/scrubber/Cargo.toml"
        ];
      };
    };
}
