# ptj App Suite

Four apps, one library, one loop:

```
read PSBTs → join → show result → repeat until done
```

## The core

```rust
// This is the entire library API that matters:
fn join(a: &[u8], b: &[u8]) -> Vec<u8>
fn is_ok(psbt: &[u8]) -> bool
fn sort(psbt: &[u8], seed: &[u8]) -> Vec<u8>
```

Everything below is about getting bytes from person A to person B.

## Shared directory model

Every transport is a shared directory. Files appear. You join
whatever you find. The transport is how files get there.

```
┌─────────────────────────────────────┐
│           shared directory          │
│                                     │
│  alice.psbt  bob.psbt  carol.psbt  │
│                                     │
└──────┬──────────┬──────────┬────────┘
       │          │          │
     Alice       Bob       Carol
     (join)     (join)     (join)
       │          │          │
       └──────────┴──────────┘
              same result
```

______________________________________________________________________

## 1. ptj CLI (POSIX)

### Transports

**Filesystem (default):**

```bash
# Alice
ptj create --input txid:0 --output addr:0.01 > session/alice.psbt

# Bob (on the same machine, shared folder, NFS, sshfs, whatever)
ptj create --input txid:1 --output addr:0.01 > session/bob.psbt

# Either of them
ptj join session/*.psbt | ptj sort --seed $(head -c16 /dev/urandom | xxd -p) > final.psbt
```

**Directory watcher (interactive):**

```bash
ptj net --dir ./session/ --mine mine.psbt
# Watches ./session/ for new .psbt files
# Joins on every change
# Prints status: "2/3 peers, join clean" or "conflict in tx_version"
# Ctrl-C exports the current result
```

**Payjoin directory (remote):**

```bash
ptj net --directory https://payjo.in --mine mine.psbt
# Writes mine.psbt to a linked mailbox
# Polls for peers' PSBTs
# Same join loop
```

**Nostr (mdk):**

```bash
ptj net --nostr --npub npub1alice... --peers npub1bob...,npub1carol... --mine mine.psbt
# Sends session invite via NIP-44 DM
# PSBTs exchanged as encrypted DMs to a session-specific npub
# Push: peers' wallets get notified via relay
```

For recurring collaborators (monthly creator support pact, business
partners), nostr is the natural fit: you already have each other's
npubs. The session invite is a DM. No codes, no scanning.

**Magic wormhole (one-shot pairing):**

```bash
# Alice
ptj net --wormhole --mine mine.psbt
# prints: 7-guitarist-revenge

# Bob
ptj net --join 7-guitarist-revenge --mine mine.psbt
# Wormhole transfers session ticket
# Falls through to directory watcher or payjoin directory
```

**Iroh (peer-to-peer sync):**

```bash
ptj net --iroh --mine mine.psbt
# prints: iroh document ticket (long base32 string)
# or share via wormhole: ptj net --iroh --wormhole --mine mine.psbt

# Bob
ptj net --iroh --ticket <ticket> --mine mine.psbt
```

Each peer publishes their PSBT as an iroh document entry. Iroh's
set reconciliation syncs entries across all peers. NAT traversal
via iroh relay servers. Late joiners catch up automatically.

No polling: iroh pushes updates as they arrive. No directory
server: true P2P (relay only assists with NAT, doesn't store
data). Persistent entries survive disconnects.

Iroh uses custom QUIC protocols, so it runs on POSIX and mobile
(raw socket access) but not in browsers.

### The loop

```bash
while true; do
    psbts=$(ls "$dir"/*.psbt 2>/dev/null)
    result=$(ptj join $psbts)
    if ptj check "$result"; then
        echo "Ready. $n peers, no conflicts."
        # user reviews and signs
        break
    fi
    sleep 1
done
```

______________________________________________________________________

## 2. Web demo (browser)

A single page at `ptj.app`. No install. No signup. No backend
(except a signaling server for WebRTC and optionally a payjoin
directory proxy).

### User flow

1. Alice opens `ptj.app`, pastes or uploads her PSBT
1. Gets a room link: `ptj.app/#room=abc123`
1. Shares the link (text, QR on screen, airdrop)
1. Bob opens the link, pastes his PSBT
1. Both see the merged result update live
1. Each exports the result, signs in their own wallet, uploads
   the signed version
1. Page combines signatures, shows the final transaction

### Transports

**QR code on screen:**
The page displays the current PSBT as an animated QR (UR format).
Another phone scans it with camera. No internet needed between
the two devices, just line of sight.

For the return path (phone → browser): the phone displays its
PSBT as QR, the browser uses the laptop's camera to scan.

This works for splitting the tab: one person has the web page
on their laptop, everyone else scans with their phone wallet.

**WebRTC (peer-to-peer):**
The room link encodes a WebRTC offer. When Bob opens it, the
browser establishes a direct DTLS-encrypted connection to Alice.
A lightweight signaling server (could be a static site +
Cloudflare Workers) brokers the initial handshake.

After signaling, all PSBT exchange is peer-to-peer. The signaling
server sees that two peers connected but not what they exchange.

Multiple peers: each new joiner connects to all existing peers
(full mesh for small groups, gossip for larger ones). The lattice
join means any peer can relay to any other and duplicates are
harmless.

**Payjoin directory (via OHTTP proxy):**
The page polls a payjoin directory via `fetch()`. OHTTP
encapsulation ensures the directory can't read the PSBTs or
identify the participants.

This is the async fallback: if Bob can't open the link right now,
Alice's PSBT waits in the directory. Bob joins later.

### Architecture

```
┌─────────────────────────────────────┐
│          ptj.app (static)           │
│                                     │
│  ┌─────────┐  ┌──────┐  ┌───────┐  │
│  │ QR scan/ │  │WebRTC│  │ fetch │  │
│  │ display  │  │      │  │(OHTTP)│  │
│  └────┬─────┘  └──┬───┘  └──┬────┘  │
│       └───────────┴─────────┘       │
│              shared array           │
│       ┌─────────────────────┐       │
│       │  concurrent-psbt    │       │
│       │  (WASM)             │       │
│       │  join() / sort()    │       │
│       └─────────────────────┘       │
└─────────────────────────────────────┘
```

The library compiles to WASM. The page loads it. Each transport
pushes bytes into the same array. The join runs on every update.
~50KB WASM bundle.

### Splitting the tab with strangers

1. You're at dinner. Open `ptj.app` on your phone
1. Create a PSBT: "pay 0.005 BTC to this restaurant address"
1. Show the room QR to the table
1. Everyone scans, adds their share
1. The page shows: "6 peers, total 0.03 BTC to restaurant, clean"
1. Everyone taps "sign" in their wallet
1. One person broadcasts

Time: under a minute. No app install for anyone. The web page
is the coordinator, but it's running locally in each person's
browser. No server sees the transaction.

______________________________________________________________________

## 3. Android demo

A standalone app. Not a full wallet (no key management, no coin
selection). It creates PSBTs from user input, joins them, and
exports for signing in the user's real wallet.

### User flow

1. Open app, enter: input (txid:vout from your wallet), output
   (destination address + amount)
1. Tap "Create Session" → shows a QR code and a share sheet
1. Friends tap "Join" → scan QR or accept share link
1. Everyone sees the merged PSBT build up in real time
1. Tap "Export" → opens in their wallet app for signing
1. Import signed PSBT back → app combines signatures
1. Tap "Broadcast"

### Transports

**Nearby Share / BLE:**
Android's Nearby Connections API. Discover peers on the same WiFi
or via Bluetooth. No internet, no QR scanning, just "tap to join."

The app advertises a session ID. Nearby devices see it and connect.
PSBTs flow over the local channel. Works in airplane mode.

For splitting the tab: everyone at the table has the app, one
person creates, others see it appear in their "nearby" list.

**QR code (camera):**
Display PSBT as animated QR (UR). Scan with camera. Works
cross-platform: Android ↔ iOS, Android ↔ web page, Android ↔
hardware wallet.

Two-way: display your PSBT, scan theirs. The lattice join merges
whatever arrives in whatever order.

**Nostr (mdk):**
NIP-44 encrypted DMs. Session invite via DM to known npubs. Push
notifications via relay. For recurring collaborators who already
have each other's npubs. MLS groups via whitenoise for multi-party
forward secrecy.

**Iroh:**
Full iroh node on the device. Set reconciliation sync. NAT
traversal via relay. No polling, push-based updates. Works well
for small groups where all peers are online.

**Payjoin directory:**
HTTP polling via OkHttp. Same linked mailbox protocol as CLI and
web. OHTTP for metadata privacy.

This is the remote fallback: session persists even if peers aren't
on the same network. Share the session link via any messaging app.

### Architecture

```
┌─────────────────────────────────────┐
│          ptj Android app            │
│                                     │
│  ┌─────────┐  ┌──────┐  ┌───────┐  │
│  │ Nearby   │  │Camera│  │OkHttp │  │
│  │ Connect  │  │ QR   │  │(OHTTP)│  │
│  └────┬─────┘  └──┬───┘  └──┬────┘  │
│       └───────────┴─────────┘       │
│              shared list            │
│       ┌─────────────────────┐       │
│       │  concurrent-psbt    │       │
│       │  (JNI via UniFFI)   │       │
│       │  join() / sort()    │       │
│       └─────────────────────┘       │
└─────────────────────────────────────┘
```

Rust core compiled to a shared library (`.so`). UniFFI generates
Kotlin bindings. The transport layer is pure Kotlin using platform
APIs. The Rust core is the same code as CLI and web.

______________________________________________________________________

## 4. iOS demo

Same concept as Android. Swift UI. The platform differences
are in the transports.

### Transports

**Multipeer Connectivity (AirDrop-like):**
Apple's framework for local peer discovery and communication.
WiFi + Bluetooth. Automatic, zero-config. The closest thing to
"tap to join" on iOS.

Peers appear in a browser sheet. Tap to connect. PSBTs flow over
the Multipeer session. Works offline.

For splitting the tab: one person creates a session, everyone
else sees it appear on their phone like an AirDrop prompt.

**QR code (camera):**
Same as Android. Display animated QR, scan with camera. The UR
standard provides cross-platform interop with hardware wallets
(Keystone, Foundation, etc.).

**Nostr (mdk):**
Same as Android. NIP-44 DMs, MLS groups via whitenoise. Push
notifications via relay (or APNs bridge).

**Iroh:**
Same as Android. Full iroh node. QUIC-native, works well on iOS
which has good UDP socket support.

**Payjoin directory:**
URLSession polling with OHTTP. Same protocol.

### Architecture

```
┌─────────────────────────────────────┐
│          ptj iOS app                │
│                                     │
│  ┌─────────┐  ┌──────┐  ┌───────┐  │
│  │Multipeer │  │Camera│  │URLSes │  │
│  │Connectiv.│  │ QR   │  │(OHTTP)│  │
│  └────┬─────┘  └──┬───┘  └──┬────┘  │
│       └───────────┴─────────┘       │
│              shared array           │
│       ┌─────────────────────┐       │
│       │  concurrent-psbt    │       │
│       │  (C FFI via UniFFI) │       │
│       │  join() / sort()    │       │
│       └─────────────────────┘       │
└─────────────────────────────────────┘
```

Same Rust core, compiled to a static library (`.a`). UniFFI
generates Swift bindings.

______________________________________________________________________

## Cross-platform matrix

| Transport | CLI | Web | Android | iOS |
|---|---|---|---|---|
| Filesystem / dir watcher | ✅ | ❌ | ❌ | ❌ |
| Payjoin directory (OHTTP) | ✅ | ✅ | ✅ | ✅ |
| QR code (animated, UR) | ❌ | ✅ (display+scan) | ✅ | ✅ |
| Nostr / mdk | ✅ | ❌ | ✅ | ✅ |
| Iroh | ✅ | ❌ | ✅ | ✅ |
| Magic wormhole | ✅ | ❌ | ❌ | ❌ |
| WebRTC | ❌ | ✅ | ❌ | ❌ |
| Nearby / BLE | ❌ | ❌ | ✅ | ❌ |
| Multipeer Connectivity | ❌ | ❌ | ❌ | ✅ |

Every platform has at least one local transport (no internet) and
multiple remote transports. The payjoin directory is the universal
fallback. Nostr and iroh run everywhere except the browser (nostr
needs native relay connections, iroh needs QUIC). WebRTC fills the
browser's P2P gap.

## Splitting the tab: how it actually works

Six people at a restaurant. Three have iPhones, two have Android,
one has nothing but a browser.

1. **iPhone user** opens ptj, creates a session paying 0.005 BTC
   to the restaurant's address (scanned from the bill's QR)

1. **Other iPhone users** see the session via Multipeer Connectivity.
   Tap to join. Add their inputs and outputs.

1. **Android users** see a QR code on the first iPhone's screen.
   Scan it. Their app joins via QR, then switches to payjoin
   directory for ongoing sync.

1. **Browser user** gets a link texted to them. Opens `ptj.app`
   on their phone browser. Pastes their PSBT. Joins via WebRTC
   to anyone else who has the link, plus payjoin directory as
   fallback.

1. Everyone's screen shows the same thing: 6 inputs, 1 output
   (merged restaurant payments), 6 change outputs. Total:
   0.03 BTC to restaurant.

1. Each person taps "export to wallet", signs in their own wallet
   app (Blue Wallet, Muun, Sparrow, whatever), imports the signed
   PSBT back.

1. One person collects all signatures (they flow through the same
   transports) and broadcasts.

Meanwhile, two of the iPhone users know each other's npubs. Their
apps also sync via nostr in the background, so if one of them
leaves the restaurant before signing, the signed PSBT still
reaches them via relay push notification.

Three different platforms, five different transports, one
transaction. The lattice doesn't care.

## What to build first

1. **`ptj` CLI with `--dir` watcher.** The foundation. Tests
   everything.

1. **Web demo at `ptj.app`.** WebRTC + QR display. No backend.
   Static site + WASM. Proves the concept works without installing
   anything.

1. **Android app with QR + directory.** First mobile platform.
   UniFFI bindings. Proves the Rust core works on mobile.

1. **iOS app with Multipeer + QR.** Second mobile platform.
   Same UniFFI bindings.

1. **Cross-platform interop test.** All four apps in one room,
   splitting a tab on regtest.
