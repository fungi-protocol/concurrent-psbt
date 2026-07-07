//! The out-of-process transport plugin HOST (`plugin-transports` feature).
//!
//! `PluginTransport` spawns a plugin binary, speaks Cap'n Proto twoparty RPC
//! over the child's stdin/stdout (the wire contract lives in
//! transport-plugin-api), and implements the async [`Transport`] seam by
//! forwarding `publish`/`collect` as RPC calls. Plugins exist for transport
//! stacks whose dependency trees cannot share the workspace Cargo.lock (see
//! contrib/design/transport-plugins.md); to the sync driver a plugin is just
//! another `Box<dyn Transport>`.
//!
//! # Threading shape (actor at the edge, again)
//!
//! capnp-rpc's `RpcSystem` is deliberately `!Send` (single-threaded vats),
//! while the [`Transport`] seam is `Send`. Same tension the iroh backend
//! resolves, same resolution: the vat lives on a DEDICATED actor thread with
//! its own current-thread tokio runtime + `LocalSet`; the `Send` handle the
//! driver holds sends requests over an mpsc channel and awaits oneshot
//! replies. No `block_on` inside the driver's runtime.
//!
//! # Lifecycle
//!
//! spawn child (stdio piped, `kill_on_drop`) -> bootstrap the `Plugin`
//! capability -> `handshake` (exact PROTOCOL_VERSION match; opaque config KV
//! passthrough; the plugin answers which channel kind it serves) -> request
//! the matching transport capability -> serve `publish`/`collect` until the
//! handle drops -> dropping the handle closes the request channel, the actor
//! ends, the vat (and the child's stdin) drops, the child exits — with
//! `kill_on_drop` as the backstop. Supervision beyond that backstop
//! (restart-on-crash, handshake deadlines killing a wedged child) is design
//! future work, deliberately not scaffolded here.
//!
//! An ATTRIBUTABLE plugin is driven through its own interface and its sender
//! ids are dropped on `collect`, mirroring `transport_core::Attributed` — the
//! driver seam only moves opaque bytes.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use transport_core::{Error, Result, Transport};
use transport_plugin_api::capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use transport_plugin_api::transport_capnp::{handshake, handshake_result, plugin};
use transport_plugin_api::{PROTOCOL_VERSION, capnp};

/// How long `spawn` waits for the child to complete the handshake before
/// giving up. Generous: a plugin doing real work (e.g. connecting to a
/// mixnet) performs that work lazily AFTER the handshake, not during it.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// One driver request forwarded to the actor thread; the oneshot carries the
/// RPC outcome back across the `Send` boundary.
enum Request {
    Publish(Vec<u8>, oneshot::Sender<Result<()>>),
    Collect(oneshot::Sender<Result<Vec<Vec<u8>>>>),
}

/// A spawned transport plugin, driven through the [`Transport`] seam.
///
/// The value the sync driver boxes: `Send`, async, opaque bytes. All RPC and
/// child-process state lives on the actor thread behind the request channel.
pub struct PluginTransport {
    /// `Some` until drop; taken there so the channel closes BEFORE the actor
    /// thread is joined (the loop only ends once the channel closes).
    requests: Option<mpsc::UnboundedSender<Request>>,
    actor: Option<std::thread::JoinHandle<()>>,
}

impl PluginTransport {
    /// Spawn `binary` as a transport plugin and complete the handshake.
    ///
    /// `config` is opaque key/value passthrough handed to the plugin in the
    /// handshake (peer addresses, credential paths, ... — ptj never
    /// interprets it). Blocks the calling (sync-side) thread until the
    /// handshake settles or [`HANDSHAKE_TIMEOUT`] elapses; `build_transport`
    /// always runs on the sync side of the runtime boundary, so blocking
    /// here is legal (and matches the str0m handshake precedent).
    ///
    /// # Errors
    ///
    /// Precise, stage-named failures: spawn (binary path + io error),
    /// handshake transport/protocol errors (a non-plugin binary typically
    /// yields premature EOF), an explicit plugin refusal (its structured
    /// `Error`), a protocol version mismatch (both versions named), or the
    /// handshake timeout.
    pub fn spawn(binary: &Path, config: Vec<(String, String)>) -> Result<Self> {
        let (requests_tx, requests_rx) = mpsc::unbounded_channel();
        // std mpsc, not tokio: the spawning side is sync and has no runtime.
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<()>>(1);
        // For error text after the PathBuf moves into the actor thread.
        let display = binary_display(binary);
        let binary = binary.to_path_buf();

        let actor = std::thread::Builder::new()
            .name("ptj-plugin-host".to_string())
            .spawn(move || actor_main(&binary, config, &ready_tx, requests_rx))
            .map_err(|error| Error::new(format!("spawning plugin host thread: {error}")))?;

        match ready_rx.recv_timeout(HANDSHAKE_TIMEOUT) {
            Ok(Ok(())) => Ok(PluginTransport {
                requests: Some(requests_tx),
                actor: Some(actor),
            }),
            Ok(Err(error)) => {
                // Setup failed; the actor is already winding down. Join it so
                // no thread outlives the error return.
                let _ = actor.join();
                Err(error)
            }
            Err(_) => {
                // The child neither completed nor failed the handshake in
                // time (a wedged binary). Dropping `requests_tx` lets the
                // actor end once its setup future settles; the child is
                // killed by `kill_on_drop`. We do NOT join here — the actor
                // may still be blocked on the wedged handshake.
                drop(requests_tx);
                Err(Error::new(format!(
                    "transport plugin {display} did not complete the handshake within {}s",
                    HANDSHAKE_TIMEOUT.as_secs()
                )))
            }
        }
    }
}

#[async_trait]
impl Transport for PluginTransport {
    async fn publish(&mut self, message: Vec<u8>) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_request(Request::Publish(message, reply_tx))?;
        reply_rx
            .await
            .map_err(|_| Error::new("plugin host actor dropped a publish reply (plugin gone?)"))?
    }

    async fn collect(&mut self) -> Result<Vec<Vec<u8>>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send_request(Request::Collect(reply_tx))?;
        reply_rx
            .await
            .map_err(|_| Error::new("plugin host actor dropped a collect reply (plugin gone?)"))?
    }
}

impl PluginTransport {
    fn send_request(&self, request: Request) -> Result<()> {
        self.requests
            .as_ref()
            .expect("request channel present until drop")
            .send(request)
            .map_err(|_| Error::new("plugin host actor is gone (plugin exited or crashed?)"))
    }
}

impl Drop for PluginTransport {
    fn drop(&mut self) {
        // Close the request channel first — that is what ends the actor loop
        // — then join so the vat/child teardown completes before we return.
        drop(self.requests.take());
        if let Some(actor) = self.actor.take() {
            let _ = actor.join();
        }
    }
}

/// The negotiated wire client: which capnp interface `publish`/`collect` are
/// forwarded to. The attributable arm drops sender ids on collect (the
/// driver seam moves opaque bytes; `transport_core::Attributed` precedent).
enum WireClient {
    Anonymous(transport_plugin_api::transport_capnp::transport::Client),
    Attributable(transport_plugin_api::transport_capnp::attributable_transport::Client),
}

/// Everything that runs on the dedicated actor thread: runtime + LocalSet +
/// vat + request loop. Infallible at this signature — failures are reported
/// through `ready_tx` (setup) or per-request oneshots (steady state).
fn actor_main(
    binary: &Path,
    config: Vec<(String, String)>,
    ready_tx: &std::sync::mpsc::SyncSender<Result<()>>,
    mut requests_rx: mpsc::UnboundedReceiver<Request>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready_tx.send(Err(Error::new(format!(
                "building plugin host runtime: {error}"
            ))));
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(async move {
        let client = match connect_and_handshake(binary, config).await {
            Ok(client) => {
                let _ = ready_tx.send(Ok(()));
                client
            }
            Err(error) => {
                let _ = ready_tx.send(Err(error));
                return;
            }
        };
        // Steady state: forward driver requests as RPC calls until the
        // handle drops (channel closes). Requests are served one at a time —
        // the sync driver is a single logical task, so there is nothing to
        // pipeline.
        while let Some(request) = requests_rx.recv().await {
            match request {
                Request::Publish(message, reply) => {
                    let _ = reply.send(rpc_publish(&client, &message).await);
                }
                Request::Collect(reply) => {
                    let _ = reply.send(rpc_collect(&client).await);
                }
            }
        }
        // Falling out of the loop drops the bootstrap client and the vat's
        // spawned RPC task with the LocalSet; the child sees EOF on stdin and
        // exits (kill_on_drop reaps it if it does not).
    }));
}

/// Spawn the child, wire the vat over its stdio, run the handshake, and
/// return the negotiated transport capability.
async fn connect_and_handshake(
    binary: &Path,
    config: Vec<(String, String)>,
) -> Result<WireClient> {
    let mut child = tokio::process::Command::new(binary)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // The plugin's stderr passes through to ptj's: plugin diagnostics
        // stay visible, and stderr is the one stream NOT carrying RPC.
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            Error::new(format!(
                "spawning transport plugin {}: {error}",
                binary_display(binary)
            ))
        })?;

    let child_stdout = child.stdout.take().expect("stdout was piped");
    let child_stdin = child.stdin.take().expect("stdin was piped");
    let network = twoparty::VatNetwork::new(
        futures::io::BufReader::new(child_stdout.compat()),
        futures::io::BufWriter::new(child_stdin.compat_write()),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    );
    let mut rpc_system = RpcSystem::new(Box::new(network), None);
    let plugin_client: plugin::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);
    // The vat's event loop. Its errors (disconnect) surface through the
    // per-call RPC results, so the task's own outcome is not interesting;
    // holding `child` in the task ties the process handle's lifetime (and
    // kill_on_drop) to the vat's.
    tokio::task::spawn_local(async move {
        let _ = rpc_system.await;
        drop(child);
    });

    let display = binary_display(binary);
    let mut request = plugin_client.handshake_request();
    {
        let mut hello = request.get().init_hello();
        hello.set_protocol_version(PROTOCOL_VERSION);
        // The kind the host PREFERS; the plugin's answer states what it
        // actually serves, and the host follows the answer.
        hello.set_channel_kind(handshake::ChannelKind::Anonymous);
        let mut entries = hello.init_config(u32::try_from(config.len()).map_err(|_| {
            Error::new("plugin config passthrough has more than u32::MAX entries")
        })?);
        for (index, (key, value)) in config.iter().enumerate() {
            let mut entry = entries.reborrow().get(index as u32);
            entry.set_key(key.as_str());
            entry.set_value(value.as_str());
        }
    }
    let response = request.send().promise.await.map_err(|error| {
        Error::new(format!(
            "plugin handshake with {display}: {error} (is this binary a ptj transport plugin?)"
        ))
    })?;
    let response = response
        .get()
        .and_then(|results| results.get_result())
        .map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?;

    let answer = match response
        .which()
        .map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?
    {
        handshake_result::Ok(answer) => {
            answer.map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?
        }
        handshake_result::Err(refusal) => {
            let message = refusal
                .and_then(|refusal| refusal.get_message())
                .map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?;
            let message = message
                .to_string()
                .map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?;
            return Err(Error::new(format!(
                "plugin {display} refused the handshake: {message}"
            )));
        }
    };

    let plugin_version = answer.get_protocol_version();
    if plugin_version != PROTOCOL_VERSION {
        return Err(Error::new(format!(
            "plugin {display} protocol version mismatch: host speaks {PROTOCOL_VERSION}, plugin \
             answered {plugin_version} (rebuild the plugin against the host's \
             transport-plugin-api)"
        )));
    }

    match answer
        .get_channel_kind()
        .map_err(|error| Error::new(format!("plugin handshake with {display}: {error}")))?
    {
        handshake::ChannelKind::Anonymous => {
            let response = plugin_client
                .anonymous_request()
                .send()
                .promise
                .await
                .map_err(|error| {
                    Error::new(format!("requesting {display}'s anonymous transport: {error}"))
                })?;
            let transport = response
                .get()
                .and_then(|results| results.get_transport())
                .map_err(|error| {
                    Error::new(format!("requesting {display}'s anonymous transport: {error}"))
                })?;
            Ok(WireClient::Anonymous(transport))
        }
        handshake::ChannelKind::Attributable => {
            let response = plugin_client
                .attributable_request()
                .send()
                .promise
                .await
                .map_err(|error| {
                    Error::new(format!(
                        "requesting {display}'s attributable transport: {error}"
                    ))
                })?;
            let transport = response
                .get()
                .and_then(|results| results.get_transport())
                .map_err(|error| {
                    Error::new(format!(
                        "requesting {display}'s attributable transport: {error}"
                    ))
                })?;
            Ok(WireClient::Attributable(transport))
        }
    }
}

/// Forward one `publish` over the negotiated interface.
async fn rpc_publish(client: &WireClient, message: &[u8]) -> Result<()> {
    match client {
        WireClient::Anonymous(client) => {
            let mut request = client.publish_request();
            request.get().set_message(message);
            request
                .send()
                .promise
                .await
                .map(|_| ())
                .map_err(|error| Error::new(format!("plugin publish: {error}")))
        }
        WireClient::Attributable(client) => {
            let mut request = client.publish_request();
            request.get().set_message(message);
            request
                .send()
                .promise
                .await
                .map(|_| ())
                .map_err(|error| Error::new(format!("plugin publish: {error}")))
        }
    }
}

/// Forward one `collect` over the negotiated interface. The attributable arm
/// drops the sender ids — the driver seam only moves opaque bytes (a caller
/// wanting attribution belongs on a richer seam, not this one).
async fn rpc_collect(client: &WireClient) -> Result<Vec<Vec<u8>>> {
    let rpc_error = |error: capnp::Error| Error::new(format!("plugin collect: {error}"));
    match client {
        WireClient::Anonymous(client) => {
            let response = client
                .collect_request()
                .send()
                .promise
                .await
                .map_err(rpc_error)?;
            let response = response.get().map_err(rpc_error)?;
            let list = response.get_messages().map_err(rpc_error)?;
            let mut messages = Vec::with_capacity(list.len() as usize);
            for message in list.iter() {
                messages.push(message.map_err(rpc_error)?.to_vec());
            }
            Ok(messages)
        }
        WireClient::Attributable(client) => {
            let response = client
                .collect_request()
                .send()
                .promise
                .await
                .map_err(rpc_error)?;
            let response = response.get().map_err(rpc_error)?;
            let list = response.get_messages().map_err(rpc_error)?;
            let mut messages = Vec::with_capacity(list.len() as usize);
            for pair in list.iter() {
                // senderId dropped: opaque-bytes seam (see fn doc).
                messages.push(pair.get_message().map_err(rpc_error)?.to_vec());
            }
            Ok(messages)
        }
    }
}

/// The plugin binary as shown in error text.
fn binary_display(binary: &Path) -> String {
    binary.display().to_string()
}

/// Split `key=value` config entries from the CLI (`--plugin-config`,
/// repeatable) into the handshake's KV pairs. The value may contain further
/// `=`s; only the first splits. A missing `=` is an error naming the entry.
pub fn parse_config_entries(entries: &[String]) -> Result<Vec<(String, String)>> {
    entries
        .iter()
        .map(|entry| {
            entry
                .split_once('=')
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .ok_or_else(|| {
                    Error::new(format!(
                        "--plugin-config entry '{entry}' is not of the form key=value"
                    ))
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_entries_split_on_first_equals() {
        let parsed = parse_config_entries(&[
            "nym-address=abc.def@gateway".to_string(),
            "note=a=b=c".to_string(),
        ])
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                ("nym-address".to_string(), "abc.def@gateway".to_string()),
                ("note".to_string(), "a=b=c".to_string()),
            ]
        );
    }

    #[test]
    fn config_entry_without_equals_is_a_precise_error() {
        let error = parse_config_entries(&["oops".to_string()]).unwrap_err();
        assert!(error.to_string().contains("'oops'"), "got: {error}");
        assert!(error.to_string().contains("key=value"), "got: {error}");
    }

    #[test]
    fn spawn_failure_names_the_binary() {
        // `PluginTransport` is not `Debug`, so match the error out rather
        // than `unwrap_err()` (the transport-nym skeleton test precedent).
        let Err(error) = PluginTransport::spawn(
            Path::new("/nonexistent/ptj-plugin-that-does-not-exist"),
            Vec::new(),
        ) else {
            panic!("spawning a nonexistent binary must fail");
        };
        assert!(
            error
                .to_string()
                .contains("/nonexistent/ptj-plugin-that-does-not-exist"),
            "got: {error}"
        );
        assert!(error.to_string().contains("spawning"), "got: {error}");
    }

    /// PluginTransport must satisfy the driver seam's bounds (it is boxed
    /// into `Box<dyn Transport>`, which moves across the driver's threads).
    #[test]
    fn plugin_transport_is_a_send_transport() {
        fn assert_send_transport<T: Transport + Send>() {}
        assert_send_transport::<PluginTransport>();
    }
}
