import Mathlib.Algebra.Order.BigOperators.Group.Finset
import Mathlib.Data.Finset.Card
import Mathlib.Data.Finset.Prod

/-! Finite weighted payment graphs and pairwise netting of GRANT totals. -/
namespace PaymentGraph

variable {V : Type*}

/-- Cancel opposite GRANT totals, leaving only their net difference. -/
def net (w : V → V → ℕ) (u v : V) : ℕ := w u v - w v u

/-- Netting removes all opposing positive obligations. -/
theorem net_has_no_opposites (w : V → V → ℕ) (u v : V) :
    ¬ (0 < net w u v ∧ 0 < net w v u) := by
  unfold net
  omega

/-- Repeating the netting operation changes nothing. -/
theorem net_idempotent (w : V → V → ℕ) (u v : V) :
    net (net w) u v = net w u v := by
  unfold net
  omega

/-- Signed incoming minus outgoing balance over the participant set. -/
def balance (w : V → V → ℕ) (participants : Finset V) (v : V) : ℤ :=
  ∑ u ∈ participants, ((w u v : ℤ) - (w v u : ℤ))

/-- Pairwise netting preserves each participant's financial net balance.
    This does not claim that it preserves an unnormalized dependency graph. -/
theorem net_preserves_balance (w : V → V → ℕ) (participants : Finset V) (v : V) :
    balance (net w) participants v = balance w participants v := by
  unfold balance
  apply Finset.sum_congr rfl
  intro u hu
  unfold net
  omega

/-- Incoming grants restricted to a finite set of possible payers. -/
def incoming (w : V → V → ℕ) (payers : Finset V) (v : V) : ℕ :=
  ∑ u ∈ payers, w u v

/-- Outgoing obligations restricted to a finite set of payees. -/
def outstanding (w : V → V → ℕ) (unconfirmed : Finset V) (v : V) : ℕ :=
  ∑ u ∈ unconfirmed, w v u

/-- Internal incoming and outgoing totals count exactly the same weighted edges. -/
theorem internal_conservation (w : V → V → ℕ) (s : Finset V) :
    (∑ v ∈ s, incoming w s v) = ∑ v ∈ s, outstanding w s v := by
  unfold incoming outstanding
  exact Finset.sum_comm

/-- Every nonempty finite weighted directed graph has a nonnegative-balance
    vertex. No acyclicity or pairwise-netting premise is required. -/
theorem exists_nonnegative (w : V → V → ℕ) (s : Finset V)
    (hne : s.Nonempty) :
    ∃ v ∈ s, outstanding w s v ≤ incoming w s v := by
  by_contra h
  have hlt_each : ∀ v ∈ s, incoming w s v < outstanding w s v := by
    intro v hv
    exact lt_of_not_ge (fun ready => h ⟨v, hv, ready⟩)
  have hlt := Finset.sum_lt_sum_of_nonempty hne hlt_each
  have heq := internal_conservation w s
  omega

variable [DecidableEq V]

/-- Incoming and outgoing obligations both restricted to the remaining graph. -/
def Eligible (w : V → V → ℕ) (s : Finset V) (v : V) : Prop :=
  outstanding w s v ≤ incoming w s v

/-- A peeling derivation visits each remaining vertex once, and records
    direct validation as well as nonnegative residual balance at every step. -/
inductive PeelingOrder (w : V → V → ℕ) (validates : Finset V → V → Prop) :
    Finset V → Prop where
  | done : PeelingOrder w validates ∅
  | peel {s : Finset V} (v : V) (member : v ∈ s)
      (eligible : Eligible w s v) (checked : validates s v)
      (rest : PeelingOrder w validates (s.erase v)) : PeelingOrder w validates s

/-- A complete validated order exists if every nonempty remainder has an
    arithmetically eligible participant who passes the supplied validation. -/
theorem validated_peeling (w : V → V → ℕ) (p : Finset V)
    (validates : Finset V → V → Prop)
    (bridge : ∀ s ⊆ p, s.Nonempty →
      ∃ v ∈ s, Eligible w s v ∧ validates s v) :
    PeelingOrder w validates p := by
  suffices h : ∀ s : Finset V, s ⊆ p → PeelingOrder w validates s from h p (by rfl)
  intro s
  refine Finset.strongInductionOn s ?_
  intro remaining ih hsub
  by_cases hne : remaining.Nonempty
  · obtain ⟨v, hv, eligible, checked⟩ := bridge remaining hsub hne
    exact PeelingOrder.peel v hv eligible checked
      (ih (remaining.erase v) (Finset.erase_ssubset hv)
        (Finset.Subset.trans (Finset.erase_subset v remaining) hsub))
  · have hempty := Finset.not_nonempty_iff_eq_empty.mp hne
    subst remaining
    exact PeelingOrder.done

/-- Unconditional graph theorem, with no claim about output validation. -/
theorem peeling_order (w : V → V → ℕ) (p : Finset V) :
    PeelingOrder w (fun _ _ => True) p := by
  apply validated_peeling
  intro remaining _ hne
  obtain ⟨v, hv, eligible⟩ := exists_nonnegative w remaining hne
  exact ⟨v, hv, eligible, trivial⟩

/-- The same result applies after pairwise netting of arbitrary grant totals. -/
theorem netted_peeling_order (grants : V → V → ℕ) (p : Finset V) :
    PeelingOrder (net grants) (fun _ _ => True) p := peeling_order (net grants) p

end PaymentGraph
