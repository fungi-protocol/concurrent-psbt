#[cfg(feature = "webgui")]
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug, Clone)]
#[command(name = "ptj")]
pub struct Cli {
    /// Write command output atomically to a file instead of stdout
    #[arg(short = 'o', long = "output-file", global = true)]
    pub output: Option<PathBuf>,
    /// Encoding to use with --output-file
    #[arg(long = "output-file-format", value_enum, default_value_t = OutputFileFormat::Base64, global = true)]
    pub output_file_format: OutputFileFormat,
    /// Write raw PSBT bytes to --output-file or sync --state
    #[arg(long, global = true)]
    pub binary: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Assign unique ids to inputs/outputs that lack them (spec identity fields)
    AssignIds(AssignIdsConfig),
    /// Split a constructor PSBT into atomic unordered fragments
    Atomize(AtomizeConfig),
    /// Print the capability catalog: this build's typed transport surface
    Capabilities(CapabilitiesConfig),
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
    /// Import a Bitcoin Core-compatible BIP 174 PSBT as ordered BIP 370
    ImportBip174(ImportBip174Config),
    /// Inspect a PSBT without transforming it
    Inspect(InspectConfig),
    /// Mark a safe BIP 370 constructor PSBT unordered for lattice joining
    MakeUnordered(MakeUnorderedConfig),
    /// Declare an explicit fee contribution (spec § Termination metadata)
    Fee(FeeConfig),
    /// Attach a payment a participant wants constructed (negotiation metadata)
    Pay(PayConfig),
    /// Attach a confirmation of the converged transaction prior to signing
    Confirm(ConfirmConfig),
    /// Decode the payments and confirmations negotiated in a PSBT
    Payments(PaymentsConfig),
    /// Sort a PSBT into BIP 370 order
    Sort(SortConfig),
    /// Join local PSBT sources and print the converged state
    Sync(SyncConfig),
    /// Serve the offline web GUI
    #[cfg(feature = "webgui")]
    Webgui(WebguiConfig),
    /// Open the interactive terminal UI (WIP placeholder)
    #[cfg(feature = "tui")]
    Tui(TuiConfig),
}

impl Command {
    pub fn reads_stdin(&self) -> bool {
        match self {
            Command::AssignIds(config) => is_stdin_path(&config.file),
            Command::Atomize(config) => is_stdin_path(&config.file),
            Command::Concatenate(config) => config.files.iter().any(|path| is_stdin_path(path)),
            Command::ExportBip174(config) => is_stdin_path(&config.file),
            Command::ImportBip174(config) => is_stdin_path(&config.file),
            Command::Fee(config) => is_stdin_path(&config.file),
            Command::Inspect(config) => is_stdin_path(&config.file),
            Command::Join(config) => config.files.iter().any(|path| is_stdin_path(path)),
            Command::MakeUnordered(config) => is_stdin_path(&config.file),
            Command::Pay(config) => is_stdin_path(&config.file),
            Command::Confirm(config) => is_stdin_path(&config.file),
            Command::Payments(config) => is_stdin_path(&config.file),
            Command::Sort(config) => is_stdin_path(&config.file),
            Command::Sync(config) => {
                !config.ongoing && config.sources.iter().any(|path| is_stdin_path(path))
            }
            Command::Capabilities(_) => false,
            Command::Create(_) => false,
            #[cfg(feature = "webgui")]
            Command::Webgui(_) => false,
            #[cfg(feature = "tui")]
            Command::Tui(_) => false,
        }
    }

    pub(crate) fn stdin_source_count(&self) -> usize {
        match self {
            Command::AssignIds(config) => usize::from(is_stdin_path(&config.file)),
            Command::Atomize(config) => usize::from(is_stdin_path(&config.file)),
            Command::Concatenate(config) => config
                .files
                .iter()
                .filter(|path| is_stdin_path(path))
                .count(),
            Command::ExportBip174(config) => usize::from(is_stdin_path(&config.file)),
            Command::ImportBip174(config) => usize::from(is_stdin_path(&config.file)),
            Command::Fee(config) => usize::from(is_stdin_path(&config.file)),
            Command::Inspect(config) => usize::from(is_stdin_path(&config.file)),
            Command::Join(config) => config
                .files
                .iter()
                .filter(|path| is_stdin_path(path))
                .count(),
            Command::MakeUnordered(config) => usize::from(is_stdin_path(&config.file)),
            Command::Pay(config) => usize::from(is_stdin_path(&config.file)),
            Command::Confirm(config) => usize::from(is_stdin_path(&config.file)),
            Command::Payments(config) => usize::from(is_stdin_path(&config.file)),
            Command::Sort(config) => usize::from(is_stdin_path(&config.file)),
            Command::Sync(config) => config
                .sources
                .iter()
                .filter(|path| is_stdin_path(path))
                .count(),
            Command::Capabilities(_) => 0,
            Command::Create(_) => 0,
            #[cfg(feature = "webgui")]
            Command::Webgui(_) => 0,
            #[cfg(feature = "tui")]
            Command::Tui(_) => 0,
        }
    }
}

fn is_stdin_path(path: &Path) -> bool {
    path == Path::new("-")
}

/// No options: the catalog is a compile-time fact of this binary (see
/// `crate::capabilities`), so there is nothing to configure.
#[derive(Args, Debug, Clone)]
pub struct CapabilitiesConfig {}

#[derive(Args, Debug, Clone)]
pub struct AssignIdsConfig {
    /// Manual id assignment `<in|out>:<index>=<bytes>` (repeatable). `out`
    /// sets PSBT_OUT_UNIQUE_ID, `in` sets the optional PSBT_IN_UNIQUE_ID
    /// outpoint suffix; bytes accept hex/base58/bech32 by character set
    #[arg(long = "id")]
    pub ids: Vec<IdAssignment>,
    /// Also assign fresh random 16-byte ids to outputs still missing one
    /// (the default when no --id is given)
    #[arg(long)]
    pub auto: bool,
    /// Replace an existing unique id that differs from the requested one
    /// (default: error; matching ids are always accepted idempotently)
    #[arg(long)]
    pub overwrite: bool,
    /// PSBT file to assign ids in
    pub file: PathBuf,
}

/// One `--id <in|out>:<index>=<bytes>` directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdAssignment {
    pub target: IdTarget,
    pub index: usize,
    pub id: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdTarget {
    Input,
    Output,
}

impl FromStr for IdAssignment {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (selector, id) = value
            .split_once('=')
            .ok_or_else(|| format!("expected <in|out>:<index>=<bytes>, got {value}"))?;
        let (kind, index) = selector
            .split_once(':')
            .ok_or_else(|| format!("expected selector <in|out>:<index>, got {selector}"))?;
        let target = match kind {
            "in" | "input" => IdTarget::Input,
            "out" | "output" => IdTarget::Output,
            other => return Err(format!("unknown id target {other} (expected in or out)")),
        };
        let index = index
            .parse::<usize>()
            .map_err(|error| format!("invalid {kind} index {index}: {error}"))?;
        let id = crate::bytes_arg::parse_bytes_arg(id)?;
        Ok(Self { target, index, id })
    }
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
    /// Sort seed as hex (or base58/bech32, detected from the character set)
    #[arg(long)]
    pub seed: Option<HexSeed>,
    /// Accept an ordering seed below the spec minimum of 128 bits (16 bytes)
    #[arg(long = "allow-short-seed")]
    pub allow_short_seed: bool,
    /// Ordering mode for the unordered PSBT
    #[arg(long = "ordering", value_enum, default_value_t = OrderingArg::Unset)]
    pub ordering: OrderingArg,
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
pub struct ImportBip174Config {
    /// Mark the imported PSBT's inputs and outputs modifiable (BIP 174 has no
    /// TX_MODIFIABLE field; this is an explicit assertion, off by default)
    #[arg(long)]
    pub modifiable: bool,
    /// BIP 174 PSBT file to import
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
    /// Sort seed as hex (or base58/bech32; overrides embedded seed)
    #[arg(long)]
    pub seed: Option<HexSeed>,
    /// Accept an ordering seed below the spec minimum of 128 bits (16 bytes)
    #[arg(long = "allow-short-seed")]
    pub allow_short_seed: bool,
    /// PSBT file to sort
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct SyncConfig {
    /// Transport backend to sync over
    #[arg(long = "transport", value_enum, default_value_t = TransportKind::Local)]
    pub transport: TransportKind,
    /// State PSBT file to update atomically with the converged result
    #[arg(long = "state")]
    pub state: Option<PathBuf>,
    /// Import an iroh-docs write ticket from this file and sync through that document
    #[arg(long = "iroh-ticket")]
    pub iroh_ticket: Option<PathBuf>,
    /// Create an iroh-docs document and write its ticket to this file
    #[arg(long = "iroh-ticket-out")]
    pub iroh_ticket_out: Option<PathBuf>,
    /// Milliseconds to wait for iroh peers before joining visible document entries
    #[arg(long = "iroh-wait-ms", default_value_t = 5000)]
    pub iroh_wait_ms: u64,
    /// WebRTC handshake role for the str0m / webrtc-rs transports. WebRTC is
    /// asymmetric at setup: exactly one peer must run with `offer` and the
    /// other with `answer` (who is who is decided out of band, like agreeing
    /// on the signaling files). Required by both WebRTC transports.
    #[arg(long = "webrtc-role", value_enum)]
    pub webrtc_role: Option<WebrtcRoleArg>,
    /// File this peer APPENDS its outbound signaling blobs to (hex, one per
    /// line): its SDP offer/answer and any trickle-ICE candidates. Deliver the
    /// file (or each new line) to the peer — it is their `--signal-in` — over
    /// any out-of-band channel; blobs are opaque. Manual signaling is the
    /// stopgap until the oblivious BIP-77 directory transport
    /// (transport-payjoin-dir) carries this exchange.
    #[arg(long = "signal-out")]
    pub signal_out: Option<PathBuf>,
    /// File this peer polls for the peer's signaling blobs (the peer's
    /// `--signal-out`, same hex-line format). It may not exist yet at start;
    /// it is polled until `--signal-timeout-ms` elapses.
    #[arg(long = "signal-in")]
    pub signal_in: Option<PathBuf>,
    /// Local UDP bind address for the str0m transport's ICE host candidate
    /// (opaque to ptj; str0m parses it). The default lets the OS pick a port.
    #[arg(long = "webrtc-bind", default_value = "0.0.0.0:0")]
    pub webrtc_bind: String,
    /// STUN/TURN server URI for WebRTC ICE (repeatable). Opaque strings passed
    /// through to the selected WebRTC backend. Empty = host candidates only
    /// (LAN / already-reachable peers).
    #[arg(long = "ice-server")]
    pub ice_servers: Vec<String>,
    /// Milliseconds to wait for the peer's signaling blobs and for the WebRTC
    /// data channel to open before giving up.
    #[arg(long = "signal-timeout-ms", default_value_t = 60_000)]
    pub signal_timeout_ms: u64,
    /// The transport plugin binary to spawn for `--transport plugin`: a path,
    /// or a bare name resolved on PATH (the OS spawn does the resolving; ptj
    /// passes it through). The binary must speak the transport-plugin-api
    /// Cap'n Proto protocol over its stdio.
    #[arg(long = "plugin")]
    pub plugin: Option<PathBuf>,
    /// `key=value` config entry passed through to the plugin's handshake
    /// (repeatable). Opaque to ptj — keys and values mean whatever the
    /// selected plugin says they mean (peer addresses, credential paths, ...).
    #[arg(long = "plugin-config")]
    pub plugin_config: Vec<String>,
    /// Keep polling local sources and updating the state PSBT
    #[arg(long, alias = "continual")]
    pub ongoing: bool,
    /// Milliseconds to wait between ongoing sync polls
    #[arg(long = "poll-interval-ms", default_value_t = 1000)]
    pub poll_interval_ms: u64,
    /// Stop ongoing sync after this many polls
    #[arg(long = "max-iterations", hide = true)]
    pub max_iterations: Option<usize>,
    /// PSBT files or directories of .psbt files to join
    pub sources: Vec<PathBuf>,
}

impl SyncConfig {
    /// Whether this sync runs over a real network transport (anything other than
    /// the default file/dir `local` transport). Network syncs go through
    /// `commands::sync::build_transport` rather than the plain local file path.
    pub(crate) fn uses_network(&self) -> bool {
        self.transport != TransportKind::Local
    }
}

/// Which transport backend `ptj sync` moves bytes over. Every non-`local`
/// variant is behind an optional cargo feature that pulls the standalone
/// `transport-<name>` crate; selecting one without its feature yields a clear
/// "rebuild with `--features <name>`" error at runtime.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// File/dir transport: positional PSBT sources plus `--state` (the default).
    Local,
    /// iroh-docs collaborative document (feature `iroh-sync`).
    Iroh,
    /// Tor onion-service transport (feature `arti`).
    Arti,
    /// Nym mixnet transport (feature `nym`).
    Nym,
    /// I2P transport via emissary (feature `emissary`).
    Emissary,
    /// Nostr-MLS (MDK) transport (feature `mdk`).
    Mdk,
    /// WebRTC data channel via sans-IO str0m (feature `str0m`). Value name
    /// pinned explicitly — heck's kebab-case would otherwise split at the
    /// digit ("str0-m").
    #[value(name = "str0m")]
    Str0m,
    /// WebRTC data channel via the async webrtc-rs stack (feature `webrtc-rs`).
    WebrtcRs,
    /// BIP-77 payjoin-directory mailbox over OHTTP (feature `payjoin-dir`).
    PayjoinDir,
    /// An out-of-process transport plugin (feature `plugin-transports`): a
    /// separate binary named by `--plugin`, spawned by ptj and driven over
    /// its stdio via Cap'n Proto RPC. For transport stacks whose dependency
    /// trees cannot share this workspace's Cargo.lock.
    Plugin,
    // TODO(transport-nostr): unauthored — add a `Nostr` variant (feature
    // `nostr`) when the transport-nostr crate exists.
}

/// Which end of the WebRTC offer/answer handshake this peer plays (the str0m
/// and webrtc-rs transports). Plain setup data handed through to the selected
/// transport crate's own `Role`; ptj never decides a role on its own.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebrtcRoleArg {
    /// This peer creates the data channel and the SDP offer.
    Offer,
    /// This peer receives the offer and produces the SDP answer.
    Answer,
}

/// WIP: no options yet. Candidates once the TUI is real: a state/--sources
/// pair like `sync` (which document to converge on), a read-only flag, and a
/// network selector for address validation in the pay screen.
#[cfg(feature = "tui")]
#[derive(Args, Debug, Clone)]
pub struct TuiConfig {}

#[cfg(feature = "webgui")]
#[derive(Args, Debug, Clone)]
pub struct WebguiConfig {
    /// Address to bind
    #[arg(long, default_value = "127.0.0.1")]
    pub host: IpAddr,
    /// Port to bind
    #[arg(long, default_value_t = 8035)]
    pub port: u16,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileFormat {
    /// Write base64 text, matching stdout.
    Base64,
    /// Write raw PSBT bytes. Only valid for commands that emit one PSBT.
    Binary,
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

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingArg {
    /// Sorter mode is unset; explicit keys are used when present, otherwise derived from seed.
    Unset,
    /// Sort keys are derived from the global seed.
    Deterministic,
    /// Sort keys must be provided explicitly on every input and output.
    Explicit,
}

impl FromStr for OrderingArg {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "unset" => Ok(Self::Unset),
            "deterministic" | "det" => Ok(Self::Deterministic),
            "explicit" => Ok(Self::Explicit),
            other => Err(format!(
                "unknown ordering '{other}' (expected: unset, deterministic, explicit)"
            )),
        }
    }
}

#[derive(Args, Debug, Clone)]
pub struct FeeConfig {
    /// Amount in satoshis explicitly contributed as fees (the field codec's
    /// bare u64; spec § Termination)
    #[arg(long = "amount-sats")]
    pub amount_sats: u64,
    /// Encrypt the fee record with the group secret
    #[arg(long)]
    pub encrypt: bool,
    /// Out-of-band shared secret as hex (required with --encrypt)
    #[arg(long)]
    pub secret: Option<HexSeed>,
    /// PSBT file to attach the fee contribution to
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct PayConfig {
    /// Recipient in addr:amount_btc format
    #[arg(long = "to")]
    pub to: OutputArg,
    /// Optional payment label
    #[arg(long)]
    pub label: Option<String>,
    /// Payer peer id as 32-byte hex (defaults to unspecified/zero)
    #[arg(long)]
    pub payer: Option<Hex32>,
    /// Network the recipient address must be valid for
    #[arg(long = "network", default_value_t = NetworkArg(bitcoin::Network::Bitcoin))]
    pub network: NetworkArg,
    /// Encrypt the payment record with the group secret
    #[arg(long)]
    pub encrypt: bool,
    /// Out-of-band shared secret as hex (required with --encrypt)
    #[arg(long)]
    pub secret: Option<HexSeed>,
    /// Add N indistinguishable dummy payments (requires --encrypt)
    #[arg(long, default_value_t = 0)]
    pub dummy: u32,
    /// PSBT file to attach the payment to
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct ConfirmConfig {
    /// Confirming peer id as 32-byte hex (defaults to unspecified/zero)
    #[arg(long = "peer-id")]
    pub peer_id: Option<Hex32>,
    /// Encrypt the confirmation record with the group secret
    #[arg(long)]
    pub encrypt: bool,
    /// Out-of-band shared secret as hex (required with --encrypt)
    #[arg(long)]
    pub secret: Option<HexSeed>,
    /// PSBT file to confirm
    pub file: PathBuf,
}

#[derive(Args, Debug, Clone)]
pub struct PaymentsConfig {
    /// Out-of-band shared secret as hex (decrypts encrypted entries)
    #[arg(long)]
    pub secret: Option<HexSeed>,
    /// Emit the report as JSON
    #[arg(long)]
    pub json: bool,
    /// PSBT file to read negotiation metadata from
    pub file: PathBuf,
}

/// A fixed 32-byte value parsed liberally (hex, base58, or bech32 detected
/// from the character set; see [`crate::bytes_arg`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hex32([u8; 32]);

impl Hex32 {
    pub fn into_array(self) -> [u8; 32] {
        self.0
    }
}

impl FromStr for Hex32 {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let bytes = crate::bytes_arg::parse_bytes_arg(value)?;
        let len = bytes.len();
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| format!("expected 32 bytes (64 hex chars), got {len}"))
    }
}

/// A byte-string argument parsed liberally (hex, base58, or bech32 detected
/// from the character set; see [`crate::bytes_arg`]). The name is historical:
/// hex remains the canonical form, and any string of hex digits parses as hex.
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
        crate::bytes_arg::parse_bytes_arg(value).map(Self)
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

