# Transport Plugins

Out-of-process transports: a plugin is a **separate binary with its own
Cargo.lock**, spawned by `ptj`, spoken to over the child's stdin/stdout
with Cap'n Proto twoparty RPC.

This document records the decision and its contract. The abstract idea
(transports behind Cap'n Proto or WIT interface specifications) appears
in [transports.md](transports.md) and [traits.md](traits.md); this is
the concrete native-plugin half of it, motivated by a problem no amount
of in-process factoring can solve.

## Motivation: one lockfile cannot hold both

Cargo permits exactly one crate with a given `links` key per lockfile,
and therefore exactly one version of a `-sys` crate wrapping a given
native library. Two transports we want collide there:

- `transport-arti`: `arti-client 0.44` with `static-sqlite` pulls
  `rusqlite/bundled` → `libsqlite3-sys 0.37` (`links = "sqlite3"`).
- `transport-nym`: `nym-sdk 1.21` needs `libsqlite3-sys ^0.30.1`
  (same `links` key).

Feature gating does not help: **optional dependencies still resolve
into the lockfile**. The workspace lock pins `libsqlite3-sys 0.37.0`
today, which is why the in-workspace `transport-nym` crate's `nym`
feature sits empty — the grounded nym backend exists (the
transport-grounding lane wrote and verified it against `nym-sdk 1.21.2`) and simply cannot land its dependency next to arti's.

Waiting for upstreams to converge on one sqlite pin is not a strategy;
this class of conflict will recur (any two heavyweight SDKs may pin
incompatible `-sys` crates, ring/openssl vintages, protobuf codegens,
...). The resolution space is per-lockfile, so the fix is more
lockfiles: one per conflicting stack, each in its own process.

## Architecture

```
ptj (host)                                plugin (separate binary,
 sync driver                               own Cargo.lock)
 Box<dyn Transport>                        e.g. plugins/ptj-transport-nym
   │                                         │
   ▼                                         ▼
 PluginTransport ──spawn──► child process (stdio piped)
   │                                         │
   capnp-rpc twoparty vat ◄── stdin/stdout ──► capnp-rpc twoparty vat
   (client side)                             (server side; bootstrap
                                              exports Plugin)
```

- The **schema is the contract**: `crates/transport-plugin-api` holds
  `schema/transport.capnp` plus generated Rust bindings, and re-exports
  `capnp`/`capnp-rpc` so host and plugins build against one grounded
  RPC stack. Any language that speaks Cap'n Proto over stdio can
  implement a plugin; nothing in the contract is Rust-specific.
- The wire interface mirrors `transport-core`'s channel seam exactly:
  `publish(Data)` / `collect() -> List(Data)` moving opaque bytes, plus
  an attributable variant whose `collect` pairs each payload with the
  transport-supplied sender identity. No dedup, no ordering — the
  lattice join stays in the host, and its idempotence keeps duplicate
  delivery free.
- In the host, a plugin is just another `Box<dyn Transport>` selected
  by `--transport plugin --plugin <binary>` (ptj's `plugin-transports`
  feature). capnp-rpc's vat is single-threaded (`!Send`) while the
  `Transport` seam is `Send`, so the vat lives on a dedicated actor
  thread — the same actor-at-the-edge shape the iroh backend uses.
- Config is an opaque key/value passthrough (`--plugin-config k=v`,
  repeatable, duplicate keys allowed) delivered in the handshake. ptj
  never interprets it; it is the plugin's constructor arguments.

### Schema sketch

The authoritative schema lives in
`crates/transport-plugin-api/schema/transport.capnp`; its shape:

```capnp
interface Plugin {                      # the bootstrap capability
  handshake @0 (hello :Handshake) -> (result :HandshakeResult);
  anonymous @1 () -> (transport :Transport);
  attributable @2 () -> (transport :AttributableTransport);
}

interface Transport {
  publish @0 (message :Data) -> ();
  collect @1 () -> (messages :List(Data));
}

interface AttributableTransport {
  publish @0 (message :Data) -> ();
  collect @1 () -> (messages :List(AttributedMessage));
}

struct AttributedMessage { senderId @0 :Data; message @1 :Data; }

struct Handshake {
  protocolVersion @0 :UInt32;
  channelKind     @1 :ChannelKind;      # anonymous | attributable
  config          @2 :List(ConfigEntry); # opaque k=v passthrough
}

struct HandshakeResult { union { ok @0 :Handshake; err @1 :Error; } }
struct Error { message @0 :Text; }
```

Handshake refusal travels as the explicit `HandshakeResult.err` (a
structured message); `publish`/`collect` failures travel as Cap'n
Proto RPC exceptions — the transport error type is a human-readable
string on both sides of the wire.

### Lifecycle

1. **Spawn.** The host starts the plugin binary with stdin/stdout
   piped (`kill_on_drop` set) and stderr inherited — stderr is the one
   stream not carrying RPC, so plugin diagnostics stay visible.
1. **Handshake.** The host bootstraps the `Plugin` capability and
   calls `handshake` with its protocol version, preferred channel
   kind, and the config entries. The plugin answers with its own
   version and the kind it actually serves, or refuses (structured
   error). The host enforces an **exact protocol version match** —
   there is deliberately no compatibility window until a second
   protocol version exists to be compatible with.
1. **Serve.** The host requests the transport capability matching the
   negotiated kind and forwards the sync driver's `publish`/`collect`
   as RPCs, one at a time (the driver is a single logical task —
   nothing to pipeline). Expensive setup (e.g. connecting to a mixnet
   gateway) belongs AFTER the handshake, lazily on first use, so
   spawn/handshake stay prompt.
1. **Shutdown.** Dropping the host handle closes the request channel,
   ends the vat, and closes the child's stdin; the plugin exits on
   EOF. `kill_on_drop` reaps a child that does not.

### The native-only boundary

Plugins are a **native** (POSIX process) mechanism, deliberately
scoped:

- **Browser/PWA transports stay in-process** behind the existing
  frontend `Backend` seam (WebRTC, nostr-ws, OHTTP mailbox in the
  demo-gui/PWA stacks). A browser cannot spawn binaries; nothing here
  applies to it.
- **In-process Rust transports remain the default and are not
  deprecated.** iroh, str0m, the payjoin directory — anything whose
  dependency tree coexists in the workspace lock keeps the simpler,
  faster in-process path. A transport moves out of process only when
  its stack is heavy or lock-incompatible (nym today; arti equally
  could be if the conflict were ever inverted).
- WASM-component plugins (the WIT half sketched in transports.md)
  remain the portable/sandboxed future; they are not displaced by
  this, and the interface shapes are kept aligned so a transport
  can move between the two.

### Supervision and restart

Scaffolded today: bounded handshake wait (a wedged binary cannot hang
`ptj sync` forever), `kill_on_drop` as the reaper backstop, and
per-stage errors (spawn / handshake / per-RPC) so a crashed plugin
surfaces as a precise transport error on the next call rather than a
hang.

Deliberately NOT scaffolded yet (future work, in dependency order):

1. restart-on-crash with fresh spawn + handshake (safe because the
   protocol is stateless between calls: the lattice join makes
   republish-after-restart idempotent);
1. exponential backoff and a crash-loop budget, after which the
   transport reports permanently failed;
1. killing a wedged child at handshake timeout (today it is killed at
   drop);
1. a transport-metadata method on the wire contract (e.g. "our mixnet
   address", which the nym plugin currently announces on stderr).

### Security posture

- **Plugin binaries are user-installed programs.** `--plugin` names a
  binary the user chose to place on their system, exactly as trusted
  as any other program they run. ptj adds no curation, no downloading,
  no registry — installing a plugin IS granting it your authority.
- **No dynamic code in-process.** Plugins are processes behind a byte
  pipe, never `dlopen`'d libraries: a plugin cannot corrupt ptj's
  memory, and the blast surface of its (heavy, transitively-audited)
  dependency tree is confined to its own address space. The host
  parses nothing from the plugin except capnp messages.
- **The web GUI cannot reach plugins.** A browser request that could
  name a binary to execute would be remote code execution; the
  `/api/sync` path pins the plugin selection off. Choosing to run a
  plugin is a CLI-user decision only.
- Message bytes stay opaque end to end, and folding remains fail-safe
  under `SIGHASH_ALL` (see [security.md](security.md)): a malicious
  plugin can withhold or garble messages — denial of service on one
  transport — but cannot make the lattice sign something the user did
  not confirm.
