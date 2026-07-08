// sneakernet — the always-on, NO-NETWORK transport (D4, offline-first.md).
//
// GROUNDED. Requires nothing but the (SW-cached) app shell and, for QR scanning,
// the camera permission. Modes: paste/type, file import/export, clipboard, and
// animated UR-QR display + camera scan. Every mode pushes bytes into the shared
// PSBT array — an AnonymousChannel whose "network" is a human moving a device or
// a file. This is the transport that makes airplane-mode / line-of-sight work.

import type { AnonymousChannel, PwaTransport } from "./channel.js";

export class SneakernetTransport implements PwaTransport {
  readonly kind = "sneakernet" as const;
  readonly grounded = true;

  // Everything received via paste/drop/scan lands here; recv() drains a snapshot.
  private inbox: Uint8Array[] = [];

  async start(): Promise<void> {
    // No sockets. Wiring of DOM paste/drop/file/clipboard/QR handlers to
    // ingest() is done by the shell when this transport is active.
  }

  async stop(): Promise<void> {
    // Nothing to tear down; detach DOM handlers at the shell level.
  }

  channel(): AnonymousChannel {
    return {
      // "send" for sneakernet = present the current PSBT for the human to move
      // (render an animated UR QR / offer a download). The concrete rendering is
      // a shell/UI concern; the channel records the outbound blob so recv()'s
      // snapshot-includes-own-sends contract holds.
      send: async (message: Uint8Array) => {
        this.inbox.push(message);
        // UI: (re)render the outbound QR / update the export payload.
      },
      recv: async () => this.inbox.slice(),
    };
  }

  // Ingest a PSBT that arrived out-of-band (pasted base64, dropped file, scanned
  // QR chunk assembled into a full UR). Called by the shell's DOM handlers.
  ingest(psbtBytes: Uint8Array): void {
    this.inbox.push(psbtBytes);
  }
}

// --- Helpers the shell wires to DOM (kept transport-local, no network) ---

// Decode a pasted base64 PSBT string into bytes (paste/type mode). The shared
// frontend already recognizes base64 PSBTs in the paste dropzone; this is the
// bytes-level counterpart for the sneakernet channel.
export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64.trim());
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// Offer the current result as a file download (export mode). Uses the export-bip174
// / raw bytes produced by the WASM backend; this only handles the download side.
export function downloadPsbt(bytes: Uint8Array, filename: string): void {
  // .slice() re-anchors on a fresh ArrayBuffer: BlobPart rejects a view that
  // could be SharedArrayBuffer-backed.
  const blob = new Blob([bytes.slice()], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// NOTE: animated UR-QR encode + camera scan/decode (chunking a large PSBT across
// frames) is a browser JS concern layered on top of this channel. It is a UI
// module the shell owns; the transport only cares about the assembled PSBT bytes.
// No network is involved in any sneakernet mode.
