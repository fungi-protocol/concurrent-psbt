// ohttp-mailbox — thin PWA adaptor over the signaling-ohttp component (D6).
//
// The BIP 77 payjoin-directory-over-OHTTP client is its OWN component
// (signaling-ohttp). This file is only the PWA-side adaptor that:
//   (1) presents the AnonymousChannel shape for the async offline fallback, and
//   (2) presents the Signaling handle that webrtc.ts consumes for SDP/ICE.
// BOTH roles are ONE mechanism / ONE mailbox: the directory carries the WebRTC
// handshake AND, once the data channel is up, remains the offline fallback for a
// peer who is not online.
//
// CRITICAL (D6): the client IP is hidden from the directory by the OHTTP relay;
// the directory only relays HPKE-sealed blobs it cannot read. NEVER a direct
// signaling server, NEVER the PWA origin. ohttp/payjoin are UNGROUNDED -> this
// adaptor DELEGATES to the (deferred) signaling-ohttp client and, until that
// client lands, returns a clear "not configured" error.

import type { AnonymousChannel, PwaTransport } from "./channel.js";

// The signaling-ohttp client interface this adaptor delegates to. Provided by the
// separate signaling-ohttp component; declared here as the contract the PWA needs.
// Blobs are opaque HPKE-sealed bytes to the directory.
export interface SignalingOhttpClient {
  // send/recv the async PSBT fallback (mailbox slots), bare opaque bytes.
  put(blob: Uint8Array): Promise<void>;
  poll(): Promise<Uint8Array[]>;
  // Signaling helpers for WebRTC (SDP/ICE as sealed blobs in ordered slots).
  isInitiator: boolean;
  sendOffer(sdp: RTCSessionDescriptionInit): Promise<void>;
  recvOffer(): Promise<RTCSessionDescriptionInit>;
  sendAnswer(sdp: RTCSessionDescriptionInit): Promise<void>;
  recvAnswer(): Promise<RTCSessionDescriptionInit>;
  sendCandidate(c: RTCIceCandidateInit): Promise<void>;
  recvCandidates(): Promise<RTCIceCandidateInit[]>;
}

// The Signaling handle webrtc.ts consumes. It is exactly the signaling subset of
// the client above.
export interface Signaling {
  isInitiator: boolean;
  sendOffer(sdp: RTCSessionDescriptionInit): Promise<void>;
  recvOffer(): Promise<RTCSessionDescriptionInit>;
  sendAnswer(sdp: RTCSessionDescriptionInit): Promise<void>;
  recvAnswer(): Promise<RTCSessionDescriptionInit>;
  sendCandidate(c: RTCIceCandidateInit): Promise<void>;
  recvCandidates(): Promise<RTCIceCandidateInit[]>;
}

// The async offline-fallback transport (AnonymousChannel over the mailbox).
export class OhttpMailboxTransport implements PwaTransport {
  readonly kind = "ohttp-mailbox" as const;
  // ohttp/payjoin are ungrounded; this is a deferred skeleton until the
  // signaling-ohttp client is wired.
  readonly grounded = false;

  constructor(private readonly client: SignalingOhttpClient | undefined) {}

  async start(): Promise<void> {
    if (this.client === undefined) {
      throw new Error(
        "ohttp-mailbox: signaling-ohttp client not configured " +
          "(ground-deps: ohttp + rust-payjoin v2 BIP-77 directory client)",
      );
    }
  }

  async stop(): Promise<void> {
    // Nothing to close; polling is driven by the shell.
  }

  channel(): AnonymousChannel {
    return {
      send: async (message: Uint8Array) => {
        if (this.client === undefined) {
          throw new Error("ohttp-mailbox: not configured");
        }
        await this.client.put(message);
      },
      recv: async () => {
        if (this.client === undefined) return [];
        return this.client.poll();
      },
    };
  }

  // Expose the signaling subset for webrtc.ts.
  signaling(): Signaling {
    if (this.client === undefined) {
      throw new Error("ohttp-mailbox: signaling not configured");
    }
    return this.client;
  }
}
