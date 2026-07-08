//! The real I2P streaming path (compiled only with the `emissary` feature).
//!
//! emissary-core is an EMBEDDED I2P router: it runs inside our process, exactly
//! like transport-arti's in-process Tor client. There is no external i2pd/Java
//! router to install — we build the router here and open a stream to the peer
//! through it. One delta from the original sketch, grounded against the real
//! emissary-core 0.4 API: the router's only client-facing surface is the
//! SAMv3/I2CP listener it hosts (`emissary_core`'s `sam`/`destination` modules
//! are private), so the streaming path speaks SAMv3 — to OUR OWN in-process
//! router on a loopback port it chose, not to an external bridge. This is
//! exactly how emissary-core's own integration tests drive streaming (see
//! `emissary-core-0.4.0/tests/sam.rs`), using the same `yosemite` SAMv3 client
//! crate used here.
//!
//! The router is async and so is the [`AnonymousChannel`] seam, but the router,
//! session, and stream handles must all live on ONE runtime for the channel's
//! whole lifetime — so, like transport-iroh's backend, the live state is an
//! ACTOR pinned to its own runtime: a dedicated OS thread owns a
//! single-threaded tokio runtime, bootstraps the router + session + stream on
//! it, and then drains an `mpsc` request loop. [`I2pStream`] holds only the
//! request sender; the async `send_framed`/`drain_framed` do an
//! `mpsc`+`oneshot` round-trip and never `block_on`. Once the stream is open it
//! is just bytes: we put transport-core length-prefixed frames on it, one
//! framed [`Message`] envelope per record.
//!
//! [`AnonymousChannel`]: transport_core::AnonymousChannel
//! [`Message`]: transport_core::Message
//!
//! Flow (embedded router, in-process):
//!   1. Build + start the router (`emissary_core::router::Router` over the
//!      tokio `Runtime` impl from emissary-util) with a SAMv3 listener on an
//!      ephemeral loopback port, and spawn it on the actor's runtime — the
//!      `Router` value IS the router event loop future.
//!   2. Open a `yosemite` streaming session against that listener. Our
//!      destination key persists under `state_dir`, so re-runs keep the same
//!      `.b32.i2p` address. Session creation completes once the router has
//!      tunnels for it — the I2P analogue of arti's descriptor-publish wait.
//!   3. Connect a stream to the peer destination; from there it is a raw
//!      bidirectional byte pipe we frame over.
//!
//! GROUNDING NOTE — deltas from a production I2P node, deliberately out of
//! scope for this transport crate:
//!   * no reseed wiring: a cold router with an empty netdb cannot build
//!     tunnels on the public network until it learns router infos
//!     (emissary-util ships a `reseeder`; wiring it is bootstrap policy, not
//!     byte-moving, and belongs to the caller/integration layer);
//!   * router identity (signing/static keys) and the netdb are ephemeral —
//!     only the DESTINATION key persists in `state_dir` (that is the part that
//!     names us on the network);
//!   * dial-only: this channel `connect`s to a peer destination; nothing here
//!     `accept`s inbound streams yet (the authored surface takes a peer
//!     destination as input, so the accepting side is a future addition).
//! This module keeps zero privacy/threat-model reasoning: anonymity is a
//! property of the embedded I2P router, not of this code. We only move opaque
//! bytes.

use std::path::Path;
use std::time::Duration;

use emissary_core::router::Router;
use emissary_core::runtime::Runtime as _;
use emissary_core::{Config, Ntcp2Config, SamConfig};
use emissary_util::runtime::tokio::Runtime as TokioRuntime;
use rand::Rng as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::{mpsc, oneshot};
use yosemite::{style, DestinationKind, RouterApi, Session, SessionOptions};

use transport_core::{deframe, frame, Error, Result};

/// File under `state_dir` holding our persistent I2P destination private key
/// (the base64 blob yosemite's `DestinationKind::Persistent` takes). Reusing it
/// across runs reuses our stable `.b32.i2p` address.
const DESTINATION_KEY_FILE: &str = "destination.key";

/// How long `drain_framed` waits for new bytes before reporting "nothing new".
const POLL_WINDOW: Duration = Duration::from_millis(50);

/// One request the async channel methods hand to the actor. Each carries a
/// `oneshot` sender the actor replies on; the caller `.await`s the receiver.
enum Request {
    /// Write one already-framed record onto the stream (from `send_framed`).
    Send {
        framed: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    /// Drain every complete framed record that has arrived (from `drain_framed`).
    Drain {
        reply: oneshot::Sender<Result<Vec<Vec<u8>>>>,
    },
}

/// The live I2P state the actor owns for the channel's lifetime: the streaming
/// session, the connected stream, and the reassembly buffer. Confined to the
/// actor's runtime thread — never sent to a caller. (The router itself runs as
/// a spawned task on the same runtime.)
struct Actor {
    /// The SAMv3 streaming session on our embedded router. Kept alive for the
    /// stream's lifetime — dropping it closes the session (and our lease set).
    _session: Session<style::Stream>,
    /// The connected I2P stream to the peer (an async duplex byte pipe).
    stream: yosemite::Stream,
    /// Bytes read off the stream but not yet formed into a complete frame.
    /// `drain` pulls whole records out of here; partial tails remain.
    rx: Vec<u8>,
}

/// Load the persisted destination key from `state_dir`, or generate a fresh one
/// through the router's SAMv3 API and persist it.
async fn load_or_generate_destination(
    state_dir: &str,
    sam_port: u16,
) -> std::result::Result<DestinationKind, String> {
    let path = Path::new(state_dir).join(DESTINATION_KEY_FILE);
    if let Ok(private_key) = std::fs::read_to_string(&path) {
        return Ok(DestinationKind::Persistent { private_key });
    }

    let (_destination, private_key) = RouterApi::new(sam_port)
        .generate_destination()
        .await
        .map_err(|e| format!("generating destination: {e}"))?;
    std::fs::create_dir_all(state_dir)
        .map_err(|e| format!("creating state dir {state_dir}: {e}"))?;
    std::fs::write(&path, &private_key)
        .map_err(|e| format!("persisting destination key: {e}"))?;
    Ok(DestinationKind::Persistent { private_key })
}

impl Actor {
    /// Bring the embedded router up on the actor's runtime, open a streaming
    /// session against its SAMv3 listener, and dial the peer. Mirrors
    /// `emissary-core-0.4.0/tests/sam.rs` (`make_router` + `streaming_works`),
    /// minus the test-network knobs (`net_id`/`allow_local`/`insecure_tunnels`).
    async fn bootstrap(config: &crate::EmissaryConfig) -> std::result::Result<Self, String> {
        // 1. Build + start the embedded router. NTCP2 on an ephemeral port for
        //    router-to-router transport; SAMv3 on an ephemeral loopback port as
        //    the client surface. `Router` binds its listeners inside `new`, so
        //    the SAM address is known as soon as it returns.
        let router_config = Config {
            ntcp2: Some(Ntcp2Config {
                port: 0u16,
                ipv4_host: None,
                ipv6_host: None,
                ipv4: true,
                ipv6: false,
                publish: false,
                ml_kem: None,
                disable_pq: false,
                iv: {
                    let mut iv = [0u8; 16];
                    TokioRuntime::rng().fill_bytes(&mut iv);
                    iv
                },
                key: {
                    let mut key = [0u8; 32];
                    TokioRuntime::rng().fill_bytes(&mut key);
                    key
                },
            }),
            samv3_config: Some(SamConfig {
                tcp_port: 0u16,
                udp_port: 0u16,
                host: "127.0.0.1".to_string(),
            }),
            ..Default::default()
        };

        let (router, _events, _router_info) = Router::<TokioRuntime>::new(router_config, None, None)
            .await
            .map_err(|e| format!("router build failed: {e}"))?;
        let sam_tcp = router
            .protocol_address_info()
            .sam_tcp
            .ok_or_else(|| "router exposed no SAMv3 listener".to_string())?;
        // The `Router` value is the router's event loop; run it for as long as
        // this actor's runtime lives.
        tokio::spawn(router);

        // 2. Open a streaming session (persistent destination from state_dir).
        //    `Session::new` completes once the router has built tunnels for the
        //    session — this wait is the tunnel-readiness gate.
        let destination = load_or_generate_destination(&config.state_dir, sam_tcp.port()).await?;
        let mut session = Session::<style::Stream>::new(SessionOptions {
            nickname: config.session_label.clone(),
            samv3_tcp_port: sam_tcp.port(),
            destination,
            ..Default::default()
        })
        .await
        .map_err(|e| format!("streaming session failed: {e}"))?;

        // 3. Dial the peer destination.
        let stream = session
            .connect(&config.peer_destination)
            .await
            .map_err(|e| format!("cannot reach peer {}: {e}", config.peer_destination))?;

        Ok(Actor {
            _session: session,
            stream,
            rx: Vec::new(),
        })
    }

    /// Write one already-framed record onto the stream.
    async fn handle_send(&mut self, framed: Vec<u8>) -> Result<()> {
        self.stream.write_all(&framed).await.map_err(Error::from)?;
        self.stream.flush().await.map_err(Error::from)
    }

    /// Pull any currently-available bytes off the stream into `rx`, then return
    /// every complete framed record among them. A read that does not complete
    /// within the poll window means "nothing new right now" and is not an error
    /// — this is polling, matching the pull cadence transport-core expects.
    async fn handle_drain(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut buf = [0u8; 8192];
        match tokio::time::timeout(POLL_WINDOW, self.stream.read(&mut buf)).await {
            Ok(Ok(0)) => {}                                    // peer closed
            Ok(Ok(n)) => self.rx.extend_from_slice(&buf[..n]), // new bytes
            Ok(Err(e)) => return Err(Error::from(e)),          // read failed
            Err(_elapsed) => {}                                // nothing available
        }

        let mut out = Vec::new();
        while let Some(record) = deframe(&mut self.rx)? {
            out.push(record);
        }
        Ok(out)
    }

    /// Drain requests until every [`I2pStream`] handle is dropped (the channel
    /// closes). Runs on the actor's own runtime; the live I2P state stays
    /// confined here.
    async fn run(mut self, mut requests: mpsc::Receiver<Request>) {
        while let Some(request) = requests.recv().await {
            match request {
                Request::Send { framed, reply } => {
                    // A dropped receiver only means the caller went away; ignore.
                    let _ = reply.send(self.handle_send(framed).await);
                }
                Request::Drain { reply } => {
                    let _ = reply.send(self.handle_drain().await);
                }
            }
        }
        // Channel closed: fall out of the loop, dropping `self` (session,
        // stream) and — once the runtime winds down — the spawned router.
    }
}

/// A connected I2P stream backed by an embedded emissary-core router.
///
/// Holds only the request channel to the actor thread; the router, session, and
/// stream live on the actor's runtime. The async `send_framed`/`drain_framed`
/// talk to it over `mpsc` + `oneshot` — no `block_on`.
pub struct I2pStream {
    requests: mpsc::Sender<Request>,
    // The actor thread's join handle. Kept so the thread's lifetime is tied to
    // this handle; on drop the `requests` sender closes, the actor loop ends,
    // and the runtime (owned by that thread) winds down.
    _actor: std::thread::JoinHandle<()>,
}

impl I2pStream {
    /// Start the embedded router and connect a stream to the peer named in
    /// `config`. Synchronous: this is a constructor (called from the sync
    /// `EmissaryChannel::connect`), so it spawns the actor thread + runtime
    /// ONCE and waits (over a bootstrap channel) for setup to finish.
    pub fn connect(config: &crate::EmissaryConfig) -> Result<Self> {
        let config = config.clone();
        let (request_tx, request_rx) = mpsc::channel::<Request>(32);
        // Reports the bootstrap outcome (ready, or a setup error string) back
        // to this constructor so `connect` stays synchronous.
        let (boot_tx, boot_rx) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

        let actor = std::thread::Builder::new()
            .name("transport-emissary-actor".to_string())
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
                    match Actor::bootstrap(&config).await {
                        Ok(actor) => {
                            // Setup done: report readiness, then serve.
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
            .map_err(|error| {
                Error::new(format!("transport-emissary: spawning actor thread: {error}"))
            })?;

        // Wait for bootstrap to finish before returning a usable handle.
        boot_rx
            .recv()
            .map_err(|_| Error::new("transport-emissary: actor thread exited before bootstrap"))?
            .map_err(|message| Error::new(format!("transport-emissary: {message}")))?;

        Ok(I2pStream {
            requests: request_tx,
            _actor: actor,
        })
    }

    /// Write one message as a single length-prefixed frame onto the stream.
    /// Runs on the CALLER's runtime; the stream I/O runs on the actor's. No
    /// `block_on` — this is the async seam's actor-at-the-edge shape.
    pub async fn send_framed(&mut self, message: &[u8]) -> Result<()> {
        // transport-core buffer framing: u32 BE length prefix + value (enforces
        // the 16 MiB MAX_FRAME_LEN cap).
        let framed = frame(message);
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Send {
                framed,
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::new("transport-emissary send: actor thread is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("transport-emissary send: actor dropped the reply"))?
    }

    /// Read whatever bytes are currently available and return every complete
    /// framed record among them (bare bytes — no sender identity). Partial
    /// tails are retained for the next call. Returns an empty vec when nothing
    /// new has arrived (a timed-out read is not an error). Same actor
    /// round-trip as [`send_framed`](Self::send_framed); no `block_on`.
    pub async fn drain_framed(&mut self) -> Result<Vec<Vec<u8>>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Drain { reply: reply_tx })
            .await
            .map_err(|_| Error::new("transport-emissary recv: actor thread is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("transport-emissary recv: actor dropped the reply"))?
    }
}
