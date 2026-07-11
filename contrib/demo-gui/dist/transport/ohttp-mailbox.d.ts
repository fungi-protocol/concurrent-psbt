import type { AnonymousChannel, PwaTransport } from "./channel.js";
export interface SignalingOhttpClient {
    put(blob: Uint8Array): Promise<void>;
    poll(): Promise<Uint8Array[]>;
    isInitiator: boolean;
    sendOffer(sdp: RTCSessionDescriptionInit): Promise<void>;
    recvOffer(): Promise<RTCSessionDescriptionInit>;
    sendAnswer(sdp: RTCSessionDescriptionInit): Promise<void>;
    recvAnswer(): Promise<RTCSessionDescriptionInit>;
    sendCandidate(c: RTCIceCandidateInit): Promise<void>;
    recvCandidates(): Promise<RTCIceCandidateInit[]>;
}
export interface Signaling {
    isInitiator: boolean;
    sendOffer(sdp: RTCSessionDescriptionInit): Promise<void>;
    recvOffer(): Promise<RTCSessionDescriptionInit>;
    sendAnswer(sdp: RTCSessionDescriptionInit): Promise<void>;
    recvAnswer(): Promise<RTCSessionDescriptionInit>;
    sendCandidate(c: RTCIceCandidateInit): Promise<void>;
    recvCandidates(): Promise<RTCIceCandidateInit[]>;
}
export declare class OhttpMailboxTransport implements PwaTransport {
    private readonly client;
    readonly kind: "ohttp-mailbox";
    readonly grounded = false;
    constructor(client: SignalingOhttpClient | undefined);
    start(): Promise<void>;
    stop(): Promise<void>;
    channel(): AnonymousChannel;
    signaling(): Signaling;
}
