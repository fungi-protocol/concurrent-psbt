# Environments and User Stories

How the protocol maps onto real platforms, real constraints, and
real people trying to batch a transaction together.

## Three platforms

| | POSIX | Web | Mobile |
|---|---|---|---|
| **Runtime** | Native binary, full OS access | Browser sandbox, WASM | App sandbox, OS-mediated APIs |
| **Networking** | Sockets, Tor, mDNS, BLE | fetch, WebRTC, WebSocket | Platform HTTP, BLE, NFC, WiFi Direct |
| **Storage** | Filesystem | IndexedDB, ephemeral | App sandbox, Keychain |
| **Background** | Daemon, cron, systemd | Service Worker (limited) | Background tasks (power-managed, OS kills) |
| **Crypto** | Full control (libsecp256k1) | WebCrypto + WASM | Platform keystore, SE, HSM |
| **Key access** | File, hardware wallet via USB/serial | In-memory only (no HW wallet) | Secure Enclave, platform biometrics |
| **NAT** | Can listen (with port forward) | Cannot listen (browser sandbox) | Cannot listen (carrier NAT) |
| **Power** | Unlimited | Tab can be closed/frozen | OS throttles background, kills apps |
| **Local discovery** | mDNS, BLE, WiFi Direct, NFC | WebBLE (Chrome only), no mDNS | BLE, NFC, WiFi Direct, AirDrop |

### POSIX (desktop, server, hardware wallet companion)

Full capabilities. Can run Tor, listen on sockets, access hardware
wallets via USB, run background daemons, store keys on disk. The
`ptj` CLI lives here. Cap'n Proto plugins run as child processes.

Constraints: none fundamental. NAT requires port forwarding or relay
for inbound connections.

### Web (browser extension, hosted wallet, casual join page)

The most constrained but the most accessible. Cannot listen for
connections (no server sockets). Cannot access hardware wallets
(no USB/serial). Keys must live in memory (WebCrypto or WASM).
Tabs can be closed mid-session.

But: WebRTC provides NAT-punching peer-to-peer channels. WASM runs
the lattice join at near-native speed. A "join this transaction"
link can onboard someone who has never heard of PSBTs.

WASM transport components run here natively. The browser is the
WASM host.

### Mobile (phone wallet app)

The middle ground. Has BLE, NFC, WiFi Direct for proximity. Has
platform HTTP for remote. Has Secure Enclave for keys. But: the
OS aggressively manages power. Background tasks are unreliable.
Push notifications require a server.

For proximity scenarios (meetup coinjoin, splitting the tab), mobile
is ideal: NFC tap to pair, BLE or WiFi Direct to exchange PSBTs,
camera for animated QR. For remote scenarios, mobile needs push
notification support (nostr relay, APNs/FCM via a server) to avoid
polling.

## User story taxonomy

Two axes classify every collaborative transaction:

### Axis 1: Proximity

**Proximate:** Participants are physically together. Can use
cameras, NFC, BLE, WiFi Direct, sound, visual confirmation.
No internet required. No NAT problem. No relay metadata.

**Remote:** Participants are separated by network. Need relay/server
infrastructure for NAT traversal. Metadata exposure to relays.
May be asynchronous (different timezones, different availability).

### Axis 2: Prior relationship

**Strangers:** No pre-existing channel. Need an introduction
mechanism (wormhole code, QR scan, public session link). No
pre-shared keys. No identity beyond this session.

**Acquaintances:** Have a pre-existing E2E encrypted channel
(nostr DMs, Signal, Matrix, email with PGP). Can send session
invitations directly. May have long-term identity (npub, PGP key).
May have transacted before.

### The four quadrants

```
                    Proximate              Remote
              ┌─────────────────────┬─────────────────────┐
              │                     │                     │
  Strangers   │  Meetup coinjoin    │  Batched payments   │
              │  Splitting the tab  │  Group deal         │
              │  Point-of-sale      │  Crowdfunding       │
              │                     │                     │
              │  QR scan, NFC tap   │  Wormhole code      │
              │  BLE, WiFi Direct   │  WebRTC link        │
              │                     │  Directory           │
              ├─────────────────────┼─────────────────────┤
              │                     │                     │
  Acquaint-   │  Multi-meetup       │  Net settlement     │
  ances       │  coinjoin           │  Creator support    │
              │  Family inheritance │  Recurring payments │
              │                     │  Exchange ops       │
              │                     │  Hawala network     │
              │  QR + pre-shared    │                     │
              │  contact info       │  Nostr DM           │
              │                     │  Matrix room        │
              │                     │  Email              │
              └─────────────────────┴─────────────────────┘
```

Each quadrant has different constraints:

**Proximate + Strangers:** Introduction must be instant and
physical. No time to exchange contact info. QR code or NFC tap.
Transport is local (BLE, WiFi Direct, animated QR). No internet
dependency. Session is ephemeral. Works on all three platforms
(POSIX via camera, mobile natively, web via WebRTC if both
have internet).

**Proximate + Acquaintances:** Can use pre-shared contacts for
richer coordination. The multi-meetup coinjoin lives here: people
who know each other meet periodically and extend the transaction.
Introduction uses existing contacts. Transport can mix local
(in-person exchange) and remote (sharing the PSBT between meetups
via encrypted messaging).

**Remote + Strangers:** The hardest quadrant. Need public
infrastructure: wormhole relay, payjoin directory, or a web page.
Neither party can be assumed to have any specific software
installed. WebRTC via a join link is the lowest friction. Directory
mailboxes are the most private. Both need a relay of some kind.

**Remote + Acquaintances:** The most common case for recurring
transactions. Pre-existing E2E channel means introduction is
trivial (send a session ticket via DM). Transport can be any
channel they already use. Push notifications via nostr relay or
Matrix homeserver. This is where automation lives: the wallet
recognizes the monthly pattern and pre-builds contributions.

### How proximity and relationship map to mechanisms

| | Introduction | Transport | Key exchange |
|---|---|---|---|
| **Prox + Stranger** | QR scan, NFC tap | Animated QR, BLE, WiFi Direct | Ephemeral, in-band |
| **Prox + Acquaint** | Contact lookup | Same + sneakernet relay | Pre-shared (npub, PGP) |
| **Remote + Stranger** | Wormhole code, join link | Directory, WebRTC | Ephemeral, wormhole-mediated |
| **Remote + Acquaint** | DM, email | Nostr, Matrix, email, directory | Pre-shared (NIP-44, PGP) |

### How the platforms serve the quadrants

| | POSIX | Web | Mobile |
|---|---|---|---|
| **Prox + Stranger** | Camera (QR) | WebBLE (limited) | NFC, BLE, camera, WiFi Direct |
| **Prox + Acquaint** | Camera + contacts | WebBLE + contacts | Full local stack + contacts |
| **Remote + Stranger** | All transports | WebRTC, directory | HTTP-based transports |
| **Remote + Acquaint** | All transports | WebRTC, WebSocket | Nostr (push), HTTP |

Mobile dominates the proximate quadrants (NFC, BLE, camera are
native). Web dominates remote + strangers (zero install). POSIX
dominates remote + acquaintances (full transport flexibility,
background operation) and hardware wallet workflows (USB access).

## How this informs the traits

### Transport must be synchronous and pull-based

Mobile and web both have constrained event models. Mobile background
tasks are killed unpredictably. Browser tabs freeze. A push-based
trait (`on_message` callback) requires the caller to maintain a
long-lived event loop, which is unreliable on both platforms.

Pull-based (`collect()` returns current messages) works everywhere:

- POSIX: call in a loop with `sleep`
- Web: call from `setInterval` or `requestAnimationFrame`
- Mobile: call from a timer, or on push notification wakeup

The transport implementation converts push to pull internally. A
nostr relay handler appends to a buffer; `collect()` drains it. This
is the adaptor pattern: push transport → internal buffer → pull
trait.

### Introducer must support both interactive and non-interactive modes

**Interactive:** QR scan (mobile camera), wormhole code entry
(all platforms), NFC tap (mobile). These require user interaction
to complete.

**Non-interactive:** Contact-based DM (wallet auto-accepts session
invitations from known peers). This enables the recurring payment
automation scenarios.

The trait doesn't distinguish: `create_session` and `join_session`
are the same calls. Whether they block for user input or resolve
immediately from contacts is an implementation detail.

### Session must tolerate interruption

Mobile apps get killed. Browser tabs close. The Session state
machine must be serializable: save to disk on interrupt, restore on
wake. The state is small (the joined PSBT + confirmation list), so
serialization is cheap.

This means Session should be `Clone + Serialize + Deserialize` (or
at least representable as bytes). The Cap'n Proto schema already
achieves this: each RPC call returns the new state, so the host can
persist it between invocations.

For `ptj` CLI: each invocation is stateless. Read files, process,
export. No persistence needed.

For `ptj net`: the session state lives in memory during the
interactive flow, but could be checkpointed to a file for
resumption.

### Local discovery needs platform abstraction

BLE, NFC, WiFi Direct, and mDNS are all platform-specific APIs.
The library shouldn't know about any of them. Instead, local
discovery is a transport that produces messages, just like any
other `Transport`.

A mobile app implements `BleTransport` using platform BLE APIs.
A POSIX tool implements `MdnsTransport` using mDNS. Both satisfy
the same trait. The library joins PSBTs regardless.

The introduction layer similarly abstracts local discovery:
`NfcIntroducer` taps to exchange a `SessionTicket`.
`QrIntroducer` scans to get one. Both implement `Introducer`.

### NAT traversal is the transport's problem, not the library's

The library is IO-free. NAT is an IO concern. Each transport
handles it differently:

- WebRTC: ICE/STUN/TURN built into the browser
- Iroh: iroh relay servers
- Tor: onion routing eliminates the problem
- BLE/NFC/WiFi Direct: local, no NAT
- Directory/Nostr: client-server, no NAT for the client

The `Transport` trait hides all of this. A NAT-punched WebRTC
channel and a file on a USB stick satisfy the same interface.

## Cross-platform composition

The most powerful scenarios combine platforms:

**Mobile creates, POSIX signs.** Alice creates a session on her
phone (NFC tap with Bob at dinner). Her phone's wallet doesn't
have the keys to a large UTXO. She goes home and imports the
session into her desktop wallet, which has hardware wallet access.
The `Session` state serialized from mobile, deserialized on desktop.

**Web onboards, mobile completes.** A "join this transaction" link
opens in a browser. The user sees the merged state, decides to
participate, and scans a QR code with their mobile wallet. The
browser did the introduction (WebRTC), the phone does the signing
(Secure Enclave).

**POSIX automates, mobile approves.** A server runs `ptj net` in
a cron job, building the monthly creator-support transaction. When
it's ready, it sends a push notification to each participant's
phone. They review and sign with biometric auth. The server
combines and broadcasts.

In each case, the same traits: `Introducer` on one device,
`Transport` bridging the gap, `Session` state transferred.
The lattice doesn't care which device computed the join.
