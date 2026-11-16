{ inputs, ... }:
{
  perSystem =
    { toolchains, ... }:
    let
      craneLib = toolchains.nightly;
      src = craneLib.cleanCargoSource inputs.self;

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      # crane builds dependencies against a dummy source whose crates use none
      # of them, so cargo reports every dependency as unused. That is noise in
      # the deps-only derivations; real builds and checks still report truly
      # unused dependencies. cargo has no command-line way to set the lint
      # level and crane's stub manifests drop the `[lints]` tables, so each
      # stub package manifest sets it itself.
      depsArgs = commonArgs // {
        extraDummyScript = ''
          for manifest in $(find $out -name Cargo.toml); do
            if grep -q '^\[package\]' "$manifest"; then
              # Stub manifests are read-only store files; replace, don't append.
              { cat "$manifest"; printf '\n[lints.cargo]\nunused_dependencies = "allow"\n'; } > "$manifest.new"
              rm -f "$manifest"
              mv "$manifest.new" "$manifest"
            fi
          done
        '';
      };

      cargoArtifactsRelease = craneLib.buildDepsOnly depsArgs;
      cargoArtifactsDev = craneLib.buildDepsOnly (depsArgs // { CARGO_PROFILE = "dev"; });

      concurrent-psbt = craneLib.buildPackage (commonArgs // { cargoArtifacts = cargoArtifactsRelease; });
    in
    {
      _module.args = {
        inherit
          commonArgs
          depsArgs
          cargoArtifactsRelease
          cargoArtifactsDev
          ;
      };

      packages.default = concurrent-psbt;
    };
}
