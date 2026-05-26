use std::marker::PhantomData;

use psbt_v2::v2::Psbt;

use crate::lattice::join::Join;
use crate::sorter::{SortMode, check_sort_mode};
use crate::tx::{ResultUnorderedPsbt, UnorderedPsbtError};

use super::{Constructor, Modifiable};

/// Result-domain version of [`Constructor`]. Implements [`Join`] for N-way merging.
///
/// Obtain via [`Constructor::wrap`], [`Constructor::try_join`], or
/// [`ResultConstructor::try_from_psbt`]. Use [`is_ok`](Self::is_ok) to check
/// for conflicts and [`try_unwrap`](Self::try_unwrap) to recover a clean [`Constructor`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResultConstructor<M, S>(
    pub(super) ResultUnorderedPsbt,
    pub(super) PhantomData<(M, S)>,
);

impl<M, S> ResultConstructor<M, S> {
    /// Parse a v2 PSBT directly into the result domain.
    ///
    /// This is the entry point for N-way joins: parse each PSBT into
    /// `ResultConstructor`, then fold with `Join`.
    ///
    /// Unlike `Constructor::try_from_psbt`, this does **not** validate
    /// `tx_modifiable_flags` or the sort fields. In the result domain, PSBTs
    /// with differing flags are loaded as-is; mismatches surface when
    /// joined, which is the correct behavior for N-way merges.
    ///
    /// # Errors
    /// Returns the errors of [`ResultUnorderedPsbt::try_from_psbt`].
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, UnorderedPsbtError> {
        Ok(Self(ResultUnorderedPsbt::try_from_psbt(psbt)?, PhantomData))
    }
}

impl<M: Modifiable, S: SortMode> ResultConstructor<M, S> {
    /// Return `true` if [`try_unwrap`](Self::try_unwrap) would succeed.
    pub fn is_ok(&self) -> bool {
        ResultConstructor::<M, S>(self.0.clone(), PhantomData)
            .try_unwrap()
            .is_ok()
    }

    /// Extract a clean [`Constructor`] if no conflicts remain,
    /// `tx_modifiable_flags` match `M`, and the sort fields select `S`.
    ///
    /// # Errors
    /// Returns `Err(self)` otherwise.
    pub fn try_unwrap(self) -> Result<Constructor<M, S>, Self> {
        match self.0.try_unwrap() {
            Ok(psbt)
                if psbt.global.tx_modifiable_flags & 0x03 == M::EXPECTED_FLAGS
                    && check_sort_mode::<S>(&psbt).is_ok() =>
            {
                Ok(Constructor(psbt, PhantomData))
            }
            Ok(psbt) => Err(ResultConstructor(psbt.wrap(), PhantomData)),
            Err(result) => Err(ResultConstructor(result, PhantomData)),
        }
    }
}

impl<M, S> Join for ResultConstructor<M, S> {
    fn join(self, other: Self) -> Self {
        ResultConstructor(self.0.join(other.0), PhantomData)
    }
}
