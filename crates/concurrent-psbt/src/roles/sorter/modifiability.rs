//! Modifiability markers: which of a PSBT's inputs and outputs may still be
//! added to, as bits 0 and 1 of `tx_modifiable_flags` record.

use std::convert::Infallible;

use psbt_v2::v2::{self, DetermineLockTimeError, Psbt, Updater};

use super::{FlagsMismatch, SortError};

pub(crate) mod sealed {
    use psbt_v2::v2::Psbt;

    pub trait Modifiability {
        /// Hand an ordered PSBT whose flags match `Self` to the next role.
        fn sorted(
            psbt: Psbt,
        ) -> Result<<Self as super::Modifiability>::Sorted, <Self as super::Modifiability>::Error>
        where
            Self: super::Modifiability;
    }
}

/// Modifiability marker, carrying the `tx_modifiable_flags` it stands for
/// and the role a sorted PSBT moves on to.
pub trait Modifiability: sealed::Modifiability {
    /// The value of `tx_modifiable_flags & 0x03` for this modifiability.
    const EXPECTED_FLAGS: u8;

    /// The role that takes over once the PSBT is sorted: the matching BIP 370
    /// constructor while anything is modifiable, the [`Updater`] otherwise.
    type Sorted;

    /// Why handing a sorted PSBT to [`Sorted`](Self::Sorted) can fail.
    type Error: Into<SortError>;
}

/// A [`Modifiability`] under which inputs or outputs may still be added:
/// every marker but [`Unmodifiable`], which only a [`Sorter`](super::Sorter)
/// holds.
pub trait Modifiable: Modifiability {}

/// Modifiability marker: both inputs and outputs may be added (`tx_modifiable_flags` bits 0+1).
#[derive(Debug, Clone, PartialEq)]
pub enum BothModifiable {}

/// Modifiability marker: only inputs may be added (`tx_modifiable_flags` bit 0).
#[derive(Debug, Clone, PartialEq)]
pub enum InputsModifiable {}

/// Modifiability marker: only outputs may be added (`tx_modifiable_flags` bit 1).
#[derive(Debug, Clone, PartialEq)]
pub enum OutputsModifiable {}

/// Modifiability marker: neither inputs nor outputs may be added.
#[derive(Debug, Clone, PartialEq)]
pub enum Unmodifiable {}

macro_rules! modifiable {
    ($marker:ident, $flags:literal, $upstream:ident) => {
        impl Modifiability for $marker {
            const EXPECTED_FLAGS: u8 = $flags;
            type Sorted = v2::Constructor<v2::$upstream>;
            type Error = Infallible;
        }

        impl Modifiable for $marker {}

        impl sealed::Modifiability for $marker {
            fn sorted(psbt: Psbt) -> Result<v2::Constructor<v2::$upstream>, Infallible> {
                Ok(v2::Constructor::<v2::$upstream>::new(psbt).expect(concat!(
                    stringify!($marker),
                    " PSBTs carry their modifiable flags"
                )))
            }
        }
    };
}

modifiable!(BothModifiable, 0x03, Modifiable);
modifiable!(InputsModifiable, 0x01, InputsOnlyModifiable);
modifiable!(OutputsModifiable, 0x02, OutputsOnlyModifiable);

impl Modifiability for Unmodifiable {
    const EXPECTED_FLAGS: u8 = 0x00;
    type Sorted = Updater;
    type Error = DetermineLockTimeError;
}

impl sealed::Modifiability for Unmodifiable {
    fn sorted(psbt: Psbt) -> Result<Updater, DetermineLockTimeError> {
        Updater::new(psbt)
    }
}

/// Check that `flags & 0x03` matches `M`.
pub(crate) fn check_flags<M: Modifiability>(flags: u8) -> Result<(), FlagsMismatch> {
    let actual = flags & 0x03;
    if actual == M::EXPECTED_FLAGS {
        Ok(())
    } else {
        Err(FlagsMismatch {
            expected: M::EXPECTED_FLAGS,
            actual,
        })
    }
}
