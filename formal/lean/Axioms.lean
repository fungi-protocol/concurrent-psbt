import JoinResult

-- Audited by the nix check: none of these may depend on `sorryAx`.
#print axioms ConcurrentPsbt.flat_idem
#print axioms ConcurrentPsbt.flat_comm
#print axioms ConcurrentPsbt.flat_assoc
#print axioms ConcurrentPsbt.assoc_forces_flat
#print axioms ConcurrentPsbt.option_not_assoc
#print axioms ConcurrentPsbt.option_assoc_fails
