//! [`Sorter<Relaxed<_>>`]: checked constructors, seed transition, and sort impls.

use psbt_v2::v2::Psbt;

use super::sorter::{sort_by_extracted_key, Sorter, SorterError};
use super::traits::{Relaxed, Seeded, Sortable, TrySortable, Unseeded};

impl Sorter<Relaxed<Seeded>> {
    /// Construct from a [`crate::psbt::tx::UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent and a seed is present.
    pub fn new(psbt: crate::psbt::tx::UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.sort_deterministic_absent() {
            return Err(SorterError::SortModeMismatch);
        }
        if psbt.global.deterministic_sort_seed().is_none() {
            return Err(SorterError::MissingSeed);
        }
        Ok(Self::new_unchecked(psbt))
    }

    /// Sort using explicit keys where present, otherwise seed-derived (infallible).
    pub fn sort(self) -> Psbt {
        use crate::fields::GlobalFieldsExt as _;
        use crate::psbt::input::InputExt as _;
        use crate::psbt::output::OutputExt as _;
        let seed = self.0.global.deterministic_sort_seed()
            .expect("Relaxed<Seeded> always has a seed")
            .clone();
        let inputs =
            sort_by_extracted_key(self.0.inputs, |i| Some(i.take_or_derive_sort_key(&seed)))
                .expect("take_or_derive always returns Some");
        let outputs =
            sort_by_extracted_key(self.0.outputs, |o| Some(o.take_or_derive_sort_key(&seed)))
                .expect("take_or_derive always returns Some");
        let mut global = self.0.global;
        global.clear_tx_unordered();
        Psbt { global, inputs, outputs }
    }

    /// `try_sort` delegates to [`sort`] — always succeeds.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        Ok(self.sort())
    }
}

impl TrySortable for Sorter<Relaxed<Seeded>> {
    fn try_sort_psbt(self) -> Result<Psbt, crate::constructor::SortingError> {
        self.try_sort()
    }
}

impl Sortable for Sorter<Relaxed<Seeded>> {
    fn sort_psbt(self) -> Psbt {
        self.sort()
    }
}

impl Sorter<Relaxed<Unseeded>> {
    /// Construct from a [`crate::psbt::tx::UnorderedPsbt`], validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent (seed not required).
    pub fn new(psbt: crate::psbt::tx::UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.sort_deterministic_absent() {
            return Err(SorterError::SortModeMismatch);
        }
        Ok(Self::new_unchecked(psbt))
    }

    /// Provide the sort seed, transitioning to [`Sorter<Relaxed<Seeded>>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> Sorter<Relaxed<Seeded>> {
        use crate::fields::GlobalFieldsExt as _;
        self.0.global.set_deterministic_sort_seed(seed);
        Sorter::new_unchecked(self.0)
    }
}
