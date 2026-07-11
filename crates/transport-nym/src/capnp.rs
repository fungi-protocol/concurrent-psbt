//! Cap'n Proto adapter for the Nym transport plugin protocol.

use std::cell::RefCell;
use std::rc::Rc;

use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use transport_core::AnonymousChannel as _;
use transport_plugin_api::PROTOCOL_VERSION;
use transport_plugin_api::capnp as capnp_runtime;
use transport_plugin_api::capnp_rpc::{self, RpcSystem, rpc_twoparty_capnp, twoparty};
use transport_plugin_api::transport_capnp::{handshake, plugin, transport};

use crate::{NymAddress, NymTransport};

struct State {
    recipients: Vec<NymAddress>,
    backend: Option<NymTransport>,
}

async fn take_backend(state: &Rc<RefCell<State>>) -> Result<NymTransport, capnp_runtime::Error> {
    if let Some(backend) = state.borrow_mut().backend.take() {
        return Ok(backend);
    }
    let recipients = state.borrow().recipients.clone();
    NymTransport::connect(recipients)
        .await
        .map_err(|error| capnp_runtime::Error::failed(error.to_string()))
}

struct NymPlugin {
    state: Rc<RefCell<State>>,
}

impl plugin::Server for NymPlugin {
    async fn handshake(
        self: Rc<Self>,
        params: plugin::HandshakeParams,
        mut results: plugin::HandshakeResults,
    ) -> Result<(), capnp_runtime::Error> {
        let hello = params.get()?.get_hello()?;
        let host_version = hello.get_protocol_version();
        if host_version != PROTOCOL_VERSION {
            let mut refusal = results.get().init_result().init_err();
            refusal.set_message(
                format!(
                    "unsupported protocol version: this plugin speaks {PROTOCOL_VERSION}, the host sent {host_version}"
                )
                .as_str(),
            );
            return Ok(());
        }

        let mut recipients = Vec::new();
        for entry in hello.get_config()?.iter() {
            if entry.get_key()?.to_str()? == "recipient" {
                recipients.push(NymAddress(entry.get_value()?.to_str()?.to_string()));
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
    ) -> Result<(), capnp_runtime::Error> {
        results
            .get()
            .set_transport(capnp_rpc::new_client(NymRpcTransport {
                state: self.state.clone(),
            }));
        Ok(())
    }

    async fn attributable(
        self: Rc<Self>,
        _params: plugin::AttributableParams,
        _results: plugin::AttributableResults,
    ) -> Result<(), capnp_runtime::Error> {
        Err(capnp_runtime::Error::failed(
            "transport-nym provides anonymous messages".to_string(),
        ))
    }
}

struct NymRpcTransport {
    state: Rc<RefCell<State>>,
}

impl transport::Server for NymRpcTransport {
    async fn publish(
        self: Rc<Self>,
        params: transport::PublishParams,
        _results: transport::PublishResults,
    ) -> Result<(), capnp_runtime::Error> {
        let message = params.get()?.get_message()?.to_vec();
        let mut backend = take_backend(&self.state).await?;
        let result = backend.send(message).await;
        self.state.borrow_mut().backend = Some(backend);
        result.map_err(|error| capnp_runtime::Error::failed(error.to_string()))
    }

    async fn collect(
        self: Rc<Self>,
        _params: transport::CollectParams,
        mut results: transport::CollectResults,
    ) -> Result<(), capnp_runtime::Error> {
        let mut backend = take_backend(&self.state).await?;
        let collected = backend.recv().await;
        self.state.borrow_mut().backend = Some(backend);
        let collected =
            collected.map_err(|error| capnp_runtime::Error::failed(error.to_string()))?;

        let mut messages = results.get().init_messages(collected.len() as u32);
        for (index, message) in collected.iter().enumerate() {
            messages.set(index as u32, message);
        }
        Ok(())
    }
}

/// Serve the Nym plugin protocol over stdin and stdout until the host exits.
pub fn serve_stdio() -> Result<(), capnp_runtime::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| capnp_runtime::Error::failed(error.to_string()))?;
    let local = tokio::task::LocalSet::new();
    runtime.block_on(local.run_until(async move {
        let network = twoparty::VatNetwork::new(
            futures::io::BufReader::new(tokio::io::stdin().compat()),
            futures::io::BufWriter::new(tokio::io::stdout().compat_write()),
            rpc_twoparty_capnp::Side::Server,
            Default::default(),
        );
        let bootstrap: plugin::Client = capnp_rpc::new_client(NymPlugin {
            state: Rc::new(RefCell::new(State {
                recipients: Vec::new(),
                backend: None,
            })),
        });
        RpcSystem::new(Box::new(network), Some(bootstrap.client)).await
    }))
}
