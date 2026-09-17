{
  perSystem =
    {
      config,
      pkgs,
      rev,
      ...
    }:
    let
      tagged =
        tag:
        pkgs.lib.mapAttrsToList (name: _: {
          inherit name;
          path = config.checks.${name};
        }) (pkgs.lib.filterAttrs (_: tags: builtins.elem tag tags) config.checkTags);
      # linkFarm, not symlinkJoin: checks that write the same file name (every
      # tests-* check exports nextest-run.zip) must not collide.
      join = name: paths: pkgs.linkFarm "${name}-checks-${rev}" paths;
    in
    {
      checks = {
        quick = join "quick" (tagged "quick");
        lint = join "lint" (tagged "lint");
        nightly = join "nightly" (tagged "nightly");
      };
    };
}
