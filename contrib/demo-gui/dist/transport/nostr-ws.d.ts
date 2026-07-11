import type { AttributableChannel, PwaTransport } from "./channel.js";
export interface NostrConfig {
    relays: string[];
}
export declare class NostrWsTransport implements PwaTransport {
    private readonly config;
    readonly kind: "nostr";
    readonly grounded = true;
    private sockets;
    private inbox;
    private partials;
    constructor(config: NostrConfig);
    start(): Promise<void>;
    stop(): Promise<void>;
    channel(): AttributableChannel;
    private onRelayMessage;
    private sealAsEvent;
    private openFromEvent;
}
