# PWA shell — feature flags & build-mode toggles

Flags live in three places: (1) the `concurrent-psbt-wasm` crate's Cargo
features, (2) build-time JS/bundler defines, (3) runtime service-worker/manifest
config + per-session user toggles. Deferred (ungrounded) SDKs follow the
`transport-arti`/`transport-nym` skeleton pattern: **feature declared, dep
un-wired, `TODO(ground-deps)`, default compiles, feature-on authored-but-
unverified.**

## 1. `concurrent-psbt-wasm` crate Cargo features

(Reconciled: the merged wasm core carries NO transport features — the wasm core
is pure Layer-1/Layer-2, always local. The former `ptj-wasm` `webrtc`/`nostr`/
`signaling-ohttp` crate features are gone: browser transports live in the TS
layer (`src/transport/`) and, rust-side, in the `transport-str0m` /
`transport-webrtc-rs` / `transport-payjoin-dir` crates with their own features
(`str0m` / `webrtc-rs` / `payjoin-dir`). TODO(transport-nostr): a rust nostr
transport crate is unauthored.)

| Feature | Default | Grounded? | Effect |
|---|---|---|---|
| `default` | `[]` | yes | Core PSBT ops (inspect/create/join/sort/make-unordered/atomize/concatenate/import-bip174/export-bip174/pay/confirm/payments) + the `localSync` Layer-2 fold, exposed via wasm-bindgen. NO network, ever. This is the fully-grounded sneakernet PWA. |
| `debug-panic-hook` | off (on in dev) | yes | Wire `console_error_panic_hook` (via the `initPanicHook` export) for readable wasm panics in the browser console. Dev-only. |

Notes:
- The TS transports gate themselves: deferred paths return a clear runtime
  error until their deps/components are ground; grounded browser-native paths
  (web-sys RTCPeerConnection, WebSocket) work today.

## 2. Build-time JS / bundler defines

| Define | Default | Effect |
|---|---|---|
| `PTJ_BUILD` | `"pwa"` | Selects the shell. `"pwa"` → `WasmBackend`; `"webgui"` → `HttpBackend`; `"tauri"` → `TauriBackend`. All three are the ONE shared `Backend` interface; consumed by `src/backend/shell-backend.ts`. Only `"pwa"` is this component's concern. |
| `CPSBT_WASM_URL` | `"./pkg/concurrent_psbt_wasm_bg.wasm"` | Where the loader fetches the wasm binary (same-origin, cached by SW). |
| `PTJ_TRANSPORTS` | `"sneakernet"` | Comma list of TS transports COMPILED INTO the bundle. Sneakernet is always present. |
| `PTJ_CACHE_VERSION` | build hash | Service-worker cache name suffix; bump evicts stale shells. |
| `PTJ_DEFAULT_RELAYS` | `[]` | Optional default nostr relay list (empty = user must add). Never a signaling endpoint. |
| `PTJ_OHTTP_RELAY` / `PTJ_PJ_DIRECTORY` | unset | OHTTP relay + payjoin directory base URLs for signaling-ohttp. Unset until deps are ground; user-configurable in-app. NEVER the PWA origin. |

## 3. Runtime toggles (per-session, user-controlled, default OFF for network)

| Toggle | Default | Notes |
|---|---|---|
| Sneakernet (import/export/paste/QR) | ON | Always available; no network. Cannot be disabled. |
| WebRTC session | OFF | User opts in per session; requires signaling-ohttp configured. |
| Nostr session | OFF | User opts in; requires relay(s) configured. |
| OHTTP offline mailbox | OFF | The async fallback / standalone directory transport. |
| Camera (QR scan) | prompt | Browser permission; only when the user taps "scan". |

## Flag interaction summary

```
default PWA build  = concurrent-psbt-wasm default   → fully offline sneakernet + localSync, grounded, buildable today
+ TS webrtc opt-in  (signals via transport-payjoin-dir/OHTTP) → live P2P; native web-sys path grounded
+ TS nostr-ws opt-in                                → recurring-collaborator path (ws grounded)
+ OHTTP mailbox opt-in                              → async offline mailbox (also the WebRTC signaling channel)
```

The ONLY combination that is buildable-and-verifiable end-to-end today is the
default crate plus the browser-native WebRTC/WebSocket TS paths. Everything
depending on `str0m`/`webrtc-rs`/`ohttp`/`payjoin` (and the unauthored
transport-nostr) stays authored-but-unverified until those deps are ground,
exactly per constraint 5.
