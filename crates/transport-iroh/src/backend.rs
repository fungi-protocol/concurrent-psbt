//! The real iroh-docs backend for [`IrohChannel`](crate::IrohChannel).
//!
//! Compiled in ONLY under the `iroh` cargo feature. Ported faithfully from
//! `crates/ptj/src/transport/iroh.rs` (the `IrohTransport` there), adapted from
//! ptj's local synchronous `Transport` trait to transport-core's uniform ASYNC
//! [`AttributableChannel`](transport_core::AttributableChannel):
//!
//!   * `publish`/`collect` become async `send`/`recv`;
//!   * `collect`'s bare `Vec<Vec<u8>>` becomes `recv`'s `Vec<(SenderId, Vec<u8>)>`,
//!     pairing each frontier record with the [`SenderId`] derived from the
//!     `AuthorId` iroh-docs already stamped on it. That is the ENTIRE difference
//!     — carrying, verbatim, a piece of metadata the upstream crate hands us.
//!
//! # Actor at the edge (why there is no `block_on` here)
//!
//! The iroh doc/replica handles (`Doc`, `MemStore`, `AuthorId`) and the router
//! must live on ONE tokio runtime for the channel's whole lifetime. The old
//! design owned a `Runtime` inside `Node` and bridged every sync channel call
//! through `rt.block_on(...)`; the seam is async now, so that bridge is gone.
//!
//! Instead the event loop is an ACTOR pinned to its own runtime, spawned ONCE:
//!
//!   * a dedicated OS thread (`std::thread::spawn`) owns a single-threaded tokio
//!     runtime and, on it, brings the endpoint / router / docs / doc / author up
//!     (the exact wiring the old `block_on` bootstrap did), then runs a loop
//!     draining a `tokio::mpsc` request channel until every [`Node`] handle is
//!     dropped;
//!   * [`Node`] holds only the `mpsc::Sender<Request>`. The async `send`/`recv`
//!     push a [`Request`] carrying a `oneshot` reply channel and `.await` the
//!     reply — they run on the CALLER's runtime (the ptj driver edge) and never
//!     block a thread. The iroh futures run on the actor's runtime.
//!
//! This is the contract's "a push transport converts push -> pull internally":
//! the actor owns the live iroh state, the channel API stays a clean poll.
//!
//! # API grounding (verified against the pinned crate sources)
//!
//! Paths below were read from the vendored sources at the versions in
//! `Cargo.lock` (`iroh 1`, `iroh-docs 0.101.0`, `iroh-blobs 0.103`,
//! `iroh-gossip 0.101.0`):
//!   * wiring mirrors `iroh-docs-0.101.0/examples/setup.rs`;
//!   * `Endpoint::bind(presets::N0)`; `Router::builder(ep).accept(ALPN, h).spawn()`;
//!   * `MemStore` derefs to `iroh_blobs::api::Store`; `.blobs().get_bytes(hash)`;
//!   * `Docs::memory().spawn(ep, (*blobs).clone(), gossip)`;
//!   * `DocsApi::{import, create, author_create}`; `Doc::{set_bytes, share}`;
//!   * `Doc::get_many(Query::single_latest_per_key().key_prefix(..))`;
//!   * `Entry::{author, content_hash, content_len}` (iroh-docs .../src/sync.rs);
//!   * `AuthorId::as_bytes() -> &[u8; 32]` (.../src/keys.rs:376).

use std::time::Duration;

use iroh::Endpoint;
use iroh::endpoint::presets;
use iroh::protocol::Router;
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::{ALPN as BLOBS_ALPN, BlobsProtocol};
use iroh_docs::api::Doc;
use iroh_docs::api::protocol::{AddrInfoOptions, ShareMode};
use iroh_docs::protocol::Docs;
use iroh_docs::store::Query;
use iroh_docs::{ALPN as DOCS_ALPN, AuthorId, DocTicket};
use iroh_gossip::ALPN as GOSSIP_ALPN;
use iroh_gossip::net::Gossip;
use n0_future::StreamExt as _;
use tokio::sync::{mpsc, oneshot};

use transport_core::{Error, Result, SenderId};

use crate::FRONTIER_PREFIX;

/// Build this node's per-author frontier key: `FRONTIER_PREFIX ++ author bytes`.
fn frontier_key(author: AuthorId) -> Vec<u8> {
    let mut key = FRONTIER_PREFIX.to_vec();
    key.extend_from_slice(author.as_bytes());
    key
}

/// A full frontier snapshot: one `(author, tip)` entry per writer.
type Frontier = Vec<(SenderId, Vec<u8>)>;

/// One request the async channel methods hand to the actor. Each carries a
/// `oneshot` sender the actor replies on; the caller `.await`s the receiver.
enum Request {
    /// Publish our maximal tip (from `send`).
    Send {
        message: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Snapshot the whole frontier (from `recv`).
    Recv {
        reply: oneshot::Sender<Result<Frontier>>,
    },
}

/// Whether the actor should MINT a new document (returning a write ticket) or
/// JOIN one from a ticket. Chosen once, at bootstrap, by the sync constructor.
/// The ticket is boxed: it dwarfs the empty `Create` variant (clippy
/// `large_enum_variant`) and lives only for the one bootstrap hop.
enum Bootstrap {
    Create,
    Join(Box<DocTicket>),
}

/// The live iroh state the actor owns for the channel's lifetime. Confined to
/// the actor's runtime thread — never sent to a caller.
struct Actor {
    /// Hosts the blobs/gossip/docs protocol handlers. Held to keep the node
    /// alive; dropped only when the actor loop ends (all `Node` handles gone).
    _router: Router,
    /// In-memory blob store. A `Doc` record carries only a content *hash*; the
    /// bytes live here, read back via `blobs.blobs().get_bytes(hash)`.
    blobs: MemStore,
    /// The joined document (replica handle) for the shared namespace.
    doc: Doc,
    /// This node's local author — namespaces our own per-author frontier key AND
    /// is the `AuthorId` iroh-docs stamps on our records (surfaced as a SenderId).
    author: AuthorId,
}

impl Actor {
    /// Bring the endpoint / router / docs / doc / author up on the actor's
    /// runtime. Mirrors `iroh-docs-0.101.0/examples/setup.rs`. When `bootstrap`
    /// is `Create`, also mints and returns a write [`DocTicket`].
    async fn bootstrap(
        bootstrap: Bootstrap,
    ) -> std::result::Result<(Self, Option<DocTicket>), String> {
        let endpoint = Endpoint::bind(presets::N0)
            .await
            .map_err(|e| format!("binding endpoint: {e}"))?;
        let blobs = MemStore::default();
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let docs = Docs::memory()
            .spawn(endpoint.clone(), (*blobs).clone(), gossip.clone())
            .await
            .map_err(|e| format!("spawning docs: {e}"))?;

        let router = Router::builder(endpoint.clone())
            .accept(BLOBS_ALPN, BlobsProtocol::new(&blobs, None))
            .accept(GOSSIP_ALPN, gossip)
            .accept(DOCS_ALPN, docs.clone())
            .spawn();

        // A local author to write our frontier key under and to stamp our
        // records (surfaced later as a SenderId).
        let author = docs
            .api()
            .author_create()
            .await
            .map_err(|e| format!("creating author: {e}"))?;

        let (doc, ticket) = match bootstrap {
            Bootstrap::Create => {
                let _ = tokio::time::timeout(Duration::from_secs(5), endpoint.online()).await;
                let doc = docs
                    .api()
                    .create()
                    .await
                    .map_err(|e| format!("creating doc: {e}"))?;
                let ticket = doc
                    .share(ShareMode::Write, AddrInfoOptions::RelayAndAddresses)
                    .await
                    .map_err(|e| format!("creating ticket: {e}"))?;
                (doc, Some(ticket))
            }
            Bootstrap::Join(ticket) => {
                let doc = docs
                    .api()
                    .import(*ticket)
                    .await
                    .map_err(|e| format!("importing doc from ticket: {e}"))?;
                (doc, None)
            }
        };

        Ok((
            Self {
                _router: router,
                blobs,
                doc,
                author,
            },
            ticket,
        ))
    }

    /// Publish our maximal tip under our per-author frontier key. `set_bytes`
    /// supersedes our prior record, so the doc keeps at most one tip per author.
    /// No pre-`del`: a `del` would write a 0-length tombstone that `recv` skips,
    /// and a poll landing between the del and the set could drop our fragment.
    async fn handle_send(&self, message: Vec<u8>) -> Result<()> {
        let author = self.author;
        self.doc
            .set_bytes(author, frontier_key(author), message)
            .await
            .map_err(|e| Error::new(format!("iroh send: publishing frontier entry: {e}")))?;
        Ok(())
    }

    /// Snapshot the whole frontier: every author's latest tip, each paired with
    /// the `SenderId` derived from the record's `AuthorId`.
    ///
    /// Each author owns a unique key (`FRONTIER_PREFIX ++ author`), so a
    /// prefix-scoped single-latest-per-key query returns exactly one record per
    /// participant. Includes our own prior write (lattice-idempotent
    /// self-absorption). Carrying the `AuthorId` as a `SenderId` is the only
    /// thing that makes this the ATTRIBUTABLE shape.
    async fn handle_recv(&self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        let blobs = self.blobs.blobs();
        let stream = self
            .doc
            .get_many(Query::single_latest_per_key().key_prefix(FRONTIER_PREFIX))
            .await
            .map_err(|e| Error::new(format!("iroh recv: querying frontier: {e}")))?;
        // The query stream is `!Unpin`; pin it before iterating (the crate's own
        // `Doc::get_one` does the same — api.rs:415).
        tokio::pin!(stream);

        let mut out = Vec::new();
        while let Some(entry) = stream.next().await {
            let entry =
                entry.map_err(|e| Error::new(format!("iroh recv: reading frontier entry: {e}")))?;
            // Skip deletion markers / tombstones (empty content).
            if entry.content_len() == 0 {
                continue;
            }
            // The opaque sender identity the transport provides: the AuthorId
            // bytes iroh-docs stamped on this record.
            let sender = SenderId(entry.author().as_bytes().to_vec());
            // Resolve the content hash to the stored PSBT bytes.
            let bytes = blobs
                .get_bytes(entry.content_hash())
                .await
                .map_err(|e| Error::new(format!("iroh recv: reading frontier blob: {e}")))?;
            out.push((sender, bytes.to_vec()));
        }
        Ok(out)
    }

    /// Drain requests until every `Node` handle is dropped (the channel closes).
    /// Runs on the actor's own runtime; the live iroh state stays confined here.
    async fn run(self, mut requests: mpsc::Receiver<Request>) {
        while let Some(request) = requests.recv().await {
            match request {
                Request::Send { message, reply } => {
                    // A dropped receiver only means the caller went away; ignore.
                    let _ = reply.send(self.handle_send(message).await);
                }
                Request::Recv { reply } => {
                    let _ = reply.send(self.handle_recv().await);
                }
            }
        }
        // Channel closed: fall out of the loop, dropping `self` (router, doc,
        // blobs) and tearing the node down on its own runtime.
    }
}

/// A handle to a live iroh-docs actor backing one [`IrohChannel`](crate::IrohChannel).
///
/// Holds only the request channel to the actor thread; the iroh state lives on
/// the actor's runtime. The async channel methods talk to it over `mpsc` +
/// `oneshot` — no `block_on`.
pub(crate) struct Node {
    requests: mpsc::Sender<Request>,
    // The actor thread's join handle. Kept so the thread's lifetime is tied to
    // this handle; on drop the `requests` sender closes, the actor loop ends,
    // and the runtime (owned by that thread) winds down.
    _actor: std::thread::JoinHandle<()>,
}

impl Node {
    /// Create a new collaboration document; return the node and a write ticket
    /// to hand peers out of band. Spawns the actor thread + runtime ONCE and
    /// waits (over a bootstrap channel) for setup to finish. Synchronous: this
    /// is a constructor, not a channel method, called from ptj's sync
    /// `build_transport`.
    pub(crate) fn create() -> Result<(Self, DocTicket)> {
        let (node, ticket) = Self::spawn_actor(Bootstrap::Create)?;
        let ticket = ticket.ok_or_else(|| Error::new("iroh: create actor returned no ticket"))?;
        Ok((node, ticket))
    }

    /// Join the collaboration document described by `ticket`. `ticket` is the
    /// out-of-band join credential (introduction is out of scope). Synchronous
    /// constructor, as [`create`](Self::create).
    pub(crate) fn join(ticket: DocTicket) -> Result<Self> {
        let (node, _no_ticket) = Self::spawn_actor(Bootstrap::Join(Box::new(ticket)))?;
        Ok(node)
    }

    /// Spawn the actor thread with its own runtime, run the bootstrap on it, and
    /// return a ready [`Node`] plus (for `Create`) the minted write ticket. The
    /// runtime is spawned exactly ONCE here and owned by the actor thread for the
    /// channel's lifetime.
    fn spawn_actor(bootstrap: Bootstrap) -> Result<(Self, Option<DocTicket>)> {
        let (request_tx, request_rx) = mpsc::channel::<Request>(32);
        // Reports the bootstrap outcome (ready ticket, or a setup error string)
        // back to this constructor so `create`/`join` stay synchronous.
        let (boot_tx, boot_rx) =
            std::sync::mpsc::channel::<std::result::Result<Option<DocTicket>, String>>();

        let actor = std::thread::Builder::new()
            .name("transport-iroh-actor".to_string())
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
                    match Actor::bootstrap(bootstrap).await {
                        Ok((actor, ticket)) => {
                            // Setup done: hand the ticket back, then serve.
                            if boot_tx.send(Ok(ticket)).is_err() {
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
            .map_err(|error| Error::new(format!("iroh: spawning actor thread: {error}")))?;

        // Wait for bootstrap to finish before returning a usable handle.
        let ticket = boot_rx
            .recv()
            .map_err(|_| Error::new("iroh: actor thread exited before bootstrap"))?
            .map_err(|message| Error::new(format!("iroh: {message}")))?;

        Ok((
            Self {
                requests: request_tx,
                _actor: actor,
            },
            ticket,
        ))
    }

    /// Async publish: hand the actor a `Send` request and await its reply. Runs
    /// on the CALLER's runtime; the iroh future runs on the actor's. No
    /// `block_on` — this is the whole point of the async seam.
    pub(crate) async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Send {
                message,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::new("iroh send: actor thread is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("iroh send: actor dropped the reply"))?
    }

    /// Async collect: hand the actor a `Recv` request and await the frontier
    /// snapshot. Same actor round-trip as [`send`](Self::send); no `block_on`.
    pub(crate) async fn recv(&mut self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Recv { reply: reply_tx })
            .await
            .map_err(|_| Error::new("iroh recv: actor thread is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("iroh recv: actor dropped the reply"))?
    }
}

// DEFERRED OPTIMIZATION — live push via `Doc::subscribe()`:
// `DocsApi::import_and_subscribe(ticket)` / `Doc::subscribe()` yield a live
// `LiveEvent` stream. A future version can have the actor also drain that stream
// into an internal buffer and answer `Recv` from the buffer instead of issuing a
// fresh `get_many` each poll. Pure latency win; the polling `get_many` above is
// already correct and sufficient for the sync loop cadence.
