//! Iroh document setup and actor lifecycle.
//!
//! The endpoint, router, document, blob store, and author remain on one Tokio
//! runtime for their lifetime. A dedicated actor thread owns that runtime;
//! channel methods exchange requests and replies with it asynchronously.

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

/// Build this node's per-author frontier key.
fn frontier_key(author: AuthorId) -> Vec<u8> {
    let mut key = FRONTIER_PREFIX.to_vec();
    key.extend_from_slice(author.as_bytes());
    key
}

/// A full frontier snapshot: one `(author, tip)` entry per writer.
type Frontier = Vec<(SenderId, Vec<u8>)>;

/// One request to the actor.
enum Request {
    Send {
        message: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Recv {
        reply: oneshot::Sender<Result<Frontier>>,
    },
}

/// Whether to create or join a document during bootstrap.
enum Bootstrap {
    Create,
    Join(Box<DocTicket>),
}

/// Iroh state confined to the actor's runtime thread.
struct Actor {
    /// Keeps the protocol handlers alive.
    _router: Router,
    blobs: MemStore,
    doc: Doc,
    author: AuthorId,
}

impl Actor {
    /// Start the protocols and create or join a document.
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

    /// Replace this author's frontier record.
    async fn handle_send(&self, message: Vec<u8>) -> Result<()> {
        let author = self.author;
        self.doc
            .set_bytes(author, frontier_key(author), message)
            .await
            .map_err(|e| Error::new(format!("iroh send: publishing frontier entry: {e}")))?;
        Ok(())
    }

    /// Read the latest attributable record from each author's frontier key.
    async fn handle_recv(&self) -> Result<Vec<(SenderId, Vec<u8>)>> {
        let blobs = self.blobs.blobs();
        let stream = self
            .doc
            .get_many(Query::single_latest_per_key().key_prefix(FRONTIER_PREFIX))
            .await
            .map_err(|e| Error::new(format!("iroh recv: querying frontier: {e}")))?;
        tokio::pin!(stream);

        let mut out = Vec::new();
        while let Some(entry) = stream.next().await {
            let entry =
                entry.map_err(|e| Error::new(format!("iroh recv: reading frontier entry: {e}")))?;
            if entry.content_len() == 0 {
                continue;
            }
            let sender = SenderId(entry.author().as_bytes().to_vec());
            let bytes = blobs
                .get_bytes(entry.content_hash())
                .await
                .map_err(|e| Error::new(format!("iroh recv: reading frontier blob: {e}")))?;
            out.push((sender, bytes.to_vec()));
        }
        Ok(out)
    }

    /// Serve requests until the channel closes.
    async fn run(self, mut requests: mpsc::Receiver<Request>) {
        while let Some(request) = requests.recv().await {
            match request {
                Request::Send { message, reply } => {
                    let _ = reply.send(self.handle_send(message).await);
                }
                Request::Recv { reply } => {
                    let _ = reply.send(self.handle_recv().await);
                }
            }
        }
    }
}

/// A handle to a live iroh-docs actor backing one [`IrohChannel`](crate::IrohChannel).
///
/// Dropping the handle closes the request channel, ending the actor and its
/// runtime.
pub(crate) struct Node {
    requests: mpsc::Sender<Request>,
    _actor: std::thread::JoinHandle<()>,
}

impl Node {
    /// Create a document and return its write ticket.
    pub(crate) fn create() -> Result<(Self, DocTicket)> {
        let (node, ticket) = Self::spawn_actor(Bootstrap::Create)?;
        let ticket = ticket.ok_or_else(|| Error::new("iroh: create actor returned no ticket"))?;
        Ok((node, ticket))
    }

    /// Join the document described by `ticket`.
    pub(crate) fn join(ticket: DocTicket) -> Result<Self> {
        let (node, _no_ticket) = Self::spawn_actor(Bootstrap::Join(Box::new(ticket)))?;
        Ok(node)
    }

    /// Start the actor and wait for bootstrap to complete.
    fn spawn_actor(bootstrap: Bootstrap) -> Result<(Self, Option<DocTicket>)> {
        let (request_tx, request_rx) = mpsc::channel::<Request>(32);
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
                            if boot_tx.send(Ok(ticket)).is_err() {
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

    /// Send a message through the actor.
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

    /// Receive the current frontier through the actor.
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
