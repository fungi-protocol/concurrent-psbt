{
  perSystem =
    { commonArgs, ... }:
    {
      _module.args = {
        checkArgs = commonArgs // {
          dontFixup = true;
          doInstallCargoArtifacts = false;
          CARGO_PROFILE = "";
        };
      };
    };
}
