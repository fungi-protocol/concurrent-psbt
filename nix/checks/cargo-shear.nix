{
  perSystem =
    {
      commonArgs,
      pkgs,
      toolchains,
      ...
    }:
    let
      cargoVendorDir = toolchains.nightly.vendorCargoDeps commonArgs;
    in
    {
      checkTags.cargo-shear = [
        "lint"
        "nightly"
      ];

      # cargo-shear reads manifests and sources; it does not need the crate
      # built, only its dependencies vendored and cargo kept offline.
      checks.cargo-shear =
        pkgs.runCommand "cargo-shear"
          {
            inherit cargoVendorDir;
            inherit (commonArgs) src;
            nativeBuildInputs = [
              pkgs.cargo
              pkgs.cargo-shear
            ];
          }
          ''
            export CARGO_HOME="$TMPDIR/cargo-home"
            export CARGO_NET_OFFLINE=true
            mkdir -p "$CARGO_HOME"
            cp "$cargoVendorDir/config.toml" "$CARGO_HOME/config.toml"
            cd "$src"
            cargo-shear
            mkdir -p "$out"
          '';
    };
}
