-- [concurrent_psbt]: external functions, as hand-written assumptions.
--
-- Replaces the file hax generates in `Assumptions/`: hax never overwrites it, and the check
-- copies this one over the generated one. Everything declared here is something the extracted
-- code calls but neither Aeneas nor hax's `CoreModels` defines. The check fails if a theorem of
-- `Proofs.lean` depends on any of them. Only `Conflict`'s `PartialEq` (`has_duplicates`) and
-- `Debug` use them; `join` does not.
module
public import Aeneas
public import CoreModels
public import ConcurrentPsbt.Extraction.Types
@[expose] public section
open CoreModels Aeneas
open Aeneas.Std hiding namespace core alloc
open concurrent_psbt

/-- `<[T]>::split_first`, which `CoreModels` has no model of. -/
axiom CoreModels.core.slice.Slice.split_first
  {T : Type _} (s : Slice T) : RustM (core.option.Option (T × Slice T))

/-- `Debug for Conflict<V>`. The hax-generated declaration returns an extra write-back
function that `core.fmt.Debug.fmt` of `CoreModels` does not have, so it does not typecheck. -/
axiom lattice.partial.Conflict.Insts.CoreFmtDebug.fmt
  {V : Type _} (corefmtDebugInst : core.fmt.Debug V) (PartialJoinInst :
  lattice.partial.PartialJoin V) :
  lattice.partial.Conflict V → core.fmt.Formatter → RustM
    ((core.result.Result Unit core.fmt.Error) × core.fmt.Formatter)
