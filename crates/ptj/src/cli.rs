use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug, Clone)]
#[command(name = "ptj")]
pub struct Cli {
    /// Write command output atomically to a file instead of stdout
    #[arg(short = 'o', long = "output-file", global = true)]
    pub output: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Split a constructor PSBT into atomic unordered fragments
    Atomize(AtomizeConfig),
    /// Create a new PSBT with inputs and outputs
    Create(CreateConfig),
    /// Join two or more PSBTs
    Join(JoinConfig),
    /// Append ordered PSBT inputs and outputs without lattice joining
    #[command(alias = "concat")]
    Concatenate(ConcatenateConfig),
    /// Export an ordered BIP 370 PSBT as Bitcoin Core-compatible BIP 174
    #[command(alias = "to-bip174")]
    ExportBip174(ExportBip174Config),
    /// Inspect a PSBT without transforming it
    Inspect(InspectConfig),
    /// Mark a safe BIP 370 constructor PSBT unordered for lattice joining
    MakeUnordered(MakeUnorderedConfig),
    /// Sort a PSBT into BIP 370 order
    Sort(SortConfig),
}

#[derive(Args, Debug, Clone)]
pub struct AtomizeConfig {
    /// PSBT file to atomize
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct CreateConfig {
    /// Input in txid:vout format (repeatable)
    #[arg(long = "input")]
    pub inputs: Vec<OutPointArg>,
    /// Output in addr:amount_btc format (repeatable)
    #[arg(long = "output")]
    pub outputs: Vec<OutputArg>,
    /// Sort seed as hex
    #[arg(long)]
    pub seed: Option<HexSeed>,
    /// Bitcoin network (bitcoin, testnet, signet, regtest)
    #[arg(long, default_value = "bitcoin")]
    pub network: NetworkArg,
}

#[derive(Args, Debug, Clone)]
pub struct JoinConfig {
    /// PSBT files to join (at least 2)
    #[arg(required = true, num_args = 2..)]
    pub files: Vec<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct ConcatenateConfig {
    /// Ordered PSBT files to append (at least 2)
    #[arg(required = true, num_args = 2..)]
    pub files: Vec<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct ExportBip174Config {
    /// Ordered PSBT file to export
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct InspectConfig {
    /// PSBT file to inspect
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct MakeUnorderedConfig {
    /// PSBT file to mark unordered
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct SortConfig {
    /// Sort seed as hex (overrides embedded seed)
    #[arg(long)]
    pub seed: Option<HexSeed>,
    /// PSBT file to sort
    pub file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkArg(pub bitcoin::Network);

impl FromStr for NetworkArg {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "bitcoin" | "mainnet" => Ok(Self(bitcoin::Network::Bitcoin)),
            "testnet" | "testnet3" => Ok(Self(bitcoin::Network::Testnet)),
            "signet" => Ok(Self(bitcoin::Network::Signet)),
            "regtest" => Ok(Self(bitcoin::Network::Regtest)),
            other => Err(format!(
                "unknown network '{other}' (expected: bitcoin, testnet, signet, regtest)"
            )),
        }
    }
}

impl std::fmt::Display for NetworkArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HexSeed(Vec<u8>);

impl HexSeed {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl FromStr for HexSeed {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        decode_hex(value).map(Self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutPointArg {
    pub txid: bitcoin::Txid,
    pub vout: u32,
}

impl OutPointArg {
    pub fn into_outpoint(self) -> bitcoin::OutPoint {
        bitcoin::OutPoint {
            txid: self.txid,
            vout: self.vout,
        }
    }
}

impl FromStr for OutPointArg {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (txid, vout) = value
            .split_once(':')
            .ok_or_else(|| "--input expects txid:vout".to_string())?;
        Ok(Self {
            txid: txid
                .parse()
                .map_err(|error| format!("invalid txid {txid}: {error}"))?,
            vout: vout
                .parse()
                .map_err(|error| format!("invalid vout {vout}: {error}"))?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputArg {
    pub address_text: String,
    pub address: bitcoin::Address<bitcoin::address::NetworkUnchecked>,
    pub amount: bitcoin::Amount,
}

impl FromStr for OutputArg {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (address, amount) = value
            .split_once(':')
            .ok_or_else(|| "--output expects address:amount_btc".to_string())?;
        Ok(Self {
            address_text: address.to_string(),
            address: address
                .parse()
                .map_err(|error| format!("invalid address {address}: {error}"))?,
            amount: bitcoin::Amount::from_str_in(amount, bitcoin::Denomination::Bitcoin)
                .map_err(|error| format!("invalid amount {amount}: {error}"))?,
        })
    }
}

fn decode_hex(value: &str) -> std::result::Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err(format!("hex string has odd length: {value}"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or_else(|| format!("invalid hex: {value}"))?;
            let low = hex_nibble(pair[1]).ok_or_else(|| format!("invalid hex: {value}"))?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
