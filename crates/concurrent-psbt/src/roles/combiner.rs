#![allow(clippy::result_large_err)]

//! BIP 174-style strict Combiner for ordered PSBTs.
//!
//! Once the sorter fixes ordering (clearing `PSBT_GLOBAL_TX_UNORDERED`),
//! the input and output sets can no longer change; Updaters and Signers
//! only add data to existing entries, so combining stays monotone.
//! Positional identity replaces the unordered keys: entry `i` of every
//! operand must describe the same input or output, and each field joins
//! in the strict result domain — disagreement is a conflict, never a pick.
//!
//! This is deliberately not a projection through
//! [`UnorderedPsbt`](crate::tx::UnorderedPsbt): unordered serialization
//! shuffles by design, while an ordered PSBT must preserve its committed
//! order, and re-parsing a sorted artifact into the CRDT domain would
//! resurrect tombstoned elements.

use psbt_v2::v2::Psbt;

use crate::global::{GlobalExt, ResultGlobal};
use crate::input::{InputExt, ResultInput};
use crate::lattice::join::Join;
use crate::lattice::partial::Conflict;
use crate::output::{OutputExt, ResultOutput};

/// Result-domain wrapper around an ordered [`Psbt`].
///
/// Produced by lifting each operand via [`ResultOrderedPsbt::from_psbt`]
/// and folding with [`Join`]. Use [`ResultOrderedPsbt::is_ok`] to check
/// for conflicts and [`ResultOrderedPsbt::try_unwrap`] to extract the
/// combined PSBT with its element order intact.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultOrderedPsbt {
    global: ResultGlobal,
    inputs: Vec<ResultInput>,
    outputs: Vec<ResultOutput>,
}

impl ResultOrderedPsbt {
    /// Lift an ordered PSBT into the result domain.
    pub fn from_psbt(psbt: Psbt) -> Self {
        let inputs: Vec<ResultInput> = psbt.inputs.into_iter().map(InputExt::wrap).collect();
        let outputs: Vec<ResultOutput> = psbt.outputs.into_iter().map(OutputExt::wrap).collect();

        let mut global = psbt.global.wrap();
        // Flag inconsistent counts as conflicts rather than silently correcting.
        // The global count fields dictate parsing; mismatches indicate a
        // malformed PSBT, surfaced as a conflict singleton.
        if let Ok(n) = &global.input_count
            && *n != inputs.len()
        {
            global.input_count = Err(Conflict::from_values([*n, inputs.len()]));
        }
        if let Ok(n) = &global.output_count
            && *n != outputs.len()
        {
            global.output_count = Err(Conflict::from_values([*n, outputs.len()]));
        }

        Self {
            global,
            inputs,
            outputs,
        }
    }

    /// Return `true` if every field is conflict-free.
    pub fn is_ok(&self) -> bool {
        self.global.is_ok()
            && self.inputs.iter().all(ResultInput::is_ok)
            && self.outputs.iter().all(ResultOutput::is_ok)
    }

    /// Visit each conflicted field across global, inputs, and outputs.
    ///
    /// The callback receives `(section, field_name, &dyn Debug)` where
    /// section is `"global"`, `"input:<index>"`, or `"output:<index>"`.
    pub fn for_each_conflict(&self, mut f: impl FnMut(&str, &str, &dyn std::fmt::Debug)) {
        self.global.for_each_conflict(|field, conflict| {
            f("global", field, conflict);
        });
        for (index, input) in self.inputs.iter().enumerate() {
            input.for_each_conflict(|field, conflict| {
                f(&format!("input:{index}"), field, conflict);
            });
        }
        for (index, output) in self.outputs.iter().enumerate() {
            output.for_each_conflict(|field, conflict| {
                f(&format!("output:{index}"), field, conflict);
            });
        }
    }

    /// Extract the combined [`Psbt`] if no conflicts remain.
    ///
    /// Element order is preserved from the operands.
    ///
    /// # Errors
    /// Returns `Err(self)` if any field contains a conflict.
    pub fn try_unwrap(self) -> Result<Psbt, Self> {
        if !self.is_ok() {
            return Err(self);
        }
        Ok(Psbt {
            global: self.global.try_unwrap().expect("verified all fields are Ok"),
            inputs: self
                .inputs
                .into_iter()
                .map(|input| input.try_unwrap().expect("verified all fields are Ok"))
                .collect(),
            outputs: self
                .outputs
                .into_iter()
                .map(|output| output.try_unwrap().expect("verified all fields are Ok"))
                .collect(),
        })
    }
}

impl Join for ResultOrderedPsbt {
    fn join(self, other: Self) -> Self {
        // Positional zip: entry i of each operand must describe the same
        // element. Operands of different lengths already conflict in the
        // strict input_count/output_count join; unpaired tails are kept so
        // the conflict report shows the full picture.
        Self {
            global: self.global.join(other.global),
            inputs: zip_join(self.inputs, other.inputs),
            outputs: zip_join(self.outputs, other.outputs),
        }
    }
}

fn zip_join<T: Join>(left: Vec<T>, right: Vec<T>) -> Vec<T> {
    let mut joined = Vec::with_capacity(left.len().max(right.len()));
    let mut left = left.into_iter();
    let mut right = right.into_iter();
    loop {
        match (left.next(), right.next()) {
            (Some(l), Some(r)) => joined.push(l.join(r)),
            (Some(l), None) => joined.push(l),
            (None, Some(r)) => joined.push(r),
            (None, None) => return joined,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use bitcoin::hashes::Hash;
    use psbt_v2::raw;
    use psbt_v2::v2::{Global, Input, Output};

    fn input(txid_byte: u8) -> Input {
        Input::new(&bitcoin::OutPoint {
            txid: bitcoin::Txid::from_byte_array([txid_byte; 32]),
            vout: 0,
        })
    }

    fn output(amount: u64) -> Output {
        Output {
            amount: bitcoin::Amount::from_sat(amount),
            ..Output::default()
        }
    }

    fn psbt(inputs: Vec<Input>, outputs: Vec<Output>) -> Psbt {
        Psbt {
            global: Global {
                input_count: inputs.len(),
                output_count: outputs.len(),
                ..Global::default()
            },
            inputs,
            outputs,
        }
    }

    fn unknown_key(type_value: u8) -> raw::Key {
        raw::Key {
            type_value,
            key: vec![],
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn combining_adds_data_to_fixed_entries() {
            // Two parties enrich the same ordered skeleton independently
            // (the update/sign phase): the combine unions their data.
            let mut left = psbt(vec![input(1), input(2)], vec![output(10)]);
            left.inputs[0].unknowns.insert(unknown_key(0xa0), vec![1]);
            let mut right = left.clone();
            right.inputs[0].unknowns.clear();
            right.inputs[1].unknowns.insert(unknown_key(0xa1), vec![2]);

            let combined = ResultOrderedPsbt::from_psbt(left)
                .join(ResultOrderedPsbt::from_psbt(right))
                .try_unwrap()
                .expect("monotone additions combine cleanly");
            assert_eq!(combined.inputs[0].unknowns[&unknown_key(0xa0)], vec![1]);
            assert_eq!(combined.inputs[1].unknowns[&unknown_key(0xa1)], vec![2]);
        }

        #[test]
        fn order_is_preserved() {
            let left = psbt(vec![input(2), input(1)], vec![output(5), output(7)]);
            let combined = ResultOrderedPsbt::from_psbt(left.clone())
                .join(ResultOrderedPsbt::from_psbt(left))
                .try_unwrap()
                .expect("identical operands combine cleanly");
            assert_eq!(
                combined.inputs[0].previous_txid,
                bitcoin::Txid::from_byte_array([2; 32])
            );
            assert_eq!(combined.outputs[0].amount.to_sat(), 5);
            assert_eq!(combined.outputs[1].amount.to_sat(), 7);
        }

        #[test]
        fn positional_identity_mismatch_conflicts() {
            // Same input set, different order: position i disagrees on the
            // outpoint, which is a conflict — ordered operands committed to
            // their order.
            let left = psbt(vec![input(1), input(2)], vec![]);
            let right = psbt(vec![input(2), input(1)], vec![]);
            let combined =
                ResultOrderedPsbt::from_psbt(left).join(ResultOrderedPsbt::from_psbt(right));
            assert!(!combined.is_ok());

            let mut sections = Vec::new();
            combined.for_each_conflict(|section, field, _| {
                sections.push((section.to_owned(), field.to_owned()));
            });
            assert!(
                sections
                    .iter()
                    .any(|(section, field)| section == "input:0" && field == "previous_txid")
            );
            assert!(combined.try_unwrap().is_err());
        }

        #[test]
        fn length_mismatch_conflicts() {
            let left = psbt(vec![input(1)], vec![]);
            let right = psbt(vec![input(1), input(2)], vec![]);
            let combined =
                ResultOrderedPsbt::from_psbt(left).join(ResultOrderedPsbt::from_psbt(right));
            assert!(!combined.is_ok());

            let mut fields = Vec::new();
            combined.for_each_conflict(|section, field, _| {
                if section == "global" {
                    fields.push(field.to_owned());
                }
            });
            assert!(fields.contains(&"input_count".to_owned()));
        }

        #[test]
        fn conflicting_output_value_conflicts() {
            let left = psbt(vec![], vec![output(10)]);
            let right = psbt(vec![], vec![output(11)]);
            let combined =
                ResultOrderedPsbt::from_psbt(left).join(ResultOrderedPsbt::from_psbt(right));
            assert!(!combined.is_ok());
            assert!(combined.try_unwrap().is_err());
        }

        #[test]
        fn malformed_count_is_a_conflict() {
            let mut malformed = psbt(vec![input(1)], vec![]);
            malformed.global.input_count = 3;
            assert!(!ResultOrderedPsbt::from_psbt(malformed).is_ok());
        }
    }
}
