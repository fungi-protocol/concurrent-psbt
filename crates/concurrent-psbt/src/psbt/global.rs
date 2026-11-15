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
    ///
    /// A new global map from [`Default::default`] sets `PSBT_GLOBAL_TX_UNORDERED`.
    source: Global,
    /// Result-domain counterpart of [`Global`].
    ///
    /// Produced by joining [`Global`] values lifted with [`Global::wrap`].
    /// Use [`ResultGlobal::is_ok`] to check for conflicts and
    /// [`ResultGlobal::try_unwrap`] to extract.
    result: ResultGlobal,
    conflicts: GlobalConflict, GlobalField,
    v2: psbt_v2::v2::Global,
    fields: {
        tx_version: transaction::Version => TxVersion,
        fallback_lock_time: Option<absolute::LockTime> => FallbackLockTime(absolute::LockTime),
        tx_modifiable_flags: u8 => TxModifiableFlags,
        xpubs: BTreeMap<Xpub, KeySource> => Xpub[Xpub](KeySource),
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>> => Proprietary[raw::ProprietaryKey](Vec<u8>),
        unknowns: BTreeMap<raw::Key, Vec<u8>> => Unknown[raw::Key](Vec<u8>),
    },
    proprietary: {
        pub(crate) unordered: bool = TxUnorderedField => Unordered,
        pub(crate) sort_key_derivation: Option<SortKeyDerivation> = SortKeyDerivationField => SortKeyDerivation(SortKeyDerivation),
        pub(crate) sort_seed: Option<SortSeed> = SortSeedField => SortSeed(SortSeed),
        pub(crate) uuid: Option<TxUuid> = TxUuidField => TxUuid(TxUuid),
    },
}

impl Default for Global {
    fn default() -> Self {
        Self::from_v2_fields(psbt_v2::v2::Global::default(), true, None, None, None)
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
    /// The length of `PSBT_GLOBAL_TX_UUID` is outside the allowed range.
    TxUuid(InvalidTxUuid),
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
            Self::TxUuid(error) => error.fmt(f),
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
    /// The unordered marker follows [`is_unordered`](Self::is_unordered).
    pub(crate) fn into_v2(self, input_count: usize, output_count: usize) -> psbt_v2::v2::Global {
        self.encode_v2(psbt_v2::v2::Global {
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
/// Auxiliary input to derived sort keys, alongside `PSBT_GLOBAL_TX_UUID`.
pub const PSBT_GLOBAL_SORT_SEED_SUBTYPE: u8 = 0x11;

/// A `PSBT_GLOBAL_SORT_SEED` value: auxiliary bytes that the sort seed hashes
/// together with the transaction's [`TxUuid`].
///
/// This seed may commit to extra information such as a protocol transcript.
/// Derived sort keys use the same bytes for a missing seed and an empty one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSeed(Vec<u8>);

impl SortSeed {
    /// Access the raw bytes of the seed.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for SortSeed {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl crate::values::IdempotentValue for SortSeed {}

/// Subtype for `PSBT_GLOBAL_TX_UUID`.
///
/// Identifies the transaction being constructed, and is the mandatory input
/// to derived sort keys.
pub const PSBT_GLOBAL_TX_UUID_SUBTYPE: u8 = 0x13;

/// A 16- to 32-byte identifier for the transaction being constructed.
///
/// [`Self::generate`] produces a random 16-byte UUID.
/// Sorting requires this identifier and uses it to derive every sort key.
/// Different UUIDs conflict when two PSBTs join.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxUuid(Vec<u8>);

impl TxUuid {
    /// The minimum UUID length in bytes.
    pub const MIN_LEN: usize = 16;
    /// The maximum UUID length in bytes.
    pub const MAX_LEN: usize = 32;

    /// Access the raw bytes of the UUID.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Generate a fresh random 16-byte UUID from the operating system's RNG,
    /// as [`UniqueId::generate`](crate::output::UniqueId::generate) does.
    ///
    /// # Panics
    /// Panics if the operating system's RNG is unavailable.
    pub fn generate() -> Self {
        use rand::{RngCore, TryRngCore, rngs::OsRng};

        let mut bytes = [0u8; Self::MIN_LEN];
        OsRng.unwrap_err().fill_bytes(&mut bytes);
        Self(bytes.to_vec())
    }
}

/// UUID bytes whose length is outside the allowed range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTxUuid(pub Vec<u8>);

impl TryFrom<Vec<u8>> for TxUuid {
    type Error = InvalidTxUuid;

    fn try_from(bytes: Vec<u8>) -> Result<Self, InvalidTxUuid> {
        if !(Self::MIN_LEN..=Self::MAX_LEN).contains(&bytes.len()) {
            return Err(InvalidTxUuid(bytes));
        }
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for InvalidTxUuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PSBT_GLOBAL_TX_UUID is {} bytes, expected {} to {}",
            self.0.len(),
            TxUuid::MIN_LEN,
            TxUuid::MAX_LEN
        )
    }
}

impl std::error::Error for InvalidTxUuid {}

impl crate::values::IdempotentValue for TxUuid {}

/// Subtype for `PSBT_GLOBAL_SORT_KEY_DERIVATION`.
///
/// `0x01` = explicit sort keys MUST NOT be set.
/// `0x00` = explicit sort keys required.
pub const PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE: u8 = 0x12;

/// A `PSBT_GLOBAL_SORT_KEY_DERIVATION` value: where sort keys come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKeyDerivation {
    /// `0x00`: every input and output needs an explicit sort key.
    /// Inputs, or outputs, with the same explicit key are ordered by their
    /// derived keys.
    ExplicitKeysRequired,
    /// `0x01`: inputs and outputs are ordered by their derived keys alone,
    /// and explicit sort keys are not allowed.
    ExplicitKeysForbidden,
}

impl SortKeyDerivation {
    /// Parse a field value, returning it unchanged if it is neither `0x00` nor
    /// `0x01`.
    fn from_value(value: Vec<u8>) -> Result<Self, Vec<u8>> {
        match value.as_slice() {
            [0x00] => Ok(Self::ExplicitKeysRequired),
            [0x01] => Ok(Self::ExplicitKeysForbidden),
            _ => Err(value),
        }
    }

    pub(crate) fn to_byte(self) -> u8 {
        match self {
            Self::ExplicitKeysRequired => 0x00,
            Self::ExplicitKeysForbidden => 0x01,
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
        Ok(value.map(SortSeed))
    }

    fn encode(seed: Self::Value) -> Option<Vec<u8>> {
        seed.map(|seed| seed.0)
    }
}

struct TxUuidField;

impl ProprietaryField for TxUuidField {
    const SUBTYPE: u8 = PSBT_GLOBAL_TX_UUID_SUBTYPE;
    type Value = Option<TxUuid>;
    type Error = InvalidGlobal;

    fn decode(value: Option<Vec<u8>>) -> Result<Self::Value, InvalidGlobal> {
        value
            .map(TxUuid::try_from)
            .transpose()
            .map_err(InvalidGlobal::TxUuid)
    }

    fn encode(uuid: Self::Value) -> Option<Vec<u8>> {
        uuid.map(|uuid| uuid.0)
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

    /// Get the auxiliary sort seed, if set.
    pub fn sort_seed(&self) -> Option<&[u8]> {
        self.sort_seed.as_ref().map(SortSeed::as_bytes)
    }

    /// Set the auxiliary sort seed.
    pub fn set_sort_seed(&mut self, seed: SortSeed) {
        self.sort_seed = Some(seed);
    }

    /// Get `PSBT_GLOBAL_TX_UUID`, if set.
    pub fn uuid(&self) -> Option<&[u8]> {
        self.uuid.as_ref().map(TxUuid::as_bytes)
    }

    /// Set `PSBT_GLOBAL_TX_UUID`.
    pub fn set_uuid(&mut self, uuid: TxUuid) {
        self.uuid = Some(uuid);
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
        fn generated_uuid_is_sixteen_bytes() {
            assert_eq!(TxUuid::generate().as_bytes().len(), 16);
        }

        #[test]
        fn sort_fields_are_encoded_under_the_crate_prefix() {
            let key = |subtype| raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype,
                key: vec![],
            };
            let mut global = Global::default();
            global.set_sort_seed(SortSeed::from(vec![0xaa; 16]));
            global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysForbidden);
            global.set_uuid(TxUuid::try_from(vec![0xbb; 16]).unwrap());
            assert!(global.is_unordered());
            assert_eq!(global.sort_seed(), Some([0xaa; 16].as_slice()));
            assert_eq!(global.uuid(), Some([0xbb; 16].as_slice()));
            assert_eq!(
                global.sort_key_derivation(),
                Some(SortKeyDerivation::ExplicitKeysForbidden)
            );
            let v2 = global.clone().into_v2(0, 0);
            assert_eq!(v2.proprietaries.get(&key(0x10)), Some(&vec![0x03]));
            assert_eq!(v2.proprietaries.get(&key(0x11)), Some(&vec![0xaa; 16]));
            assert_eq!(v2.proprietaries.get(&key(0x12)), Some(&vec![0x01]));
            assert_eq!(v2.proprietaries.get(&key(0x13)), Some(&vec![0xbb; 16]));
            assert_eq!(Global::try_from(v2), Ok(global.clone()));

            global.clear_unordered();
            global.set_sort_key_derivation(SortKeyDerivation::ExplicitKeysRequired);
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
            ];
            for (error, message) in cases {
                assert_eq!(error.to_string(), message);
            }
        }

        #[test]
        fn the_first_malformed_sort_field_is_reported() {
            let unordered = (PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, vec![1]);
            let derivation = (PSBT_GLOBAL_SORT_KEY_DERIVATION_SUBTYPE, vec![2]);
            assert_eq!(
                Global::try_from(v2_with_fields(&[unordered, derivation.clone()])),
                Err(InvalidGlobal::UnorderedFlag(vec![1]))
            );
            assert_eq!(
                Global::try_from(v2_with_fields(&[derivation])),
                Err(InvalidGlobal::SortKeyDerivation(vec![2]))
            );
        }

        #[test]
        fn default_sets_the_unordered_marker() {
            let global = Global::default();
            assert!(global.is_unordered());
            assert_eq!(
                global.into_v2(0, 0),
                v2_with_fields(&[(PSBT_GLOBAL_TX_UNORDERED_SUBTYPE, vec![0x03])])
            );
        }

        #[test]
        fn v2_without_a_marker_decodes_as_ordered() {
            let wire = psbt_v2::v2::Global::default();
            let global = Global::try_from(wire.clone()).unwrap();
            assert!(!global.is_unordered());
            assert_eq!(global.into_v2(0, 0), wire);
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
            assert_eq!(
                joined.conflicts().map(|c| c.path()).collect::<Vec<_>>(),
                [GlobalField::TxVersion]
            );
            assert_eq!(joined.clone().try_unwrap(), Err(joined));
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use bitcoin::Network;
        use bitcoin::bip32::{DerivationPath, Xpriv};
        use bitcoin::secp256k1::Secp256k1;
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

        /// The master xpub of `seed`, with its own fingerprint as key source.
        fn xpub_from_seed(seed: u8) -> (Xpub, KeySource) {
            let xpriv = Xpriv::new_master(Network::Bitcoin, &[seed; 16]).expect("valid seed");
            let xpub = Xpub::from_priv(&Secp256k1::new(), &xpriv);
            (xpub, (xpub.fingerprint(), DerivationPath::master()))
        }

        fn arb_global() -> impl Strategy<Value = Global> {
            (
                proptest::bool::ANY,
                0u8..4,
                proptest::bool::ANY,
                proptest::option::of(0u8..2),
                proptest::bool::ANY,
                proptest::option::of(prop_oneof![
                    Just(SortKeyDerivation::ExplicitKeysRequired),
                    Just(SortKeyDerivation::ExplicitKeysForbidden),
                ]),
                proptest::option::of(proptest::collection::vec(any::<u8>(), 0..=17)),
                proptest::option::of(proptest::collection::vec(any::<u8>(), 16..=32)),
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
                        xpub,
                        unordered,
                        derivation,
                        seed,
                        uuid,
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
                            xpubs: xpub.map(xpub_from_seed).into_iter().collect(),
                            proprietaries,
                            unknowns,
                            unordered,
                            ..Global::default()
                        };
                        if let Some(derivation) = derivation {
                            global.set_sort_key_derivation(derivation);
                        }
                        if let Some(seed) = seed {
                            global.set_sort_seed(SortSeed::from(seed));
                        }
                        if let Some(uuid) = uuid {
                            global.set_uuid(TxUuid::try_from(uuid).unwrap());
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
                    PSBT_GLOBAL_TX_UUID_SUBTYPE,
                ] {
                    shadowed
                        .proprietaries
                        .insert(crate::psbt::proprietary::key(subtype), raw.clone());
                }
                prop_assert_eq!(Global::try_from(shadowed.into_v2(0, 0)), Ok(a));
            }

            #[test]
            fn is_ok_iff_try_unwrap_succeeds(a in arb_result_global()) {
                prop_assert_eq!(a.conflicts().next().is_none(), a.is_ok());
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
                    [0x00] => SortKeyDerivation::ExplicitKeysRequired,
                    [0x01] => SortKeyDerivation::ExplicitKeysForbidden,
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
            fn sort_seed_is_any_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..=24)) {
                let v2 = v2_with_fields(&[(PSBT_GLOBAL_SORT_SEED_SUBTYPE, bytes.clone())]);
                let global = Global::try_from(v2).expect("any bytes are a seed");
                prop_assert_eq!(global.sort_seed(), Some(bytes.as_slice()));
            }

            #[test]
            fn uuid_length_is_checked(bytes in proptest::collection::vec(any::<u8>(), 0..=36)) {
                let v2 = v2_with_fields(&[(PSBT_GLOBAL_TX_UUID_SUBTYPE, bytes.clone())]);
                if (16..=32).contains(&bytes.len()) {
                    let uuid = TxUuid::try_from(bytes.clone()).unwrap();
                    prop_assert_eq!(uuid.as_bytes(), bytes.as_slice());
                    let global = Global::try_from(v2).unwrap();
                    prop_assert_eq!(global.uuid(), Some(bytes.as_slice()));
                } else {
                    let error = InvalidTxUuid(bytes.clone());
                    prop_assert_eq!(TxUuid::try_from(bytes.clone()), Err(error.clone()));
                    let error = InvalidGlobal::TxUuid(error);
                    prop_assert_eq!(error.to_string(), format!("PSBT_GLOBAL_TX_UUID is {} bytes, expected 16 to 32", bytes.len()));
                    prop_assert_eq!(Global::try_from(v2), Err(error));
                }
            }
        }
    }
}
