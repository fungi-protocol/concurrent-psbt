#![allow(clippy::result_large_err)]

//! PSBT global fields and their merging.
//!
//! [`Global`] holds the global fields of a PSBT, and [`ResultGlobal`] wraps
//! each of them in the internal result domain, enabling field-by-field merging
//! that accumulates conflicts without short-circuiting.

use std::collections::BTreeMap;

use bitcoin::bip32::{KeySource, Xpub};
use bitcoin::locktime::absolute;
use bitcoin::transaction;

use psbt_v2::raw;

use super::proprietary::ProprietaryField;

joinable_struct! {
    /// The global fields of a PSBT.
    ///
    /// Mirrors the BIP 370 global map except `PSBT_GLOBAL_VERSION`, which is
    /// always 2, and the input and output counts, which are filled in when
    /// converting to a BIP 370 PSBT.
    source: Global,
    /// Result-domain counterpart of [`Global`].
    ///
    /// Produced by joining [`Global`] values lifted with [`Global::wrap`].
    /// Use [`ResultGlobal::is_ok`] to check for conflicts and
    /// [`ResultGlobal::try_unwrap`] to extract.
    result: ResultGlobal,
    v2: psbt_v2::v2::Global,
    fields: {
        pub tx_version: transaction::Version,
        pub fallback_lock_time: Option<absolute::LockTime>,
        pub tx_modifiable_flags: u8,
        pub xpubs: BTreeMap<Xpub, KeySource>,
        pub proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        pub unknowns: BTreeMap<raw::Key, Vec<u8>>,
    },
    proprietary: {
        pub(crate) unordered: bool = TxUnorderedField,
        pub(crate) sort_key_derivation: Option<SortKeyDerivation> = SortKeyDerivationField,
        pub(crate) sort_seed: Option<SortSeed> = SortSeedField,
    },
}

impl Default for Global {
    fn default() -> Self {
        Self::from_v2_fields(psbt_v2::v2::Global::default(), false, None, None)
    }
}

/// A BIP 370 global map whose sort fields are malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidGlobal {
    /// `PSBT_GLOBAL_TX_UNORDERED` is present but its value is not `0x03`.
    UnorderedFlag(Vec<u8>),
    /// `PSBT_GLOBAL_SORT_KEY_DERIVATION` is present but its value is neither
    /// `0x00` nor `0x01`.
    SortKeyDerivation(Vec<u8>),
    /// `PSBT_GLOBAL_SORT_SEED` is shorter than [`SortSeed::MIN_LEN`].
    SortSeed(InvalidSortSeed),
}

impl std::fmt::Display for InvalidGlobal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnorderedFlag(value) => {
                write!(f, "PSBT_GLOBAL_TX_UNORDERED is {value:02x?}, not 0x03")
            }
            Self::SortKeyDerivation(value) => write!(
                f,
                "PSBT_GLOBAL_SORT_KEY_DERIVATION is {value:02x?}, not 0x00 or 0x01"
            ),
            Self::SortSeed(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for InvalidGlobal {}

/// Reads the sort fields out of the proprietary fields, and drops the version
/// and the input and output counts.
///
/// # Errors
/// Returns the first malformed sort field, in the order of
/// [`InvalidGlobal`]'s variants.
impl TryFrom<psbt_v2::v2::Global> for Global {
    type Error = InvalidGlobal;

    fn try_from(global: psbt_v2::v2::Global) -> Result<Self, InvalidGlobal> {
        Self::decode_v2(global)
    }
}

impl Global {
    /// Convert to a BIP 370 global map for a PSBT with `input_count` inputs
    /// and `output_count` outputs.
    pub(crate) fn into_v2(self, input_count: usize, output_count: usize) -> psbt_v2::v2::Global {
        self.encode_v2(psbt_v2::v2::Global {
            version: psbt_v2::Version::TWO,
            input_count,
            output_count,
            ..psbt_v2::v2::Global::default()
        })
    }
}

/// Subtype for `PSBT_GLOBAL_TX_UNORDERED`.
///
/// Value is `0x03` when set, indicating both inputs and outputs are unordered.
pub const PSBT_GLOBAL_TX_UNORDERED_SUBTYPE: u8 = 0x10;

/// Subtype for `PSBT_GLOBAL_SORT_SEED`.
///
/// At least 128 bits of randomness (or a protocol transcript commitment)
/// used to derive deterministic sort keys.
pub const PSBT_GLOBAL_SORT_SEED_SUBTYPE: u8 = 0x11;

/// A `PSBT_GLOBAL_SORT_SEED` value, at least [`SortSeed::MIN_LEN`] bytes long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSeed(Vec<u8>);

impl SortSeed {
    /// The minimum length of a sort seed in bytes (128 bits).
    pub const MIN_LEN: usize = 16;

    /// Access the raw bytes of the seed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The bytes are too short for a [`SortSeed`]; they are returned unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidSortSeed(pub Vec<u8>);

impl TryFrom<Vec<u8>> for SortSeed {
    type Error = InvalidSortSeed;

    fn try_from(bytes: Vec<u8>) -> Result<Self, InvalidSortSeed> {
        if bytes.len() < Self::MIN_LEN {
            return Err(InvalidSortSeed(bytes));
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for InvalidSortSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sort seed is {} bytes, fewer than {}",
            self.0.len(),
            SortSeed::MIN_LEN
        )
    }
}

impl std::error::Error for InvalidSortSeed {}

impl crate::values::IdempotentValue for SortSeed {}

/// Subtype for `PSBT_GLOBAL_SORT_KEY_DERIVATION`.
///
/// `0x01` = sort keys derived from seed (explicit keys MUST NOT be set).
/// `0x00` = explicit sort keys required.
pub const PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE: u8 = 0x12;

/// A `PSBT_GLOBAL_SORT_KEY_DERIVATION` value: where sort keys come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKeyDerivation {
    /// `0x00`: every input and output needs an explicit sort key, and there is
    /// no sort seed.
    Explicit,
    /// `0x01`: every sort key is derived from the sort seed, and explicit sort
    /// keys are not allowed.
    Derived,
}

impl SortKeyDerivation {
    /// Parse a field value, returning it unchanged if it is neither `0x00` nor
    /// `0x01`.
    fn from_value(value: Vec<u8>) -> Result<Self, Vec<u8>> {
        match value.as_slice() {
            [0x00] => Ok(Self::Explicit),
            [0x01] => Ok(Self::Derived),
            _ => Err(value),
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::Explicit => 0x00,
            Self::Derived => 0x01,
        }
    }
}

impl crate::values::IdempotentValue for SortKeyDerivation {}

/// `PSBT_GLOBAL_TX_UNORDERED`: `0x03` when set, absent otherwise.
struct TxUnorderedField;

impl ProprietaryField for TxUnorderedField {
    const SUBTYPE: u8 = PSBT_GLOBAL_TX_UNORDERED_SUBTYPE;
    type Value = bool;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<bool, InvalidGlobal> {
        match value {
            None => Ok(false),
            Some(value) if value == [0x03] => Ok(true),
            Some(value) => Err(InvalidGlobal::UnorderedFlag(value)),
        }
    }

    fn encode(unordered: bool) -> Option<Vec<u8>> {
        unordered.then(|| vec![0x03])
    }
}

struct SortKeyDerivationField;

impl ProprietaryField for SortKeyDerivationField {
    const SUBTYPE: u8 = PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE;
    type Value = Option<SortKeyDerivation>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(SortKeyDerivation::from_value)
            .transpose()
            .map_err(InvalidGlobal::SortKeyDerivation)
    }

    fn encode(derivation: Self::Value) -> Option<Vec<u8>> {
        derivation.map(|derivation| vec![derivation.to_byte()])
    }
}

struct SortSeedField;

impl ProprietaryField for SortSeedField {
    const SUBTYPE: u8 = PSBT_GLOBAL_SORT_SEED_SUBTYPE;
    type Value = Option<SortSeed>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(SortSeed::try_from)
            .transpose()
            .map_err(InvalidGlobal::SortSeed)
    }

    fn encode(seed: Self::Value) -> Option<Vec<u8>> {
        seed.map(|seed| seed.0)
    }
}

/// Accessors for the sort fields. A BIP 370 global map stores them as
/// proprietary fields under [`PROPRIETARY_PREFIX`](crate::PROPRIETARY_PREFIX).
impl Global {
    /// Return `true` if `PSBT_GLOBAL_TX_UNORDERED` is set.
    pub fn is_unordered(&self) -> bool {
        self.unordered
    }

    /// Set `PSBT_GLOBAL_TX_UNORDERED`.
    pub fn set_unordered(&mut self) {
        self.unordered = true;
    }

    /// Clear `PSBT_GLOBAL_TX_UNORDERED`.
    pub fn clear_unordered(&mut self) {
        self.unordered = false;
    }

    /// Get the sort seed, if set.
    pub fn sort_seed(&self) -> Option<&[u8]> {
        self.sort_seed.as_ref().map(SortSeed::as_bytes)
    }

    /// Set the sort seed.
    pub fn set_sort_seed(&mut self, seed: SortSeed) {
        self.sort_seed = Some(seed);
    }

    /// Get `PSBT_GLOBAL_SORT_KEY_DERIVATION`, if set.
    pub fn sort_key_derivation(&self) -> Option<SortKeyDerivation> {
        self.sort_key_derivation
    }

    /// Set `PSBT_GLOBAL_SORT_KEY_DERIVATION`.
    pub fn set_sort_key_derivation(&mut self, derivation: SortKeyDerivation) {
        self.sort_key_derivation = Some(derivation);
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;

    /// A BIP 370 global map with these proprietary fields, given by subtype.
    fn v2_with_fields(fields: &[(u8, Vec<u8>)]) -> psbt_v2::v2::Global {
        let mut v2 = psbt_v2::v2::Global::default();
        for (subtype, value) in fields {
            v2.proprietaries
                .insert(crate::psbt::proprietary::key(*subtype), value.clone());
        }
        v2
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn sort_fields_are_encoded_under_the_crate_prefix() {
            let key = |subtype| raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype,
                key: vec![],
            };
            let mut global = Global::default();
            global.set_unordered();
            global.set_sort_seed(SortSeed::try_from(vec![0xaa; 16]).unwrap());
            global.set_sort_key_derivation(SortKeyDerivation::Derived);
            assert!(global.is_unordered());
            assert_eq!(global.sort_seed(), Some([0xaa; 16].as_slice()));
            assert_eq!(
                global.sort_key_derivation(),
                Some(SortKeyDerivation::Derived)
            );
            let v2 = global.clone().into_v2(0, 0);
            assert_eq!(v2.proprietaries.get(&key(0x10)), Some(&vec![0x03]));
            assert_eq!(v2.proprietaries.get(&key(0x11)), Some(&vec![0xaa; 16]));
            assert_eq!(v2.proprietaries.get(&key(0x12)), Some(&vec![0x01]));
            assert_eq!(Global::try_from(v2), Ok(global.clone()));

            global.clear_unordered();
            global.set_sort_key_derivation(SortKeyDerivation::Explicit);
            assert!(!global.is_unordered());
            let v2 = global.into_v2(0, 0);
            assert!(!v2.proprietaries.contains_key(&key(0x10)));
            assert_eq!(v2.proprietaries.get(&key(0x12)), Some(&vec![0x00]));
        }

        #[test]
        fn malformed_sort_fields_display_their_value() {
            let cases = [
                (
                    InvalidGlobal::UnorderedFlag(vec![1]),
                    "PSBT_GLOBAL_TX_UNORDERED is [01], not 0x03",
                ),
                (
                    InvalidGlobal::SortKeyDerivation(vec![2]),
                    "PSBT_GLOBAL_SORT_KEY_DERIVATION is [02], not 0x00 or 0x01",
                ),
                (
                    InvalidGlobal::SortSeed(InvalidSortSeed(vec![0; 15])),
                    "sort seed is 15 bytes, fewer than 16",
                ),
            ];
            for (error, message) in cases {
                assert_eq!(error.to_string(), message);
            }
        }

        #[test]
        fn the_first_malformed_sort_field_is_reported() {
            let unordered = (PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, vec![1]);
            let derivation = (PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE, vec![2]);
            let seed = (PSBT_GLOBAL_SORT_SEED_SUBTYPE, vec![0; 15]);
            assert_eq!(
                Global::try_from(v2_with_fields(&[
                    unordered,
                    derivation.clone(),
                    seed.clone()
                ])),
                Err(InvalidGlobal::UnorderedFlag(vec![1]))
            );
            assert_eq!(
                Global::try_from(v2_with_fields(&[derivation, seed])),
                Err(InvalidGlobal::SortKeyDerivation(vec![2]))
            );
        }

        #[test]
        fn default_is_an_empty_v2_global() {
            assert_eq!(
                Global::default().into_v2(0, 0),
                psbt_v2::v2::Global::default()
            );
        }

        #[test]
        fn globals_with_different_tx_versions_conflict() {
            let with_version = |tx_version| Global {
                tx_version,
                ..Global::default()
            };
            let joined = with_version(transaction::Version::ONE)
                .wrap()
                .join(with_version(transaction::Version::TWO).wrap());
            let mut fields = Vec::new();
            joined.for_each_conflict(|field, _, _| fields.push(field.to_owned()));
            assert_eq!(fields, ["tx_version"]);
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_proprietary_key() -> impl Strategy<Value = raw::ProprietaryKey> {
            (
                proptest::collection::vec(0u8..=255, 1..=8),
                any::<u8>(),
                proptest::collection::vec(0u8..=255, 0..=4),
            )
                .prop_map(|(prefix, subtype, key)| raw::ProprietaryKey {
                    prefix,
                    subtype,
                    key,
                })
        }

        fn arb_global() -> impl Strategy<Value = Global> {
            (
                proptest::bool::ANY,
                0u8..4,
                proptest::bool::ANY,
                proptest::bool::ANY,
                proptest::option::of(prop_oneof![
                    Just(SortKeyDerivation::Explicit),
                    Just(SortKeyDerivation::Derived),
                ]),
                proptest::option::of(proptest::collection::vec(any::<u8>(), 16..=17)),
                proptest::collection::btree_map(
                    arb_proprietary_key(),
                    proptest::collection::vec(0u8..=255, 0..=8),
                    0..=2,
                ),
                proptest::collection::btree_map(
                    (any::<u8>(), proptest::collection::vec(0u8..=255, 0..=4))
                        .prop_map(|(type_value, key)| raw::Key { type_value, key }),
                    proptest::collection::vec(0u8..=255, 0..=8),
                    0..=2,
                ),
            )
                .prop_map(
                    |(
                        use_v1,
                        flags,
                        has_lock_time,
                        unordered,
                        derivation,
                        seed,
                        proprietaries,
                        unknowns,
                    )| {
                        let mut global = Global {
                            tx_version: if use_v1 {
                                transaction::Version::ONE
                            } else {
                                transaction::Version::TWO
                            },
                            tx_modifiable_flags: flags,
                            fallback_lock_time: if has_lock_time {
                                Some(absolute::LockTime::from_consensus(500_000))
                            } else {
                                None
                            },
                            proprietaries,
                            unknowns,
                            ..Global::default()
                        };
                        if unordered {
                            global.set_unordered();
                        }
                        if let Some(derivation) = derivation {
                            global.set_sort_key_derivation(derivation);
                        }
                        if let Some(seed) = seed {
                            global
                                .set_sort_seed(SortSeed::try_from(seed).expect("16 bytes or more"));
                        }
                        global
                    },
                )
        }

        fn arb_result_global() -> impl Strategy<Value = ResultGlobal> {
            prop_oneof![
                arb_global().prop_map(|g| g.wrap()),
                (arb_global(), arb_global()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        assert_join_laws!(arb_result_global());

        proptest! {
            #[test]
            fn wrap_try_unwrap_roundtrip(a in arb_global()) {
                prop_assert_eq!(a.clone().wrap().try_unwrap(), Ok(a));
            }

            #[test]
            fn v2_roundtrip_derives_version_and_counts(
                a in arb_global(),
                input_count in 0usize..4,
                output_count in 0usize..4,
            ) {
                let v2 = a.clone().into_v2(input_count, output_count);
                prop_assert_eq!(v2.version, psbt_v2::Version::TWO);
                prop_assert_eq!((v2.input_count, v2.output_count), (input_count, output_count));
                prop_assert_eq!(Global::try_from(v2), Ok(a));
            }

            #[test]
            fn sort_fields_own_their_keys(
                a in arb_global(),
                raw in proptest::collection::vec(any::<u8>(), 0..=17),
            ) {
                let mut shadowed = a.clone();
                for subtype in [
                    PSBT_GLOBAL_TX_UNORDERED_SUBTYPE,
                    PSBT_GLOBAL_SORT_SEED_SUBTYPE,
                    PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE,
                ] {
                    shadowed
                        .proprietaries
                        .insert(crate::psbt::proprietary::key(subtype), raw.clone());
                }
                prop_assert_eq!(Global::try_from(shadowed.into_v2(0, 0)), Ok(a));
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(a in arb_result_global()) {
                prop_assert_eq!(a.is_ok(), a.try_unwrap().is_ok());
            }

            #[test]
            fn unordered_flag_is_0x03(
                value in prop_oneof![Just(vec![0x03]), proptest::collection::vec(any::<u8>(), 0..=2)],
            ) {
                let v2 = v2_with_fields(&[(PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, value.clone())]);
                match Global::try_from(v2) {
                    Ok(global) => {
                        prop_assert_eq!(value, [0x03]);
                        prop_assert!(global.is_unordered());
                    }
                    Err(error) => {
                        let bytes = format!("{value:02x?}");
                        prop_assert!(error.to_string().contains(&bytes));
                        prop_assert_eq!(error, InvalidGlobal::UnorderedFlag(value));
                    }
                }
            }

            #[test]
            fn sort_key_derivation_is_0x00_or_0x01(
                value in proptest::collection::vec(0u8..=2, 0..=2),
            ) {
                let v2 = v2_with_fields(&[(PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE, value.clone())]);
                let derivation = match value.as_slice() {
                    [0x00] => SortKeyDerivation::Explicit,
                    [0x01] => SortKeyDerivation::Derived,
                    _ => {
                        let error = Global::try_from(v2).unwrap_err();
                        let bytes = format!("{value:02x?}");
                        prop_assert!(error.to_string().contains(&bytes));
                        prop_assert_eq!(error, InvalidGlobal::SortKeyDerivation(value));
                        return Ok(());
                    }
                };
                prop_assert_eq!(Global::try_from(v2).map(|g| g.sort_key_derivation()), Ok(Some(derivation)));
            }

            #[test]
            fn sort_seed_is_at_least_16_bytes(seed in proptest::collection::vec(any::<u8>(), 0..=24)) {
                let v2 = v2_with_fields(&[(PSBT_GLOBAL_SORT_SEED_SUBTYPE, seed.clone())]);
                match Global::try_from(v2) {
                    Ok(global) => {
                        prop_assert!(seed.len() >= SortSeed::MIN_LEN);
                        prop_assert_eq!(global.sort_seed(), Some(seed.as_slice()));
                    }
                    Err(error) => {
                        let length = format!("{} bytes", seed.len());
                        prop_assert!(error.to_string().contains(&length));
                        prop_assert_eq!(error, InvalidGlobal::SortSeed(InvalidSortSeed(seed)));
                    }
                }
            }
        }
    }
}
