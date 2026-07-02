//! ptj-fake-plugin — a minimal IN-MEMORY transport plugin speaking the full
//! transport-plugin-api protocol over its stdio. TEST INFRASTRUCTURE: the
//! plugin-host integration tests (tests/plugin_host.rs) spawn this binary to
//! exercise `PluginTransport`'s spawn/handshake/publish/collect paths end to
//! end over real child pipes, with no network. It doubles as the reference
//! implementation for plugin authors — a real plugin is this file with the
//! `Vec<Vec<u8>>` store replaced by an actual transport backend.
//!
//! The store is process-local, so a fake plugin only ever "collects" its own
//! publishes — exactly enough to prove the RPC path moves bytes faithfully.
//!
//! Test hooks, driven through the ordinary config passthrough (so the tests
//! also prove config REACHES the plugin):
//!
//!   * `fake-version=<n>`  — answer the handshake with protocol version `n`
//!     (lets tests trigger the host's version-mismatch error);
//!   * `fake-kind=attributable` — advertise + serve the attributable
//!     interface, sender id `b"fake-sender"` (exercises the host's
//!     sender-id-dropping arm);
//!   * `fake-refuse=<msg>` — refuse the handshake with the structured
//!     error path (`HandshakeResult.err`).

use std::cell::RefCell;
use std::rc::Rc;

use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use transport_plugin_api::PROTOCOL_VERSION;
use transport_plugin_api::capnp;
use transport_plugin_api::capnp_rpc::{self, RpcSystem, rpc_twoparty_capnp, twoparty};
use transport_plugin_api::transport_capnp::{attributable_transport, handshake, plugin, transport};

/// The in-memory "network": every publish lands here, every collect
/// snapshots it. Shared (Rc) between the bootstrap plugin and the transport
/// capability it hands out — single-threaded vat, so Rc/RefCell suffice
/// (capnp server methods receive `self: Rc<Self>`).
type Store = Rc<RefCell<Vec<Vec<u8>>>>;

struct FakePlugin {
    store: Store,
    /// The channel kind negotiated in the handshake; the capability getters
    /// only serve the negotiated kind (as a real plugin should).
    kind: Rc<RefCell<handshake::ChannelKind>>,
}

impl plugin::Server for FakePlugin {
    async fn handshake(
        self: Rc<Self>,
        params: plugin::HandshakeParams,
        mut results: plugin::HandshakeResults,
    ) -> Result<(), capnp::Error> {
        let hello = params.get()?.get_hello()?;
        let mut version = PROTOCOL_VERSION;
        let mut kind = handshake::ChannelKind::Anonymous;
        for entry in hello.get_config()?.iter() {
            let key = entry.get_key()?.to_str()?;
            let value = entry.get_value()?.to_str()?;
            match key {
                "fake-version" => {
                    version = value
                        .parse()
                        .map_err(|_| capnp::Error::failed(format!("bad fake-version '{value}'")))?;
                }
                "fake-kind" if value == "attributable" => {
                    kind = handshake::ChannelKind::Attributable;
                }
                "fake-refuse" => {
                    let mut refusal = results.get().init_result().init_err();
                    refusal.set_message(value);
                    return Ok(());
                }
                // Real config (peer addresses, ...) would be read here; the
                // fake ignores everything it does not recognize.
                _ => {}
            }
        }
        *self.kind.borrow_mut() = kind;
        let mut answer = results.get().init_result().init_ok();
        answer.set_protocol_version(version);
        answer.set_channel_kind(kind);
        Ok(())
    }

    async fn anonymous(
        self: Rc<Self>,
        _params: plugin::AnonymousParams,
        mut results: plugin::AnonymousResults,
    ) -> Result<(), capnp::Error> {
        if *self.kind.borrow() != handshake::ChannelKind::Anonymous {
            return Err(capnp::Error::failed(
                "fake plugin negotiated the attributable kind; request that interface".to_string(),
            ));
        }
        results
            .get()
            .set_transport(capnp_rpc::new_client(FakeTransport {
                store: self.store.clone(),
            }));
        Ok(())
    }

    async fn attributable(
        self: Rc<Self>,
        _params: plugin::AttributableParams,
        mut results: plugin::AttributableResults,
    ) -> Result<(), capnp::Error> {
        if *self.kind.borrow() != handshake::ChannelKind::Attributable {
            return Err(capnp::Error::failed(
                "fake plugin negotiated the anonymous kind; request that interface".to_string(),
            ));
        }
        results
            .get()
            .set_transport(capnp_rpc::new_client(FakeAttributableTransport {
                store: self.store.clone(),
            }));
        Ok(())
    }
}

struct FakeTransport {
    store: Store,
}

impl transport::Server for FakeTransport {
    async fn publish(
        self: Rc<Self>,
        params: transport::PublishParams,
        _results: transport::PublishResults,
    ) -> Result<(), capnp::Error> {
        let message = params.get()?.get_message()?.to_vec();
        self.store.borrow_mut().push(message);
        Ok(())
    }

    async fn collect(
        self: Rc<Self>,
        _params: transport::CollectParams,
        mut results: transport::CollectResults,
    ) -> Result<(), capnp::Error> {
        let store = self.store.borrow();
        let mut messages = results.get().init_messages(store.len() as u32);
        for (index, message) in store.iter().enumerate() {
            messages.set(index as u32, message);
        }
        Ok(())
    }
}

struct FakeAttributableTransport {
    store: Store,
}

impl attributable_transport::Server for FakeAttributableTransport {
    async fn publish(
        self: Rc<Self>,
        params: attributable_transport::PublishParams,
        _results: attributable_transport::PublishResults,
    ) -> Result<(), capnp::Error> {
        let message = params.get()?.get_message()?.to_vec();
        self.store.borrow_mut().push(message);
        Ok(())
    }

    async fn collect(
        self: Rc<Self>,
        _params: attributable_transport::CollectParams,
        mut results: attributable_transport::CollectResults,
    ) -> Result<(), capnp::Error> {
        let store = self.store.borrow();
        let mut messages = results.get().init_messages(store.len() as u32);
        for (index, message) in store.iter().enumerate() {
            let mut pair = messages.reborrow().get(index as u32);
            pair.set_sender_id(b"fake-sender");
            pair.set_message(message);
        }
        Ok(())
    }
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("building the fake plugin's current-thread runtime");
    let local = tokio::task::LocalSet::new();
    let result: Result<(), capnp::Error> = runtime.block_on(local.run_until(async move {
        // The plugin side of the wire: OUR stdio is the RPC stream the host
        // holds the other end of. Stderr stays free for diagnostics.
        let network = twoparty::VatNetwork::new(
            futures::io::BufReader::new(tokio::io::stdin().compat()),
            futures::io::BufWriter::new(tokio::io::stdout().compat_write()),
            rpc_twoparty_capnp::Side::Server,
            Default::default(),
        );
        let bootstrap: plugin::Client = capnp_rpc::new_client(FakePlugin {
            store: Rc::new(RefCell::new(Vec::new())),
            kind: Rc::new(RefCell::new(handshake::ChannelKind::Anonymous)),
        });
        RpcSystem::new(Box::new(network), Some(bootstrap.client)).await
    }));
    // The host closing our stdin ends the vat — that is the NORMAL shutdown
    // (it typically surfaces as a "premature EOF" error), not a failure.
    // Anything the host needed to know already traveled over the RPC stream.
    if let Err(error) = result {
        eprintln!("ptj-fake-plugin: vat ended: {error}");
    }
}
