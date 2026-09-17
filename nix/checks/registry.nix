{
  perSystem =
    { lib, ... }:
    {
      options.checkTags = lib.mkOption {
        default = { };
        description = "Aggregate groups that include each check, by check name.";
        type = lib.types.attrsOf (
          lib.types.listOf (
            lib.types.enum [
              "lint"
              "nightly"
              "quick"
            ]
          )
        );
      };
    };
}
