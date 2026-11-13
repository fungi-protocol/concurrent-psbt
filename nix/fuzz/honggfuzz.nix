{
  perSystem =
    {
      fuzzArgs,
      lib,
      pkgs,
      system,
      toolchains,
      ...
    }:
    let
      # Not in nixpkgs. The version must match the honggfuzz dependency in
      # fuzz/honggfuzz/Cargo.toml.
      cargo-hfuzz = pkgs.rustPlatform.buildRustPackage rec {
        pname = "cargo-hfuzz";
        version = "0.5.62";
        src = pkgs.fetchCrate {
          pname = "honggfuzz";
          inherit version;
          hash = "sha256-v7UH4SL+tOfyZXfBT8c8/uJ5KqtW8+jgi8Z2Dln1qJ4=";
        };
        cargoHash = "sha256-BNFuh9Ibyu/J7OOMSfQMGeRUbA71lCN3f9u75UQgkSc=";
        meta.mainProgram = "cargo-hfuzz";
      };

      libs = [
        pkgs.binutils-unwrapped.lib
        pkgs.libunwind
        pkgs.xz
      ];

      # Every target in fuzz/honggfuzz, and the honggfuzz binary itself,
      # built by cargo-hfuzz in the sandbox so that the app and the check run
      # the same binaries.
      mkTargets =
        {
          pnameSuffix,
          RUSTFLAGS ? "",
        }:
        toolchains.nightly.mkCargoDerivation (
          fuzzArgs "honggfuzz"
          // {
            inherit pnameSuffix RUSTFLAGS;
            nativeBuildInputs = [
              cargo-hfuzz
              pkgs.patchelf
            ];
            # cargo-hfuzz compiles honggfuzz from C, which needs libbfd and
            # libunwind. _FORTIFY_SOURCE, which the hardening wrapper adds,
            # clashes with glibc's own definition in memorycmp.c.
            C_INCLUDE_PATH = lib.makeSearchPath "include" [
              pkgs.binutils-unwrapped.dev
              pkgs.libunwind.dev
            ];
            LIBRARY_PATH = lib.makeLibraryPath libs;
            CFLAGS = "-O3 -U_FORTIFY_SOURCE";
            NIX_HARDENING_ENABLE = "";
            buildPhaseCargoCommand = "cargo hfuzz build";
            installPhase = ''
              mkdir -p $out/bin
              cp hfuzz_target/honggfuzz $out/bin/
              # Linking through LIBRARY_PATH leaves no runpath behind.
              patchelf --add-rpath ${lib.makeLibraryPath libs} $out/bin/honggfuzz
              for bin in src/bin/*.rs; do
                cp hfuzz_target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/$(basename $bin .rs) $out/bin/
              done
            '';
          }
        );

      targets = mkTargets { pnameSuffix = "-fuzz-honggfuzz"; };

      # honggfuzz's Makefile builds on macOS only up to 13, through xcrun.
      linuxOnly = lib.optionalAttrs (lib.hasSuffix "-linux" system);
    in
    {
      packages = linuxOnly {
        inherit cargo-hfuzz;
        fuzz-honggfuzz = targets;
      };

      apps = linuxOnly {
        fuzz-honggfuzz = {
          type = "app";
          meta.description = "Run a honggfuzz target: nix run .#fuzz-honggfuzz -- <target> [honggfuzz args]";
          program = lib.getExe (
            pkgs.writeShellApplication {
              name = "fuzz-honggfuzz";
              text = ''
                target="''${1:?usage: nix run .#fuzz-honggfuzz -- <target> [honggfuzz args...]}"
                shift
                # The same layout and persistent mode cargo hfuzz run uses.
                workspace="fuzz/honggfuzz/hfuzz_workspace/$target"
                mkdir -p "$workspace/input"
                exec ${targets}/bin/honggfuzz -W "$workspace" -f "$workspace/input" -P "$@" -- "${targets}/bin/$target"
              '';
            }
          );
        };

        # A fuzzer with broken coverage feedback runs as cleanly as one with
        # nothing to find, so assert that honggfuzz finds the crash_test panic.
        #
        # This is an app rather than a check: whether the fuzzer finds the
        # crash within the time limit depends on its random choices, and a
        # check that can fail without a change to its inputs would blame the
        # wrong commit.
        fuzz-honggfuzz-verify =
          let
            verifyTargets = mkTargets {
              pnameSuffix = "-fuzz-honggfuzz-verify-targets";
              RUSTFLAGS = "--cfg fuzzer_verify_crash";
            };
          in
          {
            type = "app";
            meta.description = "Confirm honggfuzz finds the crash_test panic: nix run .#fuzz-honggfuzz-verify -- [honggfuzz args]";
            program = lib.getExe (
              pkgs.writeShellApplication {
                name = "fuzz-honggfuzz-verify";
                runtimeInputs = [ pkgs.coreutils ];
                text = ''
                  scratch=$(mktemp -d)
                  trap 'rm -rf "$scratch"' EXIT
                  mkdir "$scratch/input"
                  # honggfuzz's exit status says nothing useful here; what it
                  # left in its workspace does.
                  ${verifyTargets}/bin/honggfuzz -W "$scratch/workspace" -f "$scratch/input" -P \
                    --timeout 5 --threads 1 --run_time 600 --exit_upon_crash "$@" \
                    -- ${verifyTargets}/bin/hfuzz_crash_test || true
                  crashes=("$scratch"/workspace/*.fuzz)
                  if [ ! -e "''${crashes[0]}" ]; then
                    echo "honggfuzz did not find the crash" >&2
                    exit 1
                  fi
                '';
              }
            );
          };
      };
    };
}
