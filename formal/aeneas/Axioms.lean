import Proofs

-- Audited by the nix check: none of these may depend on `sorryAx` or on an `axiom` the
-- extraction declares for external functions it has no model of.
#print axioms Proofs.contains_spec
#print axioms Proofs.join_mut_spec
#print axioms Proofs.join_refines
#print axioms Proofs.join_idem_of_flat
#print axioms Proofs.join_comm_of_flat
#print axioms Proofs.join_assoc_forces_flat
