use core::marker::PhantomData;

use psbt_v2::v2::Creator as Bip370Creator;
use psbt_v2::v2::Psbt;

use crate::fields::{
    is_inputs_modifiable, is_outputs_modifiable, clear_inputs_modifiable, clear_outputs_modifiable,
    psbt_global_tx_unordered, UNORDERED_VALUE,
};
use crate::tx::UnorderedPsbt;

/// Error returned when a PSBT is not suitable for an unordered Constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The `PSBT_GLOBAL_TX_UNORDERED` field is missing or has a wrong value.
    NotUnordered,
    /// The inputs-modifiable flag is not set.
    InputsNotModifiable,
    /// The outputs-modifiable flag is not set.
    OutputsNotModifiable,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotUnordered => f.write_str("PSBT is not marked unordered"),
            Error::InputsNotModifiable => f.write_str("inputs are not modifiable"),
            Error::OutputsNotModifiable => f.write_str("outputs are not modifiable"),
        }
    }
}

// -- Typestate markers -------------------------------------------------------

/// Both inputs and outputs modifiable.
#[derive(Debug, PartialEq)]
pub enum Modifiable {}
/// Only inputs modifiable (outputs locked).
#[derive(Debug, PartialEq)]
pub enum InputsOnly {}
/// Only outputs modifiable (inputs locked).
#[derive(Debug, PartialEq)]
pub enum OutputsOnly {}

mod sealed {
    pub trait Mod {}
    impl Mod for super::Modifiable {}
    impl Mod for super::InputsOnly {}
    impl Mod for super::OutputsOnly {}
}

/// Sealed trait for typestate markers.
pub trait Mod: sealed::Mod {}
impl Mod for Modifiable {}
impl Mod for InputsOnly {}
impl Mod for OutputsOnly {}

// -- Constructor -------------------------------------------------------------

/// Unordered Constructor, mirrors the BIP 370 Constructor but for unordered PSBTs.
#[derive(Debug, PartialEq)]
pub struct Constructor<M: Mod>(UnorderedPsbt, PhantomData<M>);

impl<M: Mod> Constructor<M> {
    /// Return the inner `UnorderedPsbt`.
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    // FIXME sorting methods are missing, implement sort procedure.
    // start with the PSBT_GLOBAL_SORT_DETERMINISTIC checks, asserting that if 0x00 all sort keys
    // are defined, and if 0x01 none of them are.
    //
    // then, temporarily assert that only 0x00 is supported.
    //
    // sort by them, producing a BIP370Constructor(Psbt).
    //
    // actual implementation of the deterministic derivation of sort keys should be left todo!()
    // for now.
}

impl Constructor<Modifiable> {
    /// Wrap an existing PSBT, validating it is unordered and fully modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !is_inputs_modifiable(&unordered.global) {
            return Err(Error::InputsNotModifiable);
        }
        if !is_outputs_modifiable(&unordered.global) {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock inputs: transition to `OutputsOnly`.
    pub fn no_more_inputs(mut self) -> Constructor<OutputsOnly> {
        clear_inputs_modifiable(&mut self.0.global);
        Constructor(self.0, PhantomData)
    }

    /// Lock outputs: transition to `InputsOnly`.
    pub fn no_more_outputs(mut self) -> Constructor<InputsOnly> {
        clear_outputs_modifiable(&mut self.0.global);
        Constructor(self.0, PhantomData)
    }
}

impl Constructor<InputsOnly> {
    /// Wrap an existing PSBT, validating it is unordered and inputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !is_inputs_modifiable(&unordered.global) {
            return Err(Error::InputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock inputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_inputs(mut self) -> UnorderedPsbt {
        clear_inputs_modifiable(&mut self.0.global);
        self.0
    }
}

impl Constructor<OutputsOnly> {
    /// Wrap an existing PSBT, validating it is unordered and outputs-only modifiable.
    pub fn new(psbt: Psbt) -> Result<Self, Error> {
        let unordered = UnorderedPsbt::from_psbt(psbt);
        if !unordered.is_unordered() {
            return Err(Error::NotUnordered);
        }
        if !is_outputs_modifiable(&unordered.global) {
            return Err(Error::OutputsNotModifiable);
        }
        Ok(Constructor(unordered, PhantomData))
    }

    /// Lock outputs: both sides now locked, return the `UnorderedPsbt`.
    pub fn no_more_outputs(mut self) -> UnorderedPsbt {
        clear_outputs_modifiable(&mut self.0.global);
        self.0
    }
}

// -- Creator -----------------------------------------------------------------

/// Creator for unordered PSBTs.
///
/// Sets the `PSBT_GLOBAL_TX_UNORDERED` proprietary field and both modifiable
/// flags, producing a PSBT ready for an unordered `Constructor`.
pub struct Creator(UnorderedPsbt);

impl Creator {
    pub fn new() -> Self {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();

        let mut unordered = UnorderedPsbt::from_psbt(psbt);

        unordered.global
            .proprietaries
            .insert(psbt_global_tx_unordered(), vec![UNORDERED_VALUE]);

        Creator(unordered)
    }

    /// Consume the creator and return the `UnorderedPsbt`.
    pub fn into_psbt(self) -> UnorderedPsbt {
        self.0
    }

    /// Consume the creator and return a fully-modifiable Constructor.
    pub fn constructor(self) -> Constructor<Modifiable> {
        // Convert back to Psbt for Constructor::new validation path.
        Constructor::<Modifiable>::new(self.0.to_psbt())
            .expect("Creator always produces a valid unordered PSBT")
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self::new()
    }
}

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_produces_valid_constructor() {
        let c = Creator::new().constructor();
        let unordered: UnorderedPsbt = c.into_psbt();
        assert!(unordered.is_unordered());
    }

    #[test]
    fn new_modifiable_rejects_non_unordered() {
        let psbt = Bip370Creator::new()
            .inputs_modifiable()
            .outputs_modifiable()
            .psbt();
        assert_eq!(Constructor::<Modifiable>::new(psbt), Err(Error::NotUnordered));
    }

    #[test]
    fn new_modifiable_rejects_missing_inputs_flag() {
        let mut psbt = Creator::new().into_psbt().to_psbt();
        clear_inputs_modifiable(&mut psbt.global);
        assert_eq!(Constructor::<Modifiable>::new(psbt), Err(Error::InputsNotModifiable));
    }

    #[test]
    fn new_modifiable_rejects_missing_outputs_flag() {
        let mut psbt = Creator::new().into_psbt().to_psbt();
        clear_outputs_modifiable(&mut psbt.global);
        assert_eq!(Constructor::<Modifiable>::new(psbt), Err(Error::OutputsNotModifiable));
    }

    #[test]
    fn no_more_inputs_then_no_more_outputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_inputs(); // Modifiable → OutputsOnly
        let unordered = c.no_more_outputs(); // OutputsOnly → UnorderedPsbt
        assert!(!is_inputs_modifiable(&unordered.global));
        assert!(!is_outputs_modifiable(&unordered.global));
    }

    #[test]
    fn no_more_outputs_then_no_more_inputs() {
        let c = Creator::new().constructor();
        let c = c.no_more_outputs(); // Modifiable → InputsOnly
        let unordered = c.no_more_inputs(); // InputsOnly → UnorderedPsbt
        assert!(!is_inputs_modifiable(&unordered.global));
        assert!(!is_outputs_modifiable(&unordered.global));
    }

    #[test]
    fn inputs_only_new_rejects_locked_inputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_inputs().no_more_outputs();
        assert_eq!(Constructor::<InputsOnly>::new(unordered.to_psbt()), Err(Error::InputsNotModifiable));
    }

    #[test]
    fn outputs_only_new_rejects_locked_outputs() {
        let c = Creator::new().constructor();
        let unordered = c.no_more_outputs().no_more_inputs();
        assert_eq!(Constructor::<OutputsOnly>::new(unordered.to_psbt()), Err(Error::OutputsNotModifiable));
    }
}
