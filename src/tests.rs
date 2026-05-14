use crate::{
    global::{Global, GlobalExt as _},
    input::{Input, InputExt as _, InputSet},
    lattice::join::Join,
    output::{Output, OutputExt as _, OutputSet},
    tx::UnorderedPsbt,
};
use bitcoin::{
    Amount, OutPoint, PublicKey, ScriptBuf, Sequence, TapNodeHash, TxOut, Txid, Witness, absolute,
    bip32::{ChildNumber, DerivationPath, Fingerprint},
    hashes::{Hash, hash160, ripemd160, sha256, sha256d},
    secp256k1::{self, Keypair, Secp256k1, SecretKey, XOnlyPublicKey},
    sighash::{EcdsaSighashType, TapSighashType},
    taproot::TapLeafHash,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Join helper: uniform interface for both PartialJoin types and compound types
// ---------------------------------------------------------------------------

/// Try joining two values, returning Ok if they merge cleanly.
///
/// For compound types (Global, Input, etc.) this goes through
/// wrap → Join::join → try_unwrap. For PartialJoin scalars this
/// could use try_join directly, but we go through the same path
/// for uniformity.
fn try_join_via_wrap<T>(a: T, b: T) -> Result<T, ()>
where
    T: Clone,
    T: WrapJoin,
{
    a.do_join(b)
}

use crate::lattice::partial::PartialJoin;

trait WrapJoin: Sized {
    fn do_join(self, other: Self) -> Result<Self, ()>;
}

// For PartialJoin types (scalars, BTreeMap, Option, etc.).
macro_rules! impl_wrap_join_partial {
    ($ty:ty) => {
        impl WrapJoin for $ty {
            fn do_join(self, other: Self) -> Result<Self, ()> {
                self.try_join(other).map_err(|_| ())
            }
        }
    };
}

impl_wrap_join_partial!(u32);

// Compound types: wrap + join + try_unwrap.
macro_rules! impl_wrap_join {
    ($ty:ty, $wrap:path, $join:path, $try_unwrap:path) => {
        impl WrapJoin for $ty {
            fn do_join(self, other: Self) -> Result<Self, ()> {
                $try_unwrap($join($wrap(self), $wrap(other))).map_err(|_| ())
            }
        }
    };
}

fn wrap_global(g: Global) -> crate::global::ResultGlobal {
    g.wrap()
}
fn join_global(
    a: crate::global::ResultGlobal,
    b: crate::global::ResultGlobal,
) -> crate::global::ResultGlobal {
    Join::join(a, b)
}
fn try_unwrap_global(
    r: crate::global::ResultGlobal,
) -> Result<Global, crate::global::ResultGlobal> {
    r.try_unwrap()
}
impl_wrap_join!(Global, wrap_global, join_global, try_unwrap_global);

fn wrap_input(i: Input) -> crate::input::ResultInput {
    i.wrap()
}
fn join_input(a: crate::input::ResultInput, b: crate::input::ResultInput) -> crate::input::ResultInput {
    Join::join(a, b)
}
fn try_unwrap_input(r: crate::input::ResultInput) -> Result<Input, crate::input::ResultInput> {
    r.try_unwrap()
}
impl_wrap_join!(Input, wrap_input, join_input, try_unwrap_input);

fn wrap_output(o: Output) -> crate::output::ResultOutput {
    o.wrap()
}
fn join_output(
    a: crate::output::ResultOutput,
    b: crate::output::ResultOutput,
) -> crate::output::ResultOutput {
    Join::join(a, b)
}
fn try_unwrap_output(
    r: crate::output::ResultOutput,
) -> Result<Output, crate::output::ResultOutput> {
    r.try_unwrap()
}
impl_wrap_join!(Output, wrap_output, join_output, try_unwrap_output);

fn wrap_input_set(s: InputSet) -> crate::input::ResultInputSet {
    s.wrap()
}
fn join_input_set(
    a: crate::input::ResultInputSet,
    b: crate::input::ResultInputSet,
) -> crate::input::ResultInputSet {
    Join::join(a, b)
}
fn try_unwrap_input_set(
    r: crate::input::ResultInputSet,
) -> Result<InputSet, crate::input::ResultInputSet> {
    r.try_unwrap()
}
impl_wrap_join!(InputSet, wrap_input_set, join_input_set, try_unwrap_input_set);

fn wrap_output_set(s: OutputSet) -> crate::output::ResultOutputSet {
    s.wrap()
}
fn join_output_set(
    a: crate::output::ResultOutputSet,
    b: crate::output::ResultOutputSet,
) -> crate::output::ResultOutputSet {
    Join::join(a, b)
}
fn try_unwrap_output_set(
    r: crate::output::ResultOutputSet,
) -> Result<OutputSet, crate::output::ResultOutputSet> {
    r.try_unwrap()
}
impl_wrap_join!(
    OutputSet,
    wrap_output_set,
    join_output_set,
    try_unwrap_output_set
);

// ---------------------------------------------------------------------------
// Arbitrary generators
// ---------------------------------------------------------------------------

fn arb_txout() -> impl Strategy<Value = TxOut> {
    (0u64..4, arb_script()).prop_map(|(sats, script)| TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: script,
    })
}

fn arb_script() -> impl Strategy<Value = ScriptBuf> {
    (0u8..4).prop_map(|b| ScriptBuf::from(vec![b]))
}

fn arb_height() -> impl Strategy<Value = absolute::Height> {
    (0u32..4).prop_map(|n| absolute::Height::from_consensus(n).expect("valid height"))
}

fn arb_time() -> impl Strategy<Value = absolute::Time> {
    (500_000_000u32..500_000_004)
        .prop_map(|n| absolute::Time::from_consensus(n).expect("valid time"))
}

fn arb_sighash_type() -> impl Strategy<Value = psbt_v2::PsbtSighashType> {
    prop_oneof![
        Just(psbt_v2::PsbtSighashType::from(EcdsaSighashType::All)),
        Just(psbt_v2::PsbtSighashType::from(EcdsaSighashType::None)),
        Just(psbt_v2::PsbtSighashType::from(EcdsaSighashType::Single)),
    ]
}

fn arb_witness() -> impl Strategy<Value = Witness> {
    proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..4), 0..3)
        .prop_map(|vecs| Witness::from_slice(&vecs))
}

fn arb_ripemd160() -> impl Strategy<Value = ripemd160::Hash> {
    (0u8..4).prop_map(|n| ripemd160::Hash::from_byte_array([n; 20]))
}

fn arb_hash160() -> impl Strategy<Value = hash160::Hash> {
    (0u8..4).prop_map(|n| hash160::Hash::from_byte_array([n; 20]))
}

fn arb_sha256() -> impl Strategy<Value = sha256::Hash> {
    (0u8..4).prop_map(|n| sha256::Hash::from_byte_array([n; 32]))
}

fn arb_sha256d() -> impl Strategy<Value = sha256d::Hash> {
    (0u8..4).prop_map(|n| sha256d::Hash::from_byte_array([n; 32]))
}

fn arb_tap_leaf_hash() -> impl Strategy<Value = TapLeafHash> {
    (0u8..4).prop_map(|n| TapLeafHash::from_byte_array([n; 32]))
}

fn arb_tap_node_hash() -> impl Strategy<Value = TapNodeHash> {
    (0u8..4).prop_map(|n| TapNodeHash::from_byte_array([n; 32]))
}

fn arb_preimage_map<H>(
    arb_hash: impl Strategy<Value = H>,
) -> impl Strategy<Value = std::collections::BTreeMap<H, Vec<u8>>>
where
    H: Ord + Clone + std::fmt::Debug + 'static,
{
    proptest::collection::btree_map(arb_hash, proptest::collection::vec(0u8..4u8, 0..4), 0..3)
}

fn arb_secret_key() -> impl Strategy<Value = SecretKey> {
    (1u8..5).prop_map(|n| SecretKey::from_slice(&[n; 32]).expect("valid secret key"))
}

fn arb_secp256k1_pubkey() -> impl Strategy<Value = secp256k1::PublicKey> {
    arb_secret_key()
        .prop_map(|sk| secp256k1::PublicKey::from_secret_key(&Secp256k1::signing_only(), &sk))
}

fn arb_bitcoin_pubkey() -> impl Strategy<Value = PublicKey> {
    arb_secp256k1_pubkey().prop_map(PublicKey::new)
}

fn arb_xonly_pubkey() -> impl Strategy<Value = XOnlyPublicKey> {
    arb_secret_key().prop_map(|sk| {
        Keypair::from_secret_key(&Secp256k1::signing_only(), &sk)
            .x_only_public_key()
            .0
    })
}

fn arb_key_source() -> impl Strategy<Value = bitcoin::bip32::KeySource> {
    (any::<[u8; 4]>(), proptest::collection::vec(0u32..4, 0..3)).prop_map(
        |(fp_bytes, path_indices)| {
            let fingerprint = Fingerprint::from(fp_bytes);
            let path = DerivationPath::from_iter(
                path_indices
                    .into_iter()
                    .map(|n| ChildNumber::from_normal_idx(n).expect("valid child number")),
            );
            (fingerprint, path)
        },
    )
}

fn arb_ecdsa_sig() -> impl Strategy<Value = bitcoin::ecdsa::Signature> {
    arb_secret_key().prop_map(|sk| {
        let secp = Secp256k1::signing_only();
        let sig = secp.sign_ecdsa(&secp256k1::Message::from_digest([0u8; 32]), &sk);
        bitcoin::ecdsa::Signature {
            signature: sig,
            sighash_type: EcdsaSighashType::All,
        }
    })
}

fn arb_taproot_sig() -> impl Strategy<Value = bitcoin::taproot::Signature> {
    arb_secret_key().prop_map(|sk| {
        let secp = Secp256k1::signing_only();
        let kp = Keypair::from_secret_key(&secp, &sk);
        let sig = secp.sign_schnorr_no_aux_rand(&secp256k1::Message::from_digest([0u8; 32]), &kp);
        bitcoin::taproot::Signature {
            signature: sig,
            sighash_type: TapSighashType::Default,
        }
    })
}

fn arb_raw_key() -> impl Strategy<Value = psbt_v2::raw::Key> {
    (0u8..4, proptest::collection::vec(0u8..4u8, 0..4))
        .prop_map(|(type_value, key)| psbt_v2::raw::Key { type_value, key })
}

fn arb_proprietary_key() -> impl Strategy<Value = psbt_v2::raw::ProprietaryKey> {
    (
        proptest::collection::vec(0u8..4u8, 0..4),
        0u8..4,
        proptest::collection::vec(0u8..4u8, 0..4),
    )
        .prop_map(|(prefix, subtype, key)| psbt_v2::raw::ProprietaryKey {
            prefix,
            subtype,
            key,
        })
}

// ---------------------------------------------------------------------------
// Composite generators
// ---------------------------------------------------------------------------

prop_compose! {
    fn arb_input_a()(
        byte in 0u8..4,
        vout in 0u32..2,
        sequence         in proptest::option::of(0u32..4u32),
        min_height       in proptest::option::of(arb_height()),
        min_time         in proptest::option::of(arb_time()),
        witness_utxo     in proptest::option::of(arb_txout()),
        sighash_type     in proptest::option::of(arb_sighash_type()),
        redeem_script    in proptest::option::of(arb_script()),
        witness_script   in proptest::option::of(arb_script()),
        final_script_sig in proptest::option::of(arb_script()),
    ) -> Input {
        let mut txid_bytes = [0u8; 32];
        txid_bytes[0] = byte;
        let mut input = Input::new(&OutPoint::new(Txid::from_byte_array(txid_bytes), vout));
        input.sequence = sequence.map(Sequence);
        input.min_height = min_height;
        input.min_time = min_time;
        input.witness_utxo = witness_utxo;
        input.sighash_type = sighash_type;
        input.redeem_script = redeem_script;
        input.witness_script = witness_script;
        input.final_script_sig = final_script_sig;
        input
    }
}

prop_compose! {
    fn arb_input()(
        input                in arb_input_a(),
        final_script_witness in proptest::option::of(arb_witness()),
        tap_key_sig          in proptest::option::of(arb_taproot_sig()),
        tap_internal_key     in proptest::option::of(arb_xonly_pubkey()),
        tap_merkle_root      in proptest::option::of(arb_tap_node_hash()),
        partial_sigs      in proptest::collection::btree_map(arb_bitcoin_pubkey(), arb_ecdsa_sig(), 0..3),
        bip32_derivations in proptest::collection::btree_map(arb_secp256k1_pubkey(), arb_key_source(), 0..3),
        tap_key_origins   in proptest::collection::btree_map(
            arb_xonly_pubkey(),
            (proptest::collection::vec(arb_tap_leaf_hash(), 0..3), arb_key_source()),
            0..3,
        ),
        ripemd160_preimages in arb_preimage_map(arb_ripemd160()),
        sha256_preimages    in arb_preimage_map(arb_sha256()),
        hash160_preimages   in arb_preimage_map(arb_hash160()),
        hash256_preimages   in arb_preimage_map(arb_sha256d()),
        unknowns      in proptest::collection::btree_map(arb_raw_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
        proprietaries in proptest::collection::btree_map(arb_proprietary_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
    ) -> Input {
        let mut input = input;
        input.final_script_witness = final_script_witness;
        input.tap_key_sig = tap_key_sig;
        input.tap_internal_key = tap_internal_key;
        input.tap_merkle_root = tap_merkle_root;
        input.partial_sigs = partial_sigs;
        input.bip32_derivations = bip32_derivations;
        input.tap_key_origins = tap_key_origins;
        input.ripemd160_preimages = ripemd160_preimages;
        input.sha256_preimages = sha256_preimages;
        input.hash160_preimages = hash160_preimages;
        input.hash256_preimages = hash256_preimages;
        input.unknowns = unknowns;
        input.proprietaries = proprietaries;
        input
    }
}

prop_compose! {
    fn arb_output()(
        sats              in 0u64..4,
        script_byte       in 0u8..4,
        unique_id_byte    in 0u8..16,
        redeem_script     in proptest::option::of(arb_script()),
        witness_script    in proptest::option::of(arb_script()),
        tap_internal_key  in proptest::option::of(arb_xonly_pubkey()),
        bip32_derivations in proptest::collection::btree_map(arb_secp256k1_pubkey(), arb_key_source(), 0..3),
        tap_key_origins   in proptest::collection::btree_map(
            arb_xonly_pubkey(),
            (proptest::collection::vec(arb_tap_leaf_hash(), 0..3), arb_key_source()),
            0..3,
        ),
        unknowns      in proptest::collection::btree_map(arb_raw_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
        proprietaries in proptest::collection::btree_map(arb_proprietary_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
    ) -> Output {
        let mut output = Output::new(TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: ScriptBuf::from(vec![script_byte]),
        });
        // Every output needs a unique ID for the OutputSet.
        output.proprietaries.insert(
            crate::fields::psbt_out_unique_id(),
            vec![unique_id_byte; 16],
        );
        output.redeem_script = redeem_script;
        output.witness_script = witness_script;
        output.tap_internal_key = tap_internal_key;
        output.bip32_derivations = bip32_derivations;
        output.tap_key_origins = tap_key_origins;
        output.unknowns = unknowns;
        // Merge generated proprietaries *after* unique_id to avoid overwriting it.
        for (k, v) in proprietaries {
            output.proprietaries.entry(k).or_insert(v);
        }
        output
    }
}

prop_compose! {
    fn arb_input_set()(inputs in proptest::collection::vec(arb_input(), 0..4)) -> InputSet {
        InputSet::from_iter(inputs)
    }
}

prop_compose! {
    fn arb_output_set()(outputs in proptest::collection::vec(arb_output(), 0..4)) -> OutputSet {
        OutputSet::from_iter(outputs)
    }
}

prop_compose! {
    fn arb_global()(
        input_count  in 0usize..4,
        output_count in 0usize..4,
        fallback_lock_time in proptest::option::of(prop_oneof![
            arb_height().prop_map(|h| absolute::LockTime::Blocks(h)),
            arb_time().prop_map(|t| absolute::LockTime::Seconds(t)),
        ]),
        tx_modifiable_flags in 0u8..4,
        unknowns      in proptest::collection::btree_map(arb_raw_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
        proprietaries in proptest::collection::btree_map(arb_proprietary_key(), proptest::collection::vec(0u8..4, 0..4), 0..3),
    ) -> Global {
        let mut g = Global::default();
        g.input_count = input_count;
        g.output_count = output_count;
        g.fallback_lock_time = fallback_lock_time;
        g.tx_modifiable_flags = tx_modifiable_flags;
        g.unknowns = unknowns;
        g.proprietaries = proprietaries;
        g
    }
}

// ---------------------------------------------------------------------------
// Lattice laws macro
// ---------------------------------------------------------------------------

macro_rules! laws {
    ($mod:ident, $ty:ty, $strategy:expr) => {
        mod $mod {
            use super::*;

            proptest! {
                #[test]
                fn idempotent(a in $strategy) {
                    let result = try_join_via_wrap(a.clone(), a.clone());
                    prop_assert_eq!(result, Ok(a));
                }

                #[test]
                fn commutative(a in $strategy, b in $strategy) {
                    let ab = try_join_via_wrap(a.clone(), b.clone());
                    let ba = try_join_via_wrap(b, a);
                    prop_assert_eq!(ab, ba);
                }

                #[test]
                fn associative(a in $strategy, b in $strategy, c in $strategy) {
                    let left  = try_join_via_wrap(a.clone(), b.clone()).and_then(|ab| try_join_via_wrap(ab, c.clone()));
                    let right = try_join_via_wrap(b, c).and_then(|bc| try_join_via_wrap(a, bc));
                    prop_assert_eq!(left, right);
                }
            }
        }
    };
}

laws!(laws_u32, u32, any::<u32>());
laws!(laws_global, Global, arb_global());
laws!(laws_input, Input, arb_input());
laws!(laws_output, Output, arb_output());
laws!(laws_input_set, InputSet, arb_input_set());
laws!(laws_output_set, OutputSet, arb_output_set());
