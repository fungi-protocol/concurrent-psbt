//! Tor onion-service transport for anonymous set reconciliation.
//!
//! Each participant runs an in-process Arti client and onion service. Messages
//! are length-prefixed records sent to every configured onion endpoint. Received
//! records carry no sender identity, so the transport implements only
//! [`AnonymousChannel`]. The transport retains inbound records until the next
//! [`AnonymousChannel::recv`] call; reconciliation and deduplication belong to
//! the caller.

#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Result};

/// Configuration for an [`ArtiTransport`]: where to reach peers and where to
/// listen for them.
///
/// Onion endpoints are introduced out of band and parsed by Arti.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtiConfig {
    /// The peer onion endpoints to publish to, each `"<base32>.onion:<port>"`.
    /// Every send writes one framed record to each endpoint.
    pub peers: Vec<String>,
    /// The virtual port our own onion service exposes for inbound peer streams.
    /// Peers dial `<our-onion>.onion:<listen_port>` to reach us.
    pub listen_port: u16,
    /// Nickname for the onion service's persisted keys.
    /// Reusing the same nickname across runs reuses the same `.onion` address.
    pub service_nickname: String,
}

impl ArtiConfig {
    /// A config that publishes to `peers` and listens on `listen_port`.
    pub fn new(peers: Vec<String>, listen_port: u16, service_nickname: impl Into<String>) -> Self {
        Self {
            peers,
            listen_port,
            service_nickname: service_nickname.into(),
        }
    }
}

/// A Tor onion-service backed collaborative transport.
///
/// Implements [`AnonymousChannel`]: `send` writes one framed opaque record to
/// every configured peer over Tor; `recv` drains the records received by the
/// local onion service without attaching sender identities.
pub struct ArtiTransport {
    inner: Inner,
}

impl ArtiTransport {
    /// Build a transport from `config`.
    ///
    /// Bootstraps an in-process Tor client and launches the onion service.
    pub fn new(config: ArtiConfig) -> Result<Self> {
        Ok(Self {
            inner: Inner::new(config)?,
        })
    }

    /// The `.onion` address peers should dial to reach us, if available.
    ///
    /// Returns an error until the onion service has published its descriptor.
    pub fn onion_address(&self) -> Result<String> {
        self.inner.onion_address()
    }
}

#[async_trait]
impl AnonymousChannel for ArtiTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.inner.send(message).await
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        self.inner.recv()
    }
}

mod imp;
use imp::Inner;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use transport_core::{Transport, frame, read_frame, write_frame};

    #[test]
    fn arti_transport_is_an_anonymous_channel_and_transport() {
        fn assert_anonymous<T: AnonymousChannel>() {}
        assert_anonymous::<ArtiTransport>();

        fn assert_transport<T: Transport>() {}
        assert_transport::<ArtiTransport>();

        let _ctor: fn(ArtiConfig) -> Result<ArtiTransport> = ArtiTransport::new;
    }

    #[test]
    fn wire_framing_roundtrips_without_network() {
        let first = b"first-opaque-record".to_vec();
        let second = vec![0xABu8; 4096];

        let mut stream = Vec::new();
        write_frame(&mut stream, &first).unwrap();
        write_frame(&mut stream, &second).unwrap();

        let mut cursor = Cursor::new(stream.clone());
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(first.clone()));
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(second.clone()));
        assert_eq!(read_frame(&mut cursor).unwrap(), None);

        let via_buffer = frame(&first);
        let mut cur2 = Cursor::new(via_buffer);
        assert_eq!(read_frame(&mut cur2).unwrap(), Some(first));
    }
}
