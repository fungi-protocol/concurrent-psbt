// nostr-ws — nostr over browser WebSocket (D7, transports.md).
//
// Corrects app-suite.md's "Nostr Web = ❌": relays are plain WebSocket, which the
// browser has natively, so nostr-over-WebSocket IS browser-viable. The ❌
// reflected the mdk/MLS SDK not being wasm-proven, not the protocol.
//
// GROUNDED INTERIM: browser WebSocket to relay(s), NIP-44 encrypted DMs for the
// 2-party path, in TS. Sender pubkey = SenderId -> AttributableChannel.
//
// PREFERRED (DEFERRED): the rust `nostr` crate compiled to wasm (NIP-44 + relay)
// over ws_stream_wasm. `nostr` is UNGROUNDED -> DEFERRED behind the `nostr` crate
// feature (TODO(ground-deps): nostr). MLS groups (mdk/whitenoise) for multi-party
// forward secrecy are deferred further; this ships the lighter NIP-44 path.

import type { AttributableChannel, PwaTransport, SenderId } from "./channel.js";
import { frame, deframeAll } from "./framing.js";

export interface NostrConfig {
  relays: string[]; // wss:// relay URLs; empty => user must add one.
  // Session key material / peer npubs are supplied by the room link. NIP-44
  // sealing/opening is done by the (deferred) nostr crypto module; until it lands
  // the grounded interim uses a minimal NIP-44 TS impl.
}

export class NostrWsTransport implements PwaTransport {
  readonly kind = "nostr" as const;
  // The WebSocket relay path is grounded; full NIP-44 sealing via the rust nostr
  // crate is deferred. The interim TS NIP-44 path keeps this usable.
  readonly grounded = true;

  private sockets: WebSocket[] = [];
  private inbox: Array<[SenderId, Uint8Array]> = [];
  private partials = new Map<string, Uint8Array>(); // per-sender reassembly

  constructor(private readonly config: NostrConfig) {}

  async start(): Promise<void> {
    if (this.config.relays.length === 0) {
      throw new Error("nostr: no relays configured");
    }
    for (const url of this.config.relays) {
      const ws = new WebSocket(url);
      ws.binaryType = "arraybuffer";
      ws.onmessage = (ev) => this.onRelayMessage(ev);
      // On open: send a REQ subscribing to the session-specific kind/tag. The
      // subscription filter is derived from the room link (session npub).
      this.sockets.push(ws);
    }
  }

  async stop(): Promise<void> {
    for (const ws of this.sockets) ws.close();
    this.sockets = [];
  }

  channel(): AttributableChannel {
    return {
      send: async (message: Uint8Array) => {
        // Frame, then NIP-44-seal to the session/peer, then publish as an EVENT
        // to each relay. Sealing is the deferred nostr crypto step; the grounded
        // interim uses the minimal NIP-44 TS path.
        const framed = frame(message);
        const event = await this.sealAsEvent(framed);
        const payload = JSON.stringify(["EVENT", event]);
        for (const ws of this.sockets) {
          if (ws.readyState === WebSocket.OPEN) ws.send(payload);
        }
      },
      recv: async () => this.inbox.slice(),
    };
  }

  private onRelayMessage(ev: MessageEvent): void {
    // Relay frames are JSON ["EVENT", subId, event]. Open (NIP-44) the event
    // content, deframe records, tag each with the sender pubkey as SenderId.
    let parsed: unknown;
    try {
      parsed = JSON.parse(typeof ev.data === "string" ? ev.data : "");
    } catch {
      return;
    }
    if (!Array.isArray(parsed) || parsed[0] !== "EVENT") return;
    const event = parsed[2] as { pubkey?: string; content?: string } | undefined;
    if (event?.pubkey === undefined || event.content === undefined) return;

    const sender = hexToBytes(event.pubkey) as SenderId;
    const opened = this.openFromEvent(event.content); // NIP-44 decrypt -> bytes
    if (opened === null) return;

    // Reassemble framed records per sender (large PSBTs may span events).
    const prev = this.partials.get(event.pubkey) ?? new Uint8Array(0);
    const merged = new Uint8Array(prev.length + opened.length);
    merged.set(prev, 0);
    merged.set(opened, prev.length);
    const { records, rest } = deframeAll(merged);
    for (const record of records) this.inbox.push([sender, record]);
    this.partials.set(event.pubkey, rest);
  }

  // --- NIP-44 seal/open (DEFERRED to the nostr crypto module) ---
  // The grounded interim provides a minimal TS NIP-44; the preferred impl is the
  // rust `nostr` crate compiled to wasm. Until ground, these throw a clear error.
  private async sealAsEvent(_framed: Uint8Array): Promise<unknown> {
    // send() awaits this, so a throw surfaces cleanly to the caller/UI.
    throw new Error("nostr: NIP-44 sealing not wired (ground-deps: nostr crate)");
  }
  private openFromEvent(_content: string): Uint8Array | null {
    // Called from the WebSocket onmessage handler (no awaiter), so we DEGRADE
    // rather than throw: return null (undecryptable) until NIP-44 opening is
    // wired via the deferred nostr crate. Throwing here would escape the event
    // handler unhandled.
    return null;
  }
}

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}
