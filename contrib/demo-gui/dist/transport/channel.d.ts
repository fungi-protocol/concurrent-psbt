export type SenderId = Uint8Array;
export interface AnonymousChannel {
    send(message: Uint8Array): Promise<void>;
    recv(): Promise<Uint8Array[]>;
}
export interface AttributableChannel {
    send(message: Uint8Array): Promise<void>;
    recv(): Promise<Array<[SenderId, Uint8Array]>>;
}
export interface PwaTransport {
    readonly kind: "sneakernet" | "webrtc" | "nostr" | "ohttp-mailbox";
    readonly grounded: boolean;
    start(): Promise<void>;
    stop(): Promise<void>;
    channel(): AnonymousChannel | AttributableChannel;
}
