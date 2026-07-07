//! transport-mdk — an ATTRIBUTABLE transport over MDK (Nostr MLS group messaging).
//!
//! This is ordinary messaging plumbing. It uses the upstream `mdk-core` crate
//! (the Marmot Development Kit: MLS groups carried on Nostr events) to move
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
            self.backend.send(message).await
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
            self.backend.recv().await
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
    //! # Actor at the edge (why there is no `block_on` here)
    //!
    //! The Nostr-SDK relay client owns async I/O (it runs on tokio), and the
    //! channel seam is async — so, exactly like `transport-iroh`'s backend, the
    //! live SDK state is confined to an ACTOR pinned to its own runtime:
    //!
    //!   * a dedicated OS thread owns a single-threaded tokio runtime and, on
    //!     it, brings the relay client + MLS state up, then drains a
    //!     `tokio::mpsc` request loop until every [`Backend`] handle is gone;
    //!   * [`Backend`] holds only the `mpsc::Sender<Request>`; the async
    //!     `send`/`recv` push a request carrying a `oneshot` reply channel and
    //!     `.await` the reply on the CALLER's runtime. The constructor stays
    //!     sync (it waits once, on a std channel, for bootstrap to finish).
    //!
    //! # API grounding (verified against the pinned crate sources)
    //!
    //! Read from the registry sources at the versions in `Cargo.lock`
    //! (`mdk-core 0.8.0`, `mdk-memory-storage 0.8.0`, `nostr 0.44.4`,
    //! `nostr-sdk 0.44.1`):
    //!   * `MDK::new(MdkMemoryStorage::default())`; `GroupId::from_slice`
    //!     (both via `mdk_core::prelude` — mdk-core/src/lib.rs:383, prelude.rs);
    //!   * `MDK::create_message(&GroupId, UnsignedEvent rumor, Option<Vec<EventTag>>)
    //!     -> Result<Event>` — the payload rides a nostr RUMOR (an
    //!     `UnsignedEvent`, string content), and the returned kind-445 wrapper
    //!     event is what gets published (mdk-core/src/messages/create.rs:110);
    //!   * `MDK::process_message(&Event) -> Result<MessageProcessingResult>` —
    //!     no group argument; an application message comes back as
    //!     `ApplicationMessage(Message { pubkey, content, .. })` where `pubkey`
    //!     is the sending member and `content` the decrypted rumor content
    //!     (mdk-core/src/messages/process.rs:325, mod.rs:110);
    //!   * `MDK::get_group(&GroupId) -> Result<Option<Group>>`; the wrapper
    //!     events carry `h` = hex(`group.nostr_group_id`) (mdk-core
    //!     src/groups.rs:520, build_message_event at src/groups.rs:2355);
    //!   * `Client::{new, add_relay, connect, subscribe(Filter, None),
    //!     fetch_events(Filter, Duration) -> Events, send_event(&Event)}`
    //!     (nostr-sdk/src/client/mod.rs); `Events: IntoIterator<Item = Event>`.

    use std::collections::HashSet;
    use std::time::Duration;

    use mdk_core::prelude::{GroupId, MessageProcessingResult, MDK};
    use mdk_memory_storage::MdkMemoryStorage;
    use nostr::{Alphabet, EventBuilder, EventId, Filter, Keys, Kind, PublicKey, SingleLetterTag};
    use nostr_sdk::Client;
    use tokio::sync::{mpsc, oneshot};
    use transport_core::{deframe, frame, Error, Result, SenderId};

    use super::MdkConfig;

    /// One request the async channel methods hand to the actor. Each carries a
    /// `oneshot` sender the actor replies on; the caller `.await`s the receiver.
    enum Request {
        /// Broadcast one opaque application message (from `send`).
        Send {
            message: Vec<u8>,
            reply: oneshot::Sender<Result<()>>,
        },
        /// Snapshot every application message seen so far (from `recv`).
        Recv {
            reply: oneshot::Sender<Result<Vec<(SenderId, Vec<u8>)>>>,
        },
    }

    /// The live SDK state the actor owns for the channel's lifetime: the relay
    /// client plus the MDK MLS engine. Confined to the actor's runtime thread —
    /// never sent to a caller.
    struct Actor {
        client: Client,
        mdk: MDK<MdkMemoryStorage>,
        group_id: GroupId,
        /// hex(`nostr_group_id`) — the `h` tag value MDK stamps on the group's
        /// kind-445 wrapper events; scopes our relay queries to OUR group.
        group_tag: String,
        /// Our member pubkey — the author of the rumors we send.
        pubkey: PublicKey,
        /// Every application message seen so far, paired with its sender's
        /// member pubkey bytes. A fresh snapshot of this is what `recv` returns.
        inbox: Vec<(SenderId, Vec<u8>)>,
        /// Ids of Nostr events already folded into `inbox`, so re-polling the
        /// same relay events does not duplicate them into our snapshot. This is
        /// a purely local receive-side guard against re-reading the SAME wire
        /// event twice; it is NOT the lattice dedup (which lives outside every
        /// transport and folds by content, not by event id).
        seen_events: HashSet<EventId>,
    }

    impl Actor {
        /// Load our keys, bind to the MLS group, and open the relay client, on
        /// the actor's runtime.
        async fn bootstrap(config: MdkConfig) -> std::result::Result<Self, String> {
            let keys = Keys::parse(&config.secret_key)
                .map_err(|e| format!("parsing secret key: {e}"))?;
            let pubkey = keys.public_key();

            // In-memory MLS storage: this transport is a byte-mover, not the
            // durable store; the group state is reloaded per session from the
            // welcome the pairing step delivered (pairing is out of scope here).
            let mdk = MDK::new(MdkMemoryStorage::default());
            let group_id = GroupId::from_slice(&config.group_id);

            // The group must already be in MDK's storage (the pairing step's
            // welcome puts it there): create_message/process_message both load
            // it, and its `nostr_group_id` is the `h` tag its wrapper events
            // carry on the relays — our subscription filter.
            let group = mdk
                .get_group(&group_id)
                .map_err(|e| format!("loading group: {e}"))?
                .ok_or("MLS group not found in storage (no welcome processed)")?;
            let group_tag = hex::encode(group.nostr_group_id);

            let client = Client::new(keys);
            for url in &config.relays {
                client
                    .add_relay(url.as_str())
                    .await
                    .map_err(|e| format!("add_relay {url}: {e}"))?;
            }
            client.connect().await;

            // Subscribe to OUR group's MLS message events on the relays. MDK
            // application messages ride as a dedicated Nostr event kind (445)
            // tagged with the group's nostr group id.
            client
                .subscribe(Self::group_filter(&group_tag), None)
                .await
                .map_err(|e| format!("subscribe: {e}"))?;

            Ok(Self {
                client,
                mdk,
                group_id,
                group_tag,
                pubkey,
                inbox: Vec::new(),
                seen_events: HashSet::new(),
            })
        }

        /// The relay filter for our group's wrapper events: kind 445, `h` tag =
        /// hex of the group's nostr group id.
        fn group_filter(group_tag: &str) -> Filter {
            Filter::new()
                .kind(Kind::MlsGroupMessage)
                .custom_tag(SingleLetterTag::lowercase(Alphabet::H), group_tag)
        }

        /// Encrypt `message` as an MLS application message and publish it to the
        /// group's relays as a Nostr event.
        ///
        /// We frame the payload with the transport-core length-prefixed framing
        /// before handing it to MDK so a single MLS application message carries
        /// exactly one delimited record — the SDK gives us message boundaries,
        /// but framing keeps the wire format identical to the stream transports
        /// and lets a caller pack multiple envelopes if it ever needs to. The
        /// framed record rides hex-encoded: MDK carries the payload as a nostr
        /// rumor (an `UnsignedEvent`), whose content is a String.
        async fn handle_send(&self, message: Vec<u8>) -> Result<()> {
            let framed = frame(&message);
            let rumor = EventBuilder::new(Kind::TextNote, hex::encode(framed)).build(self.pubkey);
            // MDK encrypts the rumor into an MLS application message and yields
            // the signed kind-445 wrapper event to publish to the relays.
            let event = self
                .mdk
                .create_message(&self.group_id, rumor, None)
                .map_err(|e| Error::new(format!("mdk send: create_message: {e}")))?;
            self.client
                .send_event(&event)
                .await
                .map_err(|e| Error::new(format!("mdk send: send_event: {e}")))?;
            Ok(())
        }

        /// Drain our group's wrapper events from the relays into `inbox`, then
        /// return a fresh snapshot of the whole inbox.
        ///
        /// Each MLS application message MDK decrypts yields the sending member's
        /// public key (the rumor author); we surface those pubkey bytes as the
        /// `SenderId`. We do NOT dedup by content and we do NOT order — the
        /// lattice join does that outside the transport. `seen_events` only
        /// stops us folding the SAME relay event twice.
        async fn handle_recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            // Poll whatever the relays have delivered for our group. A short
            // timeout keeps `recv` close to the driver's poll cadence.
            let events = self
                .client
                .fetch_events(Self::group_filter(&self.group_tag), Duration::from_millis(200))
                .await
                .map_err(|e| Error::new(format!("mdk recv: fetch_events: {e}")))?;

            for event in events {
                if !self.seen_events.insert(event.id) {
                    continue; // already folded this exact relay event
                }
                match self.mdk.process_message(&event) {
                    // The decrypted rumor: its author is the sending group
                    // member, its content our hex-encoded framed record.
                    Ok(MessageProcessingResult::ApplicationMessage(message)) => {
                        let sender = SenderId(message.pubkey.to_bytes().to_vec());
                        let mut buf = hex::decode(&message.content).map_err(|e| {
                            Error::new(format!("mdk recv: non-hex application payload: {e}"))
                        })?;
                        while let Some(record) = deframe(&mut buf)? {
                            self.inbox.push((sender.clone(), record));
                        }
                    }
                    // Group-management traffic (proposals, commits, ...): MDK
                    // already folded it into the group state; it carries no
                    // application payload. Skip; not our concern here.
                    Ok(_) => {}
                    Err(e) => {
                        return Err(Error::new(format!("mdk recv: process_message: {e}")));
                    }
                }
            }

            Ok(self.inbox.clone())
        }

        /// Drain requests until every [`Backend`] handle is dropped (the channel
        /// closes). Runs on the actor's own runtime; the live SDK state stays
        /// confined here.
        async fn run(mut self, mut requests: mpsc::Receiver<Request>) {
            while let Some(request) = requests.recv().await {
                match request {
                    Request::Send { message, reply } => {
                        // A dropped receiver only means the caller went away.
                        let _ = reply.send(self.handle_send(message).await);
                    }
                    Request::Recv { reply } => {
                        let _ = reply.send(self.handle_recv().await);
                    }
                }
            }
            // Channel closed: drop `self` (relay client, MLS state), tearing
            // the connection down on its own runtime.
        }
    }

    /// A handle to the live Nostr-MLS actor backing one
    /// [`MdkChannel`](crate::MdkChannel).
    ///
    /// Holds only the request channel to the actor thread; the SDK state lives
    /// on the actor's runtime. The async channel methods talk to it over
    /// `mpsc` + `oneshot` — no `block_on`.
    pub struct Backend {
        requests: mpsc::Sender<Request>,
        // The actor thread's join handle. Kept so the thread's lifetime is tied
        // to this handle; on drop the `requests` sender closes, the actor loop
        // ends, and the runtime (owned by that thread) winds down.
        _actor: std::thread::JoinHandle<()>,
    }

    impl Backend {
        /// Spawn the actor thread with its own runtime, run the bootstrap on
        /// it (keys, MLS group, relay client), and return a ready handle.
        /// Synchronous: this is a constructor, called from the sync
        /// [`MdkChannel::connect`](crate::MdkChannel::connect).
        pub fn connect(config: &MdkConfig) -> Result<Self> {
            let config = config.clone();
            let (request_tx, request_rx) = mpsc::channel::<Request>(32);
            // Reports the bootstrap outcome back to this constructor so
            // `connect` stays synchronous.
            let (boot_tx, boot_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

            let actor = std::thread::Builder::new()
                .name("transport-mdk-actor".to_string())
                .spawn(move || {
                    let rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(error) => {
                            let _ = boot_tx.send(Err(format!("building tokio runtime: {error}")));
                            return;
                        }
                    };
                    rt.block_on(async move {
                        match Actor::bootstrap(config).await {
                            Ok(actor) => {
                                // Setup done: report ready, then serve.
                                if boot_tx.send(Ok(())).is_err() {
                                    // Constructor gave up already; nothing to serve.
                                    return;
                                }
                                actor.run(request_rx).await;
                            }
                            Err(message) => {
                                let _ = boot_tx.send(Err(message));
                            }
                        }
                    });
                })
                .map_err(|error| Error::new(format!("mdk: spawning actor thread: {error}")))?;

            // Wait for bootstrap to finish before returning a usable handle.
            boot_rx
                .recv()
                .map_err(|_| Error::new("mdk: actor thread exited before bootstrap"))?
                .map_err(|message| Error::new(format!("mdk: {message}")))?;

            Ok(Self {
                requests: request_tx,
                _actor: actor,
            })
        }

        /// Async publish: hand the actor a `Send` request and await its reply.
        /// Runs on the CALLER's runtime; the SDK future runs on the actor's.
        pub async fn send(&mut self, message: Vec<u8>) -> Result<()> {
            let (reply_tx, reply_rx) = oneshot::channel();
            self.requests
                .send(Request::Send {
                    message,
                    reply: reply_tx,
                })
                .await
                .map_err(|_| Error::new("mdk send: actor thread is gone"))?;
            reply_rx
                .await
                .map_err(|_| Error::new("mdk send: actor dropped the reply"))?
        }

        /// Async collect: hand the actor a `Recv` request and await the inbox
        /// snapshot. Same actor round-trip as [`send`](Self::send).
        pub async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
            let (reply_tx, reply_rx) = oneshot::channel();
            self.requests
                .send(Request::Recv { reply: reply_tx })
                .await
                .map_err(|_| Error::new("mdk recv: actor thread is gone"))?;
            reply_rx
                .await
                .map_err(|_| Error::new("mdk recv: actor dropped the reply"))?
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
