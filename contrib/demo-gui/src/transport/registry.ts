// registry — enumerate and construct the PWA's opt-in transports.
//
// Sneakernet is always present (ON). Network transports are opt-in and default
// OFF (D5); the user enables one per session. WebRTC requires a configured
// signaling-ohttp client (D6) — never a direct signaling server. This registry is
// what the shell UI drives: list available transports, honoring build-time
// PTJ_TRANSPORTS and the grounded/deferred status of each.

import type { PwaTransport } from "./channel.js";
import { SneakernetTransport } from "./sneakernet.js";
import { WebRtcTransport } from "./webrtc.js";
import { NostrWsTransport, type NostrConfig } from "./nostr-ws.js";
import { OhttpMailboxTransport, type SignalingOhttpClient } from "./ohttp-mailbox.js";

// PTJ_TRANSPORTS is a build-time define (feature-flags.md), e.g. "sneakernet" or
// "sneakernet,webrtc,nostr". Sneakernet is forced on regardless.
declare const PTJ_TRANSPORTS: string | undefined;

export interface TransportOptions {
  // A configured signaling-ohttp client, if the user set OHTTP relay + payjoin
  // directory URLs. Required to enable webrtc; also powers the offline mailbox.
  signalingClient?: SignalingOhttpClient;
  nostr?: NostrConfig;
  iceServers?: RTCIceServer[];
}

export interface AvailableTransport {
  kind: PwaTransport["kind"];
  grounded: boolean;
  // enabled=false means compiled-in but not usable in the current config (e.g.
  // webrtc without a signaling client). The UI greys these out with a reason.
  enabled: boolean;
  reason?: string;
  make(): PwaTransport;
}

export function availableTransports(opts: TransportOptions): AvailableTransport[] {
  const compiled = parseCompiled();
  const list: AvailableTransport[] = [];

  // Sneakernet: always present, always usable, no network.
  list.push({
    kind: "sneakernet",
    grounded: true,
    enabled: true,
    make: () => new SneakernetTransport(),
  });

  if (compiled.has("webrtc")) {
    const hasSignaling = opts.signalingClient !== undefined;
    // Only attach `reason` when actually disabled: exactOptionalPropertyTypes
    // forbids assigning `undefined` to the optional `reason?: string`.
    list.push(
      withReason(
        {
          kind: "webrtc",
          grounded: true, // browser-native path
          enabled: hasSignaling,
          make: () =>
            new WebRtcTransport(
              opts.signalingClient
                ? new OhttpMailboxTransport(opts.signalingClient).signaling()
                : undefined,
              opts.iceServers ?? [],
            ),
        },
        hasSignaling
          ? undefined
          : "configure an OHTTP relay + payjoin directory (signaling-ohttp) to enable WebRTC",
      ),
    );
  }

  if (compiled.has("nostr")) {
    const hasRelays = (opts.nostr?.relays.length ?? 0) > 0;
    list.push(
      withReason(
        {
          kind: "nostr",
          grounded: true, // ws path; full NIP-44 via nostr crate deferred
          enabled: hasRelays,
          make: () => new NostrWsTransport(opts.nostr ?? { relays: [] }),
        },
        hasRelays ? undefined : "add at least one wss:// relay to enable nostr",
      ),
    );
  }

  if (compiled.has("ohttp-mailbox")) {
    const hasClient = opts.signalingClient !== undefined;
    list.push(
      withReason(
        {
          kind: "ohttp-mailbox",
          grounded: false, // ohttp/payjoin ungrounded -> deferred
          enabled: hasClient,
          make: () => new OhttpMailboxTransport(opts.signalingClient),
        },
        hasClient
          ? undefined
          : "ohttp/payjoin not yet ground; configure signaling-ohttp when available",
      ),
    );
  }

  return list;
}

// Attach an optional `reason` only when defined, satisfying
// exactOptionalPropertyTypes (which forbids `{ reason: undefined }`).
function withReason(
  base: Omit<AvailableTransport, "reason">,
  reason: string | undefined,
): AvailableTransport {
  return reason === undefined ? base : { ...base, reason };
}

function parseCompiled(): Set<string> {
  const raw = typeof PTJ_TRANSPORTS === "string" ? PTJ_TRANSPORTS : "sneakernet";
  const set = new Set(raw.split(",").map((s) => s.trim()).filter(Boolean));
  set.add("sneakernet"); // always available
  return set;
}
