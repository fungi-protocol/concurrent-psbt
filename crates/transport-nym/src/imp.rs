//! The real mixnet implementation, compiled only under `#[cfg(feature = "nym")]`.
//!
//! This is ordinary messaging plumbing over the off-the-shelf `nym-sdk` mixnet
//! client. It moves OPAQUE bytes; anonymity is a property of the mixnet, not of
//! anything implemented here. There is no threat-model reasoning in this file —
//! just build a client, send bytes to recipients, drain received bytes.
//!
//! # Sync trait over async SDK
//!
//! `nym-sdk`'s `MixnetClient` is async (tokio) and push-based (you await a
//! stream of reconstructed messages). The [`AnonymousChannel`] trait is
//! synchronous and pull-based. We bridge exactly like the iroh transport: one
//! owned [`tokio::runtime::Runtime`] and `rt.block_on(...)` per call. `recv`
//! drains whatever the mixnet has already delivered (non-blocking best-effort),
//! turning the push stream into a polling snapshot; the caller's sync loop
//! already polls on its own interval, so a non-blocking drain is sufficient.
//!
//! # API grounding (nym-sdk `mixnet` module)
//!
//! Wiring mirrors the `nym-sdk` mixnet examples:
//!   * `mixnet::MixnetClientBuilder::new_ephemeral().build().await?` builds a
//!     disconnected client; `.connect_to_mixnet().await?` connects it.
//!   * `client.nym_address() -> &Recipient` is our address to hand to peers.
//!   * `mixnet::Recipient::try_from_base58_string(s) -> Result<Recipient, _>`
//!     parses a peer's address string (we never interpret it ourselves).
//!   * `client.send_plain_message(recipient, bytes).await?` sends opaque bytes.
//!   * `client.wait_for_messages().await -> Option<Vec<ReconstructedMessage>>`
//!     yields the batch of messages the mixnet has reconstructed;
//!     `ReconstructedMessage` exposes the payload bytes as `.message`. We call it
//!     under a short timeout so `recv` is a non-blocking poll (turning the push
//!     stream into a polling snapshot). This uses only the SDK's own surface —
//!     no extra `futures`/stream dependency.

use tokio::runtime::Runtime;
use transport_core::{AnonymousChannel, Error, Result};

use nym_sdk::mixnet::{self, MixnetClient, Recipient};

use crate::{unwrap_incoming, wrap_outgoing, NymAddress};

/// The feature-on guts of [`crate::NymTransport`].
pub(crate) struct Inner {
    /// Owned multi-thread runtime; every method bridges sync->async through
    /// `rt.block_on(...)`. Held for the transport's lifetime.
    rt: Runtime,
    /// The connected mixnet client. Held inside an `Option` only so we can
    /// `disconnect` it on drop by taking ownership.
    client: Option<MixnetClient>,
    /// Parsed broadcast recipients (peer mixnet addresses). `send` fans one
    /// message out to each. Introduction / pairing is out of scope — these
    /// arrive as inputs.
    recipients: Vec<Recipient>,
    /// Our own address string, cached at connect time so `our_address` needn't
    /// re-borrow the client.
    our_address: String,
}

impl Inner {
    /// Build an ephemeral client, connect it to the mixnet, and parse the
    /// broadcast recipient addresses.
    pub(crate) fn connect(recipients: Vec<NymAddress>) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::new(format!("transport-nym: building tokio runtime: {e}")))?;

        // Parse recipient addresses up front so a bad address fails at connect
        // time, not mid-send. We never interpret the string beyond handing it to
        // the SDK's own parser.
        let recipients = recipients
            .into_iter()
            .map(|NymAddress(s)| {
                Recipient::try_from_base58_string(&s)
                    .map_err(|e| Error::new(format!("transport-nym: bad recipient address: {e}")))
            })
            .collect::<Result<Vec<_>>>()?;

        let (client, our_address) = rt.block_on(async {
            let client = mixnet::MixnetClientBuilder::new_ephemeral()
                .build()
                .map_err(|e| Error::new(format!("transport-nym: building mixnet client: {e}")))?
                .connect_to_mixnet()
                .await
                .map_err(|e| Error::new(format!("transport-nym: connecting to mixnet: {e}")))?;
            let addr = client.nym_address().to_string();
            Ok::<_, Error>((client, addr))
        })?;

        Ok(Inner {
            rt,
            client: Some(client),
            recipients,
            our_address,
        })
    }

    /// Our mixnet address, to hand to peers out of band.
    pub(crate) fn our_address(&self) -> NymAddress {
        NymAddress(self.our_address.clone())
    }
}

/// Error for when the client has already been taken (only on drop).
fn disconnected() -> Error {
    Error::new("transport-nym: mixnet client already disconnected")
}

impl AnonymousChannel for Inner {
    /// Broadcast one opaque message to every configured recipient over the
    /// mixnet. The wire body is one framed [`transport_core::Message`] envelope
    /// (see [`wrap_outgoing`]). Anonymity is the mixnet's property, not ours.
    fn send(&mut self, message: Vec<u8>) -> Result<()> {
        // Byte-transparent: frame the caller's opaque bytes verbatim so recv can
        // delimit exactly one record per reconstructed mixnet payload. The
        // transport never interprets the bytes (on the ptj path they are a
        // Message envelope; framing is envelope-agnostic).
        let payload = wrap_outgoing(&message);

        // Borrow the fields disjointly: `rt` drives the async block, `client` is
        // used inside it. Separate field borrows so one method call doesn't hold
        // all of `self` (which would conflict with `rt.block_on`).
        let Inner {
            rt,
            client,
            recipients,
            ..
        } = self;
        let client = client.as_mut().ok_or_else(disconnected)?;
        rt.block_on(async {
            for recipient in recipients.iter() {
                client
                    .send_plain_message(recipient.clone(), payload.clone())
                    .await
                    .map_err(|e| Error::new(format!("transport-nym: mixnet send: {e}")))?;
            }
            Ok::<(), Error>(())
        })
    }

    /// Drain every message the mixnet has already reconstructed for us and return
    /// them as BARE bytes — no sender identity (the anonymous contract). Each
    /// payload is one framed envelope; we unframe it back to the envelope bytes
    /// the caller expects. Non-blocking: returns whatever is ready right now.
    fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        use std::time::Duration;

        // Disjoint field borrows again: `rt` drives, `client` is polled inside.
        let Inner { rt, client, .. } = self;
        let client = client.as_mut().ok_or_else(disconnected)?;
        let raw: Vec<Vec<u8>> = rt.block_on(async {
            let mut out = Vec::new();
            // Drain the reconstructed batches that are ready right now. Each
            // `wait_for_messages` call is bounded by a short timeout so the poll
            // returns promptly when the mixnet has nothing more queued, turning
            // the push stream into a polling snapshot. The caller's sync loop
            // polls on its own interval, so this best-effort drain is sufficient.
            loop {
                match tokio::time::timeout(Duration::from_millis(50), client.wait_for_messages())
                    .await
                {
                    // A batch arrived: collect its payloads and keep draining.
                    Ok(Some(batch)) => {
                        for reconstructed in batch {
                            out.push(reconstructed.message);
                        }
                    }
                    // Stream closed, or nothing more within the poll window.
                    Ok(None) | Err(_) => break,
                }
            }
            out
        });

        // Unframe each reconstructed payload back to the exact bytes the peer
        // handed its `send` — byte-transparent, no interpretation. A payload that
        // is not exactly one framed record is skipped with no panic (a transport
        // moves bytes; it never crashes on a malformed wire blob), so the
        // anonymous contract returns only well-formed records, as bare bytes with
        // no sender identity.
        let mut messages = Vec::with_capacity(raw.len());
        for payload in raw {
            match unwrap_incoming(&payload) {
                Ok(bytes) => messages.push(bytes),
                Err(_) => { /* skip a malformed record; never crash the transport */ }
            }
        }
        Ok(messages)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Best-effort clean disconnect from the mixnet. Ignore errors: we are
        // tearing down.
        if let Some(client) = self.client.take() {
            let _ = self.rt.block_on(async { client.disconnect().await });
        }
    }
}
