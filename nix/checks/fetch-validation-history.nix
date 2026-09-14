{
  perSystem =
    { pkgs, ... }:
    {
      checks.fetch-validation-history =
        pkgs.runCommand "fetch-validation-history"
          {
            nativeBuildInputs = [ pkgs.git ];
          }
          ''
            bash ${./fetch-validation-history.sh} \
              ${../../.github/scripts/fetch-validation-history.sh}
            mkdir -p "$out"
          '';
    };
}
