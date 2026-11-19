import JoinResult
import Modulo
import PaymentGraph
import PaymentConfirmation

-- Audited by the nix check: none of these may depend on `sorryAx`.
#print axioms ConcurrentPsbt.flat_idem
#print axioms ConcurrentPsbt.flat_comm
#print axioms ConcurrentPsbt.flat_assoc
#print axioms ConcurrentPsbt.assoc_forces_flat
#print axioms ConcurrentPsbt.option_not_assoc
#print axioms ConcurrentPsbt.option_assoc_fails
#print axioms ConcurrentPsbt.Modulo.denote_join
#print axioms ConcurrentPsbt.Modulo.join_idem
#print axioms ConcurrentPsbt.Modulo.join_comm
#print axioms ConcurrentPsbt.Modulo.join_assoc
#print axioms ConcurrentPsbt.Modulo.join_congr
#print axioms ConcurrentPsbt.Modulo.self_join_equiv
#print axioms ConcurrentPsbt.Modulo.option_converges
#print axioms ConcurrentPsbt.Modulo.option_nontrivial
#print axioms ConcurrentPsbt.Modulo.flat_equiv_members
#print axioms ConcurrentPsbt.Modulo.above_sup
#print axioms ConcurrentPsbt.Modulo.above_injective
#print axioms ConcurrentPsbt.Modulo.gset_sup
#print axioms PaymentGraph.net_has_no_opposites
#print axioms PaymentGraph.net_idempotent
#print axioms PaymentGraph.net_preserves_balance
#print axioms PaymentGraph.internal_conservation
#print axioms PaymentGraph.exists_nonnegative
#print axioms PaymentGraph.validated_peeling
#print axioms PaymentGraph.peeling_order
#print axioms PaymentGraph.netted_peeling_order
#print axioms PaymentConfirmation.Trace.append
#print axioms PaymentConfirmation.Trace.one
#print axioms PaymentConfirmation.initial_sound
#print axioms PaymentConfirmation.step_sound
#print axioms PaymentConfirmation.trace_sound
#print axioms PaymentConfirmation.confirmed_protected
#print axioms PaymentConfirmation.confirmed_accounting_safe
#print axioms PaymentConfirmation.confirm_all
#print axioms PaymentConfirmation.deliver_all
#print axioms PaymentConfirmation.sign_all
#print axioms PaymentConfirmation.default_completion
