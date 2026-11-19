import PaymentGraph

/-
Fixed transaction/ledger abstraction with an explicit input-owner set `funded`.
A funded peer can CONFIRM after direct validation without a numerical peeling
guard. An unfunded peer conservatively waits for all of its payees' receipts.
Only input owners SIGN; completion means all input owners in the session signed.

Accepted receipt provenance and the requirement for every input owner's
signature are abstraction premises. Cryptographic/script enforcement, local
validation of outputs/fees, and commitment/grant-ID binding remain outside this
file. In particular an input must really require this peer's authorization;
merely having once owned an outpoint is not enough. All predicates close over
one fixed context with all relevant obligations and input authorizers.
Legal traces require peers to obey the confirmation guards.
-/
namespace PaymentConfirmation
open PaymentGraph (incoming outstanding)
variable {V : Type*} [DecidableEq V]

structure State (V : Type*) where
  pending : Finset V
  receipts : Finset (V × V)
  signed : Finset V

def initial (p : Finset V) : State V := ⟨p, ∅, ∅⟩

def edges (w : V → V → ℕ) (p : Finset V) : Finset (V × V) :=
  (p ×ˢ p).filter (fun e => 0 < w e.1 e.2)

inductive Step (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) : State V → State V → Prop where
  | confirm {s r t} (v : V) (member : v ∈ s) (participant : v ∈ p)
      (checked : validates v)
      (protection : v ∈ funded ∨ ∀ u ∈ p, 0 < w v u → (v, u) ∈ r) :
      Step w p funded validates ⟨s, r, t⟩ ⟨s.erase v, r, t⟩
  | deliver {s r t} (e : V × V) (edge : e ∈ edges w p)
      (confirmed : e.2 ∉ s) :
      Step w p funded validates ⟨s, r, t⟩ ⟨s, insert e r, t⟩
  | sign {s r t} (v : V) (participant : v ∈ p) (owner : v ∈ funded)
      (confirmed : v ∉ s)
      (all_payees : ∀ u ∈ p, 0 < w v u → (v, u) ∈ r) :
      Step w p funded validates ⟨s, r, t⟩ ⟨s, r, insert v t⟩

inductive Trace (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) : State V → State V → Prop where
  | refl (s) : Trace w p funded validates s s
  | cons {s t u} (step : Step w p funded validates s t)
      (rest : Trace w p funded validates t u) : Trace w p funded validates s u

theorem Trace.append {w : V → V → ℕ} {p funded : Finset V}
    {validates : V → Prop} {s t u : State V}
    (a : Trace w p funded validates s t) (b : Trace w p funded validates t u) :
    Trace w p funded validates s u := by
  induction a with
  | refl => exact b
  | cons step _ ih => exact Trace.cons step (ih b)

theorem Trace.one {w : V → V → ℕ} {p funded : Finset V}
    {validates : V → Prop} {s t : State V}
    (h : Step w p funded validates s t) : Trace w p funded validates s t :=
  Trace.cons h (Trace.refl t)

def Complete (p funded : Finset V) (a : State V) : Prop :=
  ∀ v ∈ p, v ∈ funded → v ∈ a.signed

def Sound (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) (a : State V) : Prop :=
  (∀ v ∈ p, v ∉ a.pending → validates v ∧
    (v ∉ funded → ∀ u ∈ p, 0 < w v u → (v, u) ∈ a.receipts)) ∧
  (∀ e ∈ a.receipts, e ∈ edges w p ∧ e.2 ∉ a.pending) ∧
  (∀ v ∈ a.signed, v ∈ p ∧ v ∈ funded ∧ v ∉ a.pending ∧
    ∀ u ∈ p, 0 < w v u → (v, u) ∈ a.receipts)

omit [DecidableEq V] in
theorem initial_sound (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) : Sound w p funded validates (initial p) := by
  simp [Sound, initial]
  intro v hv hn
  exact (hn hv).elim

theorem step_sound {w : V → V → ℕ} {p funded : Finset V}
    {validates : V → Prop} {a b : State V}
    (h : Step w p funded validates a b) (safe : Sound w p funded validates a) :
    Sound w p funded validates b := by
  cases h with
  | confirm v member participant checked protection =>
    rcases safe with ⟨hc, hr, hs⟩
    refine ⟨?_, ?_, ?_⟩
    · intro u hu hn
      by_cases huv : u = v
      · subst u
        exact ⟨checked, fun nf => protection.resolve_left nf⟩
      · exact hc u hu (by simpa [Finset.mem_erase, huv] using hn)
    · intro e he
      obtain ⟨hedge, hn⟩ := hr e he
      exact ⟨hedge, fun hmem => hn (Finset.mem_of_mem_erase hmem)⟩
    · intro u hu
      obtain ⟨hp, hf, hn, hg⟩ := hs u hu
      exact ⟨hp, hf, fun hmem => hn (Finset.mem_of_mem_erase hmem), hg⟩
  | deliver e edge confirmed =>
    rcases safe with ⟨hc, hr, hs⟩
    refine ⟨?_, ?_, ?_⟩
    · intro v hv hn
      obtain ⟨valid, guard⟩ := hc v hv hn
      exact ⟨valid, fun nf u hu hw => Finset.mem_insert_of_mem (guard nf u hu hw)⟩
    · intro f hf
      rcases Finset.mem_insert.mp hf with rfl | hf
      · exact ⟨edge, confirmed⟩
      · exact hr f hf
    · intro v hv
      obtain ⟨hp, hf, hn, hg⟩ := hs v hv
      exact ⟨hp, hf, hn, fun u hu hw => Finset.mem_insert_of_mem (hg u hu hw)⟩
  | sign v participant owner confirmed all_payees =>
    rcases safe with ⟨hc, hr, hs⟩
    refine ⟨hc, hr, ?_⟩
    intro u hu
    rcases Finset.mem_insert.mp hu with rfl | hu
    · exact ⟨participant, owner, confirmed, all_payees⟩
    · exact hs u hu

theorem trace_sound {w : V → V → ℕ} {p funded : Finset V}
    {validates : V → Prop} {a b : State V}
    (h : Trace w p funded validates a b) (safe : Sound w p funded validates a) :
    Sound w p funded validates b := by
  induction h with
  | refl => exact safe
  | cons step _ ih => exact ih (step_sound step safe)

/-- At actual input-owner completion, every peer that released its incoming
    payments has validated and has every outgoing confirmation. The proof
    splits between the funded signing veto and the unfunded confirmation gate. -/
theorem confirmed_protected {w : V → V → ℕ} {p funded : Finset V}
    {validates : V → Prop} {a : State V}
    (trace : Trace w p funded validates (initial p) a)
    (complete : Complete p funded a) {v : V} (hv : v ∈ p) (confirmed : v ∉ a.pending) :
    validates v ∧ ∀ u ∈ p, 0 < w v u → (v, u) ∈ a.receipts ∧ u ∉ a.pending := by
  obtain ⟨hc, hr, hs⟩ := trace_sound trace (initial_sound w p funded validates)
  obtain ⟨valid, unfunded⟩ := hc v hv confirmed
  refine ⟨valid, ?_⟩
  intro u hu hw
  have receipt : (v, u) ∈ a.receipts := by
    by_cases hf : v ∈ funded
    · exact (hs v (complete v hv hf)).2.2.2 u hu hw
    · exact unfunded hf u hu hw
  exact ⟨receipt, (hr (v, u) receipt).2⟩

def DirectValid (w : V → V → ℕ) (p : Finset V) (realized : V → ℤ) (v : V) : Prop :=
  (incoming w p v : ℤ) - (outstanding w p v : ℤ) ≤ realized v

def UnconfirmedOutgoing (w : V → V → ℕ) (p : Finset V) (a : State V) (v : V) : ℕ :=
  ∑ u ∈ p, if u ∈ a.pending then w v u else 0

/-- Conditional accounting safety in the note's fixed-ledger abstraction.
    Establishing `realized` from actual transaction effects is still external. -/
theorem confirmed_accounting_safe {w : V → V → ℕ} {p funded : Finset V}
    {realized : V → ℤ} {a : State V}
    (trace : Trace w p funded (DirectValid w p realized) (initial p) a)
    (complete : Complete p funded a) {v : V} (hv : v ∈ p) (confirmed : v ∉ a.pending) :
    (incoming w p v : ℤ) - (outstanding w p v : ℤ) ≤
      realized v - (UnconfirmedOutgoing w p a v : ℤ) := by
  obtain ⟨valid, payees⟩ := confirmed_protected trace complete hv confirmed
  have zero : UnconfirmedOutgoing w p a v = 0 := by
    apply Finset.sum_eq_zero
    intro u hu
    by_cases hw : 0 < w v u
    · simp [(payees u hu hw).2]
    · have hz : w v u = 0 := Nat.eq_zero_of_not_pos hw
      simp [hz]
  simpa [zero, DirectValid] using valid

/-- Under the adopted default, input owners can initiate confirmation and
    peers without inputs have no outgoing payments to wait for. -/
theorem confirm_all (w : V → V → ℕ) (p funded : Finset V) (validates : V → Prop)
    (senders_funded : ∀ v ∈ p, (∃ u ∈ p, 0 < w v u) → v ∈ funded)
    (valid : ∀ v ∈ p, validates v) (s : Finset V) (hsub : s ⊆ p)
    (r : Finset (V × V)) (t : Finset V) :
    Trace w p funded validates ⟨s, r, t⟩ ⟨∅, r, t⟩ := by
  revert hsub
  refine Finset.strongInductionOn s ?_
  intro remaining ih hsub
  by_cases hne : remaining.Nonempty
  · obtain ⟨v, hv⟩ := hne
    have vp := hsub hv
    have guard : v ∈ funded ∨ ∀ u ∈ p, 0 < w v u → (v, u) ∈ r := by
      by_cases hf : v ∈ funded
      · exact Or.inl hf
      · right
        intro u hu hw
        exact (hf (senders_funded v vp ⟨u, hu, hw⟩)).elim
    exact Trace.cons (Step.confirm v hv vp (valid v vp) guard)
      (ih (remaining.erase v) (Finset.erase_ssubset hv)
        (Finset.Subset.trans (Finset.erase_subset v remaining) hsub))
  · have empty := Finset.not_nonempty_iff_eq_empty.mp hne
    subst remaining
    exact Trace.refl _

theorem deliver_all (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) (todo : Finset (V × V))
    (hsub : todo ⊆ edges w p) (r : Finset (V × V)) (t : Finset V) :
    Trace w p funded validates ⟨∅, r, t⟩ ⟨∅, r ∪ todo, t⟩ := by
  induction todo using Finset.induction_on with
  | empty => simpa using (Trace.refl (w := w) (p := p) (funded := funded)
      (validates := validates) (⟨∅, r, t⟩ : State V))
  | @insert e todo he ih =>
    have first := ih (fun f hf => hsub (Finset.mem_insert_of_mem hf))
    have last := Trace.one (Step.deliver (w := w) (p := p) (funded := funded)
      (validates := validates) (s := ∅) (r := r ∪ todo) (t := t) e
      (hsub (Finset.mem_insert_self _ _)) (by simp))
    simpa [Finset.union_insert] using first.append last

theorem sign_all (w : V → V → ℕ) (p funded : Finset V)
    (validates : V → Prop) (todo : Finset V)
    (hsub : todo ⊆ funded ∩ p) (r : Finset (V × V)) (t : Finset V)
    (delivered : edges w p ⊆ r) :
    Trace w p funded validates ⟨∅, r, t⟩ ⟨∅, r, t ∪ todo⟩ := by
  induction todo using Finset.induction_on with
  | empty => simpa using (Trace.refl (w := w) (p := p) (funded := funded)
      (validates := validates) (⟨∅, r, t⟩ : State V))
  | @insert v todo hv ih =>
    obtain ⟨vf, vp⟩ := Finset.mem_inter.mp (hsub (Finset.mem_insert_self v todo))
    have first := ih (fun u hu => hsub (Finset.mem_insert_of_mem hu))
    have last := Trace.one (Step.sign (w := w) (p := p) (funded := funded)
      (validates := validates) (s := ∅) (r := r) (t := t ∪ todo) v vp vf (by simp) (by
        intro u hu hw
        apply delivered
        simp [edges, vp, hu, hw]))
    simpa [Finset.union_insert] using first.append last

/-- Default-setting completion for any finite graph, including cycles.
    Requires direct validation and cooperative execution/delivery. -/
theorem default_completion (w : V → V → ℕ) (p funded : Finset V) (validates : V → Prop)
    (senders_funded : ∀ v ∈ p, (∃ u ∈ p, 0 < w v u) → v ∈ funded)
    (valid : ∀ v ∈ p, validates v) :
    ∃ a, Trace w p funded validates (initial p) a ∧ Complete p funded a := by
  have c := confirm_all w p funded validates senders_funded valid p (by rfl) ∅ ∅
  have d := deliver_all w p funded validates (edges w p) (by rfl) ∅ ∅
  have s := sign_all w p funded validates (funded ∩ p) (by rfl) (edges w p) ∅ (by rfl)
  simp only [Finset.empty_union] at d s
  refine ⟨⟨∅, edges w p, funded ∩ p⟩, c.append (d.append s), ?_⟩
  intro v vp vf
  exact Finset.mem_inter.mpr ⟨vf, vp⟩

end PaymentConfirmation
