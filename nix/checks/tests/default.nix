{
  perSystem =
    {
      checkArgs,
      commonArgs,
      depsArgs,
      pkgs,
      toolchains,
      ...
    }:
    let
      profiles = {
        dev = "dev";
        release = "release";
      };

      # Stable cargo lacks the unused_dependencies lint that depsArgs silences
      # and warns about the unknown `lints.cargo` key instead.
      mkDeps =
        tcName: profile: craneLib:
        craneLib.buildDepsOnly (
          (if tcName != "stable" then depsArgs else commonArgs) // { CARGO_PROFILE = profile; }
        );

      mkTestCheck =
        profile: craneLib: cargoArtifacts:
        craneLib.cargoNextest (
          checkArgs
          // {
            inherit cargoArtifacts;
            CARGO_PROFILE = profile;
            cargoNextestExtraArgs = "--user-config-file ${./nextest-record.toml}";
            nativeBuildInputs = [ pkgs.unzip ];
            preCheck = ''
              export NEXTEST_STATE_DIR="$TMPDIR/nextest-state"
              mkdir -p "$NEXTEST_STATE_DIR"
            '';
            postCheck = ''
              cargo nextest store export \
                --no-pager \
                --user-config-file ${./nextest-record.toml} \
                --archive-file "$out/nextest-run.zip" \
                latest
              unzip -tqq "$out/nextest-run.zip"
            '';
          }
        );

      # nextest does not run doctests; cargo test --doc does.
      mkDocTestCheck =
        profile: craneLib: cargoArtifacts:
        craneLib.cargoDocTest (
          checkArgs
          // {
            inherit cargoArtifacts;
            CARGO_PROFILE = profile;
          }
        );

      testChecks = pkgs.lib.concatMapAttrs (
        tcName: craneLib:
        pkgs.lib.concatMapAttrs (
          profName: profile:
          let
            cargoArtifacts = mkDeps tcName profile craneLib;
          in
          {
            "tests-${tcName}-${profName}" = mkTestCheck profile craneLib cargoArtifacts;
            "doctests-${tcName}-${profName}" = mkDocTestCheck profile craneLib cargoArtifacts;
          }
        ) profiles
      ) toolchains;
    in
    {
      checks = testChecks;

      checkTags = pkgs.lib.mapAttrs (
        name: _:
        [ "nightly" ]
        ++ pkgs.lib.optional (builtins.elem name [
          "tests-nightly-dev"
          "doctests-nightly-dev"
        ]) "quick"
      ) testChecks;
    };
}
