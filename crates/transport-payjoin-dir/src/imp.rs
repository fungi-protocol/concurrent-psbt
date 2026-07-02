//! The real payjoin-directory-over-OHTTP backend — compiled only with the
//! `payjoin-dir` feature.
//!
//! DEFERRED / authored-but-unverified: the deps this module uses (rust-payjoin
//! v2 / BIP-77 `DirectoryLinkedMailbox` client, `ohttp`, `bhttp`, and a
//! wasm-capable http client) are ABSENT from the workspace lock today, so this
//! module is NEVER compiled by the default build. It is written against the
//! known public surfaces of those crates; the exact method names must be
//! re-verified against pinned versions at ground-deps time (see the crate
//! `Cargo.toml` TODO(ground-deps)). Nothing here is exercised until the feature
//! is turned on with real deps wired in.
//!
//! # What OHTTP buys us (the whole reason this crate exists)
//!
//! Each directory request is OHTTP-encapsulated: the HTTP request to the
//! directory is binary-encoded (`bhttp`), HPKE-sealed to the directory's OHTTP
//! GATEWAY key (`ohttp::ClientRequest::encapsulate`), and POSTed to a SEPARATE
//! OHTTP RELAY host, which forwards the opaque ciphertext to the gateway. So:
//!
//!   * the RELAY sees our client IP but NOT the plaintext request or which
//!     directory subdirectory (slot) it targets;
//!   * the DIRECTORY sees the slot + ciphertext but NOT our client IP (the relay
//!     is the visible client);
//!   * neither a localhost/direct signaling server nor the PWA origin is ever
//!     contacted, so none of them learns the client IP.
//!
//! The slot payloads themselves are additionally E2E-encrypted between the two
//! peers at the payjoin-v2 layer, so the directory cannot read them either. This
//! crate treats that E2E ciphertext as its opaque `Vec<u8>` message body — it
//! only adds the transport-core frame around it.
//!
//! # Sync bridge + wasm
//!
//! The [`AnonymousChannel`] contract is synchronous. Native uses an owned
//! runtime + `block_on` (as arti/nym/emissary do). In the PWA there is no
//! blocking: the browser fetch is async and the whole PWA drives the sync loop
//! from an async task, so the wasm build injects a fetch-backed [`HttpSender`]
//! and the channel's poll cadence is pumped from JS. The [`HttpSender`] trait is
//! the seam that keeps ONE backend across native (reqwest) and wasm (fetch),
//! rather than forking the backend by target.

use std::sync::{Arc, Mutex};

// NOTE: these imports are illustrative of the intended dependency surface and
// are only compiled with the (deferred) `payjoin-dir` feature + real deps wired.
use ohttp::{ClientRequest, ClientResponse};
use payjoin::v2::directory::DirectoryLinkedMailbox; // BIP-77 mailbox client

use transport_core::{Error, MAX_FRAME_LEN, Result};

use crate::PayjoinDirConfig;
use crate::mailbox::{self, Role, SlotId};

/// An abstract HTTP sender so ONE backend serves native and wasm. Native wires a
/// reqwest-backed impl; the PWA wires a `web-sys`/`gloo-net` fetch-backed impl.
/// It transports OHTTP CIPHERTEXT to the relay and returns the relay's response
/// ciphertext — it never sees plaintext, so it is trivially the same on both
/// targets.
pub trait HttpSender: Send {
    /// POST `encapsulated` (OHTTP ciphertext) to `relay_url` and return the
    /// relay's response body (the gateway's OHTTP response ciphertext).
    fn post(&self, relay_url: &str, encapsulated: Vec<u8>) -> Result<Vec<u8>>;
}

/// The live payjoin-directory-over-OHTTP backend.
pub struct Inner {
    config: PayjoinDirConfig,
    /// The rust-payjoin v2 mailbox client bound to `directory_url` + the derived
    /// slot IDs. It owns the peer-to-peer E2E encryption of slot payloads.
    mailbox: DirectoryLinkedMailbox,
    /// The OHTTP key config for the directory gateway, parsed from
    /// `config.ohttp_keys`, used to seal every request.
    ohttp_keys: ohttp::KeyConfig,
    /// The http sender (reqwest on native, fetch on wasm).
    http: Box<dyn HttpSender>,
    /// Next index to WRITE on our own lane (walks forward on HTTP 409).
    write_index: u64,
    /// Next index to READ on the peer's lane (advances on each hit).
    read_index: u64,
    /// Records received from the peer's lane, buffered for the polling `recv`.
    inbound: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Inner {
    /// Prepare the mailbox client and OHTTP context. Does NOT block on a peer —
    /// the directory is store-and-forward.
    ///
    /// This is the native constructor (owns a reqwest sender). The PWA calls
    /// [`Inner::open_with_sender`] to inject a fetch-backed [`HttpSender`].
    pub fn open(config: PayjoinDirConfig) -> Result<Self> {
        // Native default sender. In the PWA build, callers use open_with_sender.
        let http: Box<dyn HttpSender> = Box::new(native_reqwest_sender());
        Self::open_with_sender(config, http)
    }

    /// Prepare the mailbox with an injected [`HttpSender`] (the wasm/PWA path).
    pub fn open_with_sender(config: PayjoinDirConfig, http: Box<dyn HttpSender>) -> Result<Self> {
        mailbox::validate_session_secret(&config.session_secret)?;

        let ohttp_keys = ohttp::KeyConfig::decode(&config.ohttp_keys)
            .map_err(|e| Error::new(format!("payjoin-dir: parsing ohttp keys: {e}")))?;

        let mailbox = DirectoryLinkedMailbox::new(&config.directory_url)
            .map_err(|e| Error::new(format!("payjoin-dir: building mailbox client: {e}")))?;

        Ok(Self {
            config,
            mailbox,
            ohttp_keys,
            http,
            write_index: 0,
            read_index: 0,
            inbound: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// POST one framed opaque record to our next write slot, walking the index
    /// forward on an HTTP 409 collision (someone already wrote that slot). Every
    /// request is OHTTP-encapsulated through the relay.
    pub fn send(&mut self, message: Vec<u8>) -> Result<()> {
        if message.len() > MAX_FRAME_LEN {
            return Err(Error::new(format!(
                "payjoin-dir send: message length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
                message.len()
            )));
        }
        // One length-prefixed record per slot (see crate::wrap_outgoing).
        let payload = crate::wrap_outgoing(&message);

        // Walk our own lane forward until the POST succeeds (409 => slot taken).
        loop {
            let slot = mailbox::slot_id(
                &self.config.session_secret,
                self.config.role,
                self.write_index,
            );
            match self.post_slot(&slot, &payload)? {
                PostOutcome::Stored => {
                    self.write_index += 1;
                    return Ok(());
                }
                PostOutcome::Collision => {
                    // Someone (a retry, or a concurrent writer) already used this
                    // slot; advance and retry. Bounded by the directory's own
                    // slot horizon in practice.
                    self.write_index += 1;
                }
            }
        }
    }

    /// GET-poll the peer's lane, advancing the read index on each hit, and
    /// return a fresh snapshot of the newly-arrived records as bare bytes. Empty
    /// slot => stop (nothing more available this poll).
    pub fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        let peer_role: Role = self.config.role.peer();
        loop {
            let slot = mailbox::slot_id(&self.config.session_secret, peer_role, self.read_index);
            match self.get_slot(&slot)? {
                Some(slot_payload) => {
                    let record = crate::unwrap_incoming(&slot_payload)?;
                    self.inbound
                        .lock()
                        .expect("inbound mutex not poisoned")
                        .push(record);
                    self.read_index += 1;
                }
                None => break, // empty slot: nothing more this poll
            }
        }
        // Snapshot every record buffered so far (self-absorption is fine — the
        // lattice join is idempotent/commutative/associative). We keep the full
        // buffer so repeated polls return a consistent growing snapshot.
        Ok(self
            .inbound
            .lock()
            .expect("inbound mutex not poisoned")
            .clone())
    }

    /// OHTTP-encapsulate a directory POST to `slot` and send it through the
    /// relay. Returns whether the directory stored it or reported a collision.
    fn post_slot(&self, slot: &SlotId, payload: &[u8]) -> Result<PostOutcome> {
        // Build the inner directory HTTP request (payjoin v2 binds the slot +
        // does the peer-to-peer E2E encryption of `payload`).
        let inner_request = self
            .mailbox
            .post_request(slot.as_bytes(), payload)
            .map_err(|e| Error::new(format!("payjoin-dir: building post request: {e}")))?;

        let response_ct = self.ohttp_roundtrip(inner_request.as_bytes())?;
        match self.mailbox.process_post_response(&response_ct) {
            Ok(()) => Ok(PostOutcome::Stored),
            Err(e) if is_slot_collision(&e) => Ok(PostOutcome::Collision),
            Err(e) => Err(Error::new(format!("payjoin-dir: post response: {e}"))),
        }
    }

    /// OHTTP-encapsulate a directory GET for `slot`; return the stored slot
    /// payload if present, else `None` (empty slot).
    fn get_slot(&self, slot: &SlotId) -> Result<Option<Vec<u8>>> {
        let inner_request = self
            .mailbox
            .get_request(slot.as_bytes())
            .map_err(|e| Error::new(format!("payjoin-dir: building get request: {e}")))?;

        let response_ct = self.ohttp_roundtrip(inner_request.as_bytes())?;
        self.mailbox
            .process_get_response(&response_ct)
            .map_err(|e| Error::new(format!("payjoin-dir: get response: {e}")))
    }

    /// Seal an inner directory HTTP request with OHTTP, POST the ciphertext to
    /// the relay, and decapsulate the relay/gateway response ciphertext.
    ///
    /// This is where the metadata privacy lives: the relay sees only ciphertext
    /// + our IP; the gateway/directory sees only ciphertext (relay is the
    /// visible client). See the module docs.
    fn ohttp_roundtrip(&self, inner_request_bytes: &[u8]) -> Result<Vec<u8>> {
        // Encapsulate the (already bhttp-encoded) request to the gateway key.
        let (encapsulated, response_ctx): (Vec<u8>, ClientResponse) =
            ClientRequest::from_config(&self.ohttp_keys)
                .map_err(|e| Error::new(format!("payjoin-dir: ohttp client: {e}")))?
                .encapsulate(inner_request_bytes)
                .map_err(|e| Error::new(format!("payjoin-dir: ohttp encapsulate: {e}")))?;

        // POST the opaque ciphertext to the SEPARATE relay host.
        let response_ct = self.http.post(&self.config.ohttp_relay_url, encapsulated)?;

        // Decapsulate the gateway's OHTTP response.
        response_ctx
            .decapsulate(&response_ct)
            .map_err(|e| Error::new(format!("payjoin-dir: ohttp decapsulate: {e}")))
    }
}

enum PostOutcome {
    Stored,
    Collision,
}

/// True if a mailbox error corresponds to an HTTP 409 (slot already written).
/// Exact predicate to be pinned against the real payjoin error type at
/// ground-deps time.
fn is_slot_collision<E: std::fmt::Display>(err: &E) -> bool {
    // Placeholder heuristic; replace with a typed match on the payjoin error
    // (HTTP 409 / ConflictingSlot) once the dep is ground.
    err.to_string().contains("409") || err.to_string().to_lowercase().contains("conflict")
}

/// Native reqwest-backed [`HttpSender`]. Placeholder constructor: the real
/// impl is wired at ground-deps time. Present so `open` type-checks in the
/// feature-on skeleton of this authored-but-unverified backend.
fn native_reqwest_sender() -> impl HttpSender {
    struct ReqwestSender;
    impl HttpSender for ReqwestSender {
        fn post(&self, _relay_url: &str, _encapsulated: Vec<u8>) -> Result<Vec<u8>> {
            // TODO(ground-deps): reqwest::blocking POST with
            // content-type: message/ohttp-req, return the body bytes.
            Err(Error::new(
                "payjoin-dir: native reqwest HttpSender not yet wired (ground-deps)",
            ))
        }
    }
    ReqwestSender
}
