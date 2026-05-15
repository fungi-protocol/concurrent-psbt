pub use psbt_v2::v2::Psbt;

use crate::global::Global;
use crate::global::GlobalExt;
use crate::global::ResultGlobal;
use crate::input::{InputSet, ResultInputSet};
use crate::lattice::join::Join;
use crate::output::{OutputSet, ResultOutputSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnorderedPsbt {
    /// The global map.
    pub global: Global,
    /// The corresponding collection for each input in the unsigned transaction.
    pub inputs: InputSet,
    /// The corresponding key-value map for each output in the unsigned transaction.
    pub outputs: OutputSet,
}

impl UnorderedPsbt {
    /// Infallible, lossy conversion from PSBT (forgets order). You probably
    /// want `crate::Constructor` instead.
    ///
    /// This constructor does not check that the PSBT is marked as unordered.
    pub(crate) fn from_psbt(psbt: Psbt) -> Self {
        Self {
            global: psbt.global,
            inputs: psbt.inputs.into_iter().collect(),
            outputs: psbt.outputs.into_iter().collect(),
        }
    }

    pub fn to_psbt(self) -> Psbt {
        Psbt {
            global: self.global,
            inputs: self.inputs.into_iter().collect(),
            outputs: self.outputs.into_iter().collect(),
        }
    }

    /// Join two `UnorderedPsbt`s.
    ///
    /// Returns `Ok` when there are no conflicts, `Err` with a
    /// conflict-annotated result otherwise.
    ///
    /// `input_count` and `output_count` are taken from the post-join set
    /// sizes, so they never cause spurious conflicts.
    pub fn join(self, other: Self) -> Result<Self, ResultUnorderedPsbt> {
        // Join content sets first so we can derive the true cardinalities.
        let inputs = self.inputs.wrap().join(other.inputs.wrap());
        let outputs = self.outputs.wrap().join(other.outputs.wrap());

        // True cardinality = number of distinct keys in the joined map,
        // regardless of whether individual values have conflicts.
        let input_count = inputs.len();
        let output_count = outputs.len();

        // Sync both globals to the post-join counts before joining globals,
        // so differing counts (e.g. 0 vs 1) don't produce a spurious conflict.
        let mut a_global = self.global;
        let mut b_global = other.global;
        a_global.input_count = input_count;
        b_global.input_count = input_count;
        a_global.output_count = output_count;
        b_global.output_count = output_count;

        let global = a_global.wrap().join(b_global.wrap());

        let result = ResultUnorderedPsbt { global, inputs, outputs };
        result.try_unwrap().map_err(|e| e)
    }

    pub fn wrap(self) -> ResultUnorderedPsbt {
        ResultUnorderedPsbt {
            global: self.global.wrap(),
            inputs: self.inputs.wrap(),
            outputs: self.outputs.wrap(),
        }
    }

    pub fn is_unordered(&self) -> bool {
        self.global
            .proprietaries
            .get(&crate::fields::psbt_global_tx_unordered())
            .is_some_and(|v| v.as_slice() == [crate::fields::UNORDERED_VALUE])
    }
}

#[derive(Debug)]
pub struct ResultUnorderedPsbt {
    /// The global map.
    pub global: ResultGlobal,
    /// The corresponding collection for each input in the unsigned transaction.
    pub inputs: ResultInputSet,
    /// The corresponding key-value map for each output in the unsigned transaction.
    pub outputs: ResultOutputSet,
}

impl ResultUnorderedPsbt {
    pub fn try_unwrap(self) -> Result<UnorderedPsbt, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(UnorderedPsbt {
            global: self
                .global
                .try_unwrap()
                .expect("verified all fields are Ok"),
            inputs: self
                .inputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
            outputs: self
                .outputs
                .try_unwrap()
                .expect("verified all fields are Ok"),
        })
    }

    pub fn is_ok(&self) -> bool {
        self.global.is_ok() && self.inputs.is_ok() && self.outputs.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psbt_v2::v2::Creator as Bip370Creator;

    fn make_unordered() -> UnorderedPsbt {
        let psbt = Bip370Creator::new().psbt();
        UnorderedPsbt::from_psbt(psbt)
    }

    #[test]
    fn wrap_try_unwrap_roundtrip() {
        let u = make_unordered();
        let wrapped = u.clone().wrap();
        assert!(wrapped.is_ok());
        let unwrapped = wrapped.try_unwrap().unwrap();
        assert_eq!(unwrapped, u);
    }

    #[test]
    fn is_ok_false_when_global_conflicts() {
        use crate::lattice::join::Join;

        let mut a = make_unordered();
        let mut b = make_unordered();
        a.global.input_count = 1;
        b.global.input_count = 2;

        let result = ResultUnorderedPsbt {
            global: a.global.wrap().join(b.global.wrap()),
            inputs: a.inputs.wrap(),
            outputs: a.outputs.wrap(),
        };
        assert!(!result.is_ok());
        assert!(result.try_unwrap().is_err());
    }
}
