//! Anonymous set reconciliation over an embedded I2P router.
//!
//! [`EmissaryChannel`] exchanges framed messages over an outbound Yosemite
//! stream connected through the in-process Emissary router. Received messages
//! carry no sender identity, so the channel implements only
//! [`transport_core::AnonymousChannel`].

#![warn(missing_docs)]

use async_trait::async_trait;
use transport_core::{AnonymousChannel, Result};

mod net;

/// Configuration for an embedded router and its outbound peer stream.
#[derive(Debug, Clone)]
pub struct EmissaryConfig {
    /// Directory containing the persistent local I2P destination key.
    pub state_dir: String,

    /// Peer I2P destination to open a stream to.
    pub peer_destination: String,

    /// A locally-unique label for our streaming session on the embedded router.
    pub session_label: String,
}

impl EmissaryConfig {
    /// Build a config from the router state directory and a peer destination,
    /// using a default session label.
    pub fn new(state_dir: impl Into<String>, peer_destination: impl Into<String>) -> Self {
        EmissaryConfig {
            state_dir: state_dir.into(),
            peer_destination: peer_destination.into(),
            session_label: "transport-emissary".to_string(),
        }
    }

    /// Override the streaming-session label.
    pub fn with_session_label(mut self, session_label: impl Into<String>) -> Self {
        self.session_label = session_label.into();
        self
    }
}

/// An anonymous framed stream through an embedded I2P router.
pub struct EmissaryChannel {
    stream: net::I2pStream,
}

impl EmissaryChannel {
    /// Start the embedded router and open a stream to the configured peer.
    pub fn connect(config: &EmissaryConfig) -> Result<Self> {
        let stream = net::I2pStream::connect(config)?;
        Ok(EmissaryChannel { stream })
    }
}

#[async_trait]
impl AnonymousChannel for EmissaryChannel {
    /// Write one opaque message as a framed stream record.
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        self.stream.send_framed(&message).await
    }

    /// Return every complete framed record received since the last call.
    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        self.stream.drain_framed().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::{Message, Transport, deframe, frame};

    fn _assert_is_anonymous_channel<C: AnonymousChannel>() {}

    fn _assert_is_transport<C: Transport>() {}

    #[test]
    fn satisfies_the_channel_and_transport_traits() {
        _assert_is_anonymous_channel::<EmissaryChannel>();
        _assert_is_transport::<EmissaryChannel>();
    }

    #[test]
    fn framing_roundtrip_over_an_in_memory_stream() {
        let outgoing = [
            Message::Psbt(b"cHNidP8BAgQC".to_vec()),
            Message::Payment(vec![0xAB; 32]),
            Message::Confirmation(vec![0xCD; 65]),
        ];
        let mut wire = Vec::new();
        for msg in &outgoing {
            wire.extend_from_slice(&frame(&msg.encode()));
        }

        let mut buf = wire;
        let mut received = Vec::new();
        while let Some(record) = deframe(&mut buf).unwrap() {
            received.push(Message::decode(&record).unwrap());
        }
        assert!(buf.is_empty(), "all records consumed, no trailing bytes");
        assert_eq!(received, outgoing);
    }

    #[test]
    fn framing_handles_partial_records() {
        let full = frame(&Message::Payment(vec![0x11; 10]).encode());
        let split = full.len() - 3;

        let mut buf = full[..split].to_vec();
        assert_eq!(deframe(&mut buf).unwrap(), None, "record not yet complete");

        buf.extend_from_slice(&full[split..]);
        let record = deframe(&mut buf).unwrap().expect("now complete");
        assert_eq!(
            Message::decode(&record).unwrap(),
            Message::Payment(vec![0x11; 10])
        );
        assert!(buf.is_empty());
    }
}
