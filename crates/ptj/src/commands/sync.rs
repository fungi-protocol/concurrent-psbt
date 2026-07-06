use std::future::Future;
use std::time::Duration;

use psbt_v2::v2::Psbt;

use crate::cli::{SyncConfig, TransportKind};
use crate::transport::message::Message;
use crate::transport::{LocalTransport, Transport};
use crate::{Error, Result, io};

/// THE async→sync boundary for the whole sync driver.
///
/// The transport seam is async; `main` and every non-sync command stay sync. So
/// the ONE place a runtime is created is here, at the `Sync` command edge: this
/// helper builds a current-thread tokio runtime and `block_on`s the async sync
/// future. There is NO other `block_on` in ptj (and none in any transport
/// crate) — the ongoing loop and the `-o`/`--state` runner route their steps
/// through this same helper (see `commands::run_sync_over_local` and
/// `lib.rs::run_ongoing_sync`). Current-thread is right: the sync loop is one
/// logical task; the iroh backend owns its OWN runtime on its actor thread.
pub(crate) fn drive_async<F, T>(future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::new(format!("building sync runtime: {error}")))?;
    runtime.block_on(future)
}

pub(super) fn run(config: SyncConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    if config.uses_network() {
        // The single top-edge runtime for a one-shot network sync.
        return drive_async(run_over_network(config, stdin));
    }
    if config.ongoing {
        return Err(Error::new(
            "ongoing sync requires --state or --output-file so the runner can update the state file",
        ));
    }
    // The single top-edge runtime for a one-shot local sync.
    drive_async(run_once(&config, stdin))
}

/// One-shot sync over a real network transport: converge our local sources,
/// publish that state, wait for peers, then fold the collected frontier. Async:
/// driven by `drive_async` at the `run` edge.
async fn run_over_network(config: SyncConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    if config.ongoing {
        return Err(Error::new("network sync does not support --ongoing yet"));
    }

    let local = run_once(&config, stdin).await?;
    let mut transport = build_transport(&config)?;
    transport
        .publish(Message::Psbt(io::encode_psbt(&local).into_bytes()).encode())
        .await?;
    tokio::time::sleep(Duration::from_millis(config.iroh_wait_ms)).await;
    sync_once_over(transport.as_mut()).await
}

/// Build the sync transport selected by `--transport`.
///
/// `local` is the default file/dir transport. Every network variant is behind
/// its own optional cargo feature pulling the standalone `transport-<name>`
/// crate; selecting one without its feature returns a clear rebuild error. The
/// feature-off arms are what the default and `iroh-sync` builds compile; the
/// feature-on arms for the deferred transports (arti/nym/emissary/mdk) reference
/// their crate constructors and are only compiled when those (currently
/// unbuildable) features are enabled.
pub(crate) fn build_transport(config: &SyncConfig) -> Result<Box<dyn Transport>> {
    match config.transport {
        TransportKind::Local => {
            // The default file/dir transport, published nowhere (the runner owns
            // the publish target / lock in the `-o`/`--state` path). Boxed so the
            // network dispatch returns one uniform `dyn Transport`.
            let transport =
                local_transport(config, None, None, crate::cli::OutputFileFormat::Base64);
            Ok(Box::new(transport))
        }

        TransportKind::Iroh => build_iroh_transport(config),
        TransportKind::Arti => build_arti_transport(config),
        TransportKind::Nym => build_nym_transport(config),
        TransportKind::Emissary => build_emissary_transport(config),
        TransportKind::Mdk => build_mdk_transport(config),
        TransportKind::Str0m => build_str0m_transport(config),
        TransportKind::WebrtcRs => build_webrtc_rs_transport(config),
        TransportKind::PayjoinDir => build_payjoin_dir_transport(config),
    }
}

#[cfg(feature = "iroh-sync")]
fn build_iroh_transport(config: &SyncConfig) -> Result<Box<dyn Transport>> {
    use std::str::FromStr as _;

    use transport_iroh::{DocTicket, IrohChannel};

    if config.iroh_ticket.is_some() && config.iroh_ticket_out.is_some() {
        return Err(Error::new(
            "use either --iroh-ticket or --iroh-ticket-out, not both",
        ));
    }

    let channel = match (&config.iroh_ticket, &config.iroh_ticket_out) {
        (Some(path), None) => {
            let ticket = std::fs::read_to_string(path).map_err(|error| {
                Error::new(format!("reading iroh ticket {}: {error}", path.display()))
            })?;
            let ticket = DocTicket::from_str(ticket.trim()).map_err(|error| {
                Error::new(format!("parsing iroh ticket {}: {error}", path.display()))
            })?;
            IrohChannel::join(ticket)?
        }
        (None, Some(path)) => {
            let (channel, ticket) = IrohChannel::create()?;
            io::write_text_atomic(path, &ticket.to_string())?;
            channel
        }
        _ => {
            return Err(Error::new(
                "iroh sync expects exactly one of --iroh-ticket or --iroh-ticket-out",
            ));
        }
    };
    // IrohChannel is attributable; the sync driver only wants opaque bytes, so
    // wrap it in `Attributed` to expose the plain `Transport` seam.
    Ok(Box::new(transport_core::Attributed::new(channel)))
}

#[cfg(not(feature = "iroh-sync"))]
fn build_iroh_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without iroh sync support; rebuild with --features iroh-sync",
    ))
}

#[cfg(feature = "arti")]
fn build_arti_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // Anonymous channel: a channel IS a Transport via the blanket impl.
    let config = transport_arti::ArtiConfig::new(Vec::new(), 0, "ptj");
    let transport = transport_arti::ArtiTransport::new(config)?;
    Ok(Box::new(transport))
}

#[cfg(not(feature = "arti"))]
fn build_arti_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without arti sync support; rebuild with --features arti",
    ))
}

#[cfg(feature = "nym")]
fn build_nym_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // Anonymous channel: a channel IS a Transport via the blanket impl.
    let transport = transport_nym::NymTransport::connect(Vec::new())?;
    Ok(Box::new(transport))
}

#[cfg(not(feature = "nym"))]
fn build_nym_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without nym sync support; rebuild with --features nym",
    ))
}

#[cfg(feature = "emissary")]
fn build_emissary_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // Anonymous channel: a channel IS a Transport via the blanket impl.
    let config = transport_emissary::EmissaryConfig::new("", "");
    let transport = transport_emissary::EmissaryChannel::connect(&config)?;
    Ok(Box::new(transport))
}

#[cfg(not(feature = "emissary"))]
fn build_emissary_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without emissary sync support; rebuild with --features emissary",
    ))
}

#[cfg(feature = "mdk")]
fn build_mdk_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // MdkChannel is attributable; wrap it in `Attributed` for the plain seam.
    let config = transport_mdk::MdkConfig::default();
    let channel = transport_mdk::MdkChannel::connect(config)?;
    Ok(Box::new(transport_core::Attributed::new(channel)))
}

#[cfg(not(feature = "mdk"))]
fn build_mdk_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without mdk sync support; rebuild with --features mdk",
    ))
}

// The WebRTC transports (str0m / webrtc-rs) need an out-of-band SIGNALING
// exchange (SDP offer/answer + trickle ICE over the payjoin-dir oblivious
// mailbox) before a data channel exists, and the payjoin-dir mailbox needs
// directory/relay/session parameters. `SyncConfig` carries none of those yet,
// so the feature-ON arms return a clear not-yet-wired error instead of
// constructing a transport that could never connect. TODO(webgui-transport-
// wiring): add the signaling/session fields to `SyncConfig` (and the webgui
// request DTO) and construct the real transports here.
#[cfg(feature = "str0m")]
fn build_str0m_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // Exercise the crate seam so `--features str0m` proves the integration
    // compiles end to end (the type is a Transport via the blanket impl).
    fn _is_transport<T: Transport>() {}
    _is_transport::<transport_str0m::Str0mTransport>();
    Err(Error::new(
        "str0m transport is not yet selectable here: WebRTC needs the out-of-band \
         SDP/ICE signaling exchange (payjoin-dir mailbox) wired into the sync config",
    ))
}

#[cfg(not(feature = "str0m"))]
fn build_str0m_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without str0m sync support; rebuild with --features str0m",
    ))
}

#[cfg(feature = "webrtc-rs")]
fn build_webrtc_rs_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    fn _is_transport<T: Transport>() {}
    _is_transport::<transport_webrtc_rs::WebrtcRsTransport>();
    Err(Error::new(
        "webrtc-rs transport is not yet selectable here: WebRTC needs the out-of-band \
         SDP/ICE signaling exchange (payjoin-dir mailbox) wired into the sync config",
    ))
}

#[cfg(not(feature = "webrtc-rs"))]
fn build_webrtc_rs_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without webrtc-rs sync support; rebuild with --features webrtc-rs",
    ))
}

#[cfg(feature = "payjoin-dir")]
fn build_payjoin_dir_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    fn _is_transport<T: Transport>() {}
    _is_transport::<transport_payjoin_dir::PayjoinDirChannel>();
    Err(Error::new(
        "payjoin-dir transport is not yet selectable here: the BIP-77 directory \
         mailbox needs directory/relay/session parameters wired into the sync config",
    ))
}

#[cfg(not(feature = "payjoin-dir"))]
fn build_payjoin_dir_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(
        "ptj was built without payjoin-dir sync support; rebuild with --features payjoin-dir",
    ))
}

// TODO(transport-nostr): unauthored — when the transport-nostr crate exists,
// add the `nostr` feature, the `TransportKind::Nostr` variant, and the
// build_nostr_transport arm pair here, mirroring the arms above.

pub(crate) async fn run_once(config: &SyncConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    // One-shot `ptj sync` with no runner-supplied publish target: a
    // LocalTransport that reads the configured sources and `--state` and returns
    // the converged result. Writing is the runner's concern (lib.rs decides the
    // publish target / format and wraps the step in a file lock), so the
    // transport built here publishes nowhere (no-op) and we just return the join.
    let mut transport = local_transport(config, stdin, None, crate::cli::OutputFileFormat::Base64);
    sync_once_over(&mut transport).await
}

/// One convergence step over any transport. Transport-agnostic. Async: it
/// `.await`s the transport seam; run it via `drive_async` at a sync edge.
///
/// gather -> converge (the EXISTING engine, untouched) -> publish.
pub(crate) async fn sync_once_over(transport: &mut dyn Transport) -> Result<Psbt> {
    sync_step(transport).await.map(|(joined, _messages)| joined)
}

/// One convergence step that also surfaces out-of-band negotiation messages
/// (payments, confirmations) collected alongside PSBTs. The CLI sneakernet
/// path carries negotiation inside the PSBT, so `sync_once_over` discards
/// them; GUI flows convey them out of band and want them back.
pub(crate) async fn sync_step(transport: &mut dyn Transport) -> Result<(Psbt, Vec<Message>)> {
    // gather: drain the transport, decode envelopes (legacy raw PSBTs fall
    // back cleanly), and split PSBTs from negotiation messages.
    let mut psbts = Vec::new();
    let mut messages = Vec::new();
    for bytes in transport.collect().await? {
        match Message::decode(&bytes)? {
            Message::Psbt(payload) => {
                psbts.push(io::parse_psbt_bytes("transport", &payload)?);
            }
            other => messages.push(other),
        }
    }

    // converge: the EXISTING engine — reduce(Join::join), conflict reporting,
    // try_unwrap. Order/dedup are irrelevant (idempotent/commutative join).
    let joined = super::join::join_psbts(psbts)?;

    // publish: broadcast our converged local state back to participants.
    transport
        .publish(Message::Psbt(io::encode_psbt(&joined).into_bytes()).encode())
        .await?;
    Ok((joined, messages))
}

/// Build the default file/dir transport for a sync invocation.
///
/// `publish_target` is where the converged result is written (the `--state`
/// path, or the global `-o`/`--output-file` path when the runner supplies one).
/// `output_format` selects base64 vs binary on publish.
pub(crate) fn local_transport(
    config: &SyncConfig,
    stdin: Option<&[u8]>,
    publish_target: Option<std::path::PathBuf>,
    output_format: crate::cli::OutputFileFormat,
) -> LocalTransport {
    LocalTransport::new(
        config.sources.clone(),
        config.state.clone(),
        stdin,
        publish_target,
        output_format,
    )
}

pub(crate) fn validate_ongoing(config: &SyncConfig, stdin: Option<&[u8]>) -> Result<()> {
    if config
        .sources
        .iter()
        .any(|source| io::is_stdin_path(source))
    {
        return Err(Error::new(
            "ongoing sync cannot use '-' because stdin is a one-shot source",
        ));
    }
    if stdin.is_some_and(|bytes| !bytes.is_empty()) {
        return Err(Error::new("ongoing sync cannot consume runner stdin"));
    }
    if config.max_iterations == Some(0) {
        return Err(Error::new("--max-iterations must be greater than zero"));
    }
    Ok(())
}

pub(crate) fn poll_interval(config: &SyncConfig) -> Duration {
    Duration::from_millis(config.poll_interval_ms)
}
