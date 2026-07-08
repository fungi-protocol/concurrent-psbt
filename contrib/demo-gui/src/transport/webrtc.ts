// webrtc — browser-native RTCPeerConnection data-channel transport (D7, transports.md).
//
// PREFERRED IMPL: the browser's own RTCPeerConnection (grounded; web-sys has it,
// and here we use the DOM API directly from TS). ONE reliable-ordered data
// channel; each message is a framed record (frame/deframe); received binary
// payloads are buffered and drained by recv() (push -> pull). A data channel has
// no verifiable sender identity -> AnonymousChannel.
//
// ALTERNATE IMPL (DEFERRED): str0m-in-wasm. str0m is UNGROUNDED, so it stays
// authored-but-unverified behind the `webrtc` crate feature's str0m sub-path
// (TODO(ground-deps): str0m). The browser-native path below is what ships.
//
// SIGNALING (CRITICAL, D6): SDP offer/answer + trickle ICE are exchanged ONLY
// through the signaling-ohttp component (BIP 77 payjoin directory via OHTTP
// relay). NEVER a signaling server or the PWA origin (either would learn the
// client IP). This transport consumes a Signaling handle it does not create.

import type { AnonymousChannel, PwaTransport } from "./channel.js";
import { frame, deframeAll } from "./framing.js";
import type { Signaling } from "./ohttp-mailbox.js";

export class WebRtcTransport implements PwaTransport {
  readonly kind = "webrtc" as const;
  // The browser-native path IS grounded; the transport as a whole depends on
  // signaling-ohttp being configured (ungrounded until ohttp/payjoin land), so
  // effectively it is enabled only when signaling is available.
  readonly grounded = true;

  private pc: RTCPeerConnection | null = null;
  private dc: RTCDataChannel | null = null;
  private inbox: Uint8Array[] = [];
  private partial = new Uint8Array(0);

  // signaling: the ONLY path SDP/ICE may travel. Provided by the shell after the
  // signaling-ohttp client is configured. If undefined, start() refuses to run
  // (never fall back to a direct signaling server).
  constructor(
    private readonly signaling: Signaling | undefined,
    private readonly iceServers: RTCIceServer[] = [],
  ) {}

  async start(): Promise<void> {
    if (this.signaling === undefined) {
      throw new Error(
        "webrtc: no signaling-ohttp channel configured; refusing to signal " +
          "(a direct signaling server would leak the client IP)",
      );
    }

    this.pc = new RTCPeerConnection({ iceServers: this.iceServers });

    // Trickle ICE: each local candidate is sealed and sent through the OHTTP
    // mailbox; remote candidates arrive the same way (drained in pollSignaling).
    this.pc.onicecandidate = (ev) => {
      if (ev.candidate && this.signaling) {
        void this.signaling.sendCandidate(ev.candidate.toJSON());
      }
    };

    // Either side may open the channel; handle both for mesh joins.
    this.pc.ondatachannel = (ev) => this.attachDataChannel(ev.channel);

    // Offerer opens a reliable-ordered channel; answerer receives via ondatachannel.
    const dc = this.pc.createDataChannel("ptj", { ordered: true });
    this.attachDataChannel(dc);

    // The offer/answer handshake and ICE polling loop run through signaling.
    await this.performHandshake();
  }

  async stop(): Promise<void> {
    this.dc?.close();
    this.pc?.close();
    this.dc = null;
    this.pc = null;
  }

  channel(): AnonymousChannel {
    return {
      send: async (message: Uint8Array) => {
        if (this.dc === null || this.dc.readyState !== "open") {
          throw new Error("webrtc: data channel not open");
        }
        // One framed record per message (records survive SCTP fragmentation and
        // multiple envelopes on the channel).
        this.dc.send(frame(message));
      },
      recv: async () => {
        // Fresh snapshot of everything buffered from ondatamessage so far.
        return this.inbox.slice();
      },
    };
  }

  private attachDataChannel(dc: RTCDataChannel): void {
    dc.binaryType = "arraybuffer";
    dc.onmessage = (ev) => {
      const chunk = new Uint8Array(ev.data as ArrayBuffer);
      // Accumulate and drain complete framed records; retain the trailing partial.
      const merged = new Uint8Array(this.partial.length + chunk.length);
      merged.set(this.partial, 0);
      merged.set(chunk, this.partial.length);
      const { records, rest } = deframeAll(merged);
      for (const record of records) this.inbox.push(record);
      this.partial = rest;
    };
    this.dc = dc;
  }

  // Offer/answer over the OHTTP mailbox. Which side offers is decided by the room
  // link (initiator flag); both sides then trickle ICE through the same mailbox.
  private async performHandshake(): Promise<void> {
    const sig = this.signaling!;
    if (sig.isInitiator) {
      const offer = await this.pc!.createOffer();
      await this.pc!.setLocalDescription(offer);
      await sig.sendOffer(offer);
      const answer = await sig.recvAnswer();
      await this.pc!.setRemoteDescription(answer);
    } else {
      const offer = await sig.recvOffer();
      await this.pc!.setRemoteDescription(offer);
      const answer = await this.pc!.createAnswer();
      await this.pc!.setLocalDescription(answer);
      await sig.sendAnswer(answer);
    }
    // Remote trickle candidates are drained by a shell-driven poll loop that
    // calls sig.recvCandidates() and feeds this.pc.addIceCandidate().
  }

  // Called by the shell's poll loop to apply remote ICE candidates.
  async applyRemoteCandidate(init: RTCIceCandidateInit): Promise<void> {
    await this.pc?.addIceCandidate(init);
  }
}
