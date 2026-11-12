# Stateright model of replicas gossiping `JoinResult` states (formal/stateright).
#
# The model is a standalone Rust crate that depends on `concurrent-psbt` by path, so it runs the
# crate's real `Join` code. It is built with cargo like any other crate, no extra tooling.
#
# - flat: `try_join` is `Ok` iff equal.
# - option: `try_join` has a nontrivial LUB (`absent` below everything).
# - tree: `absent` below everything and `x` below `x1`, with `y` incomparable to both, so one
#   candidate in a conflict can be refined while another stays.
#
# The BFS exhausts each state space with no violation of `preservation` or `convergence`.
{
  perSystem =
    {
      pkgs,
      toolchains,
      commonArgs,
      ...
    }:
    let
      # Stable is enough: neither the crate nor Stateright needs a nightly feature here (the
      # crate's only `feature(..)` is behind the `coverage_nightly` cfg, which this build never sets).
      craneLib = toolchains.stable;

      # The crate lives outside the cargo workspace and has its own lock file; patches never
      # touch it, so the vendored dependencies are shared by every `src`.
      cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ../../formal/stateright/Cargo.lock; };

      # Model-check `src` under each config in turn; the first violation fails the build.
      stateright =
        {
          pname,
          src ? commonArgs.src,
          configs,
        }:
        craneLib.mkCargoDerivation (
          commonArgs
          // {
            inherit pname src cargoVendorDir;
            version = "0";
            cargoArtifacts = null;
            buildPhaseCargoCommand = ''
              set -o pipefail
              mkdir -p $out
              cargo run --release --locked --manifest-path formal/stateright/Cargo.toml -- \
                ${pkgs.lib.escapeShellArgs configs} 2>&1 | tee $out/stateright.log
            '';
            installPhase = "true";
          }
        );
    in
    {
      checks.formal-stateright = stateright {
        pname = "formal-stateright";
        configs = [
          "flat"
          "option"
          "tree"
        ];
      };
    };
}
