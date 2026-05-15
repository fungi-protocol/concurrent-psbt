//! [`Sorter<ExplicitSortKeys>`]: checked constructor and sort impls.

use psbt_v2::v2::Psbt;

use super::sorter::{sort_by_extracted_key, Sorter, SorterError};
use super::traits::{ExplicitSortKeys, Sortable, TrySortable};

impl Sorter<ExplicitSortKeys> {
    /// Construct from an unordered PSBT, validating that
    /// `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x00`.
    pub fn new(psbt: crate::psbt::tx::UnorderedPsbt) -> Result<Self, SorterError> {
        use crate::fields::GlobalFieldsExt as _;
        if !psbt.global.is_sort_explicit() {
            return Err(SorterError::SortModeMismatch);
        }
        Ok(Self::new_unchecked(psbt))
    }

    /// Sort by explicit per-input/output sort keys.
    ///
    /// Returns `Err` if any key is missing or two items share the same key.
    pub fn try_sort(self) -> Result<Psbt, crate::constructor::SortingError> {
        use crate::fields::GlobalFieldsExt as _;
        use crate::psbt::input::InputExt as _;
        use crate::psbt::output::OutputExt as _;
        let inputs = sort_by_extracted_key(self.0.inputs, |i| i.take_sort_key())?;
        let outputs = sort_by_extracted_key(self.0.outputs, |o| o.take_sort_key())?;
        let mut global = self.0.global;
        global.clear_tx_unordered();
        Ok(Psbt { global, inputs, outputs })
    }

    /// Sort by explicit per-input/output sort keys (infallible variant).
    pub fn sort(self) -> Psbt {
        self.try_sort()
            .expect("ExplicitSortKeys: all sort keys must be present and distinct")
    }
}

impl TrySortable for Sorter<ExplicitSortKeys> {
    fn try_sort_psbt(self) -> Result<Psbt, crate::constructor::SortingError> {
        self.try_sort()
    }
}

impl Sortable for Sorter<ExplicitSortKeys> {
    fn sort_psbt(self) -> Psbt {
        self.sort()
    }
}
