{
  perSystem =
    {
      commonArgs,
      pkgs,
      ...
    }:
    {
      checkTags.cargo-sort = [
        "lint"
        "nightly"
      ];

      checks.cargo-sort =
        pkgs.runCommand "cargo-sort"
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
