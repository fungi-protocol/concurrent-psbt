use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;
use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::OutputUniqueIdExt;

use ptj::cli::{Cli, Command, HexSeed, NetworkArg, OutPointArg, OutputArg};

const TXID: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const ADDRESS: &str = "1BoatSLRHtKNngkdXEeobR76b53LETtpyT";

#[test]
fn create_command_parses_typed_values_at_the_boundary() {
    let cli = Cli::try_parse_from([
        "ptj",
        "--output-file",
        "created.psbt",
        "create",
        "--input",
        &format!("{TXID}:7"),
        "--output",
        &format!("{ADDRESS}:0.00123456"),
        "--seed",
        "abcd",
        "--network",
        "regtest",
    ])
    .unwrap();

    let Command::Create(config) = cli.command else {
        panic!("expected create command");
    };

    assert_eq!(cli.output, Some(PathBuf::from("created.psbt")));
    assert_eq!(config.inputs[0].txid.to_string(), TXID);
    assert_eq!(config.inputs[0].vout, 7);
    assert_eq!(config.outputs[0].address_text, ADDRESS);
    assert_eq!(config.outputs[0].amount, bitcoin::Amount::from_sat(123_456));
    assert_eq!(
        config.seed.as_ref().map(HexSeed::as_bytes),
        Some(&[0xab, 0xcd][..])
    );
    assert_eq!(config.network, NetworkArg(bitcoin::Network::Regtest));
}

#[test]
fn join_and_sort_commands_parse_to_config_types() {
    let join = Cli::try_parse_from(["ptj", "join", "a.psbt", "b.psbt"]).unwrap();
    let Command::Join(config) = join.command else {
        panic!("expected join command");
    };
    assert_eq!(
        config.files,
        vec![PathBuf::from("a.psbt"), PathBuf::from("b.psbt")]
    );

    let concatenate = Cli::try_parse_from(["ptj", "concat", "a.psbt", "b.psbt"]).unwrap();
    let Command::Concatenate(config) = concatenate.command else {
        panic!("expected concatenate command");
    };
    assert_eq!(
        config.files,
        vec![PathBuf::from("a.psbt"), PathBuf::from("b.psbt")]
    );

    let export = Cli::try_parse_from(["ptj", "to-bip174", "ordered.psbt"]).unwrap();
    let Command::ExportBip174(config) = export.command else {
        panic!("expected export-bip174 command");
    };
    assert_eq!(config.file, PathBuf::from("ordered.psbt"));

    let sort = Cli::try_parse_from(["ptj", "sort", "--seed", "abcd", "joined.psbt"]).unwrap();
    let Command::Sort(config) = sort.command else {
        panic!("expected sort command");
    };
    assert_eq!(config.file, PathBuf::from("joined.psbt"));
    assert_eq!(
        config.seed.as_ref().map(HexSeed::as_bytes),
        Some(&[0xab, 0xcd][..])
    );

    let webgui =
        Cli::try_parse_from(["ptj", "webgui", "--host", "127.0.0.1", "--port", "8035"]).unwrap();
    let Command::Webgui(config) = webgui.command else {
        panic!("expected webgui command");
    };
    assert_eq!(config.host, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(config.port, 8035);
}

#[test]
fn typed_arguments_reject_malformed_values() {
    assert!(NetworkArg::from_str("liquid").is_err());
    assert!(HexSeed::from_str("abc").is_err());
    assert!(HexSeed::from_str("zz").is_err());
    assert!(OutPointArg::from_str("not-an-outpoint").is_err());
    assert!(OutPointArg::from_str(&format!("{TXID}:not-a-vout")).is_err());
    assert!(OutputArg::from_str(&format!("{ADDRESS}:not-an-amount")).is_err());
    assert!(OutputArg::from_str(ADDRESS).is_err());
}

#[test]
fn create_emits_real_unordered_psbt_bytes() {
    let psbt = run_to_psbt([
        "ptj",
        "create",
        "--network",
        "regtest",
        "--input",
        &format!("{TXID}:7"),
        "--output",
        &format!("{}:0.00123456", regtest_address(1)),
        "--seed",
        "abcd",
    ]);

    assert_eq!(psbt.global.input_count, 1);
    assert_eq!(psbt.global.output_count, 1);
    assert_eq!(psbt.global.tx_modifiable_flags & 0x03, 0x03);
    assert!(psbt.global.is_unordered());
    assert_eq!(psbt.global.sort_seed(), Some(&[0xab, 0xcd][..]));
    assert_eq!(psbt.inputs[0].previous_txid.to_string(), TXID);
    assert_eq!(psbt.inputs[0].spent_output_index, 7);
    assert_eq!(psbt.outputs[0].amount, bitcoin::Amount::from_sat(123_456));
    assert!(psbt.outputs[0].has_unique_id());
}

#[test]
fn join_is_idempotent_on_real_psbt_files() {
    let temp = tempfile::tempdir().unwrap();
    let a = write_psbt(temp.path(), "a.psbt", create_psbt(TXID, 0, 1, 50_000));
    let b = write_psbt(
        temp.path(),
        "b.psbt",
        create_psbt(
            "0000000000000000000000000000000000000000000000000000000000000002",
            1,
            2,
            70_000,
        ),
    );

    let joined = run_to_psbt(["ptj", "join", path_str(&a), path_str(&b)]);
    assert_eq!(joined.global.input_count, 2);
    assert_eq!(joined.global.output_count, 2);

    let joined_path = write_psbt(temp.path(), "joined.psbt", joined);
    let idempotent = run_to_psbt([
        "ptj",
        "join",
        path_str(&joined_path),
        path_str(&a),
        path_str(&b),
    ]);
    assert_eq!(idempotent.global.input_count, 2);
    assert_eq!(idempotent.global.output_count, 2);
}

#[test]
fn sort_makes_join_paths_byte_identical() {
    let temp = tempfile::tempdir().unwrap();
    let a = write_psbt(temp.path(), "a.psbt", create_psbt(TXID, 0, 1, 50_000));
    let b = write_psbt(
        temp.path(),
        "b.psbt",
        create_psbt(
            "0000000000000000000000000000000000000000000000000000000000000002",
            1,
            2,
            70_000,
        ),
    );

    let ab = run_to_psbt(["ptj", "join", path_str(&a), path_str(&b)]);
    let ba = run_to_psbt(["ptj", "join", path_str(&b), path_str(&a)]);
    let ab_path = write_psbt(temp.path(), "ab.psbt", ab);
    let ba_path = write_psbt(temp.path(), "ba.psbt", ba);

    let sorted_ab = run_to_psbt(["ptj", "sort", "--seed", "deadbeef", path_str(&ab_path)]);
    let sorted_ba = run_to_psbt(["ptj", "sort", "--seed", "deadbeef", path_str(&ba_path)]);

    assert!(!sorted_ab.global.is_unordered());
    assert_eq!(psbt_bytes(&sorted_ab), psbt_bytes(&sorted_ba));
}

#[test]
fn concatenate_appends_ordered_psbts_without_lattice_joining() {
    let temp = tempfile::tempdir().unwrap();
    let a = write_psbt(temp.path(), "a.psbt", sorted_psbt(TXID, 0, 1, 50_000));
    let b = write_psbt(
        temp.path(),
        "b.psbt",
        sorted_psbt(
            "0000000000000000000000000000000000000000000000000000000000000002",
            1,
            2,
            70_000,
        ),
    );

    let concatenated = run_to_psbt(["ptj", "concatenate", path_str(&a), path_str(&b)]);

    assert_eq!(concatenated.global.input_count, 2);
    assert_eq!(concatenated.global.output_count, 2);
    assert!(!concatenated.global.is_unordered());
    assert_eq!(concatenated.inputs[0].previous_txid.to_string(), TXID);
    assert_eq!(
        concatenated.inputs[1].previous_txid.to_string(),
        "0000000000000000000000000000000000000000000000000000000000000002"
    );
    assert_eq!(
        concatenated.outputs[0].amount,
        bitcoin::Amount::from_sat(50_000)
    );
    assert_eq!(
        concatenated.outputs[1].amount,
        bitcoin::Amount::from_sat(70_000)
    );
}

#[test]
fn concatenate_rejects_unordered_psbts() {
    let temp = tempfile::tempdir().unwrap();
    let unordered = write_psbt(
        temp.path(),
        "unordered.psbt",
        create_psbt(TXID, 0, 1, 50_000),
    );
    let ordered = write_psbt(
        temp.path(),
        "ordered.psbt",
        sorted_psbt(
            "0000000000000000000000000000000000000000000000000000000000000002",
            1,
            2,
            70_000,
        ),
    );

    let error = run_error([
        "ptj",
        "concatenate",
        path_str(&unordered),
        path_str(&ordered),
    ]);
    assert!(error.to_string().contains("ordered PSBT"));
}

#[test]
fn concatenate_rejects_different_global_contexts_before_appending() {
    let temp = tempfile::tempdir().unwrap();
    let a = write_psbt(temp.path(), "a.psbt", sorted_psbt(TXID, 0, 1, 50_000));
    let mut different_global = sorted_psbt(
        "0000000000000000000000000000000000000000000000000000000000000002",
        1,
        2,
        70_000,
    );
    different_global.global.tx_version = bitcoin::transaction::Version::ONE;
    let b = write_psbt(temp.path(), "b.psbt", different_global);

    let error = run_error(["ptj", "concatenate", path_str(&a), path_str(&b)]);
    let message = error.to_string();

    assert!(message.contains(path_str(&b)));
    assert!(message.contains("global fields"));
    assert!(message.contains("discard or reorder global information"));
}

#[test]
fn export_bip174_turns_ordered_bip370_into_unsigned_transaction_psbt() {
    let temp = tempfile::tempdir().unwrap();
    let ordered = write_psbt(temp.path(), "ordered.psbt", sorted_psbt(TXID, 0, 1, 50_000));

    let exported = run_to_bip174(["ptj", "export-bip174", path_str(&ordered)]);

    assert_eq!(exported.version, 0);
    assert_eq!(exported.unsigned_tx.input.len(), 1);
    assert_eq!(exported.unsigned_tx.output.len(), 1);
    assert_eq!(
        exported.unsigned_tx.input[0]
            .previous_output
            .txid
            .to_string(),
        TXID
    );
    assert_eq!(exported.unsigned_tx.input[0].previous_output.vout, 0);
    assert_eq!(
        exported.unsigned_tx.output[0].value,
        bitcoin::Amount::from_sat(50_000)
    );
    assert_eq!(exported.outputs[0].proprietary.len(), 1);
}

#[test]
fn export_bip174_preserves_input_sequence() {
    let temp = tempfile::tempdir().unwrap();
    let mut ordered = sorted_psbt(TXID, 0, 1, 50_000);
    ordered.inputs[0].sequence = Some(bitcoin::Sequence(0xffff_fffd));
    let ordered = write_psbt(temp.path(), "ordered.psbt", ordered);

    let exported = run_to_bip174(["ptj", "export-bip174", path_str(&ordered)]);

    assert_eq!(
        exported.unsigned_tx.input[0].sequence,
        bitcoin::Sequence(0xffff_fffd)
    );
}

#[test]
fn export_bip174_defaults_absent_input_sequence_to_final() {
    let temp = tempfile::tempdir().unwrap();
    let ordered = write_psbt(temp.path(), "ordered.psbt", sorted_psbt(TXID, 0, 1, 50_000));

    let exported = run_to_bip174(["ptj", "export-bip174", path_str(&ordered)]);

    assert_eq!(
        exported.unsigned_tx.input[0].sequence,
        bitcoin::Sequence::MAX
    );
}

#[test]
fn export_bip174_rejects_unordered_psbts() {
    let temp = tempfile::tempdir().unwrap();
    let unordered = write_psbt(
        temp.path(),
        "unordered.psbt",
        create_psbt(TXID, 0, 1, 50_000),
    );

    let error = run_error(["ptj", "export-bip174", path_str(&unordered)]);
    assert!(error.to_string().contains("run `ptj sort` first"));
}

#[test]
fn commands_reject_bip174_inputs_explicitly_until_import_exists() {
    let temp = tempfile::tempdir().unwrap();
    let ordered = write_psbt(temp.path(), "ordered.psbt", sorted_psbt(TXID, 0, 1, 50_000));
    let core_psbt = ptj::run(Cli::try_parse_from(["ptj", "export-bip174", path_str(&ordered)]).unwrap()).unwrap();
    let core_path = temp.path().join("core.psbt");
    std::fs::write(&core_path, core_psbt).unwrap();

    let error = run_error(["ptj", "sort", path_str(&core_path)]);
    assert!(error.to_string().contains("BIP 174"));
    assert!(error.to_string().contains("import"));
}

#[test]
fn run_or_write_atomically_writes_output_file() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("created.psbt");

    let cli = Cli::try_parse_from([
        "ptj",
        "--output-file",
        path_str(&output),
        "create",
        "--network",
        "regtest",
        "--input",
        &format!("{TXID}:7"),
        "--output",
        &format!("{}:0.00123456", regtest_address(1)),
        "--seed",
        "abcd",
    ])
    .unwrap();

    assert_eq!(ptj::run_or_write(cli).unwrap(), None);
    let written = std::fs::read_to_string(output).unwrap();
    let psbt = decode_psbt(&written);
    assert_eq!(psbt.global.input_count, 1);
    assert_eq!(psbt.global.output_count, 1);
    assert!(psbt.global.is_unordered());
}

#[test]
fn run_or_write_can_replace_an_input_file_after_joining() {
    let temp = tempfile::tempdir().unwrap();
    let target = write_psbt(temp.path(), "target.psbt", create_psbt(TXID, 0, 1, 50_000));
    let other = write_psbt(
        temp.path(),
        "other.psbt",
        create_psbt(
            "0000000000000000000000000000000000000000000000000000000000000002",
            1,
            2,
            70_000,
        ),
    );

    let cli = Cli::try_parse_from([
        "ptj",
        "--output-file",
        path_str(&target),
        "join",
        path_str(&target),
        path_str(&other),
    ])
    .unwrap();

    assert_eq!(ptj::run_or_write(cli).unwrap(), None);
    let updated = decode_psbt(&std::fs::read_to_string(&target).unwrap());
    assert_eq!(updated.global.input_count, 2);
    assert_eq!(updated.global.output_count, 2);
}

#[test]
fn webgui_embeds_static_offline_assets() {
    let index = ptj::webgui::asset("/").unwrap();
    assert_eq!(index.content_type, "text/html; charset=utf-8");
    assert!(
        std::str::from_utf8(index.body)
            .unwrap()
            .contains("Partial Transaction Joiner")
    );

    let script = ptj::webgui::asset("/dist/app.js?v=cache-busted").unwrap();
    assert_eq!(script.content_type, "text/javascript; charset=utf-8");
    assert!(
        std::str::from_utf8(script.body)
            .unwrap()
            .contains("createInitialState")
    );

    assert!(ptj::webgui::asset("/missing.js").is_none());
}

fn run_to_psbt<const N: usize>(args: [&str; N]) -> psbt_v2::v2::Psbt {
    let output = ptj::run(Cli::try_parse_from(args).unwrap()).unwrap();
    decode_psbt(&output)
}

fn run_error<const N: usize>(args: [&str; N]) -> ptj::Error {
    ptj::run(Cli::try_parse_from(args).unwrap()).unwrap_err()
}

fn run_to_bip174<const N: usize>(args: [&str; N]) -> psbt_v2::v0::bitcoin::Psbt {
    ptj::run(Cli::try_parse_from(args).unwrap())
        .unwrap()
        .parse()
        .unwrap()
}

fn create_psbt(txid: &str, vout: u32, address_seed: u8, amount_sats: u64) -> psbt_v2::v2::Psbt {
    run_to_psbt([
        "ptj",
        "create",
        "--network",
        "regtest",
        "--input",
        &format!("{txid}:{vout}"),
        "--output",
        &format!(
            "{}:{}",
            regtest_address(address_seed),
            btc_value(amount_sats)
        ),
        "--seed",
        "abcd",
    ])
}

fn sorted_psbt(txid: &str, vout: u32, address_seed: u8, amount_sats: u64) -> psbt_v2::v2::Psbt {
    let temp = tempfile::tempdir().unwrap();
    let unordered = write_psbt(
        temp.path(),
        "unordered.psbt",
        create_psbt(txid, vout, address_seed, amount_sats),
    );
    run_to_psbt(["ptj", "sort", "--seed", "abcd", path_str(&unordered)])
}

fn write_psbt(dir: &std::path::Path, name: &str, psbt: psbt_v2::v2::Psbt) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, encode_psbt(&psbt)).unwrap();
    path
}

fn psbt_bytes(psbt: &psbt_v2::v2::Psbt) -> Vec<u8> {
    psbt_v2::v2::Psbt::serialize(psbt)
}

fn encode_psbt(psbt: &psbt_v2::v2::Psbt) -> String {
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD.encode(psbt_bytes(psbt))
}

fn decode_psbt(encoded: &str) -> psbt_v2::v2::Psbt {
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    let bytes = BASE64_STANDARD.decode(encoded.trim()).unwrap();
    psbt_v2::v2::Psbt::deserialize(&bytes).unwrap()
}

fn regtest_address(seed: u8) -> String {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let secret = bitcoin::secp256k1::SecretKey::from_slice(&[seed; 32]).unwrap();
    let public_key = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret);
    let public_key = bitcoin::CompressedPublicKey::from_slice(&public_key.serialize()).unwrap();
    bitcoin::Address::p2wpkh(&public_key, bitcoin::Network::Regtest).to_string()
}

fn btc_value(amount_sats: u64) -> String {
    bitcoin::Amount::from_sat(amount_sats).to_btc().to_string()
}

fn path_str(path: &std::path::Path) -> &str {
    path.to_str().unwrap()
}
