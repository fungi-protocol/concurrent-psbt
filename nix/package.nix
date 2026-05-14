{ inputs, ... }:
{
  perSystem =
    { craneLibNightly, ... }:
    let
      src = craneLibNightly.cleanCargoSource inputs.self;

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      cargoArtifacts = craneLibNightly.buildDepsOnly commonArgs;

      lattice-psbt = craneLibNightly.buildPackage (commonArgs // { inherit cargoArtifacts; });
    in
    {
      _module.args = {
        inherit commonArgs cargoArtifacts;
      };

      packages.default = lattice-psbt;
    };
}
