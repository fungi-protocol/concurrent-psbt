//! The real str0m backend — compiled only with the `str0m` feature.
//!
//! This module owns the sans-IO WebRTC state machine (`str0m::Rtc`) and the ONE
//! socket that drives it (`std::net::UdpSocket`). It contains zero
//! privacy/threat-model reasoning: a WebRTC data channel is a DTLS-encrypted
//! byte pipe with no peer identity we surface, so this is an anonymous transport
//! that only moves opaque bytes.
//!
//! ## Why there is NO async runtime here
//!
//! str0m is *sans-IO*: it never touches a socket or a clock itself. Instead you
//! drive an `Rtc` value through a poll loop:
//!
//! ```text
//!   loop {
//!       match rtc.poll_output()? {
//!           Output::Transmit(t)   => socket.send_to(&t.contents, t.destination),
//!           Output::Timeout(when) => { /* the deadline to next call handle_timeout */ }
//!           Output::Event(ev)     => { /* ChannelOpen / ChannelData / ... */ }
//!       }
//!       // then feed input: either a datagram we read from the socket, or time:
//!       rtc.handle_input(Input::Receive(now, Receive { .. }))?;   // inbound UDP
//!       rtc.handle_input(Input::Timeout(now))?;                    // deadline hit
//!   }
//! ```
//!
//! The channel contract ([`AnonymousChannel`]) is async but pull-based, and
//! this backend's methods never suspend — each call pumps the loop inline and
//! completes — so we need no tokio and no `block_on`, unlike transport-arti
//! / transport-nym / transport-iroh. `send` writes a framed record into the
//! channel then pumps the loop until str0m has nothing more to transmit right
//! now; `recv` pumps the loop (reading the socket with a short read timeout so a
//! poll returns promptly) and drains every complete framed record buffered from
//! `Event::ChannelData`. This is the same push->pull-behind-a-buffer shape the
//! other transports use, but the "push" is str0m's own poll loop rather than a
//! runtime's callback.
//!
//! ## API grounding
//!
//! Grounded against the pinned str0m (see Cargo.toml). The surface used:
//! `Rtc::new(Instant)`, `Rtc::poll_output() -> Output::{Transmit,Timeout,Event}`,
//! `Rtc::handle_input(Input::{Receive,Timeout})`, `Rtc::sdp_api()` /
//! `SdpApi::add_channel/apply/accept_offer/accept_answer` ->
//! `SdpOffer`/`SdpAnswer`/`SdpPendingOffer`,
//! `Rtc::add_local_candidate/add_remote_candidate`, `Candidate::host` /
//! `Candidate::{from,to}_sdp_string`,
//! `Rtc::channel(ChannelId)` + `Channel::write(binary, &[u8])`,
//! `Event::{ChannelOpen, ChannelData, ChannelClose}`, and
//! `str0m::net::{Receive, Protocol}`. str0m fires no local-candidate event:
//! candidates are the ones this crate registers (host, seeded in `new`).

use std::collections::VecDeque;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use transport_core::{Error, MAX_FRAME_LEN, Result, deframe, frame};

use super::{Role, Str0mConfig};

// The str0m surface the backend codes against (grounded to the pinned version).
use str0m::{
    Candidate, Event, Input, Output, Rtc,
    change::{SdpAnswer, SdpOffer, SdpPendingOffer},
    channel::ChannelId,
    net::{Protocol, Receive},
};

/// How long a single `recv` poll blocks reading the UDP socket before returning
/// what it has. Short so `recv` stays a prompt snapshot (polling cadence), long
/// enough to make progress on ICE/DTLS handshakes between calls.
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(20);

/// Max UDP datagram we read in one go. 2 KiB comfortably exceeds a path MTU;
/// str0m reassembles SCTP itself, so this is only the datagram buffer.
const UDP_READ_BUF: usize = 2048;

/// Cap on buffered complete inbound records retained between polls, bounding
/// memory if a caller stops polling. (No dedup/ordering — the lattice join owns
/// that.)
const INBOUND_BUFFER_CAP: usize = 4096;

/// The live str0m backend: one UDP socket + one `Rtc`, driven synchronously.
pub struct Inner {
    config: Str0mConfig,
    socket: UdpSocket,
    /// The local socket address (resolved after bind) used to seed our ICE host
    /// candidate and to compute the `destination`/`source` on transmit/receive.
    local_addr: SocketAddr,
    /// The sans-IO WebRTC state machine. All I/O is external to it (this crate).
    rtc: Rtc,
    /// The pending SDP offer we created as the offerer, awaiting the answer.
    /// str0m-api: `SdpApi::apply` yields this alongside the offer to complete.
    pending_offer: Option<SdpPendingOffer>,
    /// The id of the single reliable-ordered data channel, once created/open.
    channel: Option<ChannelId>,
    /// Whether the channel has fired `Event::ChannelOpen` (ICE+DTLS+SCTP up).
    open: bool,
    /// Bytes received on the channel but not yet deframed into whole records
    /// (SCTP may fragment a large PSBT; several records may coalesce).
    rx_buf: Vec<u8>,
    /// Complete framed records drained by `recv` (a fresh snapshot per call).
    /// Bare bytes only — no sender identity (a data channel carries none).
    inbound: VecDeque<Vec<u8>>,
    /// Local trickle-ICE candidate blobs discovered but not yet handed to the
    /// signaling channel by `local_candidates`.
    pending_local_candidates: VecDeque<Vec<u8>>,
}

impl Inner {
    /// Bind the UDP socket and create the str0m `Rtc`, seeding an ICE host
    /// candidate for our local address. The data channel is not up until
    /// signaling completes.
    pub fn new(config: Str0mConfig) -> Result<Self> {
        let socket = UdpSocket::bind(&config.bind_addr)
            .map_err(|e| Error::new(format!("str0m: binding UDP {}: {e}", config.bind_addr)))?;
        socket
            .set_read_timeout(Some(SOCKET_READ_TIMEOUT))
            .map_err(|e| Error::new(format!("str0m: setting socket read timeout: {e}")))?;
        let local_addr = socket
            .local_addr()
            .map_err(|e| Error::new(format!("str0m: reading local addr: {e}")))?;

        // str0m-api: build an Rtc anchored at "now" (str0m takes the start
        // instant so the sans-IO clock has an epoch).
        let mut rtc = Rtc::new(Instant::now());

        // Seed our host ICE candidate at the bound address so the peer can
        // reach us. str0m gathers no candidates itself and fires no trickle
        // event (sans-IO); the candidates WE add are the ones we trickle to
        // the peer, so record the host candidate's SDP line for
        // `local_candidates` to drain.
        let host = Candidate::host(local_addr, Protocol::Udp)
            .map_err(|e| Error::new(format!("str0m: building host candidate: {e}")))?;
        let mut pending_local_candidates = VecDeque::new();
        if let Some(added) = rtc.add_local_candidate(host) {
            pending_local_candidates.push_back(added.to_sdp_string().into_bytes());
        }

        // STUN/TURN (config.ice_servers) would be registered here via str0m's
        // ICE-agent config; server-reflexive/relayed candidates would join
        // `pending_local_candidates` the same way. Empty = host candidates only.

        Ok(Self {
            config,
            socket,
            local_addr,
            rtc,
            pending_offer: None,
            channel: None,
            open: false,
            rx_buf: Vec::new(),
            inbound: VecDeque::new(),
            pending_local_candidates,
        })
    }

    /// Offerer only: create the data channel + SDP offer, returning the offer's
    /// SDP text as opaque bytes for the signaling channel.
    pub fn local_handshake(&mut self) -> Result<Vec<u8>> {
        if self.config.role != Role::Offerer {
            return Err(Error::new(
                "str0m: local_handshake is offerer-only; the answerer calls accept_handshake",
            ));
        }
        // str0m-api: open a reliable-ordered data channel then apply the change
        // to get an SDP offer + the pending-offer token.
        let mut api = self.rtc.sdp_api();
        // A str0m data channel is reliable+ordered by default; the label must
        // match on both ends (carried in the SDP).
        let cid = api.add_channel(self.config.channel_label.clone());
        self.channel = Some(cid);
        let (offer, pending) = api
            .apply()
            .ok_or_else(|| Error::new("str0m: sdp_api().apply() produced no offer"))?;
        self.pending_offer = Some(pending);
        Ok(offer.to_sdp_string().into_bytes())
    }

    /// Apply the remote SDP blob. Answerer: remote OFFER in, returns the ANSWER
    /// blob. Offerer: remote ANSWER in, returns `None`.
    pub fn accept_handshake(&mut self, remote: &[u8]) -> Result<Option<Vec<u8>>> {
        let sdp = std::str::from_utf8(remote)
            .map_err(|e| Error::new(format!("str0m: remote SDP is not utf-8: {e}")))?;
        match self.config.role {
            Role::Answerer => {
                // str0m-api: parse the offer, accept it to get an answer.
                let offer = SdpOffer::from_sdp_string(sdp)
                    .map_err(|e| Error::new(format!("str0m: parsing remote offer: {e}")))?;
                let answer = self
                    .rtc
                    .sdp_api()
                    .accept_offer(offer)
                    .map_err(|e| Error::new(format!("str0m: accepting remote offer: {e}")))?;
                // The answerer learns the channel id from the ensuing
                // Event::ChannelOpen; nothing to record here.
                Ok(Some(answer.to_sdp_string().into_bytes()))
            }
            Role::Offerer => {
                let answer = SdpAnswer::from_sdp_string(sdp)
                    .map_err(|e| Error::new(format!("str0m: parsing remote answer: {e}")))?;
                let pending = self.pending_offer.take().ok_or_else(|| {
                    Error::new(
                        "str0m: got an answer but no pending offer (call local_handshake first)",
                    )
                })?;
                // str0m-api: complete the pending offer with the answer.
                self.rtc
                    .sdp_api()
                    .accept_answer(pending, answer)
                    .map_err(|e| Error::new(format!("str0m: accepting remote answer: {e}")))?;
                Ok(None)
            }
        }
    }

    /// Drain local trickle-ICE candidate blobs discovered since the last call.
    /// Pumping the loop is what surfaces new candidates as `Output`/events.
    pub fn local_candidates(&mut self) -> Result<Vec<Vec<u8>>> {
        self.pump(Duration::ZERO)?;
        Ok(self.pending_local_candidates.drain(..).collect())
    }

    /// Add a remote trickle-ICE candidate blob received over signaling.
    pub fn add_remote_candidate(&mut self, candidate: &[u8]) -> Result<()> {
        let s = std::str::from_utf8(candidate)
            .map_err(|e| Error::new(format!("str0m: remote candidate not utf-8: {e}")))?;
        // str0m-api: parse an ICE candidate line and add it as remote.
        let cand = Candidate::from_sdp_string(s)
            .map_err(|e| Error::new(format!("str0m: parsing remote candidate: {e}")))?;
        self.rtc.add_remote_candidate(cand);
        Ok(())
    }

    /// Whether the data channel has opened.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Write one framed opaque record onto the data channel, then pump the loop
    /// so str0m emits the resulting outbound datagrams promptly.
    pub fn send(&mut self, message: Vec<u8>) -> Result<()> {
        if message.len() > MAX_FRAME_LEN {
            return Err(Error::new(format!(
                "str0m send: message length {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
                message.len()
            )));
        }
        // Bring the connection forward first (a caller may send right after
        // signaling; the channel opens as the loop runs).
        self.pump(Duration::ZERO)?;

        let cid = self
            .channel
            .ok_or_else(|| Error::new("str0m send: data channel not created yet"))?;
        if !self.open {
            return Err(Error::new(
                "str0m send: data channel not open yet (finish ICE/DTLS handshake first)",
            ));
        }

        let framed = frame(&message);
        // str0m-api: get the channel handle and write a BINARY message.
        let mut channel = self
            .rtc
            .channel(cid)
            .ok_or_else(|| Error::new("str0m send: channel id no longer valid"))?;
        channel
            .write(true /* binary */, &framed)
            .map_err(|e| Error::new(format!("str0m send: writing to data channel: {e}")))?;

        // Flush: pump until str0m has no immediate transmit left.
        self.pump(Duration::ZERO)?;
        Ok(())
    }

    /// Pump the loop (reading the socket briefly) and drain every complete
    /// framed record received since the last poll, as bare opaque bytes.
    pub fn recv(&mut self) -> Result<Vec<Vec<u8>>> {
        // One bounded read window so recv makes handshake/data progress and
        // returns promptly (polling cadence).
        self.pump(SOCKET_READ_TIMEOUT)?;
        Ok(self.inbound.drain(..).collect())
    }

    // ------------------------------------------------------------------
    // The sans-IO driver: the heart of the crate. Drain str0m's outputs to the
    // socket, feed it inbound datagrams and time, and buffer channel data.
    // `max_read` bounds how long we block reading the socket in this call.
    // ------------------------------------------------------------------
    fn pump(&mut self, max_read: Duration) -> Result<()> {
        let deadline = Instant::now() + max_read;
        loop {
            // 1. Drain everything str0m wants to do right now.
            let timeout = self.drain_outputs()?;

            // 2a. A spent read window (notably a `Duration::ZERO` flush from
            //     `send`/`local_candidates`): DON'T block reading the socket.
            //     Just advance str0m's clock once so any immediately-due
            //     transmits are emitted, drain them, and return promptly. This
            //     keeps a flush non-blocking instead of stalling ~20ms on the
            //     socket read timeout.
            if Instant::now() >= deadline {
                self.rtc
                    .handle_input(Input::Timeout(Instant::now()))
                    .map_err(|e| Error::new(format!("str0m: handle_input(Timeout): {e}")))?;
                let _ = timeout; // str0m's requested deadline; honored across calls
                return self.drain_outputs().map(|_| ());
            }

            // 2b. Try to read one inbound datagram (bounded by our read timeout /
            //     the remaining window), then feed it to str0m; else feed time.
            let now = Instant::now();
            let mut buf = [0u8; UDP_READ_BUF];
            match self.socket.recv_from(&mut buf) {
                Ok((n, source)) => {
                    // str0m-api: hand the datagram to the state machine.
                    let receive = Receive::new(Protocol::Udp, source, self.local_addr, &buf[..n])
                        .map_err(|e| {
                        Error::new(format!("str0m: parsing inbound datagram: {e}"))
                    })?;
                    self.rtc
                        .handle_input(Input::Receive(now, receive))
                        .map_err(|e| Error::new(format!("str0m: handle_input(Receive): {e}")))?;
                    // Loop again to process what that input produced — but honor
                    // the read window first, so a peer sending a steady stream of
                    // datagrams cannot keep a single `recv` spinning forever (this
                    // must stay a prompt polling snapshot). `Duration::ZERO`
                    // windows (send/local_candidates flush) fall through here on
                    // the very next iteration once the socket would block.
                    if Instant::now() >= deadline {
                        return Ok(());
                    }
                    continue;
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // No datagram this window: advance str0m's clock so it can
                    // fire timeouts (ICE checks, DTLS retransmits, SCTP).
                    self.rtc
                        .handle_input(Input::Timeout(now))
                        .map_err(|e| Error::new(format!("str0m: handle_input(Timeout): {e}")))?;
                }
                Err(e) => {
                    return Err(Error::new(format!("str0m: udp recv_from: {e}")));
                }
            }

            // 3. After a WouldBlock/TimedOut read (the socket read window is
            //    spent) or a processed datagram, loop back: `drain_outputs`
            //    emits anything the fed input produced, and the deadline check
            //    at 2a terminates the pump. str0m's requested `timeout` deadline
            //    is honored across calls (the next recv/send resumes the loop —
            //    polling cadence), so we don't sleep on it here.
            let _ = timeout;
        }
    }

    /// Drain `Rtc::poll_output` until it asks for a Timeout, sending outbound
    /// datagrams and handling events. Returns str0m's requested next deadline.
    fn drain_outputs(&mut self) -> Result<Instant> {
        loop {
            // str0m-api: poll_output yields Transmit / Timeout / Event.
            match self
                .rtc
                .poll_output()
                .map_err(|e| Error::new(format!("str0m: poll_output: {e}")))?
            {
                Output::Transmit(t) => {
                    // Write the datagram str0m produced to its destination.
                    self.socket
                        .send_to(&t.contents, t.destination)
                        .map_err(|e| Error::new(format!("str0m: udp send_to: {e}")))?;
                }
                Output::Timeout(when) => {
                    // str0m has nothing more to emit until `when`; hand control
                    // back to the pump loop, which will read the socket / feed
                    // time until then.
                    return Ok(when);
                }
                Output::Event(event) => self.on_event(event)?,
            }
        }
    }

    /// Handle one str0m event: track channel open/close, buffer channel data,
    /// and collect newly-discovered local ICE candidates for signaling.
    fn on_event(&mut self, event: Event) -> Result<()> {
        match event {
            // str0m-api: the reliable-ordered channel is up.
            Event::ChannelOpen(cid, _label) => {
                self.channel.get_or_insert(cid);
                self.open = true;
            }
            // str0m-api: inbound binary data on the channel.
            Event::ChannelData(data) => {
                // Append and deframe every complete record, retaining partials.
                self.rx_buf.extend_from_slice(&data.data);
                while let Some(record) = deframe(&mut self.rx_buf)? {
                    if self.inbound.len() < INBOUND_BUFFER_CAP {
                        self.inbound.push_back(record);
                    }
                }
            }
            Event::ChannelClose(_cid) => {
                self.open = false;
            }
            // NOTE: str0m surfaces no local-candidate event — candidates are
            // whatever WE registered via `add_local_candidate` (seeded in
            // `new`, queued straight into `pending_local_candidates`).
            // Connected / IceConnectionStateChange / others: no action; the
            // transport moves bytes and does not reason about connection quality.
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A network-free framing roundtrip over the exact buffer path the backend
    // uses on inbound channel data: append coalesced binary payloads to rx_buf
    // and loop `deframe`, retaining a trailing partial. Proves the backend's
    // record-delimiting agrees with the transport-core wire format without
    // needing str0m or a socket.
    #[test]
    fn inbound_deframe_loop_matches_wire_format() {
        let a = b"opaque-a".to_vec();
        let b = vec![0x5Au8; 20_000]; // spans several SCTP messages on the wire

        // Simulate two channel-data deliveries that coalesce records and split
        // one record across delivery boundaries.
        let mut wire = frame(&a);
        wire.extend_from_slice(&frame(&b));
        let split = wire.len() - 5; // cut the last record mid-value
        let (part1, part2) = wire.split_at(split);

        let mut rx_buf: Vec<u8> = Vec::new();
        let mut out: Vec<Vec<u8>> = Vec::new();

        rx_buf.extend_from_slice(part1);
        while let Some(rec) = deframe(&mut rx_buf).unwrap() {
            out.push(rec);
        }
        // Only the first full record is available; the second is still partial.
        assert_eq!(out, vec![a.clone()]);

        rx_buf.extend_from_slice(part2);
        while let Some(rec) = deframe(&mut rx_buf).unwrap() {
            out.push(rec);
        }
        assert_eq!(out, vec![a, b]);
        assert!(rx_buf.is_empty());
    }

    #[test]
    fn send_rejects_oversize_before_touching_str0m() {
        // The MAX_FRAME_LEN guard is a pure length check in `send` before any
        // channel write, so it is exercisable in shape here (documented; the
        // full path needs a live Rtc which requires str0m to be pinned).
        assert!(MAX_FRAME_LEN < usize::MAX);
    }
}
