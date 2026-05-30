use std::fs;
use std::process;

use clap::{Parser, Subcommand};

use concurrent_psbt::Join;
use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
use concurrent_psbt::roles::{Creator, constructor::dynamic};
use concurrent_psbt::sort::{Deterministic, Sorter};
use psbt_v2::v2::{Input, Output, Psbt};

#[derive(Parser)]
#[command(name = "ptj")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new PSBT with inputs and outputs
    Create {
        /// Input in txid:vout format (repeatable)
        #[arg(long = "input")]
        inputs: Vec<String>,
        /// Output in addr:amount format (repeatable)
        #[arg(long = "output")]
        outputs: Vec<String>,
        /// Sort seed as hex
        #[arg(long)]
        seed: Option<String>,
        /// Bitcoin network (bitcoin, testnet, signet, regtest)
        #[arg(long, default_value = "bitcoin")]
        network: String,
    },
    /// Join two or more PSBTs
    Join {
        /// PSBT files to join (at least 2)
        #[arg(required = true, num_args = 2..)]
        files: Vec<String>,
    },
    /// Sort a PSBT deterministically
    Sort {
        /// Sort seed as hex (overrides embedded seed)
        #[arg(long)]
        seed: Option<String>,
        /// PSBT file to sort
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Create {
            inputs,
            outputs,
            seed,
            network,
        } => cmd_create(&inputs, &outputs, seed.as_deref(), &network),
        Command::Join { files } => cmd_join(&files),
        Command::Sort { seed, file } => cmd_sort(seed.as_deref(), &file),
    }
}

// ── ptj create ─────────────────────────────────────────────────────

fn cmd_create(inputs: &[String], outputs: &[String], seed: Option<&str>, network: &str) {
    let network = match network {
        "bitcoin" | "mainnet" => bitcoin::Network::Bitcoin,
        "testnet" | "testnet3" => bitcoin::Network::Testnet,
        "signet" => bitcoin::Network::Signet,
        "regtest" => bitcoin::Network::Regtest,
        other => {
            eprintln!(
                "error: unknown network '{other}' (expected: bitcoin, testnet, signet, regtest)"
            );
            process::exit(1);
        }
    };
    let parsed_inputs: Vec<(String, u32)> = inputs
        .iter()
        .map(|val| {
            let parts: Vec<&str> = val.splitn(2, ':').collect();
            if parts.len() != 2 {
                eprintln!("error: --input expects txid:vout");
                process::exit(1);
            }
            let vout: u32 = parts[1].parse().unwrap_or_else(|_| {
                eprintln!("error: invalid vout: {}", parts[1]);
                process::exit(1);
            });
            (parts[0].to_string(), vout)
        })
        .collect();

    let parsed_outputs: Vec<(String, bitcoin::Amount)> = outputs
        .iter()
        .map(|val| {
            let parts: Vec<&str> = val.splitn(2, ':').collect();
            if parts.len() != 2 {
                eprintln!("error: --output expects address:amount_btc");
                process::exit(1);
            }
            let amount = bitcoin::Amount::from_str_in(parts[1], bitcoin::Denomination::Bitcoin)
                .unwrap_or_else(|e| {
                    eprintln!("error: invalid amount '{}': {e}", parts[1]);
                    process::exit(1);
                });
            (parts[0].to_string(), amount)
        })
        .collect();

    let seed = seed.map(decode_hex);

    let mut ctor = Creator::new().build();

    for (txid_hex, vout) in &parsed_inputs {
        let txid = txid_hex.parse().unwrap_or_else(|e| {
            eprintln!("error: invalid txid {txid_hex}: {e}");
            process::exit(1);
        });
        ctor = ctor.input(Input::new(&bitcoin::OutPoint { txid, vout: *vout }));
    }

    for (addr_str, amount) in &parsed_outputs {
        let addr: bitcoin::Address<bitcoin::address::NetworkUnchecked> =
            addr_str.parse().unwrap_or_else(|e| {
                eprintln!("error: invalid address {addr_str}: {e}");
                process::exit(1);
            });
        let addr = addr.require_network(network).unwrap_or_else(|e| {
            eprintln!("error: address {addr_str} not valid for {network}: {e}");
            process::exit(1);
        });
        let script_pubkey = addr.script_pubkey();

        let mut output = Output {
            amount: *amount,
            script_pubkey,
            ..Output::default()
        };

        // Auto-generate PSBT_OUT_UNIQUE_ID (16 bytes of randomness)
        let uid: [u8; 16] = rand::random();
        let key = psbt_v2::raw::ProprietaryKey {
            prefix: concurrent_psbt::PROPRIETARY_PREFIX.to_vec(),
            subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
            key: vec![],
        };
        output.proprietaries.insert(key, uid.to_vec());

        ctor = ctor.output(output);
    }

    // Set global sort fields
    let mut psbt = ctor.into_inner();
    psbt.global.set_unordered();
    if let Some(s) = seed {
        psbt.global.set_sort_seed(s);
        psbt.global.set_sort_deterministic(0x01);
    }
    // Re-set modifiable flags (into_inner consumed the constructor)
    psbt.global.tx_modifiable_flags = 0x03;

    emit_psbt(&psbt.into_psbt());
}

// ── ptj join ───────────────────────────────────────────────────────

fn cmd_join(files: &[String]) {
    let constructors: Vec<_> = files.iter().map(|f| parse_psbt(f)).collect();

    let result = constructors
        .into_iter()
        .map(dynamic::ResultConstructor::wrap)
        .reduce(|a, b| a.join(b))
        .unwrap();

    if !result.is_ok() {
        eprintln!("error: join produced conflicting fields");
        eprintln!();
        result.for_each_conflict(|section, field, conflict| {
            eprintln!("  {section}.{field}: {conflict:?}");
        });
        process::exit(1);
    }

    let ctor = result.try_unwrap().expect("checked is_ok");
    emit_psbt(&ctor.into_psbt());
}

// ── ptj sort ───────────────────────────────────────────────────────

fn cmd_sort(seed_override: Option<&str>, file: &str) {
    let seed_override = seed_override.map(decode_hex);

    let ctor = parse_psbt(file);
    let mut psbt = ctor.into_inner();

    if let Some(s) = seed_override {
        psbt.global.set_sort_seed(s);
    }

    let sorter: Sorter<Deterministic> = Sorter::from_unordered_psbt(psbt);

    let ordered = sorter.into_ordered_psbt().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });

    emit_psbt(&ordered);
}

// ── Shared helpers ─────────────────────────────────────────────────

fn parse_psbt(path: &str) -> dynamic::Constructor {
    let raw = fs::read(path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });

    let bytes = if raw.starts_with(b"psbt") {
        raw
    } else {
        let text = String::from_utf8(raw).unwrap_or_else(|_| {
            eprintln!("error: {path} is neither binary PSBT nor valid UTF-8");
            process::exit(1);
        });
        use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
        BASE64_STANDARD.decode(text.trim()).unwrap_or_else(|e| {
            eprintln!("error decoding base64 {path}: {e}");
            process::exit(1);
        })
    };

    let psbt = Psbt::deserialize(&bytes).unwrap_or_else(|e| {
        eprintln!("error parsing {path}: {e}");
        process::exit(1);
    });

    dynamic::Constructor::try_from_psbt(psbt).unwrap_or_else(|e| {
        eprintln!("error: {path}: {e}");
        process::exit(1);
    })
}

fn emit_psbt(psbt: &Psbt) {
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    let bytes = Psbt::serialize(psbt);
    println!("{}", BASE64_STANDARD.encode(&bytes));
}

fn decode_hex(s: &str) -> Vec<u8> {
    if !s.len().is_multiple_of(2) {
        eprintln!("error: hex string has odd length: {s}");
        process::exit(1);
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|_| {
                eprintln!("error: invalid hex: {s}");
                process::exit(1);
            })
        })
        .collect()
}
