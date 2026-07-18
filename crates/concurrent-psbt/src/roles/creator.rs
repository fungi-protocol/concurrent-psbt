use std::marker::PhantomData;

use psbt_v2::v2::Global;

use crate::global::GlobalSortExt;
use crate::input::InputSet;
use crate::output::OutputSet;
use crate::sorter::{Deterministic, ExplicitSortKeys, Unset};
use crate::tx::UnorderedPsbt;

use super::constructor::{BothModifiable, Constructor, InputsModifiable, OutputsModifiable};

/// Entry point for creating new PSBTs.
///
/// Choose the sort mode before calling [`build`](Self::build). The constructor
/// carries that mode until finalization, where the [`crate::sorter::Sorter`]
/// applies the selected order.
///
/// Use [`Creator::build`] (or the modifiability-specific variants) to produce
/// a [`Constructor`] with the desired modifiability flags set in the global.
pub struct Creator<S = Unset> {
    sort: SortConfig,
    _sort: PhantomData<S>,
}

enum SortConfig {
    Unset,
    Deterministic(Vec<u8>),
    ExplicitSortKeys,
}

impl SortConfig {
    fn apply(self, global: &mut Global) {
        match self {
            Self::Unset => {}
            Self::Deterministic(seed) => {
                global.set_sort_seed(seed);
                global.set_sort_deterministic(0x01);
            }
            Self::ExplicitSortKeys => global.set_sort_deterministic(0x00),
        }
    }
}

impl Creator<Unset> {
    /// Create a new [`Creator`].
    pub fn new() -> Self {
        Creator {
            sort: SortConfig::Unset,
            _sort: PhantomData,
        }
    }

    /// Choose deterministic ordering for the constructor.
    pub fn deterministic(self, seed: Vec<u8>) -> Creator<Deterministic> {
        Creator {
            sort: SortConfig::Deterministic(seed),
            _sort: PhantomData,
        }
    }

    /// Choose explicit sort-key ordering for the constructor.
    pub fn explicit_sort_keys(self) -> Creator<ExplicitSortKeys> {
        Creator {
            sort: SortConfig::ExplicitSortKeys,
            _sort: PhantomData,
        }
    }
}

impl<S> Creator<S> {
    fn into_unordered_psbt(self, flags: u8) -> UnorderedPsbt {
        let mut global = Global {
            tx_modifiable_flags: flags,
            ..Global::default()
        };
        self.sort.apply(&mut global);
        UnorderedPsbt {
            global,
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        }
    }

    /// Build a [`Constructor`] with both inputs and outputs modifiable (`flags = 0x03`).
    pub fn build(self) -> Constructor<BothModifiable, S> {
        Constructor::from_unordered(self.into_unordered_psbt(0x03))
    }

    /// Build a [`Constructor`] with only inputs modifiable (`flags = 0x01`).
    pub fn build_inputs_modifiable(self) -> Constructor<InputsModifiable, S> {
        Constructor::from_unordered(self.into_unordered_psbt(0x01))
    }

    /// Build a [`Constructor`] with only outputs modifiable (`flags = 0x02`).
    pub fn build_outputs_modifiable(self) -> Constructor<OutputsModifiable, S> {
        Constructor::from_unordered(self.into_unordered_psbt(0x02))
    }
}

impl Default for Creator<Unset> {
    fn default() -> Self {
        Creator::new()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    use crate::global::GlobalSortExt;
    use crate::output::OutputUniqueIdExt;

    fn exercise_creator(seed: Vec<u8>) {
        let both = Creator::new().build().into_inner();
        assert_eq!(both.global.tx_modifiable_flags, 0x03);
        assert!(both.inputs.is_empty());
        assert!(both.outputs.is_empty());

        assert_eq!(
            Creator::default()
                .build_inputs_modifiable()
                .into_inner()
                .global
                .tx_modifiable_flags,
            0x01
        );
        assert_eq!(
            Creator::new()
                .build_outputs_modifiable()
                .into_inner()
                .global
                .tx_modifiable_flags,
            0x02
        );

        let deterministic = Creator::new()
            .deterministic(seed.clone())
            .build()
            .into_inner();
        assert_eq!(deterministic.global.sort_seed(), Some(seed.as_slice()));
        assert_eq!(deterministic.global.sort_deterministic(), Some(0x01));

        let explicit = Creator::new()
            .explicit_sort_keys()
            .build()
            .into_inner();
        assert_eq!(explicit.global.sort_deterministic(), Some(0x00));

        let output = psbt_v2::v2::Output::default();
        let psbt = Creator::new()
            .build()
            .output_with_new_uid(output.clone())
            .output_with_new_uid(output.clone())
            .into_psbt();
        let first = psbt.outputs[0].unique_id().expect("Creator stamps an ID");
        let second = psbt.outputs[1].unique_id().expect("Creator stamps an ID");
        assert_ne!(first, second);
        assert_eq!(first.as_bytes().len(), 16);
        assert_eq!(second.as_bytes().len(), 16);

        let outputs_only = Creator::new()
            .build_outputs_modifiable()
            .output_with_new_uid(output)
            .into_psbt();
        assert!(outputs_only.outputs[0].has_unique_id());
    }

    #[cfg(feature = "unit-tests")]
    #[test]
    fn creator_build_modes_and_output_identity() {
        exercise_creator(vec![7; 32]);
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn creator_build_modes_and_output_identity(seed in proptest::collection::vec(any::<u8>(), 0..64)) {
                exercise_creator(seed);
            }
        }
    }
}
