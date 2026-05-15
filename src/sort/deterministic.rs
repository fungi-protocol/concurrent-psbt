//! [`Sorter<Deterministic<_>>`]: checked constructors, seed transition, and sort impls.

use psbt_v2::v2::Psbt;

use super::sorter::{derive_sort_key, sort_by_extracted_key, Sorter, SorterError};
use super::traits::{Deterministic, Seeded, Sortable, TrySortable, Unseeded};

impl Sorter<Deterministic<Seeded>> {
    /// Construct from an unordered PSBT, validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x01` and a seed is present.
    pub fn new(psbt: crate::psbt::tx::UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.is_sort_deterministic() {
            return Err(SorterError::SortModeMismatch);
        }
        if psbt.global.deterministic_sort_seed().is_none() {
            return Err(SorterError::MissingSeed);
        }
        Ok(Self::new_unchecked(psbt))
    }

    /// Sort by seed-derived keys (infallible — keys are always derivable from seed).
    pub fn sort(self) -> Psbt {
        use crate::fields::GlobalFieldsExt as _;
        use crate::psbt::input::InputExt as _;
        use crate::psbt::output::OutputExt as _;
        use super::sorter::OutPointIdentifier as _;
        let seed = self.0.global.deterministic_sort_seed()
            .expect("Deterministic<Seeded> always has a seed")
            .clone();
        let inputs = sort_by_extracted_key(self.0.inputs, |i| {
            Some(derive_sort_key(&seed, &i.out_point().to_identifier()))
        })
        .expect("derived keys are always present and distinct");
        let outputs = sort_by_extracted_key(self.0.outputs, |o| {
            Some(derive_sort_key(&seed, &o.unique_id()))
        })
        .expect("derived keys are always present and distinct");
        let mut global = self.0.global;
        global.clear_tx_unordered();
        Psbt { global, inputs, outputs }
    }

    /// `try_sort` delegates to `sort` — always succeeds.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        Ok(self.sort())
    }
}

impl TrySortable for Sorter<Deterministic<Seeded>> {
    fn try_sort_psbt(self) -> Result<Psbt, crate::constructor::SortingError> {
        self.try_sort()
    }
}

impl Sortable for Sorter<Deterministic<Seeded>> {
    fn sort_psbt(self) -> Psbt {
        self.sort()
    }
}

impl Sorter<Deterministic<Unseeded>> {
    /// Construct from an unordered PSBT, validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x01` (no seed required).
    pub fn new(psbt: crate::psbt::tx::UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.is_sort_deterministic() {
            return Err(SorterError::SortModeMismatch);
        }
        Ok(Self::new_unchecked(psbt))
    }
}
