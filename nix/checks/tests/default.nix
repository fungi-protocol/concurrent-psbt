{
  perSystem =
    {
      checkArgs,
      commonArgs,
      pkgs,
      toolchains,
      ...
    }:
    let
      profiles = {
        dev = "dev";
        release = "release";
      };

      mkDeps = profile: craneLib: craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = profile; });

      mkTestCheck =
        profile: craneLib: cargoArtifacts:
        craneLib.cargoNextest (
          checkArgs
          // {
            inherit cargoArtifacts;
            CARGO_PROFILE = profile;
            cargoNextestExtraArgs = "--user-config-file ${./nextest-record.toml}";
            nativeBuildInputs = with pkgs; [
              jq
              unzip
            ];
            preCheck = ''
              export NEXTEST_STATE_DIR="$TMPDIR/nextest-state"
              export NEXTEST_TRACE_DIR="$TMPDIR/nextest-traces"
              mkdir -p "$NEXTEST_STATE_DIR"
              mkdir -p "$NEXTEST_TRACE_DIR"
            '';
            postCheck = ''
              cargo nextest store export \
                --no-pager \
                --user-config-file ${./nextest-record.toml} \
                --archive-file "$out/nextest-run.zip" \
                latest
              unzip -tqq "$out/nextest-run.zip"
              mkdir -p "$out/chrome-traces"
              jq empty "$NEXTEST_TRACE_DIR"/*.json
              cp "$NEXTEST_TRACE_DIR"/*.json "$out/chrome-traces/"
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
            cargoArtifacts = mkDeps profile craneLib;
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
