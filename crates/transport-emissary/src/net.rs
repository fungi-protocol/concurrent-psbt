//! Embedded I2P stream backend.
//!
//! An actor thread keeps the Emissary router, Yosemite session, and stream on
//! one Tokio runtime. Yosemite connects to the embedded router's loopback SAM
//! listener; channel calls cross the actor boundary with Tokio channels.

use std::path::Path;
use std::time::Duration;

use emissary_core::router::Router;
use emissary_core::runtime::Runtime as _;
use emissary_core::{Config, Ntcp2Config, SamConfig};
use emissary_util::runtime::tokio::Runtime as TokioRuntime;
use rand::Rng as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::{mpsc, oneshot};
use yosemite::{DestinationKind, RouterApi, Session, SessionOptions, style};

use transport_core::{Error, Result, deframe, frame};

const DESTINATION_KEY_FILE: &str = "destination.key";
const POLL_WINDOW: Duration = Duration::from_millis(50);

enum Request {
    Send {
        framed: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Drain {
        reply: oneshot::Sender<Result<Vec<Vec<u8>>>>,
    },
}

struct Actor {
    _session: Session<style::Stream>,
    stream: yosemite::Stream,
    rx: Vec<u8>,
}

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
        .map_err(|error| format!("generating destination: {error}"))?;
    std::fs::create_dir_all(state_dir)
        .map_err(|error| format!("creating state dir {state_dir}: {error}"))?;
    std::fs::write(&path, &private_key)
        .map_err(|error| format!("persisting destination key: {error}"))?;
    Ok(DestinationKind::Persistent { private_key })
}

impl Actor {
    async fn bootstrap(config: &crate::EmissaryConfig) -> std::result::Result<Self, String> {
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

        let (router, _events, _router_info) =
            Router::<TokioRuntime>::new(router_config, None, None)
                .await
                .map_err(|error| format!("router build failed: {error}"))?;
        let sam_tcp = router
            .protocol_address_info()
            .sam_tcp
            .ok_or_else(|| "router exposed no SAM listener".to_string())?;
        tokio::spawn(router);

        let destination = load_or_generate_destination(&config.state_dir, sam_tcp.port()).await?;
        let mut session = Session::<style::Stream>::new(SessionOptions {
            nickname: config.session_label.clone(),
            samv3_tcp_port: sam_tcp.port(),
            destination,
            ..Default::default()
        })
        .await
        .map_err(|error| format!("streaming session failed: {error}"))?;

        let stream = session
            .connect(&config.peer_destination)
            .await
            .map_err(|error| format!("cannot reach peer {}: {error}", config.peer_destination))?;

        Ok(Actor {
            _session: session,
            stream,
            rx: Vec::new(),
        })
    }

    async fn handle_send(&mut self, framed: Vec<u8>) -> Result<()> {
        self.stream.write_all(&framed).await.map_err(Error::from)?;
        self.stream.flush().await.map_err(Error::from)
    }

    async fn handle_drain(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut buf = [0u8; 8192];
        match tokio::time::timeout(POLL_WINDOW, self.stream.read(&mut buf)).await {
            Ok(Ok(0)) => {}
            Ok(Ok(n)) => self.rx.extend_from_slice(&buf[..n]),
            Ok(Err(error)) => return Err(Error::from(error)),
            Err(_elapsed) => {}
        }

        let mut out = Vec::new();
        while let Some(record) = deframe(&mut self.rx)? {
            out.push(record);
        }
        Ok(out)
    }

    async fn run(mut self, mut requests: mpsc::Receiver<Request>) {
        while let Some(request) = requests.recv().await {
            match request {
                Request::Send { framed, reply } => {
                    let _ = reply.send(self.handle_send(framed).await);
                }
                Request::Drain { reply } => {
                    let _ = reply.send(self.handle_drain().await);
                }
            }
        }
    }
}

/// Handle for a stream owned by the embedded-router actor.
pub struct I2pStream {
    requests: mpsc::Sender<Request>,
    _actor: std::thread::JoinHandle<()>,
}

impl I2pStream {
    /// Start the embedded router and connect an outbound stream.
    pub fn connect(config: &crate::EmissaryConfig) -> Result<Self> {
        let config = config.clone();
        let (request_tx, request_rx) = mpsc::channel::<Request>(32);
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
                            if boot_tx.send(Ok(())).is_err() {
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
                Error::new(format!(
                    "transport-emissary: spawning actor thread: {error}"
                ))
            })?;

        boot_rx
            .recv()
            .map_err(|_| Error::new("transport-emissary: actor exited during bootstrap"))?
            .map_err(|message| Error::new(format!("transport-emissary: {message}")))?;

        Ok(I2pStream {
            requests: request_tx,
            _actor: actor,
        })
    }

    /// Write one length-prefixed message to the stream.
    pub async fn send_framed(&mut self, message: &[u8]) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Send {
                framed: frame(message),
                reply: reply_tx,
            })
            .await
            .map_err(|_| Error::new("transport-emissary send: actor is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("transport-emissary send: actor dropped the reply"))?
    }

    /// Return every complete framed message currently available.
    pub async fn drain_framed(&mut self) -> Result<Vec<Vec<u8>>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.requests
            .send(Request::Drain { reply: reply_tx })
            .await
            .map_err(|_| Error::new("transport-emissary recv: actor is gone"))?;
        reply_rx
            .await
            .map_err(|_| Error::new("transport-emissary recv: actor dropped the reply"))?
    }
}
