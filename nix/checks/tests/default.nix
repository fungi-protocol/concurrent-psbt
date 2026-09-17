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

      mkTestCheck =
        profile: craneLib:
        let
          deps = craneLib.buildDepsOnly (commonArgs // { CARGO_PROFILE = profile; });
        in
        craneLib.cargoNextest (
          checkArgs
          // {
            cargoArtifacts = deps;
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

      testChecks = pkgs.lib.concatMapAttrs (
        tcName: craneLib:
        pkgs.lib.mapAttrs' (
          profName: profile:
          pkgs.lib.nameValuePair "tests-${tcName}-${profName}" (mkTestCheck profile craneLib)
        ) profiles
      ) toolchains;
    in
    {
      checks = testChecks;

      checkTags = pkgs.lib.mapAttrs (
        name: _: [ "nightly" ] ++ pkgs.lib.optional (name == "tests-nightly-dev") "quick"
      ) testChecks;
    };
}
