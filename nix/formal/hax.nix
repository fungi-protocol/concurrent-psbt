# hax: extract the crate's Rust to Lean and prove things about the extraction (formal/hax).
#
# hax's Lean backend does not use hax's own frontend or engine: `cargo hax into lean` runs
# Charon and Aeneas and scaffolds a Lean package around the result, built against hax's own
# Lean models of `core` and `alloc` (`CoreModels`) and tactics (`Hax`). So this module needs
# upstream hax (the `cargo-hax` driver), the Aeneas fork commit hax pins (the `aeneas` binary
# and Charon) and the Lean library of both. The library imports Mathlib, which nixpkgs'
# `leanPackages` already packages at the Lean version (v4.35.0) hax targets.
{ inputs, lib, ... }:
{
  perSystem =
    { pkgs, system, ... }:
    let
      haxPkgs = inputs.hax.packages.${system} or { };
      aeneasPkgs = inputs.aeneas-hax.packages.${system} or { };
      supported = haxPkgs ? hax && aeneasPkgs ? aeneas;
      hax = haxPkgs.hax;
      aeneas = aeneasPkgs.aeneas;
      charon = aeneasPkgs.charon;

      craneLib = inputs.crane.mkLib pkgs;
      cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ../../Cargo.lock; };
      src = craneLib.cleanCargoSource ../..;

      lp = pkgs.leanPackages;
      scenario = "concurrent-psbt";
      package = "ConcurrentPsbt";

      aeneasLean = lp.buildLakePackage {
        pname = "lean4-aeneas";
        version = "0-unstable";
        src = "${inputs.aeneas-hax}/backends/lean";
        leanPackageName = "aeneas";
        leanDeps = [
          lp.mathlib
          lp.Cli
        ];
        # `AeneasMeta` precompiles its modules unless `CI` is set, which needs shared
        # libraries of every dependency.
        env.CI = "1";
      };

      # `Hax` (tactics) and `CoreModels` (Lean extraction of hax's models of core and alloc).
      # Its lakefile fetches `aeneas` with git; lake is told to use the Nix-built one instead.
      haxLean = lp.buildLakePackage {
        pname = "lean4-hax";
        version = "0.3.41";
        src = "${inputs.hax}/hax-lib/proof-libs/lean";
        leanPackageName = "hax";
        leanDeps = [ aeneasLean ];
        buildTargets = [
          "Hax"
          "CoreModels"
        ];
        env.CI = "1";
      };

      # `cargo hax extract` runs the scenario in `formal/hax/hax.toml` on the crate: the hax
      # driver compiles it with Charon, Aeneas turns that into Lean and hax scaffolds the Lean
      # package around it. The workspace is vendored because cargo resolves all of it, even
      # though only the library is extracted.
      extract =
        name: src:
        pkgs.runCommand name
          {
            inherit src;
            nativeBuildInputs = [ hax ];
          }
          ''
            set -o pipefail
            export HOME=$TMPDIR CARGO_HOME=$TMPDIR/cargo CARGO_NET_OFFLINE=true
            mkdir -p $CARGO_HOME $out
            cat ${cargoVendorDir}/config.toml >> $CARGO_HOME/config.toml
            cp -r $src work && chmod -R u+w work && cd work
            {
              cat ${../../formal/hax/hax.toml}
              echo '[tools]'
              echo 'charon = { path = "${charon}/bin/charon" }'
              echo 'aeneas = { path = "${aeneas}/bin/aeneas" }'
            } > hax.toml
            cargo-hax hax extract --offline ${scenario} 2>&1 | tee $out/hax.log
            cp -r crates/concurrent-psbt/proofs $out/
          '';

      lean = haxLean.lean4;
      leanPath = lib.concatMapStringsSep ":" (p: "${p}/.lake/build/lib/lean") (
        [ haxLean ] ++ haxLean.allLeanDeps
      );

      # Typecheck the extraction, the model of formal/lean and the proofs about the
      # extraction, then audit the axioms of the theorems listed in Axioms.lean: the
      # hand-written `Assumptions/FunsExternal.lean` replaces the one hax generates and
      # declares an `axiom` for each external function that hax has no model of. No theorem
      # may depend on one of them, or on `sorry`.
      verify =
        name: src:
        let
          extracted = extract "${name}-extract" src;
        in
        pkgs.runCommand name
          {
            proofs = ../../formal/hax;
            model = ../../formal/lean;
            inherit extracted;
            nativeBuildInputs = [ lean ];
          }
          ''
            set -o pipefail
            export LEAN_PATH=${leanPath}:$PWD/build
            mkdir -p build $out
            cp -r $extracted/proofs/${scenario}/lean pkg && chmod -R u+w pkg
            cp $model/JoinResult.lean $proofs/*.lean pkg/
            cp $proofs/Assumptions/*.lean pkg/${package}/Assumptions/
            cd pkg
            for m in JoinResult \
              ${package}/Assumptions/TypesExternal ${package}/Extraction/TypesExternal \
              ${package}/Extraction/Types ${package}/Assumptions/FunsExternal \
              ${package}/Extraction/FunsExternal ${package}/Extraction/Funs \
              ${package}/Extraction Proofs; do
              mkdir -p ../build/$(dirname $m)
              lean -o ../build/$m.olean $m.lean 2>&1 | tee -a $out/lean.txt
            done
            lean Axioms.lean 2>&1 | tee $out/axioms.txt
            if grep -q -e sorryAx -e "declaration uses 'sorry'" $out/*.txt ||
              grep -q -e concurrent_psbt -e CoreModels $out/axioms.txt; then
              echo "a theorem depends on sorry or on an external axiom" >&2
              exit 1
            fi
          '';
    in
    {
      packages = lib.mkIf supported {
        inherit hax;
        hax-lean = haxLean;
      };
      checks = lib.mkIf supported {
        formal-hax-tools =
          pkgs.runCommand "formal-hax-tools"
            {
              nativeBuildInputs = [
                hax
                aeneas
              ];
              lib = haxLean;
            }
            ''
              mkdir -p $out
              { cargo-hax hax --version || true; aeneas -version; charon version; } 2>&1 | tee $out/versions.txt
            '';
        formal-hax = verify "formal-hax" src;
      };
      formal.packages = lib.optionals supported [ hax ];
    };
}
