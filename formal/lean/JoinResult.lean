/-!
# `JoinResult` lattice laws

A model of `crates/concurrent-psbt/src/lattice/partial.rs` in core Lean 4 (no Mathlib).

`Conflict<V>` is compared as a set (its `PartialEq` ignores order), so it is modelled
as a characteristic function `V → Bool`. `impl Join for JoinResult<V>` is transcribed
arm for arm as `join`, parameterised by the `PartialJoin::try_join` in use.

Results:
* `flat_*`: when `try_join` is `Ok` iff its arguments are equal (the `Foo` test impl),
  `join` is idempotent, commutative and associative for *all* inputs.
* `assoc_forces_flat`: conversely, if `join` is associative *up to structural equality*
  then `try_join a b = Ok c` implies `a = b`. So any `PartialJoin` with a non-trivial least
  upper bound, which the `try_join` docs explicitly allow ("`Ok(lub)` ... if one exists"),
  breaks the `Join` laws of `JoinResult` under that equality.
* `option_not_assoc`: a concrete counterexample reachable from `Ok` values only.
-/

namespace ConcurrentPsbt

/-- `Conflict<V>` up to its set-equality `PartialEq`. -/
abbrev Conflict (V : Type) := V → Bool

def Conflict.singleton {V : Type} [DecidableEq V] (a : V) : Conflict V :=
  fun v => decide (v = a)

def Conflict.union {V : Type} (s t : Conflict V) : Conflict V :=
  fun v => s v || t v

inductive JoinResult (V : Type) where
  | ok (v : V)
  | err (c : Conflict V)

open JoinResult

variable {V : Type} [DecidableEq V]

/-- `impl<V: PartialJoin> Join for JoinResult<V>`. -/
def join (tryJoin : V → V → JoinResult V) : JoinResult V → JoinResult V → JoinResult V
  | ok a, ok b => tryJoin a b
  | err a, err b => err (a.union b)
  | err a, ok b => err (a.union (.singleton b))
  | ok a, err b => err ((Conflict.singleton a).union b)

/-- A flat `try_join`: the `Foo` impl from the `partial.rs` tests. -/
def flat (a b : V) : JoinResult V :=
  if a = b then ok a else err ((Conflict.singleton a).union (.singleton b))

section Flat

omit [DecidableEq V] in
/-- Two conflicts are equal when they agree pointwise. -/
private theorem err_ext {s t : Conflict V} (h : ∀ v, s v = t v) : err s = err t :=
  congrArg err (funext h)

theorem flat_idem (a : JoinResult V) : join flat a a = a := by
  cases a with
  | ok a => simp [join, flat]
  | err s => exact err_ext fun v => by simp [Conflict.union]

theorem flat_comm (a b : JoinResult V) : join flat a b = join flat b a := by
  cases a <;> cases b <;> simp only [join, flat]
  case ok.ok a b =>
    by_cases h : a = b
    · subst h; rfl
    · simp only [h, Ne.symm h, ↓reduceIte]
      exact err_ext fun v => by simp [Conflict.union, Bool.or_comm]
  all_goals exact err_ext fun v => by simp [Conflict.union, Bool.or_comm]

theorem flat_assoc (a b c : JoinResult V) :
    join flat (join flat a b) c = join flat a (join flat b c) := by
  cases a <;> cases b <;> cases c <;> rename_i x y z
  all_goals
    (try by_cases hxy : x = y) <;> (try by_cases hyz : y = z) <;> (try by_cases hxz : x = z) <;>
      (try subst_vars) <;>
      (try simp only [join, flat, *, ↓reduceIte]) <;>
      first
        | rfl
        | (apply err_ext; intro v
           simp [Conflict.union, Bool.or_assoc, Bool.or_comm, Bool.or_left_comm])

end Flat

/-- Associativity of `join` forces `try_join` to be flat: it may only return `Ok` on equal
arguments. The witness is `(Ok a ⊔ Ok b) ⊔ Err{c}` versus `Ok a ⊔ (Ok b ⊔ Err{c})`. -/
theorem assoc_forces_flat (tryJoin : V → V → JoinResult V)
    (assoc : ∀ x y z, join tryJoin (join tryJoin x y) z = join tryJoin x (join tryJoin y z))
    {a b c : V} (h : tryJoin a b = ok c) : a = b := by
  have e := assoc (ok a) (ok b) (err (.singleton c))
  simp only [join, h] at e
  injection e with e
  have ea := congrFun e a
  have eb := congrFun e b
  simp [Conflict.union, Conflict.singleton] at ea eb
  rw [ea, eb]

/-- A three-valued field where `absent` sits below the two concrete values: e.g. an
optional PSBT field that one participant has not filled in yet. -/
inductive Field where
  | absent
  | x
  | y
  deriving DecidableEq

/-- Non-flat `try_join`: `absent ⊔ v = v`, distinct concrete values conflict. -/
def optionJoin : Field → Field → JoinResult Field
  | .absent, b => ok b
  | a, .absent => ok a
  | a, b => if a = b then ok a else err ((Conflict.singleton a).union (.singleton b))

/-- Three participants contribute `absent`, `x` and `y`. Depending on gossip order they end
up with `Err{x, y}` or `Err{absent, x, y}`, which are not equal: replicas that saw the same
updates do not converge. -/
theorem option_not_assoc :
    join optionJoin (join optionJoin (ok .absent) (ok .x)) (ok .y) ≠
      join optionJoin (ok .absent) (join optionJoin (ok .x) (ok .y)) := by
  intro e
  simp only [join, optionJoin] at e
  injection e with e
  have := congrFun e .absent
  simp [Conflict.union, Conflict.singleton] at this

theorem option_assoc_fails :
    ¬ ∀ p q r : JoinResult Field,
      join optionJoin (join optionJoin p q) r = join optionJoin p (join optionJoin q r) :=
  fun assoc => nomatch assoc_forces_flat optionJoin assoc (a := .absent) (b := .x) rfl

end ConcurrentPsbt
