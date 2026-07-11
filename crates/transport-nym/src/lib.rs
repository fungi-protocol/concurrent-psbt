//! Nym mixnet transport for opaque collaboration messages.
//!
//! Nym delivers reconstructed messages without sender identity, so this crate
//! implements [`transport_core::AnonymousChannel`]. Each transport is
//! ephemeral and forwards the mixnet's native message payload unchanged.

#![warn(missing_docs)]

use std::time::Duration;

use async_trait::async_trait;
use nym_sdk::mixnet::{self, MixnetClient, MixnetMessageSender, Recipient};
use transport_core::{AnonymousChannel, Error, Result};

/// A peer's Nym recipient address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NymAddress(pub String);

impl NymAddress {
    /// Borrow the address as supplied by Nym.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An ephemeral Nym client broadcasting to a fixed set of recipients.
pub struct NymTransport {
    client: MixnetClient,
    recipients: Vec<Recipient>,
    our_address: NymAddress,
}

impl NymTransport {
    /// Parse recipients, create an ephemeral client, and connect to the mixnet.
    pub async fn connect(recipients: Vec<NymAddress>) -> Result<Self> {
        let recipients = recipients
            .into_iter()
            .map(|NymAddress(address)| {
                Recipient::try_from_base58_string(&address).map_err(|error| {
                    Error::new(format!("transport-nym: bad recipient address: {error}"))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let client = mixnet::MixnetClientBuilder::new_ephemeral()
            .build()
            .map_err(|error| Error::new(format!("transport-nym: building client: {error}")))?
            .connect_to_mixnet()
            .await
            .map_err(|error| Error::new(format!("transport-nym: connecting: {error}")))?;
        let our_address = NymAddress(client.nym_address().to_string());

        Ok(Self {
            client,
            recipients,
            our_address,
        })
    }

    /// Return the address peers use to send to this client.
    pub fn our_address(&self) -> &NymAddress {
        &self.our_address
    }
}

#[async_trait]
impl AnonymousChannel for NymTransport {
    async fn send(&mut self, message: Vec<u8>) -> Result<()> {
        for recipient in &self.recipients {
            self.client
                .send_plain_message(*recipient, &message)
                .await
                .map_err(|error| Error::new(format!("transport-nym: send: {error}")))?;
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut messages = Vec::new();
        while let Ok(Some(batch)) = tokio::time::timeout(
            Duration::from_millis(50),
            self.client.wait_for_messages(),
        )
        .await
        {
            messages.extend(batch.into_iter().map(|message| message.message));
        }
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport_core::Transport;

    fn assert_channel<T: AnonymousChannel + Transport>() {}

    #[test]
    fn transport_is_an_anonymous_channel() {
        assert_channel::<NymTransport>();
    }

    #[test]
    fn address_is_opaque() {
        let address = NymAddress("recipient.gateway".into());
        assert_eq!(address.as_str(), "recipient.gateway");
    }
}
