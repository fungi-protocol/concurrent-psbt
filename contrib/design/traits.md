# Trait Boundaries

## Overview

The core library is IO-free. It processes PSBTs and produces results.
All IO (network, filesystem, user interaction) lives outside the
library, behind trait boundaries.

This separation enables three usage modes from the same trait suite:

- **CLI batch**: read files, call traits once, write result
- **Interactive session**: poll a message store in a loop, react to
  state changes
- **Out-of-process plugin**: Cap'n Proto RPC or WASM component, each
  trait method is an RPC call

### The IO-free core

```rust
// Already exists in the library:
trait Join { fn join(self, other: Self) -> Self; }

// The core operation, IO-free:
fn process(state: JoinState, message: &[u8]) -> Result<JoinState, Error>;
fn export(state: &JoinState) -> Result<Vec<u8>, Error>;
```

The core never reads from the network, never writes to disk, never
blocks. It takes bytes in, returns bytes out. Everything else is the
caller's responsibility.

### MessageStore

The transport boundary. Decouples "where do messages come from" from
"how do I join them."

```rust
trait MessageStore {
    type Error;
    fn put(&mut self, message: Vec<u8>) -> Result<(), Self::Error>;
    fn list(&self) -> Result<Vec<Vec<u8>>, Self::Error>;
}
```

Implementations:

- `MemoryStore`: `Vec<Vec<u8>>`. For CLI `ptj join a.psbt b.psbt`.
- `DirectoryStore`: wraps `DirectoryLinkedMailbox`.
- `NostrStore`: wraps mdk, reads/writes via NIP-44 DMs or MLS group.
- `IrohStore`: wraps iroh document entries.
- `FileStore`: watches a directory for `.psbt` files. Sneakernet.

The interface is synchronous and pull-based. An async caller polls
`list()` periodically. A CLI caller calls `list()` once. The trait
doesn't prescribe timing.

```capnp
interface MessageStore {
    put  @0 (message :Data) -> ();
    list @1 () -> (messages :List(Data));
}
```

```wit
interface message-store {
    put: func(message: list<u8>) -> result<_, string>;
    list: func() -> list<list<u8>>;
}
```

Both Cap'n Proto and WIT use the same pull-based interface. For
push-based transports (nostr relay notifications, iroh sync events),
the transport implementation converts push to pull internally: the
push handler appends to a buffer, `list()` drains it.

### Introducer

The session establishment boundary. Produces or consumes a session
ticket.

```rust
trait Introducer {
    type Error;
    fn create_session(&mut self, local_psbt: &[u8])
        -> Result<SessionOffer, Self::Error>;
    fn join_session(&mut self, ticket: &SessionTicket)
        -> Result<(), Self::Error>;
}

struct SessionOffer {
    ticket: SessionTicket,
    display_code: Option<String>,  // "7-guitarist-revenge"
}

struct SessionTicket {
    data: Vec<u8>,  // opaque, transport-specific
}
```

Implementations:

- `WormholeIntroducer`: magic-wormhole-rs. Creates code, exchanges
  ticket.
- `NostrIntroducer`: sends NIP-44 DM with ticket to known npubs.
- `DirectIntroducer`: ticket is a pre-shared iroh NodeId or onion
  address.
- `FileIntroducer`: reads/writes ticket to a file. For scripting.
- `QrIntroducer`: displays ticket as QR, scans peer's QR.

The introducer is called once per session. After introduction, the
`MessageStore` handles ongoing communication.

```capnp
interface Introducer {
    createSession @0 (localPsbt :Data) -> (offer :SessionOffer);
    joinSession   @1 (ticket :Data) -> ();
}

struct SessionOffer {
    ticket      @0 :Data;
    displayCode @1 :Text;
}
```

```wit
interface introducer {
    record session-offer {
        ticket: list<u8>,
        display-code: option<string>,
    }
    create-session: func(local-psbt: list<u8>)
        -> result<session-offer, string>;
    join-session: func(ticket: list<u8>) -> result<_, string>;
}
```

### Session

The state machine boundary. Drives the join, confirmation, and
export steps. IO-free: the caller feeds messages in and reads
state out.

```rust
struct Session { /* ... */ }

impl Session {
    fn new(local_psbt: Vec<u8>, expected_peers: Option<usize>) -> Self;

    /// Feed a serialized PSBT. Idempotent (lattice absorbs dupes).
    fn process(&mut self, message: &[u8]) -> Result<Phase, Error>;

    /// Current phase.
    fn phase(&self) -> Phase;

    /// Export the joined PSBT (if conflict-free).
    fn export(&self) -> Result<Vec<u8>, Error>;

    /// Export the joined state even with conflicts (for diagnostics).
    fn export_raw(&self) -> Result<&JoinedState, Error>;

    /// Record a peer's confirmation.
    fn add_confirmation(&mut self, peer_id: &[u8], unique_id: &[u8]);

    /// Generate this peer's confirmation (if join is clean).
    fn local_confirmation(&self, peer_id: &[u8]) -> Option<Confirmation>;
}

enum Phase {
    Contributing,  // no peer PSBTs yet
    Converging,    // join has conflicts or missing peers
    Confirming,    // join is clean, awaiting confirmations
    Ready,         // all confirmations match, ready to sign
}

struct Confirmation {
    peer_id: Vec<u8>,
    unique_id: Vec<u8>,
}
```

The CLI flow:

```rust
let mut session = Session::new(my_psbt, None);
for file in &files {
    session.process(&fs::read(file)?)?;
}
assert!(session.phase() == Phase::Ready);
let result = session.export()?;
```

The network flow:

```rust
let mut session = Session::new(my_psbt, Some(3));
store.put(my_psbt)?;
loop {
    for msg in store.list()? {
        session.process(&msg)?;
    }
    match session.phase() {
        Phase::Confirming => {
            let conf = session.local_confirmation(my_id).unwrap();
            store.put(serialize_confirmation(&conf))?;
        }
        Phase::Ready => break,
        _ => sleep(poll_interval),
    }
}
let result = session.export()?;
sign_and_broadcast(&result);
```

The Cap'n Proto flow (external process):

```
host                          transport plugin
 |                                 |
 |-- createSession(my_psbt) ------>|
 |<-- offer(ticket, code) ---------|
 |                                 |
 |  (user shares code)             |
 |                                 |
 |-- list() ---------------------->|  (poll loop)
 |<-- [msg1, msg2] ---------------|
 |                                 |
 |  session.process(msg1)          |
 |  session.process(msg2)          |
 |  session.phase() == Confirming  |
 |                                 |
 |-- put(confirmation) ----------->|
 |                                 |
 |-- list() ---------------------->|
 |<-- [msg1, msg2, msg3, conf] ---|
 |                                 |
 |  session.phase() == Ready       |
 |  session.export() -> result     |
```

The Session processes messages but never sends them. The caller
decides what to write to the store and when. This inversion is
what makes the core IO-free: the Session is a pure function from
messages to state.

```capnp
interface Session {
    process         @0 (message :Data)     -> (phase :Phase);
    phase           @1 ()                  -> (phase :Phase);
    export          @2 ()                  -> (psbt :Data);
    addConfirmation @3 (peerId :Data,
                        uniqueId :Data)    -> (phase :Phase);
    localConfirmation @4 (peerId :Data)    -> (confirmation :Confirmation);
}

enum Phase {
    contributing @0;
    converging   @1;
    confirming   @2;
    ready        @3;
}

struct Confirmation {
    peerId   @0 :Data;
    uniqueId @1 :Data;
}
```

```wit
interface session {
    enum phase {
        contributing,
        converging,
        confirming,
        ready,
    }

    record confirmation {
        peer-id: list<u8>,
        unique-id: list<u8>,
    }

    process: func(message: list<u8>) -> result<phase, string>;
    phase: func() -> phase;
    export: func() -> result<list<u8>, string>;
    add-confirmation: func(peer-id: list<u8>, unique-id: list<u8>)
        -> phase;
    local-confirmation: func(peer-id: list<u8>)
        -> option<confirmation>;
}
```

### OutputMerger

The pre-sorting step from the spec: "Outputs with an identical
`PSBT_OUT_SCRIPT` can be merged, and their values summed."

```rust
/// Merge outputs sharing a script_pubkey by summing their amounts.
/// Called after join converges, before sorting.
/// Returns the number of outputs merged (0 = no duplicates).
fn merge_same_script_outputs(psbt: &mut UnorderedPsbt) -> usize;
```

This is a plain function, not a trait. It's part of the IO-free
core. The Sorter calls it before applying the ordering step.

Note: output merging is incompatible with silent payments (BIP 352),
where each sender produces a unique `script_pubkey` for the same
recipient address. Outputs to silent payment addresses will never
share a script and thus won't merge. This is correct behavior:
merging SP outputs would break the recipient's ability to detect
and spend them independently.

### How the traits compose

```
Introducer::create_session(my_psbt)
    → SessionOffer { ticket, display_code }
                                    (share ticket via QR, DM, etc.)
Introducer::join_session(ticket)
    → (session established)

MessageStore::put(my_psbt)          (publish to transport)

loop {
    messages = MessageStore::list()  (pull from transport)
    for msg in messages:
        Session::process(msg)        (IO-free join)

    match Session::phase():
        Confirming →
            conf = Session::local_confirmation(my_id)
            MessageStore::put(serialize(conf))
        Ready →
            psbt = Session::export()
            merge_same_script_outputs(&mut psbt)
            sort(psbt)
            sign(psbt)
            broadcast(psbt)
            break
}
```

The same trait calls, the same order, regardless of whether the
MessageStore is backed by files, a network, or a WASM component.
The Session never knows.
