// ptj-pwa service worker — offline-first app-shell cache.
//
// Strategy (see offline-first.md):
//   - install : cache the immutable app shell under a versioned cache name.
//   - activate: evict every cache whose name != the current version.
//   - fetch   : cache-first for same-origin app-shell assets (incl. .wasm),
//               SPA offline fallback to index.html for navigations,
//               and NEVER intercept/cache transport traffic (WebSocket is not a
//               fetch event; OHTTP/WebRTC pass straight through, network-only).
//
// PTJ_CACHE_VERSION is replaced at build time (feature-flags.md). The literal
// below is the dev placeholder.
const CACHE_VERSION = "PTJ_CACHE_VERSION_PLACEHOLDER";
const CACHE_NAME = `ptj-pwa-${CACHE_VERSION}`;

// The app shell: everything the local loop needs offline. The build injects the
// content-hashed asset names; this list is the canonical set of shell entries.
const APP_SHELL = [
  "./",
  "./index.html",
  "./manifest.webmanifest",
  "./styles.css",
  "./dist/app.js",
  "./dist/model.js",
  "./dist/backend/wasm-backend.js",
  "./dist/backend/wasm-loader.js",
  "./pkg/concurrent_psbt_wasm.js",
  "./pkg/concurrent_psbt_wasm_bg.wasm",
  "./icons/icon-192.png",
  "./icons/icon-512.png",
  "./icons/maskable-512.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(APP_SHELL)),
  );
  // A new SW should take over promptly so a fresh shell version is served.
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) =>
        Promise.all(
          names
            .filter((name) => name.startsWith("ptj-pwa-") && name !== CACHE_NAME)
            .map((name) => caches.delete(name)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;

  // Only GETs are cacheable app-shell requests. Everything else (POST to an
  // OHTTP relay, etc.) goes straight to the network — never cached.
  if (request.method !== "GET") {
    return; // default: let the request hit the network untouched.
  }

  const url = new URL(request.url);

  // Only same-origin app-shell assets are cached. Cross-origin (relays,
  // directories, STUN/TURN) are never intercepted.
  if (url.origin !== self.location.origin) {
    return;
  }

  // Navigations: serve cached index.html when offline (SPA offline fallback).
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request).catch(() => caches.match("./index.html")),
    );
    return;
  }

  // App-shell assets (incl. the immutable .wasm): cache-first, then network.
  event.respondWith(
    caches.match(request).then((cached) => {
      if (cached) return cached;
      return fetch(request).then((response) => {
        // Cache successful same-origin responses so subsequent loads are offline.
        if (response && response.ok && response.type === "basic") {
          const clone = response.clone();
          caches.open(CACHE_NAME).then((cache) => cache.put(request, clone));
        }
        return response;
      });
    }),
  );
});
