import type { AnonymousChannel, PwaTransport } from "./channel.js";
import type { Signaling } from "./ohttp-mailbox.js";
export declare class WebRtcTransport implements PwaTransport {
    private readonly signaling;
    private readonly iceServers;
    readonly kind: "webrtc";
    readonly grounded = true;
    private pc;
    private dc;
    private inbox;
    private partial;
    constructor(signaling: Signaling | undefined, iceServers?: RTCIceServer[]);
    start(): Promise<void>;
    stop(): Promise<void>;
    channel(): AnonymousChannel;
    private attachDataChannel;
    private performHandshake;
    applyRemoteCandidate(init: RTCIceCandidateInit): Promise<void>;
}
