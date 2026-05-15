//! Creator and CreatorWith: entry points for constructing unordered PSBTs.

use core::marker::PhantomData;

use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::Modifiable;

use crate::constructor::Constructor;
use crate::fields::GlobalFieldsExt as _;
use crate::sort::{Deterministic, ExplicitSortKeys, Relaxed, Seeded, SortMode, Unseeded};
use crate::tx::UnorderedPsbt;

/// Creator for unordered PSBTs.
///
/// Sets the `PSBT_GLOBAL_TX_UNORDERED` proprietary field and both modifiable
/// flags. By default produces a [`Constructor`] with sort mode
/// [`Relaxed<Unseeded>`]. Call [`Creator::explicit_sort_keys`] or
/// [`Creator::deterministic_sorting`] to select a different sort mode.
pub struct Creator(pub(crate) UnorderedPsbt);

impl Creator {
    pub fn new() -> Self {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();

        let mut unordered = UnorderedPsbt::unchecked_from_psbt(psbt);
        unordered.global.set_tx_unordered();

        Creator(unordered)
    }

    /// Set sort mode to explicit sort keys (`PSBT_GLOBAL_SORT_DETERMINISTIC = 0x00`).
    ///
    /// All inputs and outputs must have explicit sort keys before `try_sort` is called.
    /// Mutually exclusive with [`Creator::deterministic_sorting`].
    pub fn explicit_sort_keys(mut self) -> CreatorWith<ExplicitSortKeys> {
        self.0.global.set_sort_explicit();
        CreatorWith(self.0, PhantomData)
    }

    /// Set sort mode to deterministic sorting (`PSBT_GLOBAL_SORT_DETERMINISTIC = 0x01`).
    ///
    /// Sort keys are derived from a seed; explicit per-input/output keys are not permitted.
    /// Mutually exclusive with [`Creator::explicit_sort_keys`].
    pub fn deterministic_sorting(mut self) -> CreatorWith<Deterministic<Unseeded>> {
        self.0.global.set_sort_deterministic();
        CreatorWith(self.0, PhantomData)
    }

    /// Provide a sort seed, staying in [`Relaxed`] mode → [`Relaxed<Seeded>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
        self.0.global.set_deterministic_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }

    /// Consume the creator and return the `UnorderedPsbt`.
    pub fn into_unordered_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume the creator and return a fully-modifiable Constructor with [`Relaxed<Unseeded>`] sort mode.
    pub fn constructor(self) -> Constructor<Modifiable, Relaxed<Unseeded>> {
        Constructor::<Modifiable, Relaxed<Unseeded>>::new(self.0.to_psbt())
            .expect("Creator always produces a valid unordered PSBT")
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`Creator`] with a specific sort mode already chosen.
pub struct CreatorWith<S: SortMode>(pub(crate) UnorderedPsbt, pub(crate) PhantomData<S>);

impl<S: SortMode + 'static> CreatorWith<S> {
    /// Consume and return the `UnorderedPsbt`.
    pub fn into_unordered_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume and return a fully-modifiable Constructor with sort mode `S`.
    pub fn constructor(self) -> Constructor<Modifiable, S> {
        Constructor::<Modifiable, S>::new(self.0.to_psbt())
            .expect("CreatorWith always produces a valid unordered PSBT")
    }
}

impl CreatorWith<Deterministic<Unseeded>> {
    /// Provide the sort seed, transitioning to [`Deterministic<Seeded>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Deterministic<Seeded>> {
        self.0.global.set_deterministic_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}

impl CreatorWith<Relaxed<Unseeded>> {
    /// Provide the sort seed, transitioning to [`Relaxed<Seeded>`].
    pub fn set_seed(mut self, seed: Vec<u8>) -> CreatorWith<Relaxed<Seeded>> {
        self.0.global.set_deterministic_sort_seed(seed);
        CreatorWith(self.0, PhantomData)
    }
}
