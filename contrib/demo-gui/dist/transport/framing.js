// framing — the TS mirror of transport_core::frame / deframe.
//
// u32 big-endian length prefix + value, with the shared MAX_FRAME_LEN=16 MiB cap.
// Used by stream/byte transports (WebRTC data channel, WebSocket relay events) to
// delimit records, since a large PSBT can exceed a single SCTP/WS message and the
// driver may put multiple Message envelopes on the wire. Payload is byte-
// transparent: on the ptj path it is a transport_core::Message TLV, but the
// transport never parses it (framing delimits; Message tags — orthogonal).
export const MAX_FRAME_LEN = 16 * 1024 * 1024; // 16 MiB, matches transport-core.
/** Prefix `value` with its u32-BE length. (`Uint8Array<ArrayBuffer>`: the
 * output is freshly allocated, so consumers like `RTCDataChannel.send` /
 * `BlobPart` — which reject a possibly-SharedArrayBuffer view — accept it.) */
export function frame(value) {
    if (value.length > MAX_FRAME_LEN) {
        throw new Error(`frame exceeds MAX_FRAME_LEN (${value.length} > ${MAX_FRAME_LEN})`);
    }
    const out = new Uint8Array(4 + value.length);
    const view = new DataView(out.buffer);
    view.setUint32(0, value.length, false); // big-endian
    out.set(value, 4);
    return out;
}
/**
 * Pull the next complete record from the front of `buf`. Returns the record and
 * the remaining buffer (trailing partial retained for the next poll), or null if
 * no complete record is present yet. Mirrors transport_core::deframe's
 * loop-until-partial behavior.
 */
export function deframe(buf) {
    if (buf.length < 4)
        return null;
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    const len = view.getUint32(0, false);
    if (len > MAX_FRAME_LEN) {
        throw new Error(`framed length exceeds MAX_FRAME_LEN (${len} > ${MAX_FRAME_LEN})`);
    }
    if (buf.length < 4 + len)
        return null; // incomplete; wait for more bytes.
    const record = buf.slice(4, 4 + len);
    const rest = buf.slice(4 + len);
    return { record, rest };
}
/** Drain every complete record from `buf`, returning records + trailing partial. */
export function deframeAll(buf) {
    const records = [];
    // .slice() re-anchors the caller's view on a fresh ArrayBuffer so the
    // retained partial is always ArrayBuffer-backed.
    let rest = buf.slice(0);
    for (;;) {
        const next = deframe(rest);
        if (next === null)
            break;
        records.push(next.record);
        rest = next.rest;
    }
    return { records, rest };
}
