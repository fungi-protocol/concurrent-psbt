{ inputs, ... }:
{
  perSystem =
    { commonArgs, ... }:
    let
      rev = inputs.self.shortRev or "dirty";
    in
    {
      _module.args = {
        inherit rev;
        checkArgs = commonArgs // {
          version = rev;
          dontFixup = true;
          doInstallCargoArtifacts = false;
          CARGO_PROFILE = "";
        };
      };
    };
}
