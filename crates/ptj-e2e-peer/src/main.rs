// crates/ptj-e2e-peer/src/main.rs
//
// AUTHORED-BUT-UNVERIFIED: this bin only compiles under `--features e2e-peer`,
// which is blocked on transport-payjoin-dir's deferred network deps (and this
// file still codes against the pre-async channel seam — reconcile both when
// grounding). The default target set never builds it (required-features).
//
// ptj-e2e-peer — the RUST counterparty in the browser<->rust WebRTC e2e test.
//
// This is NOT new transport logic. It is glue that drives the already-authored
// sibling crates through their PUBLIC seams, exactly as `ptj sync` drives them:
//
//   * transport-payjoin-dir  — the BIP-77 payjoin-directory-over-OHTTP mailbox
//     (`PayjoinDirChannel`, an AnonymousChannel) wrapped by its typed
//     `SignalingChannel` for the SDP offer/answer + trickle ICE exchange.
//   * transport-str0m        — the sans-IO WebRTC data-channel backend
//     (`Str0mTransport`, an AnonymousChannel) for the P2P PSBT frames.
//   * ptj join engine        — the EXISTING lattice fold (`join_psbts`),
//     unchanged; convergence is the real thing, not a mock.
//
// Wire path (this IS the task's constraint made executable):
//   SDP/ICE  --> SignalingChannel --> PayjoinDirChannel (HPKE via OHTTP relay) --> directory
//   PSBT     --> Str0mTransport data channel (P2P, DTLS/SCTP, after ICE completes)
//
// The peer is handed ONLY (relay origin + gateway key + room secret + the dir
// URL the RELAY forwards to). It never opens a socket to the directory itself,
// and there is NO direct/localhost signaling mode to accidentally use — so a
// bypass is structurally impossible, which is what the harness's A3 assertions
// verify at socket/port/process + ciphertext-opacity granularity.
//
// The whole binary is gated behind the `e2e-peer` cargo feature via
// `[[bin]] required-features = ["e2e-peer"]` in Cargo.toml, so the default
// workspace build / `clippy --all-targets` never pull the ungrounded
// str0m/ohttp/payjoin deps. See Cargo.toml.sketch.

use std::time::{Duration, Instant};

use psbt_v2::v2::Psbt;

// transport-core: the frozen seam every transport codes against.
use transport_core::{Message, Transport};

// Sibling transport crates. Their real backends are compiled in via this bin's
// `e2e-peer` feature, which turns on their own `str0m` / `ohttp` / `payjoin-dir`
// features. (The signaling-ohttp component ships as crate `transport-payjoin-dir`.)
use transport_payjoin_dir::{
    mailbox::Role as MailboxRole, PayjoinDirChannel, PayjoinDirConfig, SignalingChannel,
    SignalingMsg,
};
use transport_str0m::{Role as WebRtcRole, Str0mConfig, Str0mTransport};

// The EXISTING lattice fold + PSBT (de)serialization, reused verbatim. In the
// integrated tree ptj exposes these from a lib target (or a shared
// `concurrent-psbt`/`ptj-core` crate); this binary CALLS them, it does not
// reimplement convergence. See the `ptj_join` shim at the bottom for the exact
// dependency edge.
use ptj_join::{encode_psbt, join_psbts, parse_psbt_base64};

/// Deadlines: ICE completion, overall convergence, and inter-poll sleep.
const ICE_DEADLINE: Duration = Duration::from_secs(30);
const CONVERGE_BUDGET: Duration = Duration::from_secs(20);
const POLL_GAP: Duration = Duration::from_millis(120);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse_env()?;

    // -- 1) Introduction/pairing channel: the BIP-77 directory reached ONLY
    //       through the OHTTP relay. Every request is OHTTP-encapsulated to the
    //       gateway key and forwarded by the relay; the peer's own socket only
    //       ever connects to the relay (this is what makes A3 bullet-1 hold).
    let dir = PayjoinDirChannel::open(PayjoinDirConfig::new(
        args.directory_url.clone(), // where the RELAY forwards; peer never dials it
        args.relay_url.clone(),     // the only host this process connects to
        hex_bytes(&args.gateway_key_hex)?,
        hex_bytes(&args.room_secret_hex)?,
        args.mailbox_role,
    ))?;
    // Typed WebRTC-signaling view over the raw mailbox: offer/answer/ICE records.
    let mut signaling = SignalingChannel::new(dir);

    // -- 2) WebRTC peer: sans-IO str0m over an owned UdpSocket. tokio-free; its
    //       poll loop IS the publish/collect cadence.
    let mut webrtc = Str0mTransport::new(Str0mConfig::new(args.webrtc_role, args.udp_bind.clone()))?;

    // -- 3) SDP/ICE exchange over the signaling channel. The blobs str0m produces
    //       are opaque bytes to signaling; the mailbox HPKE-seals them.
    perform_handshake(&mut webrtc, &mut signaling, args.webrtc_role)?;

    // -- 4) Trickle ICE until the data channel is open: pump str0m each turn,
    //       drain its local candidates to the peer, feed remote candidates in.
    let ice_in = drive_ice_to_open(&mut webrtc, &mut signaling)?;

    // A3 round-trip observability: prove we genuinely parsed a real SDP and at
    // least one real ICE candidate — i.e. the oblivious path was EXERCISED, not
    // bypassed. (The slots the harness dumps are opaque ciphertext; these stdout
    // lines are how we show the plaintext round-tripped end to end.)
    println!("E2E_RUST_SDP_PARSED=ok");
    println!("E2E_RUST_ICE_CANDIDATES={ice_in}");

    // -- 5) Convergence over the DATA CHANNEL (not the directory). This is exactly
    //       ptj::commands::sync::sync_step's cadence, transport-agnostic:
    //       gather(collect) -> Message::decode -> join_psbts -> publish.
    //       Str0mTransport is an AnonymousChannel, hence a Transport for free via
    //       transport-core's blanket impl.
    let ours = parse_psbt_base64(&std::fs::read_to_string(&args.fragment_path)?)?;
    let transport: &mut dyn Transport = &mut webrtc;
    transport.publish(Message::Psbt(encode_psbt(&ours).into_bytes()).encode())?;

    let converged = converge(transport, CONVERGE_BUDGET)?;

    // A1: emit the converged PSBT (base64) for byte-equality against the browser.
    println!("E2E_RUST_CONVERGED={}", encode_psbt(&converged));
    Ok(())
}

/// Offer/answer over the signaling mailbox. Which side offers is fixed by the
/// role handed in out of band (mirrors the PWA's `isInitiator`).
fn perform_handshake(
    webrtc: &mut Str0mTransport,
    signaling: &mut SignalingChannel<PayjoinDirChannel>,
    role: WebRtcRole,
) -> Result<(), Box<dyn std::error::Error>> {
    match role {
        WebRtcRole::Offerer => {
            // str0m produces the local SDP offer blob.
            let offer = webrtc.local_handshake()?;
            signaling.send(&SignalingMsg::Offer(offer))?;
            // Wait for the peer's answer, then apply it (offerer gets None back).
            let answer = poll_for(signaling, SignalKind::Answer)?;
            let none = webrtc.accept_handshake(&answer)?;
            debug_assert!(none.is_none(), "offerer's accept_handshake returns None");
        }
        WebRtcRole::Answerer => {
            let offer = poll_for(signaling, SignalKind::Offer)?;
            // Applying the offer yields the answer blob to send back.
            let answer = webrtc
                .accept_handshake(&offer)?
                .ok_or("answerer expected an SDP answer from accept_handshake")?;
            signaling.send(&SignalingMsg::Answer(answer))?;
        }
    }
    Ok(())
}

/// Pump str0m and trickle ICE both ways until the data channel is open. Returns
/// the number of remote ICE candidates applied (>= 1 on a real connection).
fn drive_ice_to_open(
    webrtc: &mut Str0mTransport,
    signaling: &mut SignalingChannel<PayjoinDirChannel>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + ICE_DEADLINE;
    let mut ice_in = 0usize;
    while !webrtc.is_open() {
        // Push our freshly discovered local candidates out through the mailbox.
        for cand in webrtc.local_candidates()? {
            signaling.send(&SignalingMsg::IceCandidate(cand))?;
        }
        // Pull remote signaling records; feed candidates into str0m. Offer/answer
        // records still echo on the broadcast channel; we ignore the steps not
        // for us, per SignalingChannel's documented "caller decides" rule.
        for msg in signaling.poll()? {
            if let SignalingMsg::IceCandidate(cand) = msg {
                webrtc.add_remote_candidate(&cand)?;
                ice_in += 1;
            }
        }
        if Instant::now() > deadline {
            return Err("ICE did not complete within the deadline".into());
        }
        std::thread::sleep(POLL_GAP);
    }
    Ok(ice_in)
}

/// One convergence loop over the data channel. Fold with `join_psbts` until the
/// result is stable across two polls: because the join is
/// idempotent/commutative/associative, stability == convergence. This is the
/// exact shape of `ptj::commands::sync::sync_step`, transport-agnostic.
fn converge(
    transport: &mut dyn Transport,
    budget: Duration,
) -> Result<Psbt, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + budget;
    let mut last: Option<Psbt> = None;
    loop {
        let mut psbts = Vec::new();
        for bytes in transport.collect()? {
            if let Message::Psbt(payload) = Message::decode(&bytes)? {
                psbts.push(parse_psbt_base64(&String::from_utf8(payload)?)?);
            }
            // Non-PSBT envelopes (Payment/Confirmation) are not part of the join.
        }
        let joined = join_psbts(psbts)?;
        transport.publish(Message::Psbt(encode_psbt(&joined).into_bytes()).encode())?;

        // Stable across two rounds => the peer's fragment is already folded in.
        if last.as_ref() == Some(&joined) {
            return Ok(joined);
        }
        last = Some(joined);
        if Instant::now() > deadline {
            return Err("convergence not reached within the budget".into());
        }
        std::thread::sleep(POLL_GAP);
    }
}

// ---- signaling poll helper --------------------------------------------------

#[derive(Clone, Copy)]
enum SignalKind {
    Offer,
    Answer,
}

/// Poll the signaling mailbox until a record of the wanted kind arrives.
fn poll_for(
    signaling: &mut SignalingChannel<PayjoinDirChannel>,
    want: SignalKind,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + ICE_DEADLINE;
    loop {
        for msg in signaling.poll()? {
            match (want, msg) {
                (SignalKind::Offer, SignalingMsg::Offer(v)) => return Ok(v),
                (SignalKind::Answer, SignalingMsg::Answer(v)) => return Ok(v),
                _ => {} // not the step we're waiting on; ICE/echoes ignored here
            }
        }
        if Instant::now() > deadline {
            return Err("timed out waiting for the expected signaling record".into());
        }
        std::thread::sleep(POLL_GAP);
    }
}

// ---- CLI args ---------------------------------------------------------------

struct Args {
    webrtc_role: WebRtcRole,
    mailbox_role: MailboxRole,
    /// The directory base URL (the crate always reaches it THROUGH the relay; the
    /// peer never opens a socket to it directly).
    directory_url: String,
    relay_url: String,
    gateway_key_hex: String,
    room_secret_hex: String,
    fragment_path: String,
    udp_bind: String,
}

impl Args {
    /// Parse the flat `--flag value` CLI the harness spawns us with. The role
    /// flag drives BOTH the WebRTC role (offerer/answerer) and the mailbox lane
    /// (Initiator/Responder), keeping them consistent by construction.
    fn parse_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut role = None;
        let mut directory_url = None;
        let mut relay_url = None;
        let mut gateway_key_hex = None;
        let mut room_secret_hex = None;
        let mut fragment_path = None;
        let mut udp_bind = Some("127.0.0.1:0".to_string());

        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut next = || it.next().ok_or_else(|| format!("missing value for {flag}"));
            match flag.as_str() {
                "--role" => role = Some(next()?),
                "--directory-url" => directory_url = Some(next()?),
                "--ohttp-relay" => relay_url = Some(next()?),
                "--ohttp-gateway-key" => gateway_key_hex = Some(next()?),
                "--room" => room_secret_hex = Some(next()?),
                "--fragment" => fragment_path = Some(next()?),
                "--udp-bind" => udp_bind = Some(next()?),
                other => return Err(format!("unknown flag {other}").into()),
            }
        }

        let (webrtc_role, mailbox_role) = match role.as_deref() {
            Some("offerer") => (WebRtcRole::Offerer, MailboxRole::Initiator),
            Some("answerer") => (WebRtcRole::Answerer, MailboxRole::Responder),
            other => return Err(format!("--role must be offerer|answerer, got {other:?}").into()),
        };

        Ok(Self {
            webrtc_role,
            mailbox_role,
            directory_url: directory_url.ok_or("--directory-url required")?,
            relay_url: relay_url.ok_or("--ohttp-relay required")?,
            gateway_key_hex: gateway_key_hex.ok_or("--ohttp-gateway-key required")?,
            room_secret_hex: room_secret_hex.ok_or("--room required")?,
            fragment_path: fragment_path.ok_or("--fragment required")?,
            udp_bind: udp_bind.expect("defaulted"),
        })
    }
}

/// Decode a lowercase-hex string into bytes (room secret / gateway key material).
fn hex_bytes(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if s.len() % 2 != 0 {
        return Err("hex string has an odd length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.into()))
        .collect()
}

// ---- ptj join engine shim ---------------------------------------------------
//
// In the integrated tree these are re-exported from ptj's lib target (the same
// `join_psbts` / `io::encode_psbt` / `io::parse_psbt_bytes` `ptj sync` uses), so
// convergence here is byte-for-byte the CLI's convergence. This module is a
// documentation stand-in for that dependency edge — it is NOT a reimplementation.
// TODO(share-join): promote ptj's join + psbt (de)serialization into a shared
// lib target this binary depends on, so there is exactly one join engine.
mod ptj_join {
    use psbt_v2::v2::Psbt;

    /// `ptj::io::encode_psbt` — canonical base64 encoding of a v2 PSBT.
    pub fn encode_psbt(_psbt: &Psbt) -> String {
        unimplemented!("re-export ptj::io::encode_psbt when integrated")
    }

    /// `ptj::io::parse_psbt_bytes` over a base64 string — the same fallback-
    /// tolerant parser `sync_step` uses.
    pub fn parse_psbt_base64(_b64: &str) -> Result<Psbt, Box<dyn std::error::Error>> {
        unimplemented!("re-export ptj::io::parse_psbt_bytes when integrated")
    }

    /// `ptj::commands::join::join_psbts` — the EXISTING lattice fold. Unchanged.
    pub fn join_psbts(_psbts: Vec<Psbt>) -> Result<Psbt, Box<dyn std::error::Error>> {
        unimplemented!("re-export ptj::commands::join::join_psbts when integrated")
    }
}
