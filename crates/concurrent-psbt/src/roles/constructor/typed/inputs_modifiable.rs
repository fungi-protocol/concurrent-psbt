use psbt_v2::v2::{Input, Psbt};

use crate::sorter::{Sorter, Unset};
use crate::tx::UnorderedPsbt;

use super::Constructor;
use super::super::{ConstructorError, InputsModifiable, validate_flags};

impl Constructor<InputsModifiable, Unset> {
    /// Parse a v2 PSBT into an inputs-modifiable constructor.
    ///
    /// # Errors
    /// Returns [`ConstructorError::FlagsMismatch`] if `tx_modifiable_flags`
    /// does not have bit 0 set and bit 1 clear, or propagates lower-level
    /// unordered PSBT parse errors.
    pub fn try_from_psbt(psbt: Psbt) -> Result<Self, ConstructorError> {
        validate_flags(psbt.global.tx_modifiable_flags, 0x01)?;
        Ok(Constructor::from_unordered(UnorderedPsbt::try_from_psbt(
            psbt,
        )?))
    }
}

impl<S> Constructor<InputsModifiable, S> {
    /// Add an input to the PSBT.
    pub fn input(mut self, input: Input) -> Self {
        self.0.inputs.add(input);
        self
    }

    /// Finalize inputs and transition to the [`Sorter`] for ordering.
    pub fn no_more_inputs(self) -> Sorter<S> {
        Sorter::from_unordered_psbt(self.0)
    }
}
