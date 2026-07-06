// The TypeScript mirror of transport-core's channel seam.
//
// The PWA transports do NOT import Rust code; they implement the SAME CONTRACT in
// TypeScript so the shared frontend's array-pusher expectations hold identically
// across shells (D8). send(bytes) broadcasts one opaque message; recv() returns a
// fresh snapshot of every message available (including our own prior sends),
// pull-based. The lattice join lives OUTSIDE transports and ignores SenderId.

// Opaque sender identity an attributable channel yields. Never interpreted by the
// frontend; the join ignores it (provenance is unauthenticated).
export type SenderId = Uint8Array;

// Bare-bytes channel: recv yields opaque blobs, no sender identity.
// (WebRTC data channel, OHTTP mailbox.)
export interface AnonymousChannel {
  send(message: Uint8Array): Promise<void>;
  recv(): Promise<Uint8Array[]>;
}

// Attributable channel: recv pairs each blob with the transport-supplied SenderId.
// (Nostr: sender pubkey.)
export interface AttributableChannel {
  send(message: Uint8Array): Promise<void>;
  recv(): Promise<Array<[SenderId, Uint8Array]>>;
}

// A transport the frontend can enable. Every PWA transport exposes this so the
// shell can start/stop it and drain received PSBTs into the shared array.
export interface PwaTransport {
  readonly kind: "sneakernet" | "webrtc" | "nostr" | "ohttp-mailbox";
  readonly grounded: boolean; // false => deferred skeleton (returns clear errors)
  start(): Promise<void>;
  stop(): Promise<void>;
  // Underlying channel (anonymous or attributable). The shell wraps recv() into
  // array pushes; SenderId, if present, is display-only.
  channel(): AnonymousChannel | AttributableChannel;
}
