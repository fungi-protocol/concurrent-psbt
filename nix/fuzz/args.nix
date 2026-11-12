{
  perSystem =
    { checkArgs, toolchains, ... }:
    {
      # Arguments for a crane derivation that builds the workspace in
      # fuzz/<dir>. Every fuzz workspace has a lock file of its own, which
      # also covers the targets crate the frontends depend on by path.
      _module.args.fuzzArgs =
        dir:
        checkArgs
        // {
          cargoVendorDir = toolchains.nightly.vendorCargoDeps {
            cargoLock = ../../fuzz/${dir}/Cargo.lock;
          };
          cargoArtifacts = null;
          preBuild = "cd fuzz/${dir}";
        };
    };
}
