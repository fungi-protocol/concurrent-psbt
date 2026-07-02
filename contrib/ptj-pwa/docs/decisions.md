# PWA shell — decisions

Load-bearing choices for the `ptj-pwa` shell, each with the reason and the
alternative rejected.

> **Reconciliation (2026-07-06) — D1/D2/D3 amended.** The frontier components
> forked the frontend/wasm seam three ways; the reconciled seam supersedes the
> letter of D1–D3 while keeping their intent (one JSON contract, thin wrapper,
> concurrent-psbt wasm-bindgen-free):
>
> - **D1 amended:** the seam IS a single interface — the canonical `Backend` in
>   `shared-frontend/core/backend.ts` (the very promotion D1 said it would not
>   block). `FetchLike` survives only inside `HttpBackend`.
> - **D2 amended:** the separate thin wrapper crate stands, but it is
>   `concurrent-psbt-wasm` (wasm-bindgen component) — the `ptj-wasm` authored
>   under `crate/` was merged into it (see `crate/README.md`).
> - **D3 amended:** the ONE JSON contract stands (same request/response field
>   names + error texts as webgui), but the boundary style is JsValue in/out +
>   thrown JsError with camelCase js_name exports, not `{status,body}` bytes.

## D1. The backend seam is a `FetchLike`, not a new interface (AMENDED — see above)

**Decision.** The PWA implements the EXISTING `FetchLike` seam from
`contrib/demo-gui/src/backend.ts` with a `WasmBackend`. `WasmBackend` accepts the
same `(path, {method,headers,body})` shape, dispatches on `path`, and calls a
wasm-bindgen export that runs the identical ptj `*_response_result(&[u8]) -> Result<Vec<u8>>` logic. It resolves a `FetchResponse` (`ok/status/json()`).

**Why.** `backend.ts` is already fetch-agnostic; only `app.ts` hard-binds
`window.fetch`. Matching `FetchLike` means the `backend.ts` free functions
(`createPsbt`, `joinPsbts`, `sortPsbt`, `syncPsbts`, ...) and all DTOs
(`PsbtResponse`, `AtomizeResponse`, `SyncResponse`, `PtjBackendError`) are reused
verbatim — zero fork of the frontend.

**Rejected.** A bespoke `PtjBackend` TS interface (one method per op). It is
cleaner in the abstract but would require rewriting `backend.ts`'s call
convention and every `app.ts` call site, and would diverge the DTO plumbing from
webgui. Keeping `FetchLike` is the minimal, additive change. (If the
shared-frontend component later promotes `FetchLike` to a `PtjBackend` interface,
`WasmBackend` trivially becomes one of its impls — this decision does not block
that.)

## D2. WASM wrapper is a SEPARATE thin crate, not edits to `concurrent-psbt` (AMENDED: the crate is `concurrent-psbt-wasm`)

**Decision.** Add a new crate `ptj-wasm` that depends on `concurrent-psbt` (and,
where the op logic already lives in ptj, factors the pure `*_response_result`
bodies into a shared place both ptj and ptj-wasm call). `ptj-wasm` holds ALL
`wasm-bindgen` usage and the JSON in/out shim. `concurrent-psbt` stays
wasm-bindgen-free.

**Why.** The Feasibility agent proved `concurrent-psbt` compiles to wasm with NO
wasm-bindgen dependency; wasm-bindgen is only needed to generate the JS glue that
satisfies `__wbindgen_placeholder__` (getRandomValues etc.). Keeping
wasm-bindgen out of the default check path keeps the repo's normal build clean
and matches the Feasibility note: "concurrent-psbt itself does not need
wasm-bindgen; the thin PWA-facing wrapper does."

**Rejected.** Adding `#[wasm_bindgen]` to `concurrent-psbt` directly (pollutes
the core crate and its default check with a wasm-only dep) or duplicating op
logic in JS (would re-implement PSBT byte manipulation in TS — the whole point of
WASM is to NOT do that).

## D3. Reuse the JSON request/response contract, not a typed FFI (AMENDED: JsValue/JsError boundary)

**Decision.** `ptj-wasm` exposes ONE dispatch entry per op, each taking the JSON
request bytes and returning JSON response bytes (or a JSON `{error}` with an HTTP
status), mirroring webgui's handler bodies exactly. `WasmBackend` is then a pure
`path → export` router.

**Why.** The ptj handlers already parse JSON bodies and emit `{psbt,inspect}` /
`{error}` JSON. Reusing that contract means the wasm surface is a mechanical port
of `webgui.rs`, the DTOs match byte-for-byte, and `PtjBackendError` parsing on the
JS side is unchanged. It also keeps the door open to move the op bodies into a
shared `ptj-ops`-style module consumed by BOTH `webgui.rs` and `ptj-wasm`.

**Rejected.** Rich typed `#[wasm_bindgen]` structs per op. More idiomatic but
forces a second DTO definition (Rust structs mirroring the TS DTOs) and diverges
from webgui, defeating the "one contract" property. JSON strings are the lingua
franca both shells already speak.

## D4. Sneakernet (offline, no network) is a first-class, always-on transport

**Decision.** Import / export / paste / file-drop / clipboard, plus animated QR
display + camera scan (UR format), are always available and require NO network,
NO service, NO permission beyond camera for scanning. They push into the same
shared PSBT array the network transports feed.

**Why.** The core constraint: "Local sneakernet must work fully offline on
mobile." The service worker caches the entire app shell + wasm so the whole loop
runs in airplane mode. QR is the line-of-sight transport from `app-suite.md`
(display on one device, scan with another). No design fork is needed — it is just
another array pusher.

**Rejected.** Gating sneakernet behind any online capability (would violate the
offline-first requirement).

## D5. Network transports are OPT-IN, per-session, and default OFF

**Decision.** Nostr-over-WebSocket and WebRTC are opt-in toggles a user enables
for a session. Default state is offline/sneakernet-only. Enabling a network
transport is an explicit user action (matching "opt-in browser-compatible
transports only").

**Why.** Privacy + offline-first. A PWA that silently opens sockets leaks
presence/metadata. Default-off keeps the sneakernet path pure and makes network
egress a deliberate choice. Mirrors the CLI where `ptj net` is an explicit
subcommand and transports are feature-gated.

## D6. WebRTC signaling goes through signaling-ohttp — NEVER a signaling server

**Decision.** SDP offer/answer + trickle ICE candidates are opaque HPKE-sealed
blobs moved through the **signaling-ohttp** component: a BIP 77 payjoin directory
(store-and-forward mailbox) reached via an OHTTP relay. The PWA NEVER contacts a
localhost/direct signaling server, a Cloudflare-Workers broker, or its own
origin for signaling.

**Why (critical).** A direct signaling server or the PWA origin would learn the
client IP. OHTTP hides the client IP from the directory; the directory relays only
E2E-encrypted blobs it cannot read. This corrects `app-suite.md`'s "lightweight
signaling server (static site + Cloudflare Workers)" framing, which leaks the IP.
Signaling and the async offline-delivery fallback collapse into ONE mailbox: the
same directory carries the handshake and, once the data channel is up, remains the
offline fallback for a peer who is not online.

**Rejected.** Any WebRTC signaling that touches an IP-visible endpoint. Explicitly
out of scope for the PWA to host or embed such a server.

## D7. Prefer browser-native web-sys paths; defer heavy Rust SDKs

**Decision.**

- **WebRTC:** prefer the browser's native `RTCPeerConnection`/`RTCDataChannel`
  driven from TS/wasm (grounded: `web-sys` has these; the TS side can use them
  directly). `str0m`-in-wasm is a viable-but-heavier ALTERNATE, deferred.
- **Nostr:** prefer the rust `nostr` crate compiled to wasm talking to relays over
  browser WebSocket (`ws_stream_wasm` is grounded) — but `nostr` itself is
  UNGROUNDED, so the wired implementation is DEFERRED behind a flag; a thin
  browser-WebSocket + NIP-44 TS path is the grounded interim.

**Why.** The registry HAS `wasm-bindgen`, `web-sys` (RtcPeerConnection,
RtcDataChannel, WebSocket), `ws_stream_wasm`, `getrandom`. It does NOT have
`nostr`, `str0m`, `webrtc`, `ohttp`, `payjoin`. Per constraint 5, ungrounded deps
are deferred (feature declared, dep un-wired, `TODO(ground-deps)`) so the default
skeleton compiles and feature-on stays authored-but-unverified — exactly like
`transport-arti`/`transport-nym`. The browser already implements WebRTC natively,
so compiling a Rust WebRTC stack to wasm is redundant for the primary path.

## D8. Every transport presents the transport-core channel seam shape

**Decision.** Each PWA transport is modeled as the same channel abstraction the
Rust `transport-core` defines: `send(bytes)` broadcast + `recv() -> snapshot of all messages` (pull). WebRTC and the signaling mailbox are `AnonymousChannel`
(bare bytes, no verifiable sender). Nostr is `AttributableChannel` (sender pubkey
as `SenderId`).

**Why.** Keeps the PWA transports isomorphic to the CLI/tauri transports and to
the "shared array + join on every update" model. The snapshot-includes-own-sends,
idempotent-recv contract is satisfied naturally by the lattice join, so push
transports (WebRTC ondatamessage, WebSocket relay events) convert push→pull
behind a buffer — the same trick `transport-arti`/`transport-nym` use.

**Note.** The TS transports do not import Rust code; they implement the SAME
CONTRACT in TypeScript so the shared frontend's array-pusher expectations hold
identically across shells. Where a Rust transport is compiled to wasm (deferred
nostr/str0m paths), it presents the actual `transport-core` trait.

## D9. Mobile-first, installable, offline-first

**Decision.** The shell ships a `manifest.webmanifest` (standalone display,
portrait, maskable icons, theme color) and a service worker with a
cache-first-for-shell / network-only-for-transports strategy. Target is a phone
browser that can "Add to Home Screen" and then run in airplane mode.

**Why.** The canonical use case ("You're at dinner... open ptj.app on your
phone") is mobile and often offline/line-of-sight. Install + offline cache make
the static bundle behave like an app with no store, no signup, no backend.

## D10. Versioned, content-addressed caching; transports never cached

**Decision.** The service worker caches the app shell (html/js/css/wasm/icons)
under a versioned cache name and serves it cache-first. It NEVER caches transport
traffic (WebSocket, WebRTC, OHTTP POST/GET) — those are always live. Cache
version bump on release evicts stale bundles (mirrors webgui's `?v=` cache-bust).

**Why.** Offline correctness (stale PSBT bytes would be dangerous) requires that
only the immutable app shell is cached, never session/transport data.
