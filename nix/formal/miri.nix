# Miri (formal/miri): runs the test suite in an interpreter that checks every
# memory access for undefined behaviour.
#
# Miri needs a nightly with the `miri` and `rust-src` components, and a sysroot
# (a std built with Miri's instrumentation) that `cargo miri` normally builds
# on first use by downloading std's own crates.io dependencies. The sandbox has
# no network, so those dependencies are vendored from the lockfile shipped in
# rust-src, next to the workspace's own.
{ inputs, ... }:
{
  perSystem =
    {
      pkgs,
      toolchains,
      ...
    }:
    let
      toolchain = pkgs.rust-bin.selectLatestNightlyWith (
        t:
        t.minimal.override {
          extensions = [
            "miri"
            "rust-src"
          ];
        }
      );

      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;

      cargoVendorDir = craneLib.vendorMultipleCargoDeps {
        cargoLockList = [
          (inputs.self + "/Cargo.lock")
          "${toolchain}/lib/rustlib/src/rust/library/Cargo.lock"
        ];
      };

      src = toolchains.nightly.cleanCargoSource inputs.self;

      # The check as a function of its source and of what `cargo nextest` runs.
      miriCheck =
        {
          pname,
          src,
          testArgs ? "--workspace",
        }:
        craneLib.mkCargoDerivation {
          inherit pname src cargoVendorDir;
          version = "0";
          strictDeps = true;
          cargoArtifacts = null;
          doInstallCargoArtifacts = false;
          nativeBuildInputs = [ pkgs.cargo-nextest ];
          buildPhaseCargoCommand = ''
            set -o pipefail
            export HOME=$TMPDIR
            # Miri runs the test with a clean environment; forward what proptest reads.
            # Failure persistence looks up the cwd, which isolation forbids.
            export PROPTEST_CASES=4
            export PROPTEST_DISABLE_FAILURE_PERSISTENCE=true
            export MIRIFLAGS="-Zmiri-env-forward=PROPTEST_CASES -Zmiri-env-forward=PROPTEST_DISABLE_FAILURE_PERSISTENCE"
            mkdir -p $out
            cargo miri nextest run --no-fail-fast ${testArgs} \
              2>&1 | tee $out/miri.log
          '';
          installPhase = "true";
        };
    in
    {
      checks.formal-miri = miriCheck {
        pname = "formal-miri";
        inherit src;
      };
      formal.packages = [
        toolchain
        pkgs.cargo-nextest
      ];
    };
}
