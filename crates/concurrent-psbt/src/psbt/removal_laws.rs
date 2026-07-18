#![allow(clippy::result_large_err)]

//! CRDT-law property tests for the full 2P-set join: presence sets ∪ tombstone
//! sets, over [`ResultUnorderedPsbt`](crate::tx::ResultUnorderedPsbt).
//!
//! The join under test is exactly the production join
//! ([`ResultUnorderedPsbt::join`]) — presence via
//! [`ResultInputSet`](crate::input::ResultInputSet) /
//! [`ResultOutputSet`](crate::output::ResultOutputSet) HashMap union, tombstones
//! via the `Global.proprietaries` BTreeMap union. These tests assert that
//! join stays idempotent / commutative / associative once tombstones are in the
//! mix, and that the **live-set projection** (present-and-not-tombstoned) has
//! *remove-wins* semantics: an element concurrently added by one replica and
//! tombstoned by another is absent from the projection regardless of join order
//! (monotonicity of removal).
//!
//! Two levels of assertion:
//!
//! 1. **Raw join laws** — the `ResultUnorderedPsbt` produced by any interleaving
//!    of joins is identical (`PartialEq`). Tombstones are just proprietary keys,
//!    so this is the same law the crate already proves for `proprietaries`; we
//!    re-prove it with tombstone-shaped keys present to guard against a future
//!    refactor that special-cases them.
//!
//! 2. **Projection laws** — the *live set* (after
//!    [`live_snapshot`]) is order-independent and remove-monotone. This is the
//!    property that actually matters: even though `join` is monotone in *data*,
//!    the live projection must be a deterministic function of the joined state,
//!    and once an id is tombstoned it can never reappear in the projection by
//!    adding it again.

#[cfg(all(test, feature = "prop-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod prop {
    use std::collections::BTreeSet;

    use bitcoin::hashes::Hash as _;
    use bitcoin::{Amount, OutPoint, ScriptBuf, Txid};
    use proptest::prelude::*;
    use psbt_v2::v2::{Global, Input, Output};

    use crate::input::InputSet;
    use crate::lattice::join::Join;
    use crate::output::{OutputSet, OutputUniqueIdExt, PSBT_OUT_UNIQUE_ID_SUBTYPE, UniqueId};
    use crate::removal::{
        GlobalRemovalExt, input_removal_id, retain_live_inputs, retain_live_outputs,
    };
    use crate::tx::{ResultUnorderedPsbt, UnorderedPsbt};

    // ── tiny generators (small domains → high collision rate) ───────────────

    fn make_input(txid_byte: u8, vout: u32) -> Input {
        Input::new(&OutPoint {
            txid: Txid::from_byte_array([txid_byte; 32]),
            vout,
        })
    }

    fn make_output(uid_byte: u8) -> Output {
        let mut output = Output {
            amount: Amount::from_sat(100_000),
            script_pubkey: ScriptBuf::new_op_return([uid_byte; 20]),
            ..Output::default()
        };
        output.proprietaries.insert(
            psbt_v2::raw::ProprietaryKey {
                prefix: crate::PROPRIETARY_PREFIX.to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            },
            vec![uid_byte; 16],
        );
        output
    }

    /// A grow-only "operation" a replica may hold: an added input, an added
    /// output, an input tombstone, or an output tombstone. Ids are drawn from a
    /// tiny domain (0..4) so adds and removes collide often across replicas.
    #[derive(Debug, Clone)]
    enum Op {
        AddInput(u8),
        AddOutput(u8),
        RemoveInput(u8),
        RemoveOutput(u8),
    }

    fn arb_op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0u8..4).prop_map(Op::AddInput),
            (0u8..4).prop_map(Op::AddOutput),
            (0u8..4).prop_map(Op::RemoveInput),
            (0u8..4).prop_map(Op::RemoveOutput),
        ]
    }

    /// Build a wrapped `ResultUnorderedPsbt` from a bag of ops. Adds land in the
    /// presence sets, removes land in the tombstone band on `Global`.
    fn build(ops: &[Op]) -> ResultUnorderedPsbt {
        let mut inputs = InputSet::default();
        let mut outputs = OutputSet::default();
        let mut global = Global {
            tx_modifiable_flags: 0x03,
            ..Global::default()
        };
        for op in ops {
            match *op {
                Op::AddInput(b) => inputs.add(make_input(b, 0)),
                Op::AddOutput(b) => outputs.add(make_output(b)),
                Op::RemoveInput(b) => global.remove_input(&make_input(b, 0)),
                Op::RemoveOutput(b) => global.remove_output_id(&UniqueId::new(vec![b; 16])),
            }
        }
        UnorderedPsbt {
            global,
            inputs,
            outputs,
        }
        .wrap()
    }

    fn arb_result() -> impl Strategy<Value = ResultUnorderedPsbt> {
        proptest::collection::vec(arb_op(), 0..=6).prop_map(|ops| build(&ops))
    }

    /// The live-projection pair: (surviving input removal-ids, surviving
    /// output unique-ids). Named so `live_snapshot`'s signature stays inside
    /// clippy's `type_complexity` budget.
    type LiveSets = (BTreeSet<Vec<u8>>, BTreeSet<Vec<u8>>);

    /// The live projection of a *clean* (conflict-free) joined state: the set of
    /// surviving input removal-ids and surviving output unique-ids after
    /// tombstoning. Returns `None` if the joined state has conflicts (then the
    /// projection is undefined and the law we care about is the raw join law).
    fn live_snapshot(state: &ResultUnorderedPsbt) -> Option<LiveSets> {
        let clean = state.clone().try_unwrap().ok()?;
        let global = &clean.global;

        let mut inputs: Vec<Input> = clean.inputs.clone().into_iter().collect();
        retain_live_inputs(global, &mut inputs);
        let live_inputs: BTreeSet<Vec<u8>> = inputs.iter().map(input_removal_id).collect();

        let mut outputs: Vec<Output> = clean.outputs.clone().into_iter().collect();
        retain_live_outputs(global, &mut outputs);
        let live_outputs: BTreeSet<Vec<u8>> = outputs
            .iter()
            .filter_map(|o| OutputUniqueIdExt::unique_id(o).map(|u| u.as_bytes().to_vec()))
            .collect();

        Some((live_inputs, live_outputs))
    }

    proptest! {
        // ── 1. Raw join laws with tombstones present ────────────────────────

        #[test]
        fn join_idempotent(a in arb_result()) {
            prop_assert_eq!(a.clone().join(a.clone()), a);
        }

        #[test]
        fn join_commutative(a in arb_result(), b in arb_result()) {
            prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
        }

        #[test]
        fn join_associative(a in arb_result(), b in arb_result(), c in arb_result()) {
            prop_assert_eq!(
                a.clone().join(b.clone()).join(c.clone()),
                a.join(b.join(c)),
            );
        }

        // ── 2. Live-projection laws ─────────────────────────────────────────
        //
        // These assert the behavior of the live projection (present-and-not-
        // tombstoned). They only hold when the `removal` feature is ON — with
        // it OFF `retain_live_*` are the identity (the spec's fail-safe: ignore
        // tombstones), so a tombstoned element would still appear "live" and
        // remove-wins would (correctly, for that build) not hold. The raw join
        // laws in section 1 above hold in BOTH builds and stay ungated.
        //
        // `projection_commutative` / `projection_associative` would in fact
        // still pass with the feature off (identity is trivially order-
        // independent), but we gate the whole projection block together so the
        // suite's intent — "these test removal semantics" — is unambiguous.

        /// The live projection is order-independent: joining A then B yields the
        /// same live set as B then A. (Follows from join commutativity + the
        /// projection being a pure function of joined state, but asserted end to
        /// end.)
        #[cfg(feature = "removal")]
        #[test]
        fn projection_commutative(a in arb_result(), b in arb_result()) {
            let ab = a.clone().join(b.clone());
            let ba = b.join(a);
            prop_assert_eq!(live_snapshot(&ab), live_snapshot(&ba));
        }

        /// The live projection is associative across three replicas.
        #[cfg(feature = "removal")]
        #[test]
        fn projection_associative(
            a in arb_result(),
            b in arb_result(),
            c in arb_result(),
        ) {
            let left = a.clone().join(b.clone()).join(c.clone());
            let right = a.join(b.join(c));
            prop_assert_eq!(live_snapshot(&left), live_snapshot(&right));
        }

        /// REMOVE-WINS over concurrent ADD. If one replica adds an element and
        /// another concurrently tombstones the *same* id, the element is absent
        /// from the live projection no matter which side is joined first.
        #[cfg(feature = "removal")]
        #[test]
        fn remove_wins_over_concurrent_add_input(b in 0u8..4) {
            let adder = build(&[Op::AddInput(b)]);
            let remover = build(&[Op::RemoveInput(b)]);

            let ar = adder.clone().join(remover.clone());
            let ra = remover.join(adder);

            let id = input_removal_id(&make_input(b, 0));
            for state in [&ar, &ra] {
                let (live_inputs, _) = live_snapshot(state)
                    .expect("add∪remove of a single id is conflict-free");
                prop_assert!(
                    !live_inputs.contains(&id),
                    "tombstoned input must not appear live"
                );
            }
        }

        #[cfg(feature = "removal")]
        #[test]
        fn remove_wins_over_concurrent_add_output(b in 0u8..4) {
            let adder = build(&[Op::AddOutput(b)]);
            let remover = build(&[Op::RemoveOutput(b)]);

            let ar = adder.clone().join(remover.clone());
            let ra = remover.join(adder);

            let uid = vec![b; 16];
            for state in [&ar, &ra] {
                let (_, live_outputs) = live_snapshot(state)
                    .expect("add∪remove of a single id is conflict-free");
                prop_assert!(
                    !live_outputs.contains(&uid),
                    "tombstoned output must not appear live"
                );
            }
        }

        /// MONOTONICITY OF REMOVAL. Once tombstoned, an element stays absent
        /// under any further joins — even a join that re-adds it. Adding the
        /// element back after the tombstone exists cannot resurrect it.
        #[cfg(feature = "removal")]
        #[test]
        fn removal_is_monotone_under_readd_input(b in 0u8..4, extra in arb_result()) {
            let removed = build(&[Op::AddInput(b), Op::RemoveInput(b)]);
            let readd = build(&[Op::AddInput(b)]);

            // tombstone first, then a re-add, then arbitrary further state.
            let state = removed.join(readd).join(extra);
            if let Some((live_inputs, _)) = live_snapshot(&state) {
                let id = input_removal_id(&make_input(b, 0));
                prop_assert!(
                    !live_inputs.contains(&id),
                    "re-adding a tombstoned input must not resurrect it"
                );
            }
        }

        #[cfg(feature = "removal")]
        #[test]
        fn removal_is_monotone_under_readd_output(b in 0u8..4, extra in arb_result()) {
            let removed = build(&[Op::AddOutput(b), Op::RemoveOutput(b)]);
            let readd = build(&[Op::AddOutput(b)]);
            let state = removed.join(readd).join(extra);
            if let Some((_, live_outputs)) = live_snapshot(&state) {
                let uid = vec![b; 16];
                prop_assert!(
                    !live_outputs.contains(&uid),
                    "re-adding a tombstoned output must not resurrect it"
                );
            }
        }

        /// The tombstone set itself is grow-only: joining never shrinks the set
        /// of removed ids.
        #[test]
        fn tombstone_set_is_grow_only(a in arb_result(), b in arb_result()) {
            let (a_clean, b_clean) = match (a.clone().try_unwrap(), b.clone().try_unwrap()) {
                (Ok(a), Ok(b)) => (a, b),
                _ => return Ok(()),
            };
            let before_inputs: BTreeSet<_> =
                a_clean.global.removed_inputs().into_iter().collect();
            let joined = a.join(b);
            if let Ok(joined_clean) = joined.try_unwrap() {
                let after_inputs: BTreeSet<_> =
                    joined_clean.global.removed_inputs().into_iter().collect();
                prop_assert!(before_inputs.is_subset(&after_inputs));
                let _ = &b_clean;
            }
        }

        /// A live element (added, never tombstoned) survives the projection.
        #[test]
        fn untombstoned_input_survives(b in 0u8..4) {
            let state = build(&[Op::AddInput(b)]);
            let (live_inputs, _) = live_snapshot(&state).expect("clean");
            let id = input_removal_id(&make_input(b, 0));
            prop_assert!(live_inputs.contains(&id));
        }
    }
}
