{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      checkTags.hestia-version = [
        "lint"
        "nightly"
      ];

      checks.hestia-version =
        pkgs.runCommand "hestia-version"
          {
            nativeBuildInputs = [ pkgs.yq-go ];
            src = inputs.self;
          }
          ''
            find "$src/.github/workflows" -type f \( -name '*.yml' -o -name '*.yaml' \) \
              -exec bash ${./hestia-version.sh} {} +
            mkdir -p "$out"
          '';
    };
}
