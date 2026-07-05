//! transport-mdk — an ATTRIBUTABLE transport over MDK (Nostr MLS group messaging).
//!
//! This is ordinary messaging plumbing. It uses the upstream `mdk` crate (the
//! Nostr Messaging Development Kit: MLS groups carried on Nostr events) to move
//! OPAQUE bytes between the members of an MLS group. Each application message a
//! member sends is decrypted by MDK together with the sending member's public
//! key; that member key is exactly the metadata an
//! [`AttributableChannel`] yields as a
//! [`SenderId`]. That — what a received message
//! carries about who sent it — is the ENTIRE reason this is an *attributable*
//! rather than an *anonymous* transport.
//!
//! We author no security or privacy logic here. Group confidentiality, forward
//! secrecy, and membership authentication are properties of MLS/MDK; we just
//! call the crate. The lattice join (dedup / ordering / convergence) lives
//! entirely OUTSIDE this transport — [`MdkChannel`] only sends and receives
//! opaque byte blobs.
//!
//! # Feature gating (mirrors the ptj CLI's `iroh-sync` gate)
//!
//! Everything that touches the network or the MLS SDK is behind the `mdk` cargo
//! feature (default OFF). With the feature **off** this crate compiles against
//! [`transport_core`] + `std` alone: [`MdkChannel`] still exists and still
//! implements [`AttributableChannel`], but
//! every operation that would need a relay or an MLS group returns a clear
//! `built without the "mdk" feature` error. With the feature **on**, the same
//! type is backed by a real Nostr relay client + MLS group (see the private
//! `real` module).
//!
//! The framing round-trip and the channel-trait satisfaction are exercised by
//! tests that need **no network** and run in either feature state.

// A transport moves bytes; a stringly-typed shared Error is sufficient.
#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AttributableChannel, Result, SenderId};

/// A prebuilt `built without the "mdk" feature" error, used by every operation
/// that needs the SDK when the crate is compiled as a skeleton.
///
/// Kept as a function (not a const) so the message is constructed the same way
/// as any other [`transport_core::Error`].
#[cfg(not(feature = "mdk"))]
fn built_without_feature() -> transport_core::Error {
    transport_core::Error::new(
        "transport-mdk was built without the \"mdk\" feature: \
         enable it to send/receive over Nostr MLS groups",
    )
}

/// Configuration needed to join an MLS group and reach its members over Nostr
/// relays.
///
/// This is a plain data record in BOTH feature states so a caller (the ptj CLI
/// or webgui) can construct and pass it without conditionally compiling its own
/// code. The introduction/pairing that produces these values (a Nostr DM
/// invite, a group welcome message) is DECOUPLED from the transport and out of
/// scope here — the channel receives its endpoint as an input.
#[derive(Debug, Clone, Default)]
pub struct MdkConfig {
    /// Nostr relay URLs to connect to (redundant relays improve availability).
    pub relays: Vec<String>,
    /// The MLS group id (opaque bytes MDK uses to route application messages).
    pub group_id: Vec<u8>,
    /// Our own member secret, as a nostr `nsec`/hex secret key string. Used to
    /// sign the Nostr events that carry our MLS messages. Opaque to us beyond
    /// handing it to the SDK.
    pub secret_key: String,
}

/// An attributable channel over an MDK (Nostr MLS) group.
///
/// Implements [`AttributableChannel`]:
/// `send` broadcasts one opaque application message to the group; `recv`
/// returns a fresh snapshot of every application message seen so far, each
/// paired with the sending member's public key as a [`SenderId`].
///
/// Wrap it in [`transport_core::Attributed`] to drive it through the plain
/// [`Transport`](transport_core::Transport) seam (which drops the `SenderId`),
/// or hold it directly to read sender identities (e.g. for GUI attribution).
pub struct MdkChannel {
    /// The join configuration, retained in both feature states (read back via
    /// [`MdkChannel::config`], and used by the real backend to reconnect).
    config: MdkConfig,

    /// The real Nostr-MLS backend. Present only with the `mdk` feature; without
    /// it the channel is a skeleton and every op reports the missing feature.
    #[cfg(feature = "mdk")]
    backend: real::Backend,
}

impl MdkChannel {
    /// Connect to the group's relays and join the MLS group described by
    /// `config`.
    ///
    /// With the `mdk` feature **off** this stores the config and returns an
    /// unconnected skeleton; the first `send`/`recv` reports the missing
    /// feature. With the feature **on** it opens the relay client and loads the
    /// MLS group state (see `real::Backend::connect`).
    pub fn connect(config: MdkConfig) -> Result<Self> {
        #[cfg(feature = "mdk")]
        {
            let backend = real::Backend::connect(&config)?;
            Ok(Self { config, backend })
        }
        #[cfg(not(feature = "mdk"))]
        {
            Ok(Self { config })
        }
    }

    /// Borrow the configuration this channel was built with.
    pub fn config(&self) -> &MdkConfig {
        &self.config
    }
}

#[async_trait]
impl AttributableChannel for MdkChannel {
    /// Broadcast one opaque application message to the MLS group.
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        #[cfg(feature = "mdk")]
        {
            self.backend.send(message)
        }
        #[cfg(not(feature = "mdk"))]
        {
            let _ = message;
            Err(built_without_feature())
        }
    }

    /// Return a fresh snapshot of every application message received so far,
    /// each paired with the sending member's public key as a [`SenderId`].
    async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        #[cfg(feature = "mdk")]
        {
            self.backend.recv()
        }
        #[cfg(not(feature = "mdk"))]
        {
            Err(built_without_feature())
        }
    }
}

// =============================================================================
// Real backend — compiled ONLY with the `mdk` feature. All upstream SDK usage
// (relay client, MLS group send/recv) lives here so the default build stays a
// dependency-light skeleton.
// =============================================================================
#[cfg(feature = "mdk")]
mod real {
    use super::MdkConfig;
    use nostr::prelude::*;
    use nostr_sdk::Client;
    use transport_core::{frame, Error, Result, SenderId};

    /// A blocking wrapper around the async Nostr-SDK relay client plus the MDK
    /// MLS group. It converts the SDK's push delivery (events arriving on a
    /// subscription) into the pull cadence the transport-core channel traits
    /// expect: incoming application messages are drained into `inbox` and
    /// `recv` returns a snapshot of it.
    ///
    /// transport-core carries no async runtime, so we own a small current-thread
    /// Tokio runtime here and `block_on` each relay round-trip. This keeps the
    /// `AttributableChannel` methods synchronous, matching the sync driver's
    /// `publish`/`collect` cadence.
    pub struct Backend {
        rt: tokio::runtime::Runtime,
        client: Client,
        mdk: mdk::MDK<mdk::memory::MdkMemoryStorage>,
        group_id: mdk::GroupId,
        /// Every application message seen so far, paired with its sender's
        /// member pubkey bytes. A fresh snapshot of this is what `recv` returns.
        inbox: Vec<(SenderId, Vec<u8>)>,
        /// Ids of Nostr events already folded into `inbox`, so re-polling the
        /// same relay events does not duplicate them into our snapshot. This is
        /// a purely local receive-side guard against re-reading the SAME wire
        /// event twice; it is NOT the lattice dedup (which lives outside every
        /// transport and folds by content, not by event id).
        seen_events: std::collections::HashSet<EventId>,
    }

    impl Backend {
        /// Open the relay client, load our keys, and bind to the MLS group.
        pub fn connect(config: &MdkConfig) -> Result<Self> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| Error::new(format!("mdk: building tokio runtime: {e}")))?;

            let keys = Keys::parse(&config.secret_key)
                .map_err(|e| Error::new(format!("mdk: parsing secret key: {e}")))?;

            // In-memory MLS storage: this transport is a byte-mover, not the
            // durable store; the group state is reloaded per session from the
            // welcome the pairing step delivered (pairing is out of scope here).
            let mdk = mdk::MDK::new(mdk::memory::MdkMemoryStorage::default());
            let group_id = mdk::GroupId::from_slice(&config.group_id);

            let client = Client::new(keys);
            rt.block_on(async {
                for url in &config.relays {
                    client
                        .add_relay(url)
                        .await
                        .map_err(|e| Error::new(format!("mdk: add_relay {url}: {e}")))?;
                }
                client.connect().await;

                // Subscribe to the MLS group message events on the relays. MDK
                // application messages ride as a dedicated Nostr event kind
                // addressed to the group.
                let filter = Filter::new().kind(Kind::MlsGroupMessage);
                client
                    .subscribe(filter, None)
                    .await
                    .map_err(|e| Error::new(format!("mdk: subscribe: {e}")))?;
                Ok::<(), Error>(())
            })?;

            Ok(Self {
                rt,
                client,
                mdk,
                group_id,
                inbox: Vec::new(),
                seen_events: std::collections::HashSet::new(),
            })
        }

        /// Encrypt `message` as an MLS application message and publish it to the
        /// group's relays as a Nostr event.
        ///
        /// We frame the payload with the transport-core length-prefixed framing
        /// before handing it to MDK so a single MLS application message carries
        /// exactly one delimited record — the SDK gives us message boundaries,
        /// but framing keeps the wire format identical to the stream transports
        /// and lets a caller pack multiple envelopes if it ever needs to.
        pub fn send(&mut self, message: Vec<u8>) -> Result<()> {
            let framed = frame(&message);
            let rt = &self.rt;
            let client = &self.client;
            let mdk = &self.mdk;
            let group_id = &self.group_id;
            rt.block_on(async {
                // MDK wraps our bytes into an MLS application message and yields
                // the Nostr event to publish to the group's relays.
                let event = mdk
                    .create_message(group_id, framed)
                    .map_err(|e| Error::new(format!("mdk: create_message: {e}")))?;
                client
                    .send_event(&event)
                    .await
                    .map_err(|e| Error::new(format!("mdk: send_event: {e}")))?;
                Ok::<(), Error>(())
            })
        }

        /// Drain any application messages that have arrived on the subscription
        /// into `inbox`, then return a fresh snapshot of the whole inbox.
        ///
        /// Each MLS application message MDK decrypts yields the sending member's
        /// public key; we surface those pubkey bytes as the `SenderId`. We do NOT
        /// dedup by content and we do NOT order — the lattice join does that
        /// outside the transport. `seen_events` only stops us folding the SAME
        /// relay event twice.
        pub fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            let rt = &self.rt;
            let client = &self.client;
            let mdk = &self.mdk;
            let group_id = &self.group_id;

            // Non-blocking drain: poll whatever the relays have delivered since
            // the last call. A short timeout keeps `recv` from blocking the sync
            // driver's poll loop.
            let events: Vec<Event> = rt.block_on(async {
                let filter = Filter::new().kind(Kind::MlsGroupMessage);
                client
                    .fetch_events(filter, std::time::Duration::from_millis(200))
                    .await
                    .map(|evs| evs.into_iter().collect())
                    .map_err(|e| Error::new(format!("mdk: fetch_events: {e}")))
            })?;

            for event in events {
                if !self.seen_events.insert(event.id) {
                    continue; // already folded this exact relay event
                }
                // MDK decrypts the MLS application message and reports which
                // group member sent it. We only pass along events for our group.
                match mdk.process_message(group_id, &event) {
                    Ok(Some(processed)) => {
                        // The decrypted payload is our framed record; deframe it
                        // back to the opaque bytes the caller sent.
                        let mut buf = processed.payload;
                        while let Some(record) = transport_core::deframe(&mut buf)? {
                            let sender = SenderId(processed.sender.to_bytes().to_vec());
                            self.inbox.push((sender, record));
                        }
                    }
                    // Not an application message for us (a proposal/commit, or a
                    // message for another group). Skip; not our concern here.
                    Ok(None) => {}
                    Err(e) => {
                        return Err(Error::new(format!("mdk: process_message: {e}")));
                    }
                }
            }

            Ok(self.inbox.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{deframe, frame, Attributed, Message, Transport};

    // ---- channel-trait satisfaction (no network, either feature state) ----

    // A tiny in-memory attributable channel standing in for MdkChannel's shape:
    // it satisfies the SAME transport-core trait against which MdkChannel is
    // written, so this test pins the trait contract without needing a relay or
    // the `mdk` feature. It mirrors how a real MLS group would pair each blob
    // with the sending member's pubkey (here a fixed "member key").
    #[derive(Default)]
    struct FakeGroup {
        me: Vec<u8>,
        buf: Vec<(SenderId, Vec<u8>)>,
    }

    #[async_trait]
    impl AttributableChannel for FakeGroup {
        async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            // Real MDK frames the payload before it rides the wire; do the same
            // here so the test also exercises framing on the send path.
            let framed = frame(&message);
            let mut buf = framed;
            while let Some(record) = deframe(&mut buf)? {
                self.buf.push((SenderId(self.me.clone()), record));
            }
            Ok(())
        }
        async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            Ok(self.buf.clone())
        }
    }

    #[test]
    fn mdk_channel_is_an_attributable_channel() {
        // MdkChannel must implement AttributableChannel in BOTH feature states.
        // Assert it via a trait-bound helper so the test fails to compile if the
        // impl is ever dropped.
        fn assert_attributable<C: AttributableChannel>() {}
        assert_attributable::<MdkChannel>();

        // And it must be usable through the driver seam via Attributed<_>
        // (which drops the SenderId). This is compile-time proof the wiring the
        // ptj CLI expects is satisfied.
        fn assert_transport<T: Transport>() {}
        assert_transport::<Attributed<MdkChannel>>();
    }

    #[test]
    fn skeleton_channel_reports_missing_feature() {
        // Without the `mdk` feature, connect() succeeds (it only stores config)
        // but send/recv report the missing feature clearly. With the feature on
        // this path would instead attempt a real relay connect, so only assert
        // the skeleton contract when the feature is off.
        #[cfg(not(feature = "mdk"))]
        {
            let mut ch = MdkChannel::connect(MdkConfig {
                relays: vec!["wss://relay.example".into()],
                group_id: vec![0xAB; 32],
                secret_key: "nsec-placeholder".into(),
            })
            .expect("skeleton connect stores config and does not touch the network");

            futures::executor::block_on(async {
                let sent = ch.send(b"payload".to_vec()).await;
                assert!(sent.is_err());
                assert!(sent.unwrap_err().message().contains("without the \"mdk\" feature"));

                let got = ch.recv().await;
                assert!(got.is_err());
                assert!(got.unwrap_err().message().contains("without the \"mdk\" feature"));
            });
        }
    }

    #[test]
    fn fake_group_carries_sender_id() {
        // Exercise the SAME AttributableChannel contract MdkChannel is written
        // against: send an opaque blob, recv pairs it with the member key. No
        // network, no feature required.
        futures::executor::block_on(async {
            let mut ch = FakeGroup {
                me: vec![0xEE; 32],
                ..Default::default()
            };
            ch.send(b"hello group".to_vec()).await.unwrap();
            let got = ch.recv().await.unwrap();
            assert_eq!(got, vec![(SenderId(vec![0xEE; 32]), b"hello group".to_vec())]);

            // The Attributed wrapper drops the SenderId for the plain Transport seam.
            let mut wrapped = Attributed::new(ch);
            assert_eq!(
                wrapped.collect().await.unwrap(),
                vec![b"hello group".to_vec()]
            );
        });
    }

    // ---- framing round-trip (no network, either feature state) ----

    #[test]
    fn framing_roundtrip_matches_core_wire_format() {
        // transport-mdk frames each opaque payload with transport-core framing
        // before it rides an MLS application message, and deframes on receive.
        // Prove that round-trip here without a relay: frame -> deframe recovers
        // the exact bytes, and multiple records on one buffer come back in order.
        let a = b"first opaque blob".to_vec();
        let b = Message::Psbt(b"cHNidP8BAgQC".to_vec()).encode();

        let mut wire = frame(&a);
        wire.extend_from_slice(&frame(&b));

        assert_eq!(deframe(&mut wire).unwrap(), Some(a.clone()));
        assert_eq!(deframe(&mut wire).unwrap(), Some(b.clone()));
        assert_eq!(deframe(&mut wire).unwrap(), None);
        assert!(wire.is_empty());

        // The second record is a Message envelope; it decodes back intact,
        // proving framing is orthogonal to the Message type-tag (framing
        // delimits, Message tags what a record is).
        assert_eq!(
            Message::decode(&b).unwrap(),
            Message::Psbt(b"cHNidP8BAgQC".to_vec())
        );
    }

    #[test]
    fn framing_roundtrip_empty_payload() {
        // An empty opaque payload still frames/deframes cleanly (4-byte header,
        // zero-length value) — a group member may broadcast an empty keep-alive.
        let mut wire = frame(b"");
        assert_eq!(wire.len(), 4);
        assert_eq!(deframe(&mut wire).unwrap(), Some(Vec::new()));
        assert!(wire.is_empty());
    }
}
