# PWA transports — browser-viable, opt-in

The PWA carries ONLY browser-viable transports. Heavy native p2p (iroh/arti/
nym/emissary) is NOT here (D7). Every transport presents the transport-core
channel shape (D8): `send(bytes)` broadcast + `recv() -> snapshot`, pull-based,
pushing into the shared PSBT array so `join()` runs on every update.

| Transport | Grounded? | Channel kind | Network | Default |
|---|---|---|---|---|
| Sneakernet (import/export/paste/QR) | YES | Anonymous | NONE | ON |
| WebRTC data channel | native web-sys YES / str0m deferred | Anonymous | P2P (after signaling) | OFF |
| Nostr over WebSocket | ws YES / `nostr` crate deferred | Attributable (sender pubkey) | relay WebSocket | OFF |
| OHTTP mailbox (signaling + offline fallback) | DEFERRED (ohttp/payjoin) | Anonymous | HTTP via OHTTP relay | OFF |

## 1. Sneakernet — see `offline-first.md`

Always on, no network. `src/transport/sneakernet.ts`.

## 2. WebRTC data channel — `src/transport/webrtc.ts`

**Preferred impl (grounded): browser-native `RTCPeerConnection` in TS.** The
browser natively does DTLS + ICE/STUN/TURN NAT traversal. We open ONE reliable-
ordered data channel (`createDataChannel(label, {ordered:true})`), send one
framed record per message (u32-BE length prefix + value, mirroring
`transport_core::frame`/`deframe`, `MAX_FRAME_LEN=16 MiB`), and drain framed
records buffered from `ondatamessage` (push→pull). A data channel is a bare byte
pipe with no verifiable sender identity → **AnonymousChannel**.

**Alternate impl (deferred): str0m-in-wasm.** `str0m` is sans-IO and CAN compile
to wasm, but you must still bridge its UDP transmit/receive to browser APIs —
which the browser's own `RTCPeerConnection` already does. `str0m` is UNGROUNDED,
so this path stays DEFERRED in the `transport-str0m` crate behind its `str0m`
feature (`TODO(ground-deps): str0m`). The browser-native TS path is what
actually ships.

### Signaling (CRITICAL — D6)

WebRTC needs SDP offer/answer + trickle ICE exchanged BEFORE the P2P channel
exists. This MUST go through the **signaling-ohttp** component (BIP 77 payjoin
directory via OHTTP relay), NEVER a signaling server or the PWA origin (either
would learn the client IP). Flow:

```
A: createOffer → setLocalDescription → seal(SDP) → signaling.send(mailbox slot i)
B: signaling.recv() → open(SDP offer) → setRemoteDescription → createAnswer
   → setLocalDescription → seal(SDP) → signaling.send(slot i+1)
A: signaling.recv() → open(SDP answer) → setRemoteDescription
both: trickle ICE candidates as further sealed blobs in subsequent slots
once ICE completes: PSBT frames flow P2P over the data channel (NOT the directory)
```

The SDP/ICE blobs are opaque HPKE-sealed bytes to the directory. `webrtc.ts`
consumes the signaling channel purely as the introduction/pairing channel;
`signaling-ohttp` is a separate component this transport depends on.

## 3. Nostr over WebSocket — `src/transport/nostr-ws.ts`

Nostr relays are plain WebSocket — **browser-native** (`ws_stream_wasm` is
grounded for the Rust path; browser `WebSocket` for the TS path). This CORRECTS
`app-suite.md`'s matrix, which marks "Nostr Web = ❌ (needs native relay
connections)": relays ARE WebSocket, so nostr-over-WebSocket IS browser-viable.
The ❌ reflects the mdk/MLS SDK not being wasm-proven, not the protocol.

- **Preferred (grounded interim):** browser `WebSocket` to relay(s), NIP-44
  encrypted DMs for the 2-party path, in TS. Sender pubkey = `SenderId` →
  **AttributableChannel**.
- **Preferred (deferred):** the rust `nostr` crate compiled to wasm (NIP-44 +
  relay), talking over `ws_stream_wasm`. `nostr` is UNGROUNDED and the rust-side
  transport crate is unauthored → DEFERRED (`TODO(transport-nostr): unauthored;
  TODO(ground-deps): nostr`).
- MLS groups (mdk/whitenoise) for multi-party forward secrecy are heavier and
  DEFERRED further; the browser transport ships the lighter NIP-44-over-ws path.

Push→pull: the relay subscription handler appends to an inbox; `recv()` drains a
fresh snapshot. Each app message is `frame()`-wrapped before it rides a relay
event.

## 4. OHTTP mailbox — `src/transport/ohttp-mailbox.ts` (thin wrapper over signaling-ohttp)

The BIP 77 payjoin-directory-over-OHTTP client is its OWN component
(`signaling-ohttp`). It serves TWO roles here, from ONE mechanism:

1. **WebRTC signaling** (above).
2. **Async offline fallback** — if a peer is offline, their PSBT waits in the
   directory; the other peer joins later. This is `app-suite.md`'s "async
   fallback" and it is the SAME crate/mailbox as signaling.

It is an **AnonymousChannel**: `send` = POST an HPKE-sealed blob to a mailbox
subdirectory via the OHTTP relay; `recv` = GET/poll that subdirectory, returning
bare bytes. Mailbox IDs derive from a shared secret in the room link
(`H(shared_secret || index)`), so no server assigns a room. `ohttp`/`payjoin`
are UNGROUNDED → DEFERRED. `ohttp-mailbox.ts` in this component is a thin adaptor
that presents the channel shape and delegates to the `signaling-ohttp` client;
until that client is ground, it returns a clear "signaling-ohttp not configured"
error (feature-off skeleton parity).

## Deferred-skeleton discipline (mirrors transport-arti/transport-nym)

For the Rust side the transports are their own crates with their own feature
gates — `transport-str0m` (feature `str0m`), `transport-webrtc-rs` (feature
`webrtc-rs`), `transport-payjoin-dir` (feature `payjoin-dir`; the
signaling-ohttp component). TODO(transport-nostr): a rust nostr transport crate
is unauthored. (The merged wasm core `concurrent-psbt-wasm` carries NO transport
features — it is pure Layer-1/2.) Each transport crate: feature declared, SDK
dep un-wired, `TODO(ground-deps)`, default compiles, `mod imp` gated behind the
feature, feature-off ctor/send/recv return `Error::new("... built without the
'<x>' feature")`. Trait-satisfaction + frame/deframe roundtrip tests run in
BOTH states with no network.

For the TS side: sneakernet + browser-native WebRTC + browser-native nostr-ws are
grounded and implemented; the deferred paths (`str0m`, rust `nostr` crate, ohttp
client) are stubbed to a clear runtime error until their deps/component land, so
the bundle always builds and runs offline.

## Why duplicates/reordering are safe

The lattice join is idempotent/commutative/associative and ignores `SenderId`
(provenance is unauthenticated; folding is fail-safe under `SIGHASH_ALL`). So a
broadcast data channel echoing our own sends, a relay redelivering, or a mailbox
returning all slots to all readers are all harmless — `recv()`'s
snapshot-includes-own-sends contract is satisfied by construction.
