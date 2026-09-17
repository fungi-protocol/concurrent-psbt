{
  perSystem =
    {
      commonArgs,
      pkgs,
      rev,
      ...
    }:
    {
      checkTags.cargo-sort = [
        "lint"
        "nightly"
      ];

      checks.cargo-sort =
        pkgs.runCommand "cargo-sort-${rev}"
          {
            inherit (commonArgs) src;
            nativeBuildInputs = [ pkgs.cargo-sort ];
          }
          ''
            cargo-sort --check --workspace "$src"
            mkdir -p $out
          '';
    };
}
