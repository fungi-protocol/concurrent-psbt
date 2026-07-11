// contrib/demo-gui/src/shared-frontend/core/backend.ts
//
// Shared frontend core — the Backend interface (the abstraction point).
//
// This REPLACES the free-function + FetchLike client in
// contrib/demo-gui/src/backend.ts. Previously every op was a standalone
// function whose first arg was an injected `FetchLike`, and app.ts defeated the
// injection by hard-binding `window.fetch.bind(window)` at 7 call sites
// (app.ts:686,714,732,751,774,878,900). Here the seam is promoted to an
// interface: one method per op, same DTOs. app.ts receives a single Backend
// instance at init and calls `backend.<op>(...)` — no fetch threading, no shell
// coupling. The three shells swap ONLY the implementation.
export { PtjBackendError } from "./types.js";
