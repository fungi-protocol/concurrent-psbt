use std::collections::HashMap;

use bitcoin::OutPoint;
pub use psbt_v2::v2::Input;

use crate::lattice::join::Join;
use crate::lattice::partial::PartialJoin; // for into_ok on values

use crate::collections::btreemap::BTreeMapExt as _;
use crate::collections::btreemap::Transpose as _;
use crate::collections::option::OptionExt as _;
use crate::collections::option::ResultOptionExt as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSet(HashMap<OutPoint, Input>);

impl FromIterator<Input> for InputSet {
    fn from_iter<T: IntoIterator<Item = Input>>(iter: T) -> Self {
        Self(
            iter.into_iter()
                .map(|input| (input.out_point(), input))
                .collect(),
        )
    }
}

impl IntoIterator for InputSet {
    type Item = Input;
    type IntoIter = std::collections::hash_map::IntoValues<OutPoint, Input>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_values()
    }
}

impl InputSet {
    pub fn spends_outpoint(&self, outpoint: &OutPoint) -> bool {
        self.0.contains_key(outpoint)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, input: Input) {
        self.0.insert(input.out_point(), input);
    }

    pub fn into_ok(self) -> ResultInputSet {
        // FIXME can this be generic?
        ResultInputSet(self.0.into_iter().map(|(k, v)| (k, v.into_ok())).collect())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResultInputSet(HashMap<OutPoint, ResultInput>);

impl Join for ResultInputSet {
    fn join(self, other: Self) -> Self {
        ResultInputSet(self.0.join(other.0))
    }
}

impl ResultInputSet {
    pub fn transpose(self) -> Result<InputSet, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(InputSet(
            self.0
                .into_iter()
                .map(|(k, v)| (k, v.transpose().expect("verified is_ok()")))
                .collect(),
        ))
    }

    pub fn is_ok(&self) -> bool {
        self.0.values().all(|v| v.is_ok())
    }
}

// FIXME should be pub(crate)
pub trait InputExt {
    // Input::out_point() exists but is private so we reimeplement it here
    fn out_point(&self) -> OutPoint;
    fn sort_key(&self) -> Option<&Vec<u8>>;
    fn take_sort_key(&mut self) -> Option<Vec<u8>>;

    fn into_ok(self) -> ResultInput;
}

impl InputExt for Input {
    fn out_point(self: &Input) -> OutPoint {
        OutPoint {
            txid: self.previous_txid,
            vout: self.spent_output_index,
        }
    }

    fn sort_key(&self) -> Option<&Vec<u8>> {
        self.proprietaries.get(&crate::fields::psbt_in_sort_key())
    }

    fn take_sort_key(&mut self) -> Option<Vec<u8>> {
        self.proprietaries.remove(&crate::fields::psbt_in_sort_key())
    }

    fn into_ok(self) -> ResultInput {
        ResultInput {
            // FIXME macro
            previous_txid: self.previous_txid.into_ok(),
            spent_output_index: self.spent_output_index.into_ok(),
            sequence: self.sequence.into_ok(),
            min_time: self.min_time.into_ok(),
            min_height: self.min_height.into_ok(),
            non_witness_utxo: self.non_witness_utxo.into_ok(),
            witness_utxo: self.witness_utxo.into_ok(),
            partial_sigs: self.partial_sigs.into_ok(),
            sighash_type: self.sighash_type.into_ok(),
            redeem_script: self.redeem_script.into_ok(),
            witness_script: self.witness_script.into_ok(),
            bip32_derivations: self.bip32_derivations.into_ok(),
            final_script_sig: self.final_script_sig.into_ok(),
            final_script_witness: self.final_script_witness.into_ok(),
            ripemd160_preimages: self.ripemd160_preimages.into_ok(),
            sha256_preimages: self.sha256_preimages.into_ok(),
            hash160_preimages: self.hash160_preimages.into_ok(),
            hash256_preimages: self.hash256_preimages.into_ok(),
            tap_key_sig: self.tap_key_sig.into_ok(),
            tap_script_sigs: self.tap_script_sigs.into_ok(),
            tap_scripts: self.tap_scripts.into_ok(),
            tap_key_origins: self.tap_key_origins.into_ok(),
            tap_internal_key: self.tap_internal_key.into_ok(),
            tap_merkle_root: self.tap_merkle_root.into_ok(),
            proprietaries: self.proprietaries.into_ok(),
            unknowns: self.unknowns.into_ok(),
        }
    }
}

// impl FromIterator<Input> for VecWrapper<Input> {
//     fn from_iter<T: IntoIterator<Item = Input>>(iter: T) -> Self {
//         Self(iter.into_iter().collect())
//     }
// }

// impl IntoIterator for VecWrapper<Input> {
//     type Item = Input;
//     type IntoIter = std::vec::IntoIter<Input>;

//     fn into_iter(self) -> Self::IntoIter {
//         self.0.into_iter()
//     }
// }

mod result {
    pub use std::collections::BTreeMap;

    use bitcoin::bip32::KeySource;
    use bitcoin::hashes::{hash160, ripemd160, sha256, sha256d};
    use bitcoin::key::{PublicKey, XOnlyPublicKey};
    use bitcoin::locktime::absolute;
    use bitcoin::taproot::{ControlBlock, LeafVersion, TapLeafHash, TapNodeHash};
    use bitcoin::{
        ecdsa, secp256k1, taproot, ScriptBuf, Sequence, Transaction, TxOut, Txid, Witness,
    };

    use psbt_v2::raw;
    use psbt_v2::PsbtSighashType;

    use crate::lattice::partial::JoinResult;

    // FIXME macro
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResultInput {
        /// The txid of the previous transaction whose output at `self.spent_output_index` is being spent.
        ///
        /// In other words, the output being spent by this `Input` is:
        ///
        ///  `OutPoint { txid: self.previous_txid, vout: self.spent_output_index }`
        pub previous_txid: JoinResult<Txid>,

        /// The index of the output being spent in the transaction with the txid of `self.previous_txid`.
        pub spent_output_index: JoinResult<u32>,

        /// The sequence number of this input.
        ///
        /// If omitted, assumed to be the final sequence number ([`Sequence::MAX`]).
        pub sequence: Option<JoinResult<Sequence>>,

        /// The minimum Unix timestamp that this input requires to be set as the transaction's lock time.
        pub min_time: Option<JoinResult<absolute::Time>>,

        /// The minimum block height that this input requires to be set as the transaction's lock time.
        pub min_height: Option<JoinResult<absolute::Height>>,

        /// The non-witness transaction this input spends from. Should only be
        /// `Option::Some` for inputs which spend non-segwit outputs or
        /// if it is unknown whether an input spends a segwit output.
        pub non_witness_utxo: Option<JoinResult<Transaction>>,
        /// The transaction output this input spends from. Should only be
        /// `Option::Some` for inputs which spend segwit outputs,
        /// including P2SH embedded ones.
        pub witness_utxo: Option<JoinResult<TxOut>>,
        /// A map from public keys to their corresponding signature as would be
        /// pushed to the stack from a scriptSig or witness for a non-taproot inputs.
        pub partial_sigs: BTreeMap<PublicKey, JoinResult<ecdsa::Signature>>,
        /// The sighash type to be used for this input. Signatures for this input
        /// must use the sighash type.
        pub sighash_type: Option<JoinResult<PsbtSighashType>>,
        /// The redeem script for this input.
        pub redeem_script: Option<JoinResult<ScriptBuf>>,
        /// The witness script for this input.
        pub witness_script: Option<JoinResult<ScriptBuf>>,
        /// A map from public keys needed to sign this input to their corresponding
        /// master key fingerprints and derivation paths.
        pub bip32_derivations: BTreeMap<secp256k1::PublicKey, JoinResult<KeySource>>,
        /// The finalized, fully-constructed scriptSig with signatures and any other
        /// scripts necessary for this input to pass validation.
        pub final_script_sig: Option<JoinResult<ScriptBuf>>,
        /// The finalized, fully-constructed scriptWitness with signatures and any
        /// other scripts necessary for this input to pass validation.
        pub final_script_witness: Option<JoinResult<Witness>>,
        /// TODO: Proof of reserves commitment
        /// RIPEMD160 hash to preimage map.
        pub ripemd160_preimages: BTreeMap<ripemd160::Hash, JoinResult<Vec<u8>>>,
        /// SHA256 hash to preimage map.
        pub sha256_preimages: BTreeMap<sha256::Hash, JoinResult<Vec<u8>>>,
        /// HSAH160 hash to preimage map.
        pub hash160_preimages: BTreeMap<hash160::Hash, JoinResult<Vec<u8>>>,
        /// HAS256 hash to preimage map.
        pub hash256_preimages: BTreeMap<sha256d::Hash, JoinResult<Vec<u8>>>,
        /// Serialized taproot signature with sighash type for key spend.
        pub tap_key_sig: Option<JoinResult<taproot::Signature>>,
        /// Map of `<xonlypubkey>|<leafhash>` with signature.
        pub tap_script_sigs:
            BTreeMap<(XOnlyPublicKey, TapLeafHash), JoinResult<taproot::Signature>>,
        /// Map of Control blocks to Script version pair.
        pub tap_scripts: BTreeMap<ControlBlock, JoinResult<(ScriptBuf, LeafVersion)>>,
        /// Map of tap root x only keys to origin info and leaf hashes contained in it.
        pub tap_key_origins: BTreeMap<XOnlyPublicKey, JoinResult<(Vec<TapLeafHash>, KeySource)>>,
        /// Taproot Internal key.
        pub tap_internal_key: Option<JoinResult<XOnlyPublicKey>>,
        /// Taproot Merkle root.
        pub tap_merkle_root: Option<JoinResult<TapNodeHash>>,
        /// Proprietary key-value pairs for this input.
        pub proprietaries: BTreeMap<raw::ProprietaryKey, JoinResult<Vec<u8>>>,
        /// Unknown key-value pairs for this input.
        pub unknowns: BTreeMap<raw::Key, JoinResult<Vec<u8>>>,
    }
}

pub use result::ResultInput;

impl Join for ResultInput {
    fn join(self, other: Self) -> Self {
        ResultInput {
            // FIXME macro
            previous_txid: self.previous_txid.join(other.previous_txid),
            spent_output_index: self.spent_output_index.join(other.spent_output_index),
            sequence: self.sequence.join(other.sequence),
            min_time: self.min_time.join(other.min_time),
            min_height: self.min_height.join(other.min_height),
            non_witness_utxo: self.non_witness_utxo.join(other.non_witness_utxo),
            witness_utxo: self.witness_utxo.join(other.witness_utxo),
            partial_sigs: self.partial_sigs.join(other.partial_sigs),
            sighash_type: self.sighash_type.join(other.sighash_type),
            redeem_script: self.redeem_script.join(other.redeem_script),
            witness_script: self.witness_script.join(other.witness_script),
            bip32_derivations: self.bip32_derivations.join(other.bip32_derivations),
            final_script_sig: self.final_script_sig.join(other.final_script_sig),
            final_script_witness: self.final_script_witness.join(other.final_script_witness),
            ripemd160_preimages: self.ripemd160_preimages.join(other.ripemd160_preimages),
            sha256_preimages: self.sha256_preimages.join(other.sha256_preimages),
            hash160_preimages: self.hash160_preimages.join(other.hash160_preimages),
            hash256_preimages: self.hash256_preimages.join(other.hash256_preimages),
            tap_key_sig: self.tap_key_sig.join(other.tap_key_sig),
            tap_script_sigs: self.tap_script_sigs.join(other.tap_script_sigs),
            tap_scripts: self.tap_scripts.join(other.tap_scripts),
            tap_key_origins: self.tap_key_origins.join(other.tap_key_origins),
            tap_internal_key: self.tap_internal_key.join(other.tap_internal_key),
            tap_merkle_root: self.tap_merkle_root.join(other.tap_merkle_root),
            proprietaries: self.proprietaries.join(other.proprietaries),
            unknowns: self.unknowns.join(other.unknowns),
        }
    }
}

impl ResultInput {
    pub fn transpose(self) -> Result<Input, Self> {
        if !self.is_ok() {
            return Err(self);
        }

        Ok(Input {
            // FIXME macro
            previous_txid: self.previous_txid.expect("verified all fields are Ok"),
            spent_output_index: self.spent_output_index.expect("verified all fields are Ok"),
            sequence: self
                .sequence
                .transpose()
                .expect("verified all fields are Ok"),
            min_time: self
                .min_time
                .transpose()
                .expect("verified all fields are Ok"),
            min_height: self
                .min_height
                .transpose()
                .expect("verified all fields are Ok"),
            non_witness_utxo: self
                .non_witness_utxo
                .transpose()
                .expect("verified all fields are Ok"),
            witness_utxo: self
                .witness_utxo
                .transpose()
                .expect("verified all fields are Ok"),
            partial_sigs: self
                .partial_sigs
                .transpose()
                .expect("verified all fields are Ok"),
            sighash_type: self
                .sighash_type
                .transpose()
                .expect("verified all fields are Ok"),
            redeem_script: self
                .redeem_script
                .transpose()
                .expect("verified all fields are Ok"),
            witness_script: self
                .witness_script
                .transpose()
                .expect("verified all fields are Ok"),
            bip32_derivations: self
                .bip32_derivations
                .transpose()
                .expect("verified all fields are Ok"),
            final_script_sig: self
                .final_script_sig
                .transpose()
                .expect("verified all fields are Ok"),
            final_script_witness: self
                .final_script_witness
                .transpose()
                .expect("verified all fields are Ok"),
            ripemd160_preimages: self
                .ripemd160_preimages
                .transpose()
                .expect("verified all fields are Ok"),
            sha256_preimages: self
                .sha256_preimages
                .transpose()
                .expect("verified all fields are Ok"),
            hash160_preimages: self
                .hash160_preimages
                .transpose()
                .expect("verified all fields are Ok"),
            hash256_preimages: self
                .hash256_preimages
                .transpose()
                .expect("verified all fields are Ok"),
            tap_key_sig: self
                .tap_key_sig
                .transpose()
                .expect("verified all fields are Ok"),
            tap_script_sigs: self
                .tap_script_sigs
                .transpose()
                .expect("verified all fields are Ok"),
            tap_scripts: self
                .tap_scripts
                .transpose()
                .expect("verified all fields are Ok"),
            tap_key_origins: self
                .tap_key_origins
                .transpose()
                .expect("verified all fields are Ok"),
            tap_internal_key: self
                .tap_internal_key
                .transpose()
                .expect("verified all fields are Ok"),
            tap_merkle_root: self
                .tap_merkle_root
                .transpose()
                .expect("verified all fields are Ok"),
            proprietaries: self
                .proprietaries
                .transpose()
                .expect("verified all fields are Ok"),
            unknowns: self
                .unknowns
                .transpose()
                .expect("verified all fields are Ok"),
        })
    }

    pub fn is_ok(&self) -> bool {
        // FIXME macro
        self.previous_txid.is_ok()
            && self.spent_output_index.is_ok()
            && self.sequence.is_ok()
            && self.min_time.is_ok()
            && self.min_height.is_ok()
            && self.non_witness_utxo.is_ok()
            && self.witness_utxo.is_ok()
            && self.partial_sigs.is_ok()
            && self.sighash_type.is_ok()
            && self.redeem_script.is_ok()
            && self.witness_script.is_ok()
            && self.bip32_derivations.is_ok()
            && self.final_script_sig.is_ok()
            && self.final_script_witness.is_ok()
            && self.ripemd160_preimages.is_ok()
            && self.sha256_preimages.is_ok()
            && self.hash160_preimages.is_ok()
            && self.hash256_preimages.is_ok()
            && self.tap_key_sig.is_ok()
            && self.tap_script_sigs.is_ok()
            && self.tap_scripts.is_ok()
            && self.tap_key_origins.is_ok()
            && self.tap_internal_key.is_ok()
            && self.tap_merkle_root.is_ok()
            && self.proprietaries.is_ok()
            && self.unknowns.is_ok()
    }
}

#[test]
fn test_input_set() {
    use crate::lattice::partial::PartialJoin;

    let mut oa = OutPoint::null();
    oa.vout = 0;
    let mut ob = OutPoint::null();
    ob.vout = 1;
    assert_ne!(oa, ob);

    assert_eq!(PartialJoin::try_join(oa, oa), Ok(oa));
    assert_eq!(
        PartialJoin::try_join(oa, ob),
        Err(crate::values::ConflictingValues([oa, ob].into()))
    );

    let ia = Input::new(&oa);
    let ib = Input::new(&ob);

    assert_eq!(
        Join::join(ia.clone().into_ok(), ia.clone().into_ok()).transpose(),
        Ok(ia.clone())
    );

    // Joining two differing inputs directly with one another is not allowed.
    // This shouldn't ever come up in practice since the outpoints are different
    let mut res = ia.clone().into_ok();
    res.spent_output_index = Err(crate::values::ConflictingValues([0, 1].into()));

    assert_eq!(
        Join::join(ia.clone().into_ok(), ib.clone().into_ok()).transpose(),
        Err(res),
    );

    // joining two sets that spend different inputs is well defined
    let sa = InputSet::from_iter([ia.clone()]);
    let sb = InputSet::from_iter([ib.clone()]);
    assert_ne!(sa, sb);

    assert_eq!(
        Join::join(sa.clone().into_ok(), sb.clone().into_ok()).transpose(),
        Ok(InputSet::from_iter([ia.clone(), ib.clone()])),
    );

    // joining two sets that spend the same inputs and have non conflicting values is OK
    let mut ia_with_seq = ia.clone();
    ia_with_seq.sequence = Some(bitcoin::Sequence::MAX);
    let sa_with_seq = InputSet::from_iter([ia_with_seq.clone()]);

    let joined = Join::join(sa.clone().into_ok(), sa_with_seq.clone().into_ok());
    let transposed = joined.transpose();

    assert_ne!(sa, sa_with_seq);
    assert_eq!(transposed, Ok(sa_with_seq.clone()));

    // joining two sets that spend the same inputs and have non conflicting values is OK
    let mut ia_with_other_seq = ia.clone();
    ia_with_other_seq.sequence = Some(bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF);

    let mut conflict = ia.clone().into_ok();
    conflict.sequence = Some(Err(crate::values::ConflictingValues(
        [
            bitcoin::Sequence::MAX,
            bitcoin::Sequence::ENABLE_LOCKTIME_NO_RBF,
        ]
        .into(),
    )));

    assert_eq!(
        Join::join(
            ia_with_seq.clone().into_ok(),
            ia_with_other_seq.clone().into_ok()
        ),
        conflict,
    );

    let sa_with_other_seq = InputSet::from_iter([ia_with_other_seq.clone()]);
    assert_ne!(sa_with_seq, sa_with_other_seq);
    assert_eq!(
        Join::join(
            sa_with_seq.clone().into_ok(),
            sa_with_other_seq.clone().into_ok()
        )
        .transpose(),
        Err(ResultInputSet(
            [ia_with_seq
                .clone()
                .into_ok()
                .join(ia_with_other_seq.clone().into_ok())]
            .into_iter()
            .map(|i| (oa.clone(), i))
            .collect()
        ))
    );
}

// FIXME add more unit tests
