#![allow(clippy::result_large_err)]

//! Per-output field type and output set merging.
//!
//! [`ResultOutput`] wraps each per-output field in the internal result domain.
//! [`OutputSet`] and [`ResultOutputSet`] are `HashMap<UniqueId, _>` collections
//! that key outputs by their unique identifier, enabling order-insensitive merging.

use crate::collections::hashmap::HashMapResultValue;

use bitcoin::bip32::KeySource;
use bitcoin::key::{PublicKey, XOnlyPublicKey};
use bitcoin::taproot::{TapLeafHash, TapTree};
use bitcoin::{Amount, ScriptBuf};
use psbt_v2::raw;
use psbt_v2::v2::Output;

joinable_struct! {
    /// Result-domain representation of a PSBT output.
    ///
    /// Each field mirrors its counterpart in [`psbt_v2::v2::Output`] but is
    /// represented in the internal result domain, recording either a clean value
    /// or an accumulated conflict.
    source: Output,
    result: ResultOutput,
    ext: OutputExt,
    fields: {
        amount: Amount,
        script_pubkey: ScriptBuf,
        redeem_script: Option<ScriptBuf>,
        witness_script: Option<ScriptBuf>,
        bip32_derivations: BTreeMap<PublicKey, KeySource>,
        tap_internal_key: Option<XOnlyPublicKey>,
        tap_tree: Option<TapTree>,
        tap_key_origins: BTreeMap<XOnlyPublicKey, (Vec<TapLeafHash>, KeySource)>,
        proprietaries: BTreeMap<raw::ProprietaryKey, Vec<u8>>,
        unknowns: BTreeMap<raw::Key, Vec<u8>>,
    }
}

impl HashMapResultValue for ResultOutput {
    type Clean = Output;

    fn is_ok(&self) -> bool {
        ResultOutput::is_ok(self)
    }

    fn try_unwrap(self) -> Result<Output, Self> {
        ResultOutput::try_unwrap(self)
    }
}

/// Opaque byte-vector identifier for a PSBT output.
///
/// Computed from the `PSBT_OUT_UNIQUE_ID` proprietary field, keyed by subtype
/// [`PSBT_OUT_UNIQUE_ID_SUBTYPE`]. Used as the map key in `OutputSet`
/// so that outputs can be merged in an order-independent way.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UniqueId(Vec<u8>);

impl UniqueId {
    /// Create a `UniqueId` from raw bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Access the raw bytes of this unique identifier.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume and return the raw bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Generate a fresh random 16-byte unique identifier.
    ///
    /// Each call yields a distinct identity, so copies of an otherwise
    /// identical txout can coexist as separate outputs.
    pub fn generate() -> Self {
        Self(rand::random::<[u8; 16]>().to_vec())
    }
}

impl crate::values::IdempotentValue for UniqueId {}

pub const PSBT_OUT_UNIQUE_ID_SUBTYPE: u8 = 0x01;

/// Extension trait on [`psbt_v2::v2::Output`] for accessing the unique-ID proprietary field.
pub trait OutputUniqueIdExt {
    /// Return the [`UniqueId`] stored in the `PSBT_OUT_UNIQUE_ID` proprietary field, if present.
    fn unique_id(&self) -> Option<UniqueId>;
    /// Return `true` if this output carries a [`UniqueId`].
    fn has_unique_id(&self) -> bool {
        self.unique_id().is_some()
    }
    /// Store `id` in the `PSBT_OUT_UNIQUE_ID` proprietary field, replacing
    /// any existing value.
    fn set_unique_id(&mut self, id: UniqueId);
}

impl OutputUniqueIdExt for Output {
    fn unique_id(&self) -> Option<UniqueId> {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: b"concurrent-psbt".to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.get(&key).map(|v| UniqueId(v.clone()))
    }

    fn set_unique_id(&mut self, id: UniqueId) {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: b"concurrent-psbt".to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.insert(key, id.into_bytes());
    }
}

/// Subtype for `PSBT_OUT_SORT_KEY`.
///
/// Up to 32 bytes of arbitrary data used as a lexicographic sort key.
/// Must be distinct across all outputs in a PSBT.
pub const PSBT_OUT_SORT_KEY_SUBTYPE: u8 = 0x10;

/// Extension trait on [`psbt_v2::v2::Output`] for accessing the sort key proprietary field.
pub trait OutputSortKeyExt {
    /// Get the sort key, if set.
    fn sort_key(&self) -> Option<&[u8]>;
    /// Set the sort key.
    fn set_sort_key(&mut self, key: Vec<u8>);
}

impl OutputSortKeyExt for Output {
    fn sort_key(&self) -> Option<&[u8]> {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: crate::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_OUT_SORT_KEY_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.get(&key).map(|v| v.as_slice())
    }

    fn set_sort_key(&mut self, sort_key: Vec<u8>) {
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: crate::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_OUT_SORT_KEY_SUBTYPE,
            key: vec![],
        };
        self.proprietaries.insert(key, sort_key);
    }
}
pub use super::output_set::{OutputSet, ResultOutputSet};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::lattice::join::Join;
    use crate::lattice::partial::{Conflict, JoinResult, PartialJoin};
    use bitcoin::secp256k1::{Secp256k1, SecretKey};

    const ALL_CONFLICT_FIELDS: &[&str] = &[
        "amount",
        "script_pubkey",
        "redeem_script",
        "witness_script",
        "bip32_derivations",
        "tap_internal_key",
        "tap_tree",
        "tap_key_origins",
        "proprietaries",
        "unknowns",
    ];

    fn empty_conflict<T: PartialJoin>() -> JoinResult<T> {
        Err(Conflict::from_values([]))
    }

    fn all_fields_conflict(seed: u8) -> ResultOutput {
        let mut result = Output::default().wrap();
        result.amount = empty_conflict();
        result.script_pubkey = empty_conflict();
        result.redeem_script = Some(empty_conflict());
        result.witness_script = Some(empty_conflict());
        result.tap_internal_key = Some(empty_conflict());
        result.tap_tree = Some(empty_conflict());

        let secret_key = SecretKey::from_slice(&[seed; 32]).expect("bounded nonzero seed");
        let secp_public_key =
            bitcoin::secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &secret_key);
        let public_key = PublicKey::new(secp_public_key);
        let (x_only_public_key, _) = secp_public_key.x_only_public_key();
        result
            .bip32_derivations
            .insert(public_key, empty_conflict());
        result
            .tap_key_origins
            .insert(x_only_public_key, empty_conflict());
        result.proprietaries.insert(
            raw::ProprietaryKey {
                prefix: vec![seed],
                subtype: seed,
                key: vec![seed],
            },
            empty_conflict(),
        );
        result.unknowns.insert(
            raw::Key {
                type_value: seed,
                key: vec![seed],
            },
            empty_conflict(),
        );
        result
    }

    #[cfg(feature = "unit-tests")]
    mod unit {
        use super::*;

        #[test]
        fn wrap_default_output_is_ok() {
            assert!(Output::default().wrap().is_ok());
        }

        #[test]
        fn wrap_try_unwrap_roundtrip() {
            assert!(Output::default().wrap().try_unwrap().is_ok());
        }

        #[test]
        fn join_identical_outputs_is_ok() {
            assert!(
                Output::default()
                    .wrap()
                    .join(Output::default().wrap())
                    .is_ok()
            );
        }

        #[test]
        fn join_different_amounts_conflicts() {
            let a = Output {
                amount: bitcoin::Amount::from_sat(100),
                ..Output::default()
            };
            let b = Output {
                amount: bitcoin::Amount::from_sat(200),
                ..Output::default()
            };
            assert!(!a.wrap().join(b.wrap()).is_ok());
        }

        #[test]
        fn try_unwrap_conflicting_returns_err() {
            let a = Output {
                amount: bitcoin::Amount::from_sat(100),
                ..Output::default()
            };
            let b = Output {
                amount: bitcoin::Amount::from_sat(200),
                ..Output::default()
            };
            assert!(a.wrap().join(b.wrap()).try_unwrap().is_err());
        }

        #[test]
        fn for_each_conflict_reports_every_conflicted_field() {
            let result = all_fields_conflict(1);
            let mut fields = Vec::new();
            result.for_each_conflict(|field, _| fields.push(field.to_owned()));
            assert_eq!(fields, ALL_CONFLICT_FIELDS);
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop {
        use super::*;
        use proptest::prelude::*;

        fn arb_output() -> impl Strategy<Value = Output> {
            (
                any::<u64>(),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
                proptest::option::of(proptest::collection::vec(0u8..255, 0..=8)),
            )
                .prop_map(|(amount, redeem, witness)| Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    script_pubkey: ScriptBuf::new(),
                    redeem_script: redeem.map(ScriptBuf::from_bytes),
                    witness_script: witness.map(ScriptBuf::from_bytes),
                    ..Output::default()
                })
        }

        fn arb_result_output() -> impl Strategy<Value = ResultOutput> {
            prop_oneof![
                arb_output().prop_map(|o| o.wrap()),
                (arb_output(), arb_output()).prop_map(|(a, b)| a.wrap().join(b.wrap())),
            ]
        }

        proptest! {
            #[test]
            fn for_each_conflict_reports_every_conflicted_field(seed in 1u8..=0x7f) {
                let result = all_fields_conflict(seed);
                let mut fields = Vec::new();
                result.for_each_conflict(|field, _| fields.push(field.to_owned()));
                prop_assert_eq!(fields, ALL_CONFLICT_FIELDS);
            }

            #[test]
            fn wrap_try_unwrap_roundtrip(o in arb_output()) {
                let wrapped = o.wrap();
                prop_assert!(wrapped.is_ok());
                prop_assert!(wrapped.try_unwrap().is_ok());
            }

            #[test]
            fn is_ok_reflects_content(r in arb_result_output()) {
                if r.is_ok() {
                    prop_assert!(r.try_unwrap().is_ok());
                } else {
                    prop_assert!(r.try_unwrap().is_err());
                }
            }

            #[test]
            fn idempotent(a in arb_result_output()) {
                prop_assert_eq!(a.clone().join(a.clone()), a);
            }

            #[test]
            fn commutative(a in arb_result_output(), b in arb_result_output()) {
                prop_assert_eq!(a.clone().join(b.clone()), b.join(a));
            }

            #[test]
            fn associative(
                a in arb_result_output(),
                b in arb_result_output(),
                c in arb_result_output(),
            ) {
                prop_assert_eq!(
                    a.clone().join(b.clone()).join(c.clone()),
                    a.join(b.join(c)),
                );
            }

            #[test]
            fn sort_key_roundtrips(key in proptest::collection::vec(any::<u8>(), 0..=32)) {
                let mut output = Output::default();
                prop_assert_eq!(output.sort_key(), None);
                output.set_sort_key(key.clone());
                prop_assert_eq!(output.sort_key(), Some(key.as_slice()));
            }
        }
    }

    #[cfg(feature = "prop-tests")]
    mod prop_uid {
        use super::*;
        use proptest::prelude::*;

        fn output_with_uid(uid: &[u8]) -> Output {
            let mut output = Output::default();
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            };
            output.proprietaries.insert(key, uid.to_vec());
            output
        }

        proptest! {
            #[test]
            fn unique_id_roundtrip(uid in proptest::collection::vec(0u8..=255, 1..=32)) {
                let output = output_with_uid(&uid);
                prop_assert!(output.has_unique_id());
                prop_assert_eq!(output.unique_id().unwrap().into_bytes(), uid);
            }

            #[test]
            fn unique_id_construction_and_setting(uid in proptest::collection::vec(any::<u8>(), 0..=32)) {
                let id = UniqueId::new(uid.clone());
                prop_assert_eq!(id.as_bytes(), uid.as_slice());

                let mut output = Output::default();
                output.set_unique_id(id);
                prop_assert_eq!(output.unique_id().unwrap().into_bytes(), uid);
            }

            #[test]
            fn generated_unique_id_has_expected_width(_case in any::<()>()) {
                prop_assert_eq!(UniqueId::generate().as_bytes().len(), 16);
            }

            #[test]
            fn no_uid_means_absent(amount in any::<u64>()) {
                let output = Output {
                    amount: bitcoin::Amount::from_sat(amount),
                    ..Output::default()
                };
                prop_assert!(!output.has_unique_id());
                prop_assert!(output.unique_id().is_none());
            }

            #[test]
            fn unique_id_equality(a in proptest::collection::vec(0u8..=255, 1..=8),
                                  b in proptest::collection::vec(0u8..=255, 1..=8)) {
                if a == b {
                    prop_assert_eq!(UniqueId(a.clone()), UniqueId(b));
                } else {
                    prop_assert_ne!(UniqueId(a), UniqueId(b));
                }
            }
        }
    }

    #[cfg(feature = "unit-tests")]
    mod unit_uid {
        use super::*;

        fn output_with_uid(uid: &[u8]) -> Output {
            let mut output = Output::default();
            let key = psbt_v2::raw::ProprietaryKey {
                prefix: b"concurrent-psbt".to_vec(),
                subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
                key: vec![],
            };
            output.proprietaries.insert(key, uid.to_vec());
            output
        }

        #[test]
        fn unique_id_present() {
            let output = output_with_uid(&[1, 2, 3]);
            assert!(output.has_unique_id());
            assert_eq!(output.unique_id().unwrap().into_bytes(), vec![1, 2, 3]);
        }

        #[test]
        fn unique_id_absent() {
            let output = Output::default();
            assert!(!output.has_unique_id());
            assert!(output.unique_id().is_none());
        }

        #[test]
        fn unique_id_equality() {
            assert_eq!(UniqueId(vec![1, 2, 3]), UniqueId(vec![1, 2, 3]));
        }

        #[test]
        fn unique_id_inequality() {
            assert_ne!(UniqueId(vec![1, 2, 3]), UniqueId(vec![4, 5, 6]));
        }

        #[test]
        fn unique_id_construction_generation_and_setting() {
            let id = UniqueId::new(vec![1, 2, 3]);
            assert_eq!(id.as_bytes(), &[1, 2, 3]);

            let generated = UniqueId::generate();
            assert_eq!(generated.as_bytes().len(), 16);

            let mut output = Output::default();
            output.set_unique_id(id);
            assert_eq!(output.unique_id().unwrap().into_bytes(), vec![1, 2, 3]);
        }
    }
}

// OutputSet — HashMap<UniqueId, Output> collection with Join
