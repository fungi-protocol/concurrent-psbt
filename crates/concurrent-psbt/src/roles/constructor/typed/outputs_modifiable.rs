use psbt_v2::v2::{Output, Psbt};

use crate::output::{OutputUniqueIdExt, UniqueId};
use crate::sorter::{Sorter, Unset};
use crate::tx::UnorderedPsbt;

use super::super::{ConstructorError, OutputsModifiable, validate_flags};
use super::Constructor;

impl Constructor<OutputsModifiable, Unset> {
    /// Parse a v2 PSBT into an outputs-modifiable constructor.
    ///
    /// # Errors
    /// Returns [`ConstructorError::FlagsMismatch`] if `tx_modifiable_flags`
    /// does not have bit 1 set and bit 0 clear, or propagates lower-level
    /// unordered PSBT parse errors.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        validate_flags(psbt.global.tx_modifiable_flags, 0x02)?;
        Ok(Constructor::from_unordered(UnorderedPsbt::try_from_psbt(
            psbt,
        )?))
    }
}

impl<S> Constructor<OutputsModifiable, S> {
    /// Add an output to the PSBT.
    pub fn output(mut self, output: Output) -> Self {
        self.0.outputs.add(output);
        self
    }

    /// Add an output, stamping a freshly generated [`UniqueId`].
    ///
    /// Each call generates a new random `PSBT_OUT_UNIQUE_ID`, so adding
    /// copies of the same txout yields distinct outputs.
    pub fn output_with_new_uid(self, mut output: Output) -> Self {
        output.set_unique_id(UniqueId::generate());
        self.output(output)
    }

    /// Finalize outputs and transition to the [`Sorter`] for ordering.
    pub fn no_more_outputs(self) -> Sorter<S> {
        Sorter::from_unordered_psbt(self.0)
    }
}
