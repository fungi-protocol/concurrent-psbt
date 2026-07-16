use std::future::Future;
use std::time::Duration;

use psbt_v2::v2::Psbt;

use crate::cli::{SyncConfig, TransportKind};
use crate::transport::message::Message;
use crate::transport::{LocalTransport, Transport, WatchedDirTransport};
use crate::{Error, Result, io};

/// THE async→sync boundary for the whole sync driver.
///
/// The transport seam is async; `main` and every non-sync command stay sync. So
/// the ONE place a runtime is created is here, at the `Sync` command edge: this
/// helper builds a current-thread tokio runtime and `block_on`s the async sync
/// future. There is NO other `block_on` in ptj — the ongoing loop and the
/// `-o`/`--state` runner route their steps through this same helper (see
/// `commands::run_sync_over_local` and `lib.rs::run_ongoing_sync`). Current-
/// thread is right: the sync loop is one logical task; the iroh backend owns
/// its OWN runtime on its actor thread. The webrtc-rs backend `block_on`s an
/// owned runtime at ITS edge, which is why `build_transport` always runs on
/// the sync side, never inside a `drive_async` future.
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
    if config.transport == TransportKind::WatchedDir {
        // One-shot watched-dir sync: one collect (register + seeds + stdin),
        // one join, one write-once publish back into the register. The
        // `--ongoing` loop is the runner's job (`lib.rs::run_ongoing_watched_dir`),
        // like the local `--state` loop; reaching here with it set is a bug.
        if config.ongoing {
            return Err(Error::new(
                "ongoing watched-dir sync is driven by the runner",
            ));
        }
        let mut transport = watched_dir_transport(&config, stdin)?;
        return drive_async(async move { sync_once_over(&mut transport).await });
    }
    if config.uses_network() {
        // One-shot sync over a real network transport: converge our local
        // sources, publish that state, wait for peers, then fold the collected
        // frontier.
        if config.ongoing {
            return Err(Error::new("network sync does not support --ongoing yet"));
        }
        let local = drive_async(run_once(&config, stdin))?;
        // The transport is constructed OUTSIDE the runtime, on the sync side of
        // the boundary (matching the webgui's `/api/sync` path): constructors
        // may block — str0m runs its manual file-signaling handshake here, and
        // the webrtc-rs backend `block_on`s its own owned runtime, which tokio
        // forbids from inside another runtime's context.
        let mut transport = build_transport(&config)?;
        let wait_ms = config.iroh_wait_ms;
        return drive_async(async move {
            transport
                .publish(Message::Psbt(io::encode_psbt(&local).into_bytes()).encode())
                .await?;
            tokio::time::sleep(Duration::from_millis(wait_ms)).await;
            sync_once_over(transport.as_mut()).await
        });
    }
    if config.ongoing {
        return Err(Error::new(
            "ongoing sync requires --state or --output-file so the runner can update the state file",
        ));
    }
    // The single top-edge runtime for a one-shot local sync.
    drive_async(run_once(&config, stdin))
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

        TransportKind::WatchedDir => {
            // Built in, like `local` — no feature gate. The webgui reaches the
            // register through this arm (no runner stdin on that path).
            let transport = watched_dir_transport(config, None)?;
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
        TransportKind::Plugin => build_plugin_transport(config),
    }
}

#[cfg(feature = "plugin-transports")]
fn build_plugin_transport(config: &SyncConfig) -> Result<Box<dyn Transport>> {
    let binary = config.plugin.as_ref().ok_or_else(|| {
        Error::new(
            "plugin sync requires --plugin <binary>: the transport plugin executable to spawn \
             (a path, or a bare name resolved on PATH)",
        )
    })?;
    let entries = crate::transport::plugin::parse_config_entries(&config.plugin_config)?;
    // Spawn + handshake block HERE, on the sync side of the runtime boundary
    // (the str0m handshake precedent): the driver's publish/collect find a
    // ready transport. The host is Send and owns its own actor thread, so
    // boxing it into the driver's runtime is safe.
    let transport = crate::transport::plugin::PluginTransport::spawn(binary, entries)?;
    Ok(Box::new(transport))
}

#[cfg(not(feature = "plugin-transports"))]
fn build_plugin_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(crate::capabilities::rebuild_hint("plugin")))
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
    Err(Error::new(crate::capabilities::rebuild_hint("iroh")))
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
    Err(Error::new(crate::capabilities::rebuild_hint("arti")))
}

#[cfg(feature = "nym")]
fn build_nym_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // Anonymous channel: a channel IS a Transport via the blanket impl.
    let transport = transport_nym::NymTransport::connect(Vec::new())?;
    Ok(Box::new(transport))
}

#[cfg(not(feature = "nym"))]
fn build_nym_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(crate::capabilities::rebuild_hint("nym")))
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
    Err(Error::new(crate::capabilities::rebuild_hint("emissary")))
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
    Err(Error::new(crate::capabilities::rebuild_hint("mdk")))
}

// The WebRTC transports (str0m / webrtc-rs) need an out-of-band SIGNALING
// exchange (SDP offer/answer + trickle ICE) before a data channel exists. The
// intended oblivious carrier is the BIP-77 payjoin-directory mailbox over
// OHTTP (transport-payjoin-dir — owned externally; see the payjoin-dir arm
// below). Until that lands, the feature-ON arms are wired to the MANUAL
// signaling mode: `--webrtc-role` names this peer's end of the asymmetric
// handshake, and `--signal-out`/`--signal-in` name the files the opaque blobs
// travel through (`crate::transport::signaling`), moved between the peers by
// any out-of-band means. Missing params yield errors naming the exact flag.

/// Validate the signaling/session parameters both WebRTC transports require
/// out of `SyncConfig`: the handshake role, and the manual signaling channel
/// built from the `--signal-out`/`--signal-in` pair (every blocking signaling
/// wait is bounded by `--signal-timeout-ms`, also returned for the callers'
/// own open-waits). `name` is the selected transport, for the error text.
#[cfg(any(feature = "str0m", feature = "webrtc-rs"))]
fn webrtc_params(
    config: &SyncConfig,
    name: &str,
) -> Result<(
    crate::cli::WebrtcRoleArg,
    crate::transport::signaling::FileSignaling,
    Duration,
)> {
    let role = config.webrtc_role.ok_or_else(|| {
        Error::new(format!(
            "{name} sync requires --webrtc-role (offer|answer): WebRTC setup is \
             asymmetric — exactly one peer offers and the other answers"
        ))
    })?;
    let signal_out = config.signal_out.clone().ok_or_else(|| {
        Error::new(format!(
            "{name} sync requires --signal-out <file>: this peer's SDP/ICE blobs are \
             appended there for manual delivery to the peer (oblivious BIP-77/OHTTP \
             signaling arrives with transport-payjoin-dir)"
        ))
    })?;
    let signal_in = config.signal_in.clone().ok_or_else(|| {
        Error::new(format!(
            "{name} sync requires --signal-in <file>: the peer's --signal-out is polled \
             there for its SDP/ICE blobs (oblivious BIP-77/OHTTP signaling arrives with \
             transport-payjoin-dir)"
        ))
    })?;
    let timeout = Duration::from_millis(config.signal_timeout_ms);
    let signaling = crate::transport::signaling::FileSignaling::new(signal_out, signal_in, timeout);
    Ok((role, signaling, timeout))
}

#[cfg(feature = "str0m")]
fn build_str0m_transport(config: &SyncConfig) -> Result<Box<dyn Transport>> {
    use transport_str0m::{Role, Str0mConfig, Str0mTransport};

    let (role, mut signaling, timeout) = webrtc_params(config, "str0m")?;
    let role = match role {
        crate::cli::WebrtcRoleArg::Offer => Role::Offerer,
        crate::cli::WebrtcRoleArg::Answer => Role::Answerer,
    };
    // `--webrtc-bind` / `--ice-server` pass through opaquely; str0m parses them.
    let mut str0m_config = Str0mConfig::new(role, config.webrtc_bind.clone());
    str0m_config.ice_servers = config.ice_servers.clone();
    let mut transport = Str0mTransport::new(str0m_config)?;

    // str0m exposes the handshake as manual methods (sans-IO), so ptj drives
    // the signaling exchange itself, blocking HERE on the sync side of the
    // boundary until the data channel is open — the driver's publish/collect
    // then find a ready transport.
    let early = drive_str0m_handshake(role, &mut transport, &mut signaling, timeout)?;
    Ok(Box::new(Primed {
        early,
        inner: transport,
    }))
}

/// Drive str0m's manual signaling handshake to an OPEN data channel: exchange
/// the SDP blobs over the signal files (the offer/answer already embeds the
/// host ICE candidate str0m seeds at bind time), forward/apply trickled
/// candidates, and pump the sans-IO loop until `ChannelOpen`. Returns any data
/// records a fast peer published during the final pumps, for `Primed` to
/// prepend to the first `collect` snapshot (dropping them would lose that
/// peer's one-shot publish).
#[cfg(feature = "str0m")]
fn drive_str0m_handshake(
    role: transport_str0m::Role,
    transport: &mut transport_str0m::Str0mTransport,
    signaling: &mut crate::transport::signaling::FileSignaling,
    timeout: Duration,
) -> Result<Vec<Vec<u8>>> {
    use transport_core::AnonymousChannel as _;
    use transport_str0m::Role;

    match role {
        Role::Offerer => {
            let offer = transport.local_handshake()?;
            signaling.push_blob(&offer)?;
            let answer = signaling.wait_next_blob("SDP answer")?;
            // The offerer's accept returns nothing to send back.
            transport.accept_handshake(&answer)?;
        }
        Role::Answerer => {
            let offer = signaling.wait_next_blob("SDP offer")?;
            let answer = transport.accept_handshake(&offer)?.ok_or_else(|| {
                Error::new("str0m answerer produced no SDP answer for the remote offer")
            })?;
            signaling.push_blob(&answer)?;
        }
    }

    let deadline = std::time::Instant::now() + timeout;
    let mut early = Vec::new();
    while !transport.is_open() {
        if std::time::Instant::now() >= deadline {
            return Err(Error::new(format!(
                "str0m data channel did not open within --signal-timeout-ms ({}ms): \
                 SDP was exchanged but ICE/DTLS did not complete (are both peers \
                 reachable at their candidate addresses?)",
                timeout.as_millis()
            )));
        }
        // Trickle: hand newly-discovered local candidates to the peer, apply
        // the peer's. (Host-only setups complete via the candidates already
        // embedded in the SDP; this keeps trickled ones flowing regardless.)
        for candidate in transport.local_candidates()? {
            signaling.push_blob(&candidate)?;
        }
        for candidate in signaling.poll_blobs()? {
            transport.add_remote_candidate(&candidate)?;
        }
        // `recv` pumps the sans-IO loop (a ~20ms socket read window paces this
        // loop) and is where ICE/DTLS/SCTP actually make progress.
        early.extend(poll_inline(transport.recv())??);
    }
    Ok(early)
}

/// Complete a never-suspending future on the sync side of the boundary.
///
/// str0m's channel methods are async in signature only — each call pumps the
/// sans-IO loop inline and is immediately `Ready` — so one poll with a no-op
/// waker completes it without a runtime. (`build_transport` runs outside
/// `drive_async`, so no runtime is available here by design.)
#[cfg(feature = "str0m")]
fn poll_inline<F: Future>(future: F) -> Result<F::Output> {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(output) => Ok(output),
        std::task::Poll::Pending => Err(Error::new(
            "str0m channel method suspended unexpectedly (sans-IO methods must complete inline)",
        )),
    }
}

/// A transport with records that arrived during the handshake pump prepended
/// to its first `collect` snapshot. The str0m open-wait must pump `recv`, and
/// a fast peer may publish inside the same pump that opened the channel; those
/// records belong to the driver, not the floor.
#[cfg(feature = "str0m")]
struct Primed<T: Transport> {
    early: Vec<Vec<u8>>,
    inner: T,
}

#[cfg(feature = "str0m")]
#[async_trait::async_trait]
impl<T: Transport> Transport for Primed<T> {
    async fn publish(&mut self, message: Vec<u8>) -> transport_core::Result<()> {
        self.inner.publish(message).await
    }

    async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
        let mut records = std::mem::take(&mut self.early);
        records.extend(self.inner.collect().await?);
        Ok(records)
    }
}

#[cfg(not(feature = "str0m"))]
fn build_str0m_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(crate::capabilities::rebuild_hint("str0m")))
}

#[cfg(feature = "webrtc-rs")]
fn build_webrtc_rs_transport(config: &SyncConfig) -> Result<Box<dyn Transport>> {
    // `connect` runs the whole offer/answer + trickle-ICE handshake over the
    // file signaling port on the crate's OWN runtime and blocks until the data
    // channel opens (bounded by the crate's internal timeouts). We are on the
    // sync side of the boundary, so that owned-runtime `block_on` is legal.
    //
    // KNOWN LIMIT (crate-owned, pre-existing): the webrtc-rs backend also
    // `block_on`s inside its channel methods, so DRIVING this transport from
    // the sync loop's runtime panics until transport-webrtc-rs adopts the
    // actor-at-the-edge pattern (see the note in its imp.rs; transport-iroh's
    // backend is the canonical shape). str0m is the primary, exercised WebRTC
    // backend; this arm proves the config/signaling wiring end to end.
    let transport = transport_webrtc_rs::WebrtcRsTransport::connect(webrtc_rs_config(config)?)?;
    Ok(Box::new(transport))
}

/// Map the validated CLI/webgui params onto `WebrtcRsConfig` — factored out of
/// the arm so the mapping is unit-testable without a live `connect`. The
/// timeout from `webrtc_params` still bounds the `FileSignaling` waits, but
/// webrtc-rs owns its handshake pacing, so it is not used separately here.
#[cfg(feature = "webrtc-rs")]
fn webrtc_rs_config(
    config: &SyncConfig,
) -> Result<transport_webrtc_rs::WebrtcRsConfig<crate::transport::signaling::FileSignaling>> {
    let (role, signaling, _timeout) = webrtc_params(config, "webrtc-rs")?;
    let role = match role {
        crate::cli::WebrtcRoleArg::Offer => transport_webrtc_rs::Role::Offerer,
        crate::cli::WebrtcRoleArg::Answer => transport_webrtc_rs::Role::Answerer,
    };
    Ok(transport_webrtc_rs::WebrtcRsConfig::new(
        role,
        config.ice_servers.clone(),
        signaling,
    ))
}

#[cfg(not(feature = "webrtc-rs"))]
fn build_webrtc_rs_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(crate::capabilities::rebuild_hint("webrtc-rs")))
}

// TODO(transport-payjoin-dir): OWNED EXTERNALLY (Damola). Do NOT wire this arm
// here: the BIP-77 directory mailbox's directory/relay/session parameters and
// construction land with that crate's implementation. When it arrives it also
// replaces the manual `--signal-out`/`--signal-in` files above as the
// OBLIVIOUS signaling path for the WebRTC transports (SDP/ICE blobs over the
// directory mailbox through an OHTTP relay, instead of hand-moved files).
#[cfg(feature = "payjoin-dir")]
fn build_payjoin_dir_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    fn _is_transport<T: Transport>() {}
    _is_transport::<transport_payjoin_dir::PayjoinDirChannel>();
    Err(Error::new(
        "payjoin-dir transport is not yet selectable here: the BIP-77 directory \
         mailbox needs directory/relay/session parameters wired into the sync config \
         (implementation owned externally; arriving with transport-payjoin-dir)",
    ))
}

#[cfg(not(feature = "payjoin-dir"))]
fn build_payjoin_dir_transport(_config: &SyncConfig) -> Result<Box<dyn Transport>> {
    Err(Error::new(crate::capabilities::rebuild_hint("payjoin-dir")))
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
    let (joined, messages) = sync_step_allow_empty(transport).await?;
    // The CLI contract: a step must converge SOMETHING. The message matches
    // the empty-fold error `join::join_psbts` always reported here.
    let joined = joined.ok_or_else(|| Error::new("join expects at least one PSBT file"))?;
    Ok((joined, messages))
}

/// [`sync_step`] minus the non-empty requirement: `None` means the transport
/// delivered no PSBTs at all. The caller decides whether that is legitimate —
/// the webgui's create-an-empty-shared-document path (`iroh_ticket_out` with
/// zero fragments) is; the CLI, which always folds local sources first, is
/// not. Nothing is published back on an empty gather: an empty document must
/// stay empty rather than receive a fabricated fragment.
pub(crate) async fn sync_step_allow_empty(
    transport: &mut dyn Transport,
) -> Result<(Option<Psbt>, Vec<Message>)> {
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
    if psbts.is_empty() {
        return Ok((None, messages));
    }

    // converge: the EXISTING engine — reduce(Join::join), conflict reporting,
    // try_unwrap. Order/dedup are irrelevant (idempotent/commutative join).
    let joined = super::join::join_psbts(psbts)?;

    // publish: broadcast our converged local state back to participants.
    transport
        .publish(Message::Psbt(io::encode_psbt(&joined).into_bytes()).encode())
        .await?;
    Ok((Some(joined), messages))
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

/// Build the watched-dir register transport: the first positional source is
/// the register directory (created if absent), the rest are seed PSBTs.
pub(crate) fn watched_dir_transport(
    config: &SyncConfig,
    stdin: Option<&[u8]>,
) -> Result<WatchedDirTransport> {
    if config.state.is_some() {
        return Err(Error::new(
            "watched-dir sync keeps its state in the directory; drop --state (the directory is the register)",
        ));
    }
    let mut sources = config.sources.iter();
    let dir = sources.next().ok_or_else(|| {
        Error::new(
            "watched-dir sync requires a directory source: \
             ptj sync --transport watched-dir <dir> [seed PSBTs...]",
        )
    })?;
    if io::is_stdin_path(dir) {
        return Err(Error::new(
            "watched-dir sync requires a directory as its first source, not '-'",
        ));
    }
    let seeds: Vec<std::path::PathBuf> = sources.cloned().collect();
    WatchedDirTransport::new(dir.clone(), seeds, stdin)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A `SyncConfig` for dispatch tests: the selected transport with every
    /// optional parameter absent and CLI-default scalars — except the tight
    /// signal timeout (tests must not wait 60s) and a loopback bind (no
    /// firewall prompt, no external interface).
    fn config_for(transport: TransportKind) -> SyncConfig {
        SyncConfig {
            transport,
            state: None,
            iroh_ticket: None,
            iroh_ticket_out: None,
            iroh_wait_ms: 5000,
            webrtc_role: None,
            signal_out: None,
            signal_in: None,
            webrtc_bind: "127.0.0.1:0".to_string(),
            ice_servers: Vec::new(),
            signal_timeout_ms: 50,
            plugin: None,
            plugin_config: Vec::new(),
            ongoing: false,
            poll_interval_ms: 1000,
            max_iterations: None,
            sources: Vec::new(),
        }
    }

    fn dispatch_error(config: &SyncConfig) -> String {
        build_transport(config)
            .err()
            .expect("dispatch must fail")
            .to_string()
    }

    // ---- watched-dir dispatch: built in, params validated ------------------

    #[test]
    fn watched_dir_dispatch_requires_a_directory_source() {
        let error = dispatch_error(&config_for(TransportKind::WatchedDir));
        assert!(
            error.contains("requires a directory source"),
            "got: {error}"
        );
    }

    #[test]
    fn watched_dir_dispatch_rejects_state_and_stdin_registers() {
        let mut config = config_for(TransportKind::WatchedDir);
        config.sources = vec![std::path::PathBuf::from("-")];
        let error = dispatch_error(&config);
        assert!(error.contains("not '-'"), "got: {error}");

        let dir = tempfile::tempdir().unwrap();
        config.sources = vec![dir.path().to_path_buf()];
        config.state = Some(dir.path().join("state.psbt"));
        let error = dispatch_error(&config);
        assert!(
            error.contains("the directory is the register"),
            "got: {error}"
        );

        config.state = None;
        assert!(build_transport(&config).is_ok());
    }

    // ---- feature-OFF dispatch: the precise rebuild hint -------------------

    #[cfg(not(feature = "str0m"))]
    #[test]
    fn str0m_dispatch_requires_feature() {
        let error = dispatch_error(&config_for(TransportKind::Str0m));
        assert!(error.contains("--features str0m"), "got: {error}");
    }

    #[cfg(not(feature = "webrtc-rs"))]
    #[test]
    fn webrtc_rs_dispatch_requires_feature() {
        let error = dispatch_error(&config_for(TransportKind::WebrtcRs));
        assert!(error.contains("--features webrtc-rs"), "got: {error}");
    }

    #[cfg(not(feature = "plugin-transports"))]
    #[test]
    fn plugin_dispatch_requires_feature() {
        let error = dispatch_error(&config_for(TransportKind::Plugin));
        assert!(
            error.contains("--features plugin-transports"),
            "got: {error}"
        );
    }

    // Feature-ON, param absent: the missing flag is named (the spawn/handshake
    // failure paths live with the fake plugin in tests/plugin_host.rs).
    #[cfg(feature = "plugin-transports")]
    #[test]
    fn plugin_dispatch_names_the_missing_binary_flag() {
        let error = dispatch_error(&config_for(TransportKind::Plugin));
        assert!(error.contains("--plugin"), "got: {error}");
    }

    // ---- feature-ON dispatch, params absent: each missing flag is named ----
    // (Shared `webrtc_params` shape, exercised through BOTH arms so a future
    // per-arm divergence cannot silently drop the validation.)

    #[cfg(feature = "str0m")]
    #[test]
    fn str0m_dispatch_names_each_missing_param() {
        let mut config = config_for(TransportKind::Str0m);
        let error = dispatch_error(&config);
        assert!(error.contains("--webrtc-role"), "got: {error}");
        assert!(error.contains("str0m sync requires"), "got: {error}");

        config.webrtc_role = Some(crate::cli::WebrtcRoleArg::Offer);
        let error = dispatch_error(&config);
        assert!(error.contains("--signal-out"), "got: {error}");

        config.signal_out = Some(std::path::PathBuf::from("us.sig"));
        let error = dispatch_error(&config);
        assert!(error.contains("--signal-in"), "got: {error}");
    }

    #[cfg(feature = "webrtc-rs")]
    #[test]
    fn webrtc_rs_dispatch_names_each_missing_param() {
        let mut config = config_for(TransportKind::WebrtcRs);
        let error = dispatch_error(&config);
        assert!(error.contains("--webrtc-role"), "got: {error}");
        assert!(error.contains("webrtc-rs sync requires"), "got: {error}");

        config.webrtc_role = Some(crate::cli::WebrtcRoleArg::Answer);
        let error = dispatch_error(&config);
        assert!(error.contains("--signal-out"), "got: {error}");

        config.signal_out = Some(std::path::PathBuf::from("us.sig"));
        let error = dispatch_error(&config);
        assert!(error.contains("--signal-in"), "got: {error}");
    }

    // ---- feature-ON dispatch, params present -------------------------------

    /// The str0m arm constructs a REAL transport (binds loopback UDP, creates
    /// the Rtc), emits the SDP offer through the manual signaling file, and —
    /// with no peer in a unit test — times out waiting for the answer. Live
    /// point-to-point syncs are the e2e path's job, not a unit test's.
    #[cfg(feature = "str0m")]
    #[test]
    fn str0m_offerer_emits_sdp_offer_then_times_out_awaiting_answer() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_for(TransportKind::Str0m);
        config.webrtc_role = Some(crate::cli::WebrtcRoleArg::Offer);
        config.signal_out = Some(dir.path().join("us.sig"));
        config.signal_in = Some(dir.path().join("peer.sig"));

        let error = dispatch_error(&config);
        assert!(error.contains("SDP answer"), "got: {error}");
        assert!(error.contains("timed out"), "got: {error}");

        // The offer really went out: one hex line decoding to SDP text.
        let out = std::fs::read_to_string(dir.path().join("us.sig")).unwrap();
        let line = out.lines().next().expect("an offer line must be present");
        let sdp: Vec<u8> = (0..line.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap())
            .collect();
        let sdp = String::from_utf8(sdp).unwrap();
        assert!(sdp.starts_with("v=0"), "expected an SDP offer, got: {sdp}");
    }

    /// The answerer side blocks first on the peer's offer; with no peer it
    /// times out naming what it was waiting for.
    #[cfg(feature = "str0m")]
    #[test]
    fn str0m_answerer_times_out_awaiting_offer() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config_for(TransportKind::Str0m);
        config.webrtc_role = Some(crate::cli::WebrtcRoleArg::Answer);
        config.signal_out = Some(dir.path().join("us.sig"));
        config.signal_in = Some(dir.path().join("peer.sig"));

        let error = dispatch_error(&config);
        assert!(error.contains("SDP offer"), "got: {error}");
        assert!(error.contains("timed out"), "got: {error}");
    }

    /// The webrtc-rs param mapping lands role/ICE/signaling on the crate's
    /// config (tested without a live `connect`, which blocks on a peer for up
    /// to the crate's 30s internal timeout).
    #[cfg(feature = "webrtc-rs")]
    #[test]
    fn webrtc_rs_params_map_onto_the_crate_config() {
        use transport_webrtc_rs::{SignalBlob, Signaling as _};

        let dir = tempfile::tempdir().unwrap();
        let mut config = config_for(TransportKind::WebrtcRs);
        config.webrtc_role = Some(crate::cli::WebrtcRoleArg::Answer);
        config.signal_out = Some(dir.path().join("us.sig"));
        config.signal_in = Some(dir.path().join("peer.sig"));
        config.ice_servers = vec!["stun:stun.example.org:3478".to_string()];

        let webrtc_config = webrtc_rs_config(&config).unwrap();
        assert_eq!(webrtc_config.role, transport_webrtc_rs::Role::Answerer);
        assert_eq!(
            webrtc_config.ice_servers,
            vec!["stun:stun.example.org:3478".to_string()]
        );

        // The signaling port is the manual file channel: a blob pushed through
        // the crate-facing trait lands in --signal-out.
        let mut signaling = webrtc_config.signaling;
        signaling
            .push(SignalBlob(b"sdp-json".to_vec()))
            .expect("push writes the signal-out file");
        assert!(dir.path().join("us.sig").exists());
    }

    /// `Primed` prepends handshake-time records to the first collect snapshot
    /// only, then defers to the inner transport.
    #[cfg(feature = "str0m")]
    #[test]
    fn primed_prepends_early_records_to_first_collect() {
        struct Recorder {
            published: Vec<Vec<u8>>,
        }
        #[async_trait::async_trait]
        impl Transport for Recorder {
            async fn publish(&mut self, message: Vec<u8>) -> transport_core::Result<()> {
                self.published.push(message);
                Ok(())
            }
            async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
                Ok(vec![b"live".to_vec()])
            }
        }

        let mut primed = Primed {
            early: vec![b"early".to_vec()],
            inner: Recorder {
                published: Vec::new(),
            },
        };
        let (first, second, published) = drive_async(async {
            primed.publish(b"ours".to_vec()).await?;
            let first = primed.collect().await?;
            let second = primed.collect().await?;
            Ok((first, second, primed.inner.published.clone()))
        })
        .unwrap();
        assert_eq!(first, vec![b"early".to_vec(), b"live".to_vec()]);
        assert_eq!(second, vec![b"live".to_vec()]);
        assert_eq!(published, vec![b"ours".to_vec()]);
    }

    /// An empty transport (no sources, no state, no peers): the tolerant step
    /// reports None and publishes NOTHING back — an empty shared document
    /// stays empty instead of receiving a fabricated fragment.
    #[test]
    fn sync_step_allow_empty_reports_none_and_publishes_nothing() {
        struct Empty {
            published: usize,
        }
        #[async_trait::async_trait]
        impl Transport for Empty {
            async fn publish(&mut self, _message: Vec<u8>) -> transport_core::Result<()> {
                self.published += 1;
                Ok(())
            }
            async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
                Ok(Vec::new())
            }
        }

        let mut transport = Empty { published: 0 };
        let (joined, messages) = drive_async(async {
            sync_step_allow_empty(&mut transport).await
        })
        .unwrap();
        assert!(joined.is_none());
        assert!(messages.is_empty());
        assert_eq!(transport.published, 0);
    }

    /// The strict step keeps its all-or-error contract (the CLI path always
    /// folds local sources first, so an empty gather is a caller mistake).
    #[test]
    fn sync_step_still_errors_on_an_empty_gather() {
        struct Empty;
        #[async_trait::async_trait]
        impl Transport for Empty {
            async fn publish(&mut self, _message: Vec<u8>) -> transport_core::Result<()> {
                Ok(())
            }
            async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
                Ok(Vec::new())
            }
        }

        let mut transport = Empty;
        let error = drive_async(async { sync_step(&mut transport).await })
            .expect_err("empty gather must error")
            .to_string();
        assert!(
            error.contains("join expects at least one PSBT file"),
            "got: {error}"
        );
    }
}
