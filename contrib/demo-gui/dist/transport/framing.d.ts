export declare const MAX_FRAME_LEN: number;
/** Prefix `value` with its u32-BE length. (`Uint8Array<ArrayBuffer>`: the
 * output is freshly allocated, so consumers like `RTCDataChannel.send` /
 * `BlobPart` — which reject a possibly-SharedArrayBuffer view — accept it.) */
export declare function frame(value: Uint8Array): Uint8Array<ArrayBuffer>;
/**
 * Pull the next complete record from the front of `buf`. Returns the record and
 * the remaining buffer (trailing partial retained for the next poll), or null if
 * no complete record is present yet. Mirrors transport_core::deframe's
 * loop-until-partial behavior.
 */
export declare function deframe(buf: Uint8Array): {
    record: Uint8Array<ArrayBuffer>;
    rest: Uint8Array<ArrayBuffer>;
} | null;
/** Drain every complete record from `buf`, returning records + trailing partial. */
export declare function deframeAll(buf: Uint8Array): {
    records: Uint8Array<ArrayBuffer>[];
    rest: Uint8Array<ArrayBuffer>;
};
