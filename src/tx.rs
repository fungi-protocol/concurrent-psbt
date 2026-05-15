pub use psbt_v2::v2::Psbt;

use psbt_v2::v2::{Input, Output};

/// Error returned by [`UnorderedPsbt::try_join`].
///
/// Both field-level conflicts and invariant violations (e.g. duplicate sort
/// keys) surface as [`JoinError::Conflict`]: invariant violations are
/// represented as `Conflict(vec![v])` (single-element) to distinguish them
/// from true disagreements (`Conflict(vec![v1, v2])`).
#[derive(Debug)]
pub struct JoinError(pub ResultUnorderedPsbt);

use crate::fields::GlobalFieldsExt as _;
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
    /// Build a singleton fully-modifiable unordered `UnorderedPsbt` containing only `input`.
    pub(crate) fn from_input(input: Input) -> Self {
        let psbt = psbt_v2::v2::Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .constructor_modifiable()
            .input(input)
            .psbt()
            .expect("fresh PSBT has no locktime conflict");
        let mut u = Self::unchecked_from_psbt(psbt);
        u.global.set_tx_unordered();
        u
    }

    /// Build a singleton fully-modifiable unordered `UnorderedPsbt` containing only `output`.
    pub(crate) fn from_output(output: Output) -> Self {
        let psbt = psbt_v2::v2::Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .constructor_modifiable()
            .output(output)
            .psbt()
            .expect("fresh PSBT has no locktime conflict");
        let mut u = Self::unchecked_from_psbt(psbt);
        u.global.set_tx_unordered();
        u
    }

    /// Infallible, lossy conversion from PSBT (forgets order). You probably
    /// want `crate::Constructor` instead.
    ///
    /// This constructor does not check that the PSBT is marked as unordered.
    pub(crate) fn unchecked_from_psbt(psbt: Psbt) -> Self {
        Self {
            global: psbt.global,
            inputs: psbt.inputs.into_iter().collect(),
            outputs: psbt.outputs.into_iter().collect(),
        }
    }

    /// Convert to a `Psbt`.
    ///
    /// If `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x00` and all inputs/outputs
    /// carry distinct sort keys, they are sorted by key. Otherwise the order
    /// is arbitrary (use [`Self::to_shuffled_psbt`] to force unordered output).
    /// Convert to a `Psbt` in arbitrary (hash-map) order, preserving the
    /// `PSBT_GLOBAL_TX_UNORDERED` flag.
    ///
    /// To produce a properly sorted BIP370 `Psbt`, use
    /// [`crate::constructor::Constructor::try_sort`] or
    /// [`crate::sort::Sorter`] instead.
    pub fn to_psbt(self) -> Psbt {
        self.to_shuffled_psbt()
    }

    pub fn to_shuffled_psbt(self) -> Psbt {
        Psbt {
            global: self.global,
            inputs: self.inputs.into_iter().collect(),
            outputs: self.outputs.into_iter().collect(),
        }
    }

    /// Join two `UnorderedPsbt`s.
    ///
    /// Returns `Ok` when there are no conflicts.
    /// Returns `Err(JoinError::Conflict(_))` on field-level conflicts.
    /// Join two `UnorderedPsbt`s.
    ///
    /// Returns `Ok` when there are no conflicts or invariant violations.
    /// Returns `Err(JoinError(result))` when a field-level conflict or
    /// invariant violation (e.g. duplicate sort keys) is detected.
    ///
    /// Invariant violations are embedded in the result as `Conflict(vec![v])`
    /// (single-element), distinguishable from true disagreements
    /// `Conflict(vec![v1, v2])`.
    ///
    /// `input_count` and `output_count` are derived from post-join set sizes.
    pub fn try_join(self, other: Self) -> Result<Self, JoinError> {
        self.wrap()
            .join(other.wrap())
            .try_unwrap()
            .map_err(JoinError)
    }

    pub fn wrap(self) -> ResultUnorderedPsbt {
        ResultUnorderedPsbt {
            global: self.global.wrap(),
            inputs: self.inputs.wrap(),
            outputs: self.outputs.wrap(),
        }
    }

    pub fn is_unordered(&self) -> bool {
        self.global.is_tx_unordered()
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

impl Join for ResultUnorderedPsbt {
    fn join(mut self, mut other: Self) -> Self {
        let inputs = self.inputs.join(other.inputs);
        let outputs = self.outputs.join(other.outputs);

        for global in [&mut self.global, &mut other.global] {
            global.input_count = Ok(inputs.len());
            global.output_count = Ok(outputs.len());
        }

        let global = self.global.join(other.global);

        ResultUnorderedPsbt {
            global,
            inputs,
            outputs,
        }
    }
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
        UnorderedPsbt::unchecked_from_psbt(psbt)
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
