{ inputs, ... }:
{
  perSystem = { craneLib, ... }:
    let
      src = craneLib.cleanCargoSource inputs.self;

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      lattice-psbt = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
      });
    in
    {
      packages.default = lattice-psbt;
    };
}
