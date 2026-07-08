# Offline-first — service worker, manifest, install, sneakernet

The PWA must run the FULL loop with NO network on a phone. This document is the
offline/local-first strategy.

## App shell = the installable, cached unit

The "app shell" is the immutable set of assets the loop needs:

```
index.html
manifest.webmanifest
sw.js                      (registered from index.html)
dist/app.js                (shared frontend, PWA-built)
dist/model.js              (shared frontend, pure)
dist/backend/*.js          (WasmBackend + loader + transports)
styles.css
pkg/concurrent_psbt_wasm.js        (wasm-bindgen ESM glue)
pkg/concurrent_psbt_wasm_bg.wasm   (the WASM binary)
icons/*.png                (maskable + any-purpose)
```

## Service worker (`app/sw.js`) strategy

- **install:** open a versioned cache (`ptj-pwa-<PTJ_CACHE_VERSION>`),
  `addAll(APP_SHELL)`. `skipWaiting()` so a new SW takes over promptly.
- **activate:** delete all caches whose name != the current version (evicts stale
  shells, mirrors webgui's `?v=` cache-bust). `clients.claim()`.
- **fetch:**
  - Same-origin app-shell request → **cache-first**, fall back to network, and on
    a network miss for `navigate` requests serve cached `index.html` (SPA offline
    fallback).
  - The `.wasm` → cache-first (it is immutable per version).
  - **Transport traffic is NEVER intercepted/cached.** WebSocket upgrades are not
    `fetch` events. OHTTP POST/GET to the payjoin directory + WebRTC are passed
    straight through (network-only) — caching session/transport bytes would be
    incorrect and dangerous (stale PSBTs).
- No runtime caching of cross-origin requests. Nothing about a peer's PSBT is
  ever persisted by the SW.

## Manifest (`app/manifest.webmanifest`)

Mobile-first, installable, standalone:

```json
{
  "name": "Partial Transaction Joiner",
  "short_name": "ptj",
  "start_url": "./index.html",
  "scope": "./",
  "display": "standalone",
  "orientation": "portrait",
  "background_color": "#0f1115",
  "theme_color": "#0f1115",
  "icons": [
    { "src": "icons/icon-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
    { "src": "icons/icon-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" },
    { "src": "icons/maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "maskable" }
  ]
}
```

`index.html` adds `<link rel="manifest">`, `<meta name="theme-color">`,
`<meta name="viewport" content="width=device-width, initial-scale=1,
viewport-fit=cover">`, and Apple touch icons / `apple-mobile-web-app-capable` for
iOS home-screen install.

## Sneakernet — the always-offline transport

No network of any kind. Implemented in `src/transport/sneakernet.ts`. Modes:

1. **Paste / type** — the existing dropzone accepts base64 PSBT (already in
   `app.ts` at the paste-dropzone). Pushes into the shared array.
2. **File import / export** — `<input type=file>` + a download of the current
   result (BIP-174 binary or base64), via the `import-bip174` / `export-bip174`
   ops (WASM, local). Works offline.
3. **Clipboard** — copy the current PSBT / paste from clipboard (where the
   Clipboard API is permitted); pure local.
4. **Animated QR display + camera scan (UR format)** — the line-of-sight
   transport from `app-suite.md`: show the current PSBT as an animated UR QR;
   scan another device's QR with the camera. Requires only the camera permission,
   no network. (QR encode/decode + UR chunking is a browser JS concern; the PSBT
   bytes it moves are the same array pushes.)

All four require the app shell to be cached, which the SW guarantees after first
load — so the entire "split the tab, airplane mode, line of sight" scenario works
with zero connectivity.

## Install & first-run

- First online load caches the shell; thereafter the app opens offline.
- "Add to Home Screen" (Android `beforeinstallprompt`; iOS Share → Add) makes it
  a standalone icon.
- No signup, no backend, no store — matches the `app-suite.md` "no install, no
  signup, no backend" promise; the PWA upgrades "no install" to "installable but
  optional."

## Privacy posture offline

Default state = offline sneakernet only; NO sockets open. Network transports are
opt-in (D5). The SW guarantees the shell never phones home. The only egress ever
possible is a transport the user explicitly enabled, and even then WebRTC
signaling is IP-hidden via signaling-ohttp (D6).
