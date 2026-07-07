//! ptj-transport-nym — the nym mixnet transport as an out-of-process ptj
//! plugin: serve the transport-plugin-api Cap'n Proto protocol over stdio,
//! backed by the nym-sdk mixnet client (feature `nym`; a protocol-complete
//! skeleton otherwise, mirroring the in-workspace transport crates'
//! convention).
//!
//! Spawned by `ptj sync --transport plugin --plugin ptj-transport-nym \
//!   --plugin-config recipient=<peer-address> ...`.
//!
//! # Handshake contract served here
//!
//! * protocol version: answered with OUR transport-plugin-api
//!   PROTOCOL_VERSION; a hello with a different version is refused via the
//!   structured error path (the host also enforces equality on its side).
//! * channel kind: anonymous (the mixnet delivers bare bytes, no sender
//!   identity — same classification as the in-workspace transport-nym).
//! * config: `recipient=<address>` entries (REPEATABLE — the KV list allows
//!   duplicate keys) name the peers each publish fans out to. Unknown keys
//!   are ignored.
//!
//! # Deferred connect
//!
//! The mixnet client is built lazily on the FIRST publish/collect, not
//! during the handshake: the handshake must answer promptly, and connecting
//! to a gateway takes seconds. The vat serves requests one at a time (the
//! host serializes them), so lazy initialization needs no reentrancy guard
//! beyond the RefCell. With the `nym` feature OFF the first use reports
//! "built without `nym`" instead — the protocol path stays fully testable
//! network-free.

use std::cell::RefCell;
use std::rc::Rc;

use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use transport_plugin_api::PROTOCOL_VERSION;
use transport_plugin_api::capnp;
use transport_plugin_api::capnp_rpc::{self, RpcSystem, rpc_twoparty_capnp, twoparty};
use transport_plugin_api::transport_capnp::{handshake, plugin, transport};

#[cfg(feature = "nym")]
mod nym;

/// Everything the transport capability needs, shared with the bootstrap
/// plugin (single-threaded vat: Rc/RefCell suffice).
struct State {
    /// Peer addresses from the handshake's `recipient` entries.
    recipients: Vec<String>,
    /// The lazily-connected mixnet backend (`None` until first use).
    #[cfg(feature = "nym")]
    backend: Option<nym::Backend>,
}

impl State {
    /// Connect on first use (see the module doc).
    #[cfg(feature = "nym")]
    async fn backend(&mut self) -> Result<&mut nym::Backend, capnp::Error> {
        if self.backend.is_none() {
            let backend = nym::Backend::connect(&self.recipients)
                .await
                .map_err(|error| capnp::Error::failed(format!("ptj-transport-nym: {error}")))?;
            self.backend = Some(backend);
        }
        Ok(self.backend.as_mut().expect("just initialized"))
    }
}

/// The error every transport operation reports when built without `nym` —
/// same wording convention as the in-workspace skeleton crates.
#[cfg(not(feature = "nym"))]
const BUILT_WITHOUT_NYM: &str =
    "ptj-transport-nym was built without nym support; rebuild the plugin with --features nym";

struct NymPlugin {
    state: Rc<RefCell<State>>,
}

impl plugin::Server for NymPlugin {
    async fn handshake(
        self: Rc<Self>,
        params: plugin::HandshakeParams,
        mut results: plugin::HandshakeResults,
    ) -> Result<(), capnp::Error> {
        let hello = params.get()?.get_hello()?;
        let host_version = hello.get_protocol_version();
        if host_version != PROTOCOL_VERSION {
            let mut refusal = results.get().init_result().init_err();
            refusal.set_message(
                format!(
                    "unsupported protocol version: this plugin speaks {PROTOCOL_VERSION}, \
                     the host sent {host_version}"
                )
                .as_str(),
            );
            return Ok(());
        }
        let mut recipients = Vec::new();
        for entry in hello.get_config()?.iter() {
            // `recipient` is repeatable (the KV list allows duplicate keys),
            // one peer address each; unknown keys are ignored (forward
            // compatibility).
            if entry.get_key()?.to_str()? == "recipient" {
                recipients.push(entry.get_value()?.to_str()?.to_string());
            }
        }
        self.state.borrow_mut().recipients = recipients;
        let mut answer = results.get().init_result().init_ok();
        answer.set_protocol_version(PROTOCOL_VERSION);
        answer.set_channel_kind(handshake::ChannelKind::Anonymous);
        Ok(())
    }

    async fn anonymous(
        self: Rc<Self>,
        _params: plugin::AnonymousParams,
        mut results: plugin::AnonymousResults,
    ) -> Result<(), capnp::Error> {
        results.get().set_transport(capnp_rpc::new_client(NymTransport {
            state: self.state.clone(),
        }));
        Ok(())
    }

    async fn attributable(
        self: Rc<Self>,
        _params: plugin::AttributableParams,
        _results: plugin::AttributableResults,
    ) -> Result<(), capnp::Error> {
        Err(capnp::Error::failed(
            "ptj-transport-nym is an anonymous transport (the mixnet delivers bare bytes); \
             request the anonymous interface"
                .to_string(),
        ))
    }
}

struct NymTransport {
    // Only the feature-on publish/collect read the state; the feature-off
    // skeleton methods answer without it (the transport-nym cfg_attr
    // precedent).
    #[cfg_attr(not(feature = "nym"), allow(dead_code))]
    state: Rc<RefCell<State>>,
}

impl transport::Server for NymTransport {
    #[cfg(feature = "nym")]
    async fn publish(
        self: Rc<Self>,
        params: transport::PublishParams,
        _results: transport::PublishResults,
    ) -> Result<(), capnp::Error> {
        let message = params.get()?.get_message()?.to_vec();
        // One borrow across the awaits is fine: the vat serves one request
        // at a time (the host serializes them), so no reentrant borrow can
        // occur.
        let mut state = self.state.borrow_mut();
        let backend = state.backend().await?;
        backend
            .publish(&message)
            .await
            .map_err(|error| capnp::Error::failed(format!("ptj-transport-nym: {error}")))
    }

    #[cfg(feature = "nym")]
    async fn collect(
        self: Rc<Self>,
        _params: transport::CollectParams,
        mut results: transport::CollectResults,
    ) -> Result<(), capnp::Error> {
        let mut state = self.state.borrow_mut();
        let backend = state.backend().await?;
        let collected = backend.collect().await;
        let mut messages = results.get().init_messages(collected.len() as u32);
        for (index, message) in collected.iter().enumerate() {
            messages.set(index as u32, message);
        }
        Ok(())
    }

    #[cfg(not(feature = "nym"))]
    async fn publish(
        self: Rc<Self>,
        _params: transport::PublishParams,
        _results: transport::PublishResults,
    ) -> Result<(), capnp::Error> {
        Err(capnp::Error::failed(BUILT_WITHOUT_NYM.to_string()))
    }

    #[cfg(not(feature = "nym"))]
    async fn collect(
        self: Rc<Self>,
        _params: transport::CollectParams,
        _results: transport::CollectResults,
    ) -> Result<(), capnp::Error> {
        Err(capnp::Error::failed(BUILT_WITHOUT_NYM.to_string()))
    }
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("building the plugin's current-thread runtime");
    let local = tokio::task::LocalSet::new();
    let result: Result<(), capnp::Error> = runtime.block_on(local.run_until(async move {
        // The plugin side of the wire: OUR stdio is the RPC stream the host
        // holds the other end of. Stderr stays free for diagnostics (and the
        // our-address announcement).
        let network = twoparty::VatNetwork::new(
            futures::io::BufReader::new(tokio::io::stdin().compat()),
            futures::io::BufWriter::new(tokio::io::stdout().compat_write()),
            rpc_twoparty_capnp::Side::Server,
            Default::default(),
        );
        let bootstrap: plugin::Client = capnp_rpc::new_client(NymPlugin {
            state: Rc::new(RefCell::new(State {
                recipients: Vec::new(),
                #[cfg(feature = "nym")]
                backend: None,
            })),
        });
        RpcSystem::new(Box::new(network), Some(bootstrap.client)).await
    }));
    // The host closing our stdin ends the vat — the NORMAL shutdown (it
    // typically surfaces as a "premature EOF" error), not a failure.
    if let Err(error) = result {
        eprintln!("ptj-transport-nym: vat ended: {error}");
    }
}
