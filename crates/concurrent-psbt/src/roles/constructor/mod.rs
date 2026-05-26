//! The BIP 370 Constructor role: adding inputs and outputs.
//!
//! [`typed::Constructor`] tracks modifiability in the type system.
#![allow(clippy::result_large_err)]

pub mod typed;

pub use typed::{Constructor, ResultConstructor};

use std::fmt;

use psbt_v2::v2::Output;

use crate::tx::{ResultUnorderedPsbt, UnorderedPsbtError};

/// Sealed trait for modifiability markers, carrying the expected `tx_modifiable_flags` value.
///
/// Used by [`ResultConstructor::try_unwrap`](ResultConstructor::try_unwrap)
/// to re-validate flags when extracting a typed [`Constructor`] from the result domain.
pub trait Modifiability {
    /// The expected value of `tx_modifiable_flags & 0x03` for this modifiability.
    const EXPECTED_FLAGS: u8;
}

/// Modifiability marker: both inputs and outputs may be added (`tx_modifiable_flags` bits 0+1).
#[derive(Debug, Clone, PartialEq)]
pub enum BothModifiable {}
impl Modifiability for BothModifiable {
    const EXPECTED_FLAGS: u8 = 0x03;
}

/// Modifiability marker: only inputs may be added (`tx_modifiable_flags` bit 0).
#[derive(Debug, Clone, PartialEq)]
pub enum InputsModifiable {}
impl Modifiability for InputsModifiable {
    const EXPECTED_FLAGS: u8 = 0x01;
}

/// Modifiability marker: only outputs may be added (`tx_modifiable_flags` bit 1).
#[derive(Debug, Clone, PartialEq)]
pub enum OutputsModifiable {}
impl Modifiability for OutputsModifiable {
    const EXPECTED_FLAGS: u8 = 0x02;
}

/// Error when constructing a [`Constructor`] from a PSBT.
#[derive(Debug)]
pub enum ConstructorError {
    /// An output is missing the `PSBT_OUT_UNIQUE_ID` field.
    MissingUniqueId(Box<Output>),
    /// The unordered PSBT accumulated conflicting fields while parsing.
    Conflict(Box<ResultUnorderedPsbt>),
    /// The PSBT's `tx_modifiable_flags` don't match the requested modifiability.
    FlagsMismatch {
        /// The flags value required by the modifiability type.
        expected: u8,
        /// The actual `tx_modifiable_flags & 0x03` from the PSBT.
        actual: u8,
    },
}

impl fmt::Display for ConstructorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUniqueId(_) => write!(f, "output missing PSBT_OUT_UNIQUE_ID"),
            Self::Conflict(_) => write!(f, "unordered PSBT contains conflicting fields"),
            Self::FlagsMismatch { expected, actual } => {
                write!(
                    f,
                    "tx_modifiable_flags mismatch: expected 0x{expected:02x}, got 0x{actual:02x}"
                )
            }
        }
    }
}

impl std::error::Error for ConstructorError {}

impl From<Output> for ConstructorError {
    fn from(output: Output) -> Self {
        ConstructorError::MissingUniqueId(Box::new(output))
    }
}

impl From<UnorderedPsbtError> for ConstructorError {
    fn from(error: UnorderedPsbtError) -> Self {
        match error {
            UnorderedPsbtError::MissingOutputUniqueId(output) => {
                ConstructorError::MissingUniqueId(output)
            }
            UnorderedPsbtError::Conflict(result) => ConstructorError::Conflict(result),
        }
    }
}

/// Check that `flags & 0x03` equals the expected value, returning
/// [`ConstructorError::FlagsMismatch`] if not.
fn validate_flags(flags: u8, expected: u8) -> Result<(), ConstructorError> {
    let actual = flags & 0x03;
    if actual == expected {
        Ok(())
    } else {
        Err(ConstructorError::FlagsMismatch { expected, actual })
    }
}
