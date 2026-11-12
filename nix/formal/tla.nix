# TLA+ model of replicas gossiping `JoinResult` states (formal/tla), checked
# with TLC from nixpkgs.
#
# - flat.cfg: `try_join` is `Ok` iff equal.
# - option.cfg: `try_join` has a nontrivial LUB (`Absent` below everything).
#
# TLC exhausts both state spaces with no violation of `Preservation` or
# `Convergence`. Before conflicts kept only their maximal values, option.cfg
# violated `Convergence`.
{
  perSystem =
    { pkgs, ... }:
    let
      tla = pkgs.tlaplus18;

      # Model-check `src` under each config in turn; the first violation fails the build.
      tlc =
        {
          name,
          src,
          configs,
        }:
        pkgs.runCommand name
          {
            inherit src;
            nativeBuildInputs = [ tla ];
          }
          ''
            cp -r $src/. . && chmod -R u+w .
            run() { tlc -workers 1 -cleanup -metadir "$TMPDIR/$1" -config "$1.cfg" JoinResult.tla; }

            set -o pipefail
            ${pkgs.lib.concatMapStrings (c: "run ${c} | tee ${c}.log\n") configs}
            mkdir -p $out && cp *.log $out/
          '';

      src = ../../formal/tla;
    in
    {
      formal.packages = [ tla ];

      checks.formal-tla = tlc {
        name = "formal-tla";
        inherit src;
        configs = [
          "flat"
          "option"
        ];
      };
    };
}
