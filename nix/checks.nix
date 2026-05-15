{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      craneLibNightly,
      commonArgs,
      cargoArtifacts,
      fuzz-libfuzzer-script,
      fuzz-honggfuzz-script,
      fuzz-afl-script,
      ...
    }:
    let
      fullSrc = pkgs.lib.cleanSource inputs.self;

      # 60s: instrumented fuzzer finds the 8-step state machine easily;
      # without instrumentation 256^8 is infeasible.
      timeout = "60";
    in
    {
      checks = rec {
        tests = craneLibNightly.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );

        mutants = craneLibNightly.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-mutants-smoke";
            nativeBuildInputs = [ pkgs.cargo-mutants ];
            buildPhaseCargoCommand = ''
              timeout 10 cargo mutants --no-shuffle -vV || test $? -eq 124
            '';
            installPhase = ''
              mkdir -p $out
              cp -r mutants.out/* $out/ 2>/dev/null || true
            '';
          }
        );

        coverage = craneLibNightly.mkCargoDerivation (
          commonArgs
          // {
            inherit cargoArtifacts;
            pnameSuffix = "-coverage";
            nativeBuildInputs = [ pkgs.cargo-llvm-cov ];
            buildPhaseCargoCommand = ''
              mkdir -p $out
              cargo llvm-cov --all-features --lcov --output-path $out/coverage.lcov
            '';
            installPhase = "true";
          }
        );

        fuzz-libfuzzer-verify =
          pkgs.runCommand "fuzz-libfuzzer-verify"
            {
              src = fullSrc;
            }
            ''
              export HOME=$(mktemp -d)
              cp -r $src/* .
              chmod -R u+w .

              # cargo-fuzz exits non-zero on crash — that's success for us
              export CARGO_NET_OFFLINE=true
              export RUSTFLAGS="--cfg fuzzer_verify_crash"
              if timeout ${timeout} ${fuzz-libfuzzer-script} fuzz_crash_test -- -max_total_time=50 2>&1; then
                echo "FAIL: libFuzzer did not find crash"
                exit 1
              fi
              echo "OK: libFuzzer found the crash"
              mkdir -p $out
            '';

        fuzz-honggfuzz-verify =
          pkgs.runCommand "fuzz-honggfuzz-verify"
            {
              src = fullSrc;
            }
            ''
              export HOME=$(mktemp -d)
              cp -r $src/* .
              chmod -R u+w .

              export CARGO_NET_OFFLINE=true
              export RUSTFLAGS="--cfg fuzzer_verify_crash"
              export HFUZZ_RUN_ARGS="--timeout 5 -n 1 --run_time ${timeout} --exit_upon_crash"
              ${fuzz-honggfuzz-script} hfuzz_crash_test || true

              if ls hfuzz/hfuzz_workspace/hfuzz_crash_test/SIGABRT* hfuzz/hfuzz_workspace/hfuzz_crash_test/*.fuzz 2>/dev/null; then
                echo "OK: honggfuzz found the crash"
              else
                echo "FAIL: honggfuzz did not find crash"
                exit 1
              fi
              mkdir -p $out
            '';

        clippy = craneLibNightly.cargoClippy (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
          }
        );

        fuzz-afl-verify =
          pkgs.runCommand "fuzz-afl-verify"
            {
              src = fullSrc;
              nativeBuildInputs = with pkgs; [
                gnumake
                stdenv.cc
                llvmPackages.llvm
                llvmPackages.clang
              ];
            }
            ''
              export HOME=$(mktemp -d)
              cp -r $src/* .
              chmod -R u+w .

              export CARGO_NET_OFFLINE=true
              export RUSTFLAGS="--cfg fuzzer_verify_crash"
              timeout ${timeout} ${fuzz-afl-script} afl_crash_test || true

              if ls afl/out/default/crashes/id:* 2>/dev/null; then
                echo "OK: AFL++ found the crash"
              else
                echo "FAIL: AFL++ did not find crash"
                exit 1
              fi
              mkdir -p $out
            '';

        # Fast check bundle: tests + clippy only, no fuzzing or coverage.
        # Run with: nix build .#checks.x86_64-linux.quick
        quick = pkgs.symlinkJoin {
          name = "quick-checks";
          paths = [ tests clippy ];
        };
      };
    };

  # TODO
  # - a check that denies warnings including in cfg(test)
  # - cargo machete or equivalent
  # - cargo audit
  # - nix vuln scanning
  # - ... similar QA / linting tools
}
