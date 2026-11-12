# Kani harnesses for the scrubber (crates/scrubber/src/scrub/proofs.rs).
#
# Separate from `formal-kani` because these are slow: CBMC has to unwind bitcoin's
# `io::Read`-based decoders and `BTreeMap`. No run has finished yet, so how long the check
# takes is not known; keep the symbolic inputs small rather than adding harnesses here.
#
# Reuses the `kaniCheck` that the `kani` package exposes through `passthru`, so the check
# stays in step with `formal-kani`; this module only says which crate to run.
{ lib, ... }:
{
  perSystem =
    { config, ... }:
    {
      checks = lib.mkIf (config.packages ? kani) {
        formal-kani-scrubber = config.packages.kani.kaniCheck {
          pname = "formal-kani-scrubber";
          package = "scrubber";
          # The default features only select test modules, which the harnesses don't need.
          kaniArgs = "--no-default-features";
        };
      };
    };
}
