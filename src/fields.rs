//! Proprietary PSBT field definitions for unordered transaction construction.
//!
//! All fields use the `PSBT_PROPRIETARY` type (`0xFC`) with a shared prefix.
//! The field name is used as keydata.

use psbt_v2::raw::ProprietaryKey;
use psbt_v2::v2::Global;

/// Prefix for all proprietary keys defined by this specification.
const PREFIX: &[u8] = b"lattice";

// TODO can this be const?
fn prop(subtype: u8, key: &[u8]) -> ProprietaryKey {
    ProprietaryKey {
        prefix: PREFIX.to_vec(),
        subtype,
        key: key.to_vec(),
    }
}

// -- Subtypes ----------------------------------------------------------------

/// Subtype for global fields.
const SUBTYPE_GLOBAL: u8 = 0x00;

/// Subtype for per-input fields.
const SUBTYPE_INPUT: u8 = 0x01;

/// Subtype for per-output fields.
const SUBTYPE_OUTPUT: u8 = 0x02;

// -- Global fields -----------------------------------------------------------

/// `PSBT_GLOBAL_TX_UNORDERED`
///
/// Value: unsigned 8-bit integer, MUST be `0x03`.
/// Indicates that both inputs and outputs are unordered sets.
pub fn psbt_global_tx_unordered() -> ProprietaryKey {
    prop(SUBTYPE_GLOBAL, b"PSBT_GLOBAL_TX_UNORDERED")
}

/// The required value for `PSBT_GLOBAL_TX_UNORDERED`.
/// Bit 0 = inputs unordered, Bit 1 = outputs unordered.
pub const UNORDERED_VALUE: u8 = 0x03;

/// `PSBT_GLOBAL_SORT_SEED`
///
/// Value: at least 128 bits (16 bytes) of randomness.
pub fn psbt_global_sort_seed() -> ProprietaryKey {
    prop(SUBTYPE_GLOBAL, b"PSBT_GLOBAL_SORT_SEED")
}

/// `PSBT_GLOBAL_SORT_DETERMINISTIC`
///
/// Value: `0x00` (explicit sort keys required) or `0x01` (deterministic from seed).
pub fn psbt_global_sort_deterministic() -> ProprietaryKey {
    prop(SUBTYPE_GLOBAL, b"PSBT_GLOBAL_SORT_DETERMINISTIC")
}

// -- Per-input fields --------------------------------------------------------

/// `PSBT_IN_SORT_KEY`
///
/// Value: up to 32 bytes of arbitrary data used as a lexicographic sort key.
pub fn psbt_in_sort_key() -> ProprietaryKey {
    prop(SUBTYPE_INPUT, b"PSBT_IN_SORT_KEY")
}

// -- Per-output fields -------------------------------------------------------

/// `PSBT_OUT_UNIQUE_ID`
///
/// Value: 16 bytes of randomness, universally unique.
pub fn psbt_out_unique_id() -> ProprietaryKey {
    prop(SUBTYPE_OUTPUT, b"PSBT_OUT_UNIQUE_ID")
}

/// `PSBT_OUT_SORT_KEY`
///
/// Value: up to 32 bytes of arbitrary data used as a lexicographic sort key.
pub fn psbt_out_sort_key() -> ProprietaryKey {
    prop(SUBTYPE_OUTPUT, b"PSBT_OUT_SORT_KEY")
}

// -- Global field accessors --------------------------------------------------

/// Extension trait for reading and writing custom proprietary global fields.
pub(crate) trait GlobalFieldsExt {
    /// Returns `true` if `PSBT_GLOBAL_TX_UNORDERED` is set to [`UNORDERED_VALUE`].
    fn is_tx_unordered(&self) -> bool;
    /// Sets `PSBT_GLOBAL_TX_UNORDERED` to [`UNORDERED_VALUE`].
    fn set_tx_unordered(&mut self);
    /// Removes `PSBT_GLOBAL_TX_UNORDERED`.
    fn clear_tx_unordered(&mut self);

    /// Returns `true` if `PSBT_GLOBAL_SORT_DETERMINISTIC` is absent (Relaxed mode).
    fn sort_deterministic_absent(&self) -> bool;
    /// Returns `true` if `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x00` (ExplicitSortKeys).
    fn is_sort_explicit(&self) -> bool;
    /// Returns `true` if `PSBT_GLOBAL_SORT_DETERMINISTIC` is `0x01` (Deterministic).
    fn is_sort_deterministic(&self) -> bool;
    /// Sets `PSBT_GLOBAL_SORT_DETERMINISTIC` to `0x00` (ExplicitSortKeys).
    fn set_sort_explicit(&mut self);
    /// Sets `PSBT_GLOBAL_SORT_DETERMINISTIC` to `0x01` (Deterministic).
    fn set_sort_deterministic(&mut self);

    /// Returns the sort seed if set.
    fn sort_seed(&self) -> Option<&Vec<u8>>;
    /// Sets the sort seed.
    fn set_sort_seed(&mut self, seed: Vec<u8>);
}

impl GlobalFieldsExt for Global {
    fn is_tx_unordered(&self) -> bool {
        self.proprietaries
            .get(&psbt_global_tx_unordered())
            .is_some_and(|v| v.as_slice() == [UNORDERED_VALUE])
    }

    fn set_tx_unordered(&mut self) {
        self.proprietaries
            .insert(psbt_global_tx_unordered(), vec![UNORDERED_VALUE]);
    }

    fn clear_tx_unordered(&mut self) {
        self.proprietaries.remove(&psbt_global_tx_unordered());
    }

    fn sort_deterministic_absent(&self) -> bool {
        !self
            .proprietaries
            .contains_key(&psbt_global_sort_deterministic())
    }

    fn is_sort_explicit(&self) -> bool {
        self.proprietaries
            .get(&psbt_global_sort_deterministic())
            .is_some_and(|v| v.as_slice() == [0x00])
    }

    fn is_sort_deterministic(&self) -> bool {
        self.proprietaries
            .get(&psbt_global_sort_deterministic())
            .is_some_and(|v| v.as_slice() == [0x01])
    }

    fn set_sort_explicit(&mut self) {
        self.proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x00]);
    }

    fn set_sort_deterministic(&mut self) {
        self.proprietaries
            .insert(psbt_global_sort_deterministic(), vec![0x01]);
    }

    // FIXME deterministic_sort_seed
    fn sort_seed(&self) -> Option<&Vec<u8>> {
        self.proprietaries.get(&psbt_global_sort_seed())
    }

    // FIXME rename set_deterministic_sort_seed
    fn set_sort_seed(&mut self, seed: Vec<u8>) {
        self.proprietaries.insert(psbt_global_sort_seed(), seed);
    }
}

// -- Modifiable flag helpers -------------------------------------------------
// The upstream methods are pub(crate), so we manipulate the bits directly.

const INPUTS_MODIFIABLE: u8 = 0x01;
const OUTPUTS_MODIFIABLE: u8 = 0x02;

/// Extension trait for reading and writing the modifiable flags on `Global`.
#[allow(dead_code)]
pub(crate) trait GlobalModifiableExt {
    fn is_inputs_modifiable(&self) -> bool;
    fn is_outputs_modifiable(&self) -> bool;
    fn clear_inputs_modifiable(&mut self);
    fn clear_outputs_modifiable(&mut self);
    fn set_inputs_modifiable(&mut self);
    fn set_outputs_modifiable(&mut self);
}

impl GlobalModifiableExt for Global {
    fn is_inputs_modifiable(&self) -> bool {
        self.tx_modifiable_flags & INPUTS_MODIFIABLE != 0
    }

    fn is_outputs_modifiable(&self) -> bool {
        self.tx_modifiable_flags & OUTPUTS_MODIFIABLE != 0
    }

    fn clear_inputs_modifiable(&mut self) {
        self.tx_modifiable_flags &= !INPUTS_MODIFIABLE;
    }

    fn clear_outputs_modifiable(&mut self) {
        self.tx_modifiable_flags &= !OUTPUTS_MODIFIABLE;
    }

    fn set_inputs_modifiable(&mut self) {
        self.tx_modifiable_flags |= INPUTS_MODIFIABLE;
    }

    fn set_outputs_modifiable(&mut self) {
        self.tx_modifiable_flags |= OUTPUTS_MODIFIABLE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psbt_v2::v2::Creator as Bip370Creator;

    fn make_global() -> Global {
        Bip370Creator::new().psbt().global
    }

    #[test]
    fn set_and_clear_inputs_modifiable() {
        let mut g = make_global();
        g.tx_modifiable_flags = 0;
        assert!(!g.is_inputs_modifiable());

        g.set_inputs_modifiable();
        assert!(g.is_inputs_modifiable());

        g.clear_inputs_modifiable();
        assert!(!g.is_inputs_modifiable());
    }

    #[test]
    fn set_and_clear_outputs_modifiable() {
        let mut g = make_global();
        g.tx_modifiable_flags = 0;
        assert!(!g.is_outputs_modifiable());

        g.set_outputs_modifiable();
        assert!(g.is_outputs_modifiable());

        g.clear_outputs_modifiable();
        assert!(!g.is_outputs_modifiable());
    }

    #[test]
    fn set_preserves_other_flag() {
        let mut g = make_global();
        g.tx_modifiable_flags = 0;

        g.set_inputs_modifiable();
        g.set_outputs_modifiable();
        assert!(g.is_inputs_modifiable());
        assert!(g.is_outputs_modifiable());

        g.clear_inputs_modifiable();
        assert!(!g.is_inputs_modifiable());
        assert!(g.is_outputs_modifiable());
    }
}
