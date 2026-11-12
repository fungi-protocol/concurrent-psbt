import JoinResult
import Modulo

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
