//! The mixnet backend, compiled only under `#[cfg(feature = "nym")]` (the
//! feature-off skeleton lives in main.rs).
//!
//! PORTED from the transport-grounding lane's in-workspace backend
//! (crates/transport-nym/src/imp.rs on the vqqyuxys change — the grounded
//! implementation that could not land its nym-sdk dependency in the
//! workspace lock), with ONE structural change: the actor thread is GONE.
//! That bridge existed because the in-process transport's caller runtime and
//! nym's client had to be decoupled; this plugin IS a dedicated process with
//! a single current-thread runtime and a single-threaded capnp vat, so the
//! server methods await the SDK directly. Everything the SDK is asked to do
//! — and the framing on the wire — is unchanged.
//!
//! # API grounding (carried over from the ported backend, verified against
//! the pinned registry sources at nym-sdk 1.21.2)
//!
//!   * `mixnet::MixnetClientBuilder::new_ephemeral().build()?` builds a
//!     disconnected client; `.connect_to_mixnet().await?` connects it.
//!   * `client.nym_address() -> &Recipient` is our address for peers.
//!   * `mixnet::Recipient::try_from_base58_string(s)` parses a peer address
//!     (`identity.encryption@gateway`); we never interpret it ourselves.
//!   * `client.send_plain_message(recipient, bytes).await?` sends opaque
//!     bytes (`MixnetMessageSender` trait method, in scope below).
//!   * `client.wait_for_messages().await -> Option<Vec<ReconstructedMessage>>`
//!     yields reconstructed batches; `.message` is the payload. Called under
//!     a short timeout to turn the push stream into a polling snapshot
//!     (cancel-safe stream `next()`).
//!
//! # Wire format
//!
//! Each mixnet message body is one length-prefixed `transport_core::frame`
//! record around the caller's opaque bytes — IDENTICAL to the in-process
//! transport-nym, so a plugin peer and an in-process peer interoperate.

use std::time::Duration;

use nym_sdk::mixnet::{self, MixnetClient, MixnetMessageSender, Recipient};
use transport_core::{deframe, frame};

/// A connected mixnet client plus the parsed broadcast recipients.
///
/// Confined to the plugin's single-threaded vat (held behind `Rc<RefCell>`
/// in main.rs) — nothing here is `Send` and nothing needs to be.
pub struct Backend {
    client: MixnetClient,
    /// Peer mixnet addresses `publish` fans out to. Introduction / pairing
    /// (how the user learned them) is out of scope — they arrive as
    /// handshake config.
    recipients: Vec<Recipient>,
}

impl Backend {
    /// Parse the recipient addresses, build an ephemeral client, and connect
    /// it to the mixnet. Announces our own address on stderr — the one
    /// channel not carrying RPC — so the user can hand it to peers
    /// (TODO(transport-plugins): a metadata method on the wire contract is
    /// the real answer; see the design doc).
    pub async fn connect(recipients: &[String]) -> Result<Self, String> {
        // Parse up front so a bad address fails at connect time, not
        // mid-publish. The SDK's parser is the only interpreter.
        let recipients = recipients
            .iter()
            .map(|address| {
                Recipient::try_from_base58_string(address)
                    .map_err(|error| format!("bad recipient address '{address}': {error}"))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let client = mixnet::MixnetClientBuilder::new_ephemeral()
            .build()
            .map_err(|error| format!("building mixnet client: {error}"))?
            .connect_to_mixnet()
            .await
            .map_err(|error| format!("connecting to mixnet: {error}"))?;
        eprintln!(
            "ptj-transport-nym: connected; our mixnet address (hand to peers): {}",
            client.nym_address()
        );
        Ok(Self { client, recipients })
    }

    /// Broadcast one framed payload to every configured recipient. Anonymity
    /// is the mixnet's property, not ours — this is byte-moving plumbing.
    pub async fn publish(&self, message: &[u8]) -> Result<(), String> {
        let payload = frame(message);
        for recipient in &self.recipients {
            self.client
                .send_plain_message(*recipient, &payload)
                .await
                .map_err(|error| format!("mixnet send: {error}"))?;
        }
        Ok(())
    }

    /// Drain every message the mixnet has already reconstructed, unframed
    /// back to the peers' opaque bytes. Non-blocking best-effort: each
    /// `wait_for_messages` is bounded by a short timeout, so the poll
    /// returns promptly once nothing more is queued (the host's sync driver
    /// polls on its own cadence). Malformed records are skipped — a
    /// transport moves bytes; it never crashes on a bad wire blob.
    pub async fn collect(&mut self) -> Vec<Vec<u8>> {
        let mut messages = Vec::new();
        // Ends on a closed stream (`Ok(None)`) or an empty poll window
        // (`Err(Elapsed)`) — either way, nothing more is ready right now.
        while let Ok(Some(batch)) =
            tokio::time::timeout(Duration::from_millis(50), self.client.wait_for_messages()).await
        {
            for reconstructed in batch {
                if let Some(bytes) = unframe_one(&reconstructed.message) {
                    messages.push(bytes);
                }
            }
        }
        messages
    }
}

/// Pull the opaque bytes back out of ONE framed mixnet payload; `None` for
/// anything that is not exactly one complete record (incomplete or trailing
/// bytes) — the same strictness as the in-process transport-nym.
fn unframe_one(payload: &[u8]) -> Option<Vec<u8>> {
    let mut buffer = payload.to_vec();
    match deframe(&mut buffer) {
        Ok(Some(bytes)) if buffer.is_empty() => Some(bytes),
        _ => None,
    }
}
