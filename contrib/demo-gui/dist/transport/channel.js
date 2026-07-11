// The TypeScript mirror of transport-core's channel seam.
//
// The PWA transports do NOT import Rust code; they implement the SAME CONTRACT in
// TypeScript so the shared frontend's array-pusher expectations hold identically
// across shells (D8). send(bytes) broadcasts one opaque message; recv() returns a
// fresh snapshot of every message available (including our own prior sends),
// pull-based. The lattice join lives OUTSIDE transports and ignores SenderId.
export {};
