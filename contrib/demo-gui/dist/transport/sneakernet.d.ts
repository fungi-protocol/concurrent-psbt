import type { AnonymousChannel, PwaTransport } from "./channel.js";
export declare class SneakernetTransport implements PwaTransport {
    readonly kind: "sneakernet";
    readonly grounded = true;
    private inbox;
    start(): Promise<void>;
    stop(): Promise<void>;
    channel(): AnonymousChannel;
    ingest(psbtBytes: Uint8Array): void;
}
export declare function base64ToBytes(b64: string): Uint8Array;
export declare function downloadPsbt(bytes: Uint8Array, filename: string): void;
