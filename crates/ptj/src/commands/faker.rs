//! psbt_faker-style test fixtures for the session UI (`/api/fake/*`).
//!
//! Three generators forming a pipeline — a fake wallet descriptor, a fake
//! fully-signed transaction paying it (whose outputs are spendable UTXOs in
//! the eyes of `classify`), and a fake unordered PSBT spending those UTXOs
//! to random recipients with change back to the descriptor. Every result is
//! an ordinary payload (descriptor string, raw tx hex, PSBT base64) so the
//! frontend feeds it through the same paste/classify path as real data —
//! the generators mint test data, not special objects.
//!
//! Nothing here is cryptographically meaningful: keys are random, the coin
//! transaction's input is a dummy, signatures are placeholder bytes that
//! merely satisfy the "every input carries a witness" classification
//! heuristic. Similar in spirit to Coldcard's `psbt_faker`, not a port.

use std::fmt;

use bitcoin::hashes::Hash as _;
use rand::RngCore;

use crate::cli::{CreateConfig, NetworkArg, OrderingArg, OutPointArg, OutputArg};
use crate::{Error, Result};

/// Descriptor flavors the generator can mint. BIP 84 wpkh is the default;
/// BIP 86 tr covers the taproot column of the test matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DescriptorKind {
    Wpkh,
    Tr,
}

impl DescriptorKind {
    fn purpose(self) -> u32 {
        match self {
            Self::Wpkh => 84,
            Self::Tr => 86,
        }
    }
}

impl fmt::Display for DescriptorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wpkh => write!(f, "wpkh"),
            Self::Tr => write!(f, "tr"),
        }
    }
}

impl std::str::FromStr for DescriptorKind {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "wpkh" => Ok(Self::Wpkh),
            "tr" => Ok(Self::Tr),
            other => Err(format!("unknown descriptor kind {other:?} (wpkh, tr)")),
        }
    }
}

/// Mint a ranged public descriptor over a fresh random master key, with the
/// standard `[fingerprint/purpose'/coin'/0']` origin. The private key is
/// generated and immediately discarded — the fake wallet can receive but
/// never spend, which is exactly what test data should be able to do.
pub(crate) fn fake_descriptor(
    network: bitcoin::Network,
    kind: DescriptorKind,
    rng: &mut dyn RngCore,
) -> Result<String> {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let master = bitcoin::bip32::Xpriv::new_master(network, &seed)
        .map_err(|error| Error::new(format!("deriving master key: {error}")))?;
    let fingerprint = master.fingerprint(&secp);
    let coin = match network {
        bitcoin::Network::Bitcoin => 0,
        _ => 1,
    };
    let purpose = kind.purpose();
    let path: Vec<bitcoin::bip32::ChildNumber> = [purpose, coin, 0]
        .iter()
        .map(|&index| {
            bitcoin::bip32::ChildNumber::from_hardened_idx(index)
                .map_err(|error| Error::new(format!("derivation index {index}: {error}")))
        })
        .collect::<Result<_>>()?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|error| Error::new(format!("deriving account key: {error}")))?;
    let xpub = bitcoin::bip32::Xpub::from_priv(&secp, &account);
    let descriptor = format!("{kind}([{fingerprint}/{purpose}h/{coin}h/0h]{xpub}/0/*)");
    // Round-trip through miniscript for validation and the checksum suffix —
    // the emitted string is exactly what `classify` will accept back.
    let (descriptor, _) = miniscript::Descriptor::parse_descriptor(&secp, &descriptor)
        .map_err(|error| Error::new(format!("validating generated descriptor: {error}")))?;
    Ok(descriptor.to_string())
}

/// Fake a fully-signed transaction paying `count` outputs to the descriptor
/// at derivation indices `0..count`, with random plausible amounts. The
/// single input spends a random nonexistent outpoint and carries dummy
/// witness bytes — enough to pass `classify`'s "every input carries a
/// witness" fully-signed heuristic, so the frontend mints spendable UTXO
/// objects from the outputs.
pub(crate) fn fake_coins(
    descriptor: &str,
    network: bitcoin::Network,
    count: u32,
    rng: &mut dyn RngCore,
) -> Result<bitcoin::Transaction> {
    if count == 0 {
        return Err(Error::new("count must be at least 1"));
    }
    let descriptor = parse_single_descriptor(descriptor)?;
    let output = (0..count)
        .map(|index| {
            let index = if descriptor.has_wildcard() { index } else { 0 };
            let definite = descriptor
                .at_derivation_index(index)
                .map_err(|error| Error::new(format!("deriving index {index}: {error}")))?;
            definite.address(network).map_err(|error| {
                Error::new(format!("descriptor has no address on {network}: {error}"))
            })?;
            Ok(bitcoin::TxOut {
                // 0.001–2 BTC in 0.001 steps: recognizably fake, plausibly shaped.
                value: bitcoin::Amount::from_sat((1 + u64::from(rng.next_u32()) % 2_000) * 100_000),
                script_pubkey: definite.script_pubkey(),
            })
        })
        .collect::<Result<_>>()?;
    let mut fake_txid = [0u8; 32];
    rng.fill_bytes(&mut fake_txid);
    Ok(bitcoin::Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![bitcoin::TxIn {
            previous_output: bitcoin::OutPoint {
                txid: bitcoin::Txid::from_byte_array(fake_txid),
                vout: 0,
            },
            script_sig: bitcoin::ScriptBuf::new(),
            sequence: bitcoin::Sequence::MAX,
            // Placeholder signature + key sized like a real p2wpkh witness.
            witness: bitcoin::Witness::from_slice(&[vec![0xFA; 71], vec![0xCE; 33]]),
        }],
        output,
    })
}

/// A spendable fake UTXO: the outpoint the PSBT will spend and the amount
/// it carries (the caller learned both from classifying the coins tx).
pub(crate) struct FakeUtxo {
    pub(crate) outpoint: bitcoin::OutPoint,
    pub(crate) amount: bitcoin::Amount,
}

/// How far into the descriptor the fake change output may land. Small so
/// tests (and curious users) can enumerate the range cheaply.
const CHANGE_INDEX_RANGE: u32 = 30;

/// Fake an unordered, modifiable PSBT spending the given UTXOs: equal
/// payments to `recipients` freshly-invented p2wpkh addresses, change back
/// to the descriptor at a random index, and a small random fee. Delegates
/// to the real `create` command so the result carries every invariant the
/// UI expects from a created PSBT (unordered, modifiable, output uids).
pub(crate) fn fake_psbt(
    descriptor: &str,
    network: bitcoin::Network,
    utxos: &[FakeUtxo],
    recipients: u32,
    rng: &mut dyn RngCore,
) -> Result<psbt_v2::v2::Psbt> {
    if utxos.is_empty() {
        return Err(Error::new("at least one utxo is required"));
    }
    if recipients == 0 {
        return Err(Error::new("recipients must be at least 1"));
    }
    let descriptor_text = descriptor;
    let descriptor = parse_single_descriptor(descriptor)?;
    let total = utxos
        .iter()
        .try_fold(bitcoin::Amount::ZERO, |sum, utxo| {
            sum.checked_add(utxo.amount)
        })
        .ok_or_else(|| Error::new("utxo amounts overflow"))?;
    let fee = bitcoin::Amount::from_sat(1_000 + u64::from(rng.next_u32() % 4_000));
    let dust = bitcoin::Amount::from_sat(546);
    let shares = u64::from(recipients) + 1; // recipients + change
    let spendable = total
        .checked_sub(fee)
        .filter(|spendable| *spendable >= dust * shares)
        .ok_or_else(|| {
            Error::new(format!(
                "utxo total {total} too small to fund {recipients} recipient(s) plus change and fee"
            ))
        })?;
    let share = spendable / shares;
    let change = spendable - share * u64::from(recipients);

    let mut outputs = Vec::with_capacity(recipients as usize + 1);
    for _ in 0..recipients {
        outputs.push(output_arg(fake_recipient_address(network, rng), share));
    }
    let change_index = if descriptor.has_wildcard() {
        rng.next_u32() % CHANGE_INDEX_RANGE
    } else {
        0
    };
    let change_address = descriptor
        .at_derivation_index(change_index)
        .map_err(|error| Error::new(format!("deriving change index {change_index}: {error}")))?
        .address(network)
        .map_err(|error| {
            Error::new(format!(
                "descriptor {descriptor_text} has no address on {network}: {error}"
            ))
        })?;
    outputs.push(output_arg(change_address, change));

    crate::commands::create::create_psbt(CreateConfig {
        inputs: utxos
            .iter()
            .map(|utxo| OutPointArg {
                txid: utxo.outpoint.txid,
                vout: utxo.outpoint.vout,
            })
            .collect(),
        outputs,
        seed: None,
        allow_short_seed: false,
        ordering: OrderingArg::Unset,
        network: NetworkArg(network),
    })
}

/// Parse a descriptor to its first single-path form (multipath descriptors
/// contribute their receive path), mirroring `classify`'s treatment.
fn parse_single_descriptor(
    payload: &str,
) -> Result<miniscript::Descriptor<miniscript::DescriptorPublicKey>> {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let (descriptor, _) = miniscript::Descriptor::parse_descriptor(&secp, payload)
        .map_err(|error| Error::new(format!("parsing descriptor: {error}")))?;
    if !descriptor.is_multipath() {
        return Ok(descriptor);
    }
    descriptor
        .into_single_descriptors()
        .map_err(|error| Error::new(format!("expanding multipath descriptor: {error}")))?
        .into_iter()
        .next()
        .ok_or_else(|| Error::new("multipath descriptor expanded to no paths"))
}

/// Invent a p2wpkh address nobody holds the key for — a fake recipient.
fn fake_recipient_address(network: bitcoin::Network, rng: &mut dyn RngCore) -> bitcoin::Address {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let secret = loop {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        // from_slice rejects ~2^-128 of candidates; retry on those.
        if let Ok(secret) = bitcoin::secp256k1::SecretKey::from_slice(&bytes) {
            break secret;
        }
    };
    let public = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
    let public = bitcoin::CompressedPublicKey::from_slice(&public.serialize())
        .expect("a serialized secp256k1 public key is a valid compressed key");
    bitcoin::Address::p2wpkh(&public, network)
}

fn output_arg(address: bitcoin::Address, amount: bitcoin::Amount) -> OutputArg {
    OutputArg {
        address_text: address.to_string(),
        address: address.into_unchecked(),
        amount,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use rand::SeedableRng as _;

    use super::*;

    fn rng() -> rand::rngs::StdRng {
        rand::rngs::StdRng::from_seed([7; 32])
    }

    #[test]
    fn fake_descriptors_are_ranged_public_and_classifiable() {
        for (kind, descriptor_type) in [(DescriptorKind::Wpkh, "Wpkh"), (DescriptorKind::Tr, "Tr")]
        {
            let descriptor = fake_descriptor(bitcoin::Network::Regtest, kind, &mut rng()).unwrap();
            let value = crate::commands::classify::classify(&descriptor, bitcoin::Network::Regtest)
                .unwrap();
            assert_eq!(value["kind"], "descriptor", "{descriptor}");
            assert_eq!(value["descriptor_type"], descriptor_type, "{descriptor}");
            assert_eq!(value["has_private_keys"], false, "{descriptor}");
            assert_eq!(value["is_ranged"], true, "{descriptor}");
            // Standard origin: purpose'/coin'/account' under the fingerprint
            // (miniscript's display normalizes hardened `h` to `'`).
            let purpose = kind.purpose();
            assert!(
                descriptor.contains(&format!("/{purpose}'/1'/0']")),
                "{descriptor}"
            );
        }
    }

    #[test]
    fn descriptor_kind_parses_its_own_display() {
        for kind in [DescriptorKind::Wpkh, DescriptorKind::Tr] {
            assert_eq!(kind.to_string().parse::<DescriptorKind>(), Ok(kind));
        }
        assert!("pkh".parse::<DescriptorKind>().is_err());
    }

    #[test]
    fn fake_coins_pay_the_descriptor_and_classify_fully_signed() {
        let descriptor =
            fake_descriptor(bitcoin::Network::Regtest, DescriptorKind::Wpkh, &mut rng()).unwrap();
        let transaction =
            fake_coins(&descriptor, bitcoin::Network::Regtest, 3, &mut rng()).unwrap();

        assert_eq!(transaction.output.len(), 3);
        let parsed = parse_single_descriptor(&descriptor).unwrap();
        for (index, output) in transaction.output.iter().enumerate() {
            let expected = parsed
                .at_derivation_index(index as u32)
                .unwrap()
                .script_pubkey();
            assert_eq!(output.script_pubkey, expected, "output {index}");
            assert!(output.value.to_sat() >= 100_000, "output {index}");
        }

        let hex = bitcoin::consensus::encode::serialize_hex(&transaction);
        let value = crate::commands::classify::classify(&hex, bitcoin::Network::Regtest).unwrap();
        assert_eq!(value["kind"], "transaction");
        assert_eq!(value["fully_signed"], true);
        assert_eq!(value["output_count"], 3);

        assert!(fake_coins(&descriptor, bitcoin::Network::Regtest, 0, &mut rng()).is_err());
    }

    #[test]
    fn fake_psbt_spends_the_utxos_with_change_to_the_descriptor() {
        let mut rng = rng();
        let network = bitcoin::Network::Regtest;
        let descriptor = fake_descriptor(network, DescriptorKind::Wpkh, &mut rng).unwrap();
        let coins = fake_coins(&descriptor, network, 2, &mut rng).unwrap();
        let txid = coins.compute_txid();
        let utxos: Vec<FakeUtxo> = coins
            .output
            .iter()
            .enumerate()
            .map(|(vout, output)| FakeUtxo {
                outpoint: bitcoin::OutPoint {
                    txid,
                    vout: vout as u32,
                },
                amount: output.value,
            })
            .collect();
        let total: u64 = coins
            .output
            .iter()
            .map(|output| output.value.to_sat())
            .sum();

        let psbt = fake_psbt(&descriptor, network, &utxos, 2, &mut rng).unwrap();
        let inspected = crate::commands::inspect::inspect_psbt(&psbt);
        assert_eq!(inspected["input_count"], 2);
        assert_eq!(inspected["output_count"], 3, "2 recipients + change");
        assert_eq!(inspected["ordering"], "unordered");

        // Amounts conserve: outputs + fee == utxo total.
        let outputs: u64 = psbt
            .outputs
            .iter()
            .map(|output| output.amount.to_sat())
            .sum();
        let fee = total - outputs;
        assert!((1_000..5_000).contains(&fee), "fee {fee}");

        // Exactly one output pays the descriptor (the change).
        let parsed = parse_single_descriptor(&descriptor).unwrap();
        let change_scripts: Vec<_> = (0..CHANGE_INDEX_RANGE)
            .map(|index| parsed.at_derivation_index(index).unwrap().script_pubkey())
            .collect();
        let change_outputs = psbt
            .outputs
            .iter()
            .filter(|output| change_scripts.contains(&output.script_pubkey))
            .count();
        assert_eq!(change_outputs, 1);

        assert!(fake_psbt(&descriptor, network, &[], 2, &mut rng).is_err());
        // recipients == 0 would degenerate to "send everything to change";
        // reject it like fake_coins rejects count == 0.
        let error = fake_psbt(&descriptor, network, &utxos, 0, &mut rng)
            .unwrap_err()
            .to_string();
        assert!(error.contains("recipients must be at least 1"), "{error}");
        let dust_utxo = FakeUtxo {
            outpoint: utxos[0].outpoint,
            amount: bitcoin::Amount::from_sat(1_500),
        };
        let error = fake_psbt(
            &descriptor,
            network,
            std::slice::from_ref(&dust_utxo),
            2,
            &mut rng,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("too small"), "{error}");
    }
}
