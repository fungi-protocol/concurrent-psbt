{ inputs, ... }:
{
  perSystem =
    { pkgs, ... }:
    {
      checkTags.actionlint = [
        "lint"
        "nightly"
      ];

      checks.actionlint =
        pkgs.runCommand "actionlint"
          {
            nativeBuildInputs = [ pkgs.actionlint ];
            src = inputs.self;
          }
          ''
            # actionlint 1.7.12 predates the code-quality permission scope.
            find "$src/.github/workflows" -type f \
              \( -name '*.yml' -o -name '*.yaml' \) \
              -exec actionlint -ignore 'unknown permission scope "code-quality"' {} +
            mkdir -p "$out"
          '';
    };
}
