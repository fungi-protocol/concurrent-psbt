import type { PwaTransport } from "./channel.js";
import { type NostrConfig } from "./nostr-ws.js";
import { type SignalingOhttpClient } from "./ohttp-mailbox.js";
export interface TransportOptions {
    signalingClient?: SignalingOhttpClient;
    nostr?: NostrConfig;
    iceServers?: RTCIceServer[];
}
export interface AvailableTransport {
    kind: PwaTransport["kind"];
    grounded: boolean;
    enabled: boolean;
    reason?: string;
    make(): PwaTransport;
}
export declare function availableTransports(opts: TransportOptions): AvailableTransport[];
