// RETIRED (seam reconciliation 2026-07-06). The FetchLike/FetchResponse pair
// re-declared here was one of three forked backend contracts. The canonical
// seam is the `Backend` interface in shared-frontend/core/backend.ts; FetchLike
// survives ONLY as an implementation detail of the HTTP adapter and is defined
// (and exported) in shared-frontend/backends/http.ts. Nothing in the PWA
// imports this file anymore; it is kept as a breadcrumb.

export type { FetchLike, FetchResponse } from "../shared-frontend/backends/http.js";
