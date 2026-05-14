{
  perSystem =
    {
      craneLib,
      commonArgs,
      cargoArtifacts,
      ...
    }:
    {
      checks = {
        tests = craneLib.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
          }
        );
      };
    };
}
