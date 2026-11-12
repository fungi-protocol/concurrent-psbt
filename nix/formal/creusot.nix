# Creusot deductive verifier (formal/creusot).
#
# Not in nixpkgs. Upstream's flake builds creusot-rustc, cargo-creusot, Why3, why3find and the
# SMT solvers from pinned sources; `free` selects alt-ergo-free over the non-commercially
# licensed alt-ergo. The input deliberately has no `follows`: it keeps its own nixpkgs,
# crane, flake-parts and rust-overlay pins, which upstream builds and tests the toolchain and
# solvers against.
{ inputs, lib, ... }:
{
  perSystem =
    { system, ... }:
    let
      creusotPkgs = inputs.creusot.packages.${system} or { };
      supported = creusotPkgs ? free;
      creusot = creusotPkgs.free;
    in
    {
      packages = lib.mkIf supported { inherit creusot; };
      formal.packages = lib.optionals supported [ creusot ];
    };
}
