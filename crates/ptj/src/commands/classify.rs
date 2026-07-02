//! Universal paste ingestion: deep classification of raw pasted "bitvomit"
//! for the session UI's paste surface (`/api/classify`).
//!
//! The frontend shallow-classifies; this module does the real parsing, one
//! kind at a time with charset/context detection in the `bytes_arg` spirit —
//! every decoder that was tried and failed is named in the final error:
//!
//! 1. **output descriptors** (`miniscript`): validated, normalized to the
//!    public form, private-key material flagged, first addresses derived;
//! 2. **BIP 21 / BIP 321 payment URIs** (`bitcoin-payment-instructions`,
//!    which also accepts bare addresses, BOLT 11/12 and cashu strings):
//!    methods, amounts, description; `label`/`message`/query params are
//!    extracted here at the route layer (upstream 0.7.0 validates but does
//!    not expose them). Human-readable-name and LNURL resolution need chain
//!    or web sources, which are deliberately NOT wired ("for now let's keep
//!    it manual") — the crate's `DummyHrnResolver` reports them cleanly;
//! 3. **peer identifiers**: `npub` (bech32, 32-byte payload; more kinds
//!    later);
//! 4. **fully signed transactions** (`bitcoin` consensus decode from hex or
//!    base64): txid plus per-output spendable outpoints for the create flow.
//!
//! PSBT payloads are deliberately NOT classified here: the existing routes
//! (`/api/inspect`, `/api/import-bip174`, `/api/join`) own PSBT ingestion,
//! and the error says so.

use serde_json::json;

use crate::{Error, Result};

/// Classify a pasted payload. Returns `{kind, ...details}`.
pub(crate) fn classify(payload: &str, network: bitcoin::Network) -> Result<serde_json::Value> {
    let payload = payload.trim();
    if payload.is_empty() {
        return Err(Error::new("empty payload"));
    }

    // PSBTs are handled by dedicated routes; a paste that is one gets a
    // pointer, not a competing half-parse.
    let lowered = payload.to_ascii_lowercase();
    if payload.starts_with("cHNidP") || lowered.starts_with("70736274ff") {
        return Err(Error::new(
            "the payload is a PSBT; paste it into the PSBT flows instead (/api/inspect, \
             /api/import-bip174, /api/join handle PSBT ingestion)",
        ));
    }

    let mut attempts: Vec<String> = Vec::new();

    // --- peer identifiers (npub; more kinds later) --------------------------
    // Other bech32 HRPs (bc/tb/lnbc/lno/...) fall through: they are addresses
    // or lightning objects the payment-instructions parser owns.
    if let Ok((hrp, data)) = bitcoin::bech32::decode(payload)
        && hrp.to_lowercase() == "npub"
    {
        if data.len() == 32 {
            return Ok(json!({
                "kind": "peer_id",
                "format": "npub",
                "id_hex": hex_encode(&data),
            }));
        }
        attempts.push(format!(
            "not an npub peer id (payload is {} bytes, expected 32)",
            data.len()
        ));
    }

    // --- output descriptors --------------------------------------------------
    if payload.contains('(') {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        match miniscript::Descriptor::parse_descriptor(&secp, payload) {
            Ok((descriptor, key_map)) => {
                return descriptor_json(&descriptor, !key_map.is_empty(), network);
            }
            Err(error) => attempts.push(format!("not an output descriptor ({error})")),
        }
    } else {
        attempts.push("not an output descriptor (no `(` in the payload)".to_string());
    }

    // --- fully signed transactions (hex or base64 consensus encoding) -------
    match transaction_bytes(payload) {
        Ok(bytes) => {
            match bitcoin::consensus::encode::deserialize::<bitcoin::Transaction>(&bytes) {
                Ok(transaction) => return Ok(transaction_json(&transaction, network)),
                Err(error) => attempts.push(format!("not a raw transaction ({error})")),
            }
        }
        Err(reason) => attempts.push(format!("not a raw transaction ({reason})")),
    }

    // --- payment instructions (BIP 21/321 URIs, bare addresses, BOLT 11/12) -
    // Async only because resolvers may do network I/O; with DummyHrnResolver
    // everything completes immediately (chain/web sources deliberately not
    // wired), so this ride on the shared sync-driver runtime never blocks.
    let parsed = super::sync::drive_async(async {
        bitcoin_payment_instructions::PaymentInstructions::parse(
            payload,
            network,
            &bitcoin_payment_instructions::hrn_resolution::DummyHrnResolver,
            /* supports_proof_of_payment_callbacks */ false,
        )
        .await
        .map_err(|error| Error::new(format!("{error:?}")))
    });
    match parsed {
        Ok(instructions) => return Ok(payment_json(&instructions, payload)),
        Err(error) => attempts.push(format!("not payment instructions ({error})")),
    }

    Err(Error::new(format!(
        "could not classify the payload: {}",
        attempts.join("; ")
    )))
}

/// Descriptor details: normalized PUBLIC form (private material never
/// echoes), private-key flag, and the derived script set (first addresses).
fn descriptor_json(
    descriptor: &miniscript::Descriptor<miniscript::DescriptorPublicKey>,
    has_private_keys: bool,
    network: bitcoin::Network,
) -> Result<serde_json::Value> {
    let mut value = json!({
        "kind": "descriptor",
        // to_string() prints the public normalization with its checksum;
        // parse_descriptor split any xprv/WIF material into the key map, so
        // this never echoes private keys.
        "descriptor": descriptor.to_string(),
        "descriptor_type": format!("{:?}", descriptor.desc_type()),
        "has_private_keys": has_private_keys,
        "is_ranged": descriptor.has_wildcard(),
        "is_multipath": descriptor.is_multipath(),
    });

    // Derive the first few scripts/addresses. Multipath descriptors list
    // their single-path expansions and derive from the first (receive) path.
    let single = if descriptor.is_multipath() {
        let paths = descriptor
            .clone()
            .into_single_descriptors()
            .map_err(|error| Error::new(format!("expanding multipath descriptor: {error}")))?;
        value["paths"] = json!(
            paths
                .iter()
                .map(|path| path.to_string())
                .collect::<Vec<_>>()
        );
        paths
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("multipath descriptor expanded to no paths"))?
    } else {
        descriptor.clone()
    };

    let derive_count = if single.has_wildcard() { 3 } else { 1 };
    let mut derived = Vec::new();
    for index in 0..derive_count {
        let definite = single
            .at_derivation_index(index)
            .map_err(|error| Error::new(format!("deriving descriptor index {index}: {error}")))?;
        let mut entry = json!({
            "index": index,
            "script_pubkey_hex": hex_encode(definite.script_pubkey().as_bytes()),
        });
        if let Ok(address) = definite.address(network) {
            entry["address"] = json!(address.to_string());
        }
        derived.push(entry);
    }
    value["derived"] = json!(derived);
    Ok(value)
}

/// Decode a candidate raw-transaction payload from its charset: hex when the
/// payload is entirely hex digits, otherwise standard base64.
fn transaction_bytes(payload: &str) -> std::result::Result<Vec<u8>, String> {
    if payload.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return crate::bytes_arg::decode_hex(payload);
    }
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD
        .decode(payload)
        .map_err(|error| format!("neither hex nor base64: {error}"))
}

/// Transaction details: txid plus per-output spendable outpoints for the
/// create flow ("for now let's keep it manual" — no chain source verifies
/// these; the user vouches for the paste).
fn transaction_json(
    transaction: &bitcoin::Transaction,
    network: bitcoin::Network,
) -> serde_json::Value {
    let txid = transaction.compute_txid();
    // "Fully signed" heuristic without prevout access: every input carries a
    // witness and/or a scriptSig. Script-level validity needs the spent
    // outputs, which the deliberately-unwired chain sources would provide.
    let fully_signed = !transaction.input.is_empty()
        && transaction
            .input
            .iter()
            .all(|input| !input.witness.is_empty() || !input.script_sig.is_empty());
    let outputs: Vec<_> = transaction
        .output
        .iter()
        .enumerate()
        .map(|(vout, output)| {
            let mut entry = json!({
                "outpoint": format!("{txid}:{vout}"),
                "vout": vout,
                "amount_sats": output.value.to_sat(),
                "script_pubkey_hex": hex_encode(output.script_pubkey.as_bytes()),
            });
            if let Ok(address) = bitcoin::Address::from_script(&output.script_pubkey, network) {
                entry["address"] = json!(address.to_string());
            }
            entry
        })
        .collect();
    json!({
        "kind": "transaction",
        "txid": txid.to_string(),
        "input_count": transaction.input.len(),
        "output_count": transaction.output.len(),
        "fully_signed": fully_signed,
        "outputs": outputs,
    })
}

/// Payment-instruction details, plus route-level extraction of the BIP 21
/// `label`/`message`/query params the upstream parser validates but does not
/// expose.
fn payment_json(
    instructions: &bitcoin_payment_instructions::PaymentInstructions,
    payload: &str,
) -> serde_json::Value {
    use bitcoin_payment_instructions::{PaymentInstructions, PossiblyResolvedPaymentMethod};

    let mut value = json!({ "kind": "payment" });
    match instructions {
        PaymentInstructions::FixedAmount(fixed) => {
            value["variant"] = json!("fixed_amount");
            value["amount_sats"] = json!(fixed.max_amount().map(amount_sats));
            value["onchain_amount_sats"] = json!(fixed.onchain_payment_amount().map(amount_sats));
            value["methods"] = json!(fixed.methods().iter().map(method_json).collect::<Vec<_>>());
            value["description"] = json!(fixed.recipient_description());
        }
        PaymentInstructions::ConfigurableAmount(configurable) => {
            value["variant"] = json!("configurable_amount");
            value["min_amount_sats"] = json!(configurable.min_amt().map(amount_sats));
            value["max_amount_sats"] = json!(configurable.max_amt().map(amount_sats));
            value["methods"] = json!(
                configurable
                    .methods()
                    .map(|method| match method {
                        PossiblyResolvedPaymentMethod::Resolved(method) => method_json(method),
                        PossiblyResolvedPaymentMethod::LNURLPay {
                            min_value,
                            max_value,
                            ..
                        } => json!({
                            "type": "lnurl-pay",
                            "min_amount_sats": amount_sats(min_value),
                            "max_amount_sats": amount_sats(max_value),
                        }),
                    })
                    .collect::<Vec<_>>()
            );
            value["description"] = json!(configurable.recipient_description());
        }
    }

    // BIP 21/321 query params (percent-decoded, upstream-validated).
    if let Some((scheme, rest)) = payload.split_once(':')
        && scheme.eq_ignore_ascii_case("bitcoin")
        && let Some((_, query)) = rest.split_once('?')
    {
        let mut params = serde_json::Map::new();
        for param in query.split('&') {
            let (key, encoded) = match param.split_once('=') {
                Some((key, encoded)) => (key, encoded),
                None => (param, ""),
            };
            let decoded = percent_decode(encoded);
            if key.eq_ignore_ascii_case("label") {
                value["label"] = json!(decoded);
            } else if key.eq_ignore_ascii_case("message") {
                value["message"] = json!(decoded);
            }
            params.insert(key.to_ascii_lowercase(), json!(decoded));
        }
        value["params"] = serde_json::Value::Object(params);
    }
    value
}

fn method_json(method: &bitcoin_payment_instructions::PaymentMethod) -> serde_json::Value {
    use bitcoin_payment_instructions::PaymentMethod;
    match method {
        PaymentMethod::OnChain(address) => json!({
            "type": "onchain",
            "address": address.to_string(),
            "script_pubkey_hex": hex_encode(address.script_pubkey().as_bytes()),
        }),
        PaymentMethod::LightningBolt11(invoice) => json!({
            "type": "bolt11",
            "invoice": invoice.to_string(),
            "amount_msat": invoice.amount_milli_satoshis(),
        }),
        PaymentMethod::LightningBolt12(offer) => json!({
            "type": "bolt12",
            "offer": offer.to_string(),
        }),
        PaymentMethod::Cashu(_) => json!({ "type": "cashu" }),
    }
}

fn amount_sats(amount: bitcoin_payment_instructions::amount::Amount) -> u64 {
    // Round sub-sat (msat) amounts up, like the crate's own display helper.
    amount.sats_rounding_up()
}

/// Decode `%XX` escapes (BIP 21 URIs percent-encode; `+` is NOT a space).
/// A malformed escape passes through literally — display-level liberality.
fn percent_decode(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // BIP 32 test vector 1 master keys.
    const XPUB: &str = "xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8";
    const XPRV: &str = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";

    #[test]
    fn classifies_public_ranged_descriptors() {
        let value = classify(&format!("wpkh({XPUB}/0/*)"), bitcoin::Network::Bitcoin).unwrap();
        assert_eq!(value["kind"], "descriptor");
        assert_eq!(value["has_private_keys"], false);
        assert_eq!(value["is_ranged"], true);
        assert_eq!(value["is_multipath"], false);
        // Normalized public form with its checksum.
        assert!(value["descriptor"].as_str().unwrap().contains('#'));
        let derived = value["derived"].as_array().unwrap();
        assert_eq!(derived.len(), 3);
        for entry in derived {
            assert!(entry["script_pubkey_hex"].is_string(), "{entry}");
            assert!(
                entry["address"].as_str().unwrap().starts_with("bc1"),
                "{entry}"
            );
        }
    }

    #[test]
    fn flags_private_descriptor_material_and_never_echoes_it() {
        let value = classify(&format!("wpkh({XPRV}/0/*)"), bitcoin::Network::Bitcoin).unwrap();
        assert_eq!(value["kind"], "descriptor");
        assert_eq!(value["has_private_keys"], true);
        let normalized = value["descriptor"].as_str().unwrap();
        assert!(normalized.contains("xpub"), "{normalized}");
        assert!(!normalized.contains("xprv"), "{normalized}");
        // The public and private forms of the same descriptor derive the
        // same script set.
        let public = classify(&format!("wpkh({XPUB}/0/*)"), bitcoin::Network::Bitcoin).unwrap();
        assert_eq!(value["derived"], public["derived"]);
    }

    #[test]
    fn classifies_npub_peer_ids() {
        use bitcoin::bech32::{self, Hrp};
        let npub =
            bech32::encode::<bech32::Bech32>(Hrp::parse("npub").unwrap(), &[0xAB; 32]).unwrap();
        let value = classify(&npub, bitcoin::Network::Bitcoin).unwrap();
        assert_eq!(value["kind"], "peer_id");
        assert_eq!(value["format"], "npub");
        assert_eq!(value["id_hex"], "ab".repeat(32));
    }

    #[test]
    fn classifies_bip21_uris_with_label_and_params() {
        let uri = "bitcoin:1andreas3batLhQa2FawWjeyjCqyBzypd?amount=50&label=Luke-Jr\
                   &message=Donation%20for%20project%20xyz";
        let value = classify(uri, bitcoin::Network::Bitcoin).unwrap();
        assert_eq!(value["kind"], "payment");
        assert_eq!(value["variant"], "fixed_amount");
        assert_eq!(value["amount_sats"], 5_000_000_000u64);
        let methods = value["methods"].as_array().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0]["type"], "onchain");
        assert_eq!(methods[0]["address"], "1andreas3batLhQa2FawWjeyjCqyBzypd");
        // label/message/params are extracted at this layer (upstream 0.7.0
        // validates but does not expose them).
        assert_eq!(value["label"], "Luke-Jr");
        assert_eq!(value["message"], "Donation for project xyz");
        assert_eq!(value["params"]["amount"], "50");
    }

    fn regtest_address() -> String {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let secret = bitcoin::secp256k1::SecretKey::from_slice(&[1; 32]).unwrap();
        let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
        let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
        bitcoin::Address::p2wpkh(&public_key, bitcoin::Network::Regtest).to_string()
    }

    #[test]
    fn classifies_bare_addresses_as_payment_instructions() {
        let address = regtest_address();
        let value = classify(&address, bitcoin::Network::Regtest).unwrap();
        assert_eq!(value["kind"], "payment");
        assert_eq!(value["variant"], "configurable_amount");
        let methods = value["methods"].as_array().unwrap();
        assert_eq!(methods[0]["type"], "onchain");
        assert_eq!(methods[0]["address"], address);

        // The network gate is the caller's: a regtest address on mainnet is
        // an error naming the attempts.
        let error = classify(&address, bitcoin::Network::Bitcoin)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not payment instructions"), "{error}");
    }

    fn signed_transaction() -> bitcoin::Transaction {
        bitcoin::Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: vec![bitcoin::TxIn {
                previous_output:
                    "0000000000000000000000000000000000000000000000000000000000000001:7"
                        .parse()
                        .unwrap(),
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: bitcoin::Sequence::MAX,
                witness: bitcoin::Witness::from_slice(&[vec![0xAA; 71], vec![0xBB; 33]]),
            }],
            output: vec![bitcoin::TxOut {
                value: bitcoin::Amount::from_sat(50_000),
                script_pubkey: regtest_address()
                    .parse::<bitcoin::Address<bitcoin::address::NetworkUnchecked>>()
                    .unwrap()
                    .assume_checked()
                    .script_pubkey(),
            }],
        }
    }

    #[test]
    fn classifies_fully_signed_transactions_with_spendable_outpoints() {
        let transaction = signed_transaction();
        let hex = bitcoin::consensus::encode::serialize_hex(&transaction);
        let value = classify(&hex, bitcoin::Network::Regtest).unwrap();
        assert_eq!(value["kind"], "transaction");
        assert_eq!(value["txid"], transaction.compute_txid().to_string());
        assert_eq!(value["fully_signed"], true);
        let outputs = value["outputs"].as_array().unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["vout"], 0);
        assert_eq!(outputs[0]["amount_sats"], 50_000);
        assert_eq!(
            outputs[0]["outpoint"],
            format!("{}:0", transaction.compute_txid())
        );
        assert_eq!(outputs[0]["address"], regtest_address());

        // The same bytes as base64 classify identically (charset detection).
        use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
        let base64 = BASE64_STANDARD.encode(bitcoin::consensus::encode::serialize(&transaction));
        let again = classify(&base64, bitcoin::Network::Regtest).unwrap();
        assert_eq!(again["txid"], value["txid"]);

        // An unsigned input is still classified, flagged not fully signed.
        let mut unsigned = signed_transaction();
        unsigned.input[0].witness = bitcoin::Witness::new();
        let hex = bitcoin::consensus::encode::serialize_hex(&unsigned);
        let value = classify(&hex, bitcoin::Network::Regtest).unwrap();
        assert_eq!(value["fully_signed"], false);
    }

    #[test]
    fn psbt_pastes_are_redirected_to_the_psbt_routes() {
        let error = classify("cHNidP8BAAoBAAAAAA==", bitcoin::Network::Bitcoin)
            .unwrap_err()
            .to_string();
        assert!(error.contains("/api/inspect"), "{error}");
        assert!(error.contains("/api/import-bip174"), "{error}");
    }

    #[test]
    fn unclassifiable_payloads_name_every_attempt() {
        let error = classify("!!!", bitcoin::Network::Bitcoin)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not an output descriptor"), "{error}");
        assert!(error.contains("not a raw transaction"), "{error}");
        assert!(error.contains("not payment instructions"), "{error}");

        assert!(classify("  ", bitcoin::Network::Bitcoin).is_err());
    }

    #[test]
    fn percent_decoding_is_display_level_liberal() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("50"), "50");
        // Malformed escapes pass through literally; `+` is not a space.
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("a+b"), "a+b");
    }
}
