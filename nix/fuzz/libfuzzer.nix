{
  perSystem =
    {
      fuzzArgs,
      pkgs,
      toolchains,
      ...
    }:
    let
      # Every target in fuzz/libfuzzer, instrumented by cargo-fuzz and built
      # in the sandbox, so that the app and the check run the same binaries.
      mkTargets =
        {
          pnameSuffix,
          RUSTFLAGS ? "",
        }:
        toolchains.nightly.mkCargoDerivation (
          fuzzArgs "libfuzzer"
          // {
            inherit pnameSuffix RUSTFLAGS;
            nativeBuildInputs = [ pkgs.cargo-fuzz ];
            buildPhaseCargoCommand = "cargo fuzz build --fuzz-dir .";
            installPhase = ''
              mkdir -p $out/bin
              for target in $(cargo fuzz list --fuzz-dir .); do
                cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/$target $out/bin/
              done
            '';
          }
        );

      targets = mkTargets { pnameSuffix = "-fuzz-libfuzzer"; };

      # cargo-fuzz sets this when it runs a target itself.
      asanOptions = "detect_odr_violation=0";
    in
    {
      packages.fuzz-libfuzzer = targets;

      apps.fuzz-libfuzzer = {
        type = "app";
        meta.description = "Run a libFuzzer target: nix run .#fuzz-libfuzzer -- <target> [libFuzzer args]";
        program = pkgs.lib.getExe (
          pkgs.writeShellApplication {
            name = "fuzz-libfuzzer";
            text = ''
              target="''${1:?usage: nix run .#fuzz-libfuzzer -- <target> [libFuzzer args...]}"
              shift
              # The same layout cargo fuzz run uses, relative to the checkout.
              corpus="fuzz/libfuzzer/corpus/$target"
              artifacts="fuzz/libfuzzer/artifacts/$target"
              mkdir -p "$corpus" "$artifacts"
              export ASAN_OPTIONS="${asanOptions}''${ASAN_OPTIONS:+:$ASAN_OPTIONS}"
              exec "${targets}/bin/$target" -artifact_prefix="$artifacts/" "$@" "$corpus"
            '';
          }
        );
      };

      checkTags.fuzz-libfuzzer-verify = [ "nightly" ];

      # A fuzzer whose coverage instrumentation silently broke still runs and
      # still reports nothing, so a clean run proves little. Confirm instead
      # that libFuzzer solves crash_test, which needs coverage guidance.
      checks.fuzz-libfuzzer-verify =
        let
          verifyTargets = mkTargets {
            pnameSuffix = "-fuzz-libfuzzer-verify-targets";
            RUSTFLAGS = "--cfg fuzzer_verify_crash";
          };
        in
        pkgs.runCommand "fuzz-libfuzzer-verify" { ASAN_OPTIONS = asanOptions; } ''
          mkdir -p $out
          if ${verifyTargets}/bin/fuzz_crash_test -max_total_time=60 -artifact_prefix=$out/ >fuzz.log 2>&1; then
            cat fuzz.log
            echo "libFuzzer exited cleanly without finding the crash" >&2
            exit 1
          fi
          cat fuzz.log
          grep -q "fuzzer verification crash" fuzz.log
        '';
    };
}
