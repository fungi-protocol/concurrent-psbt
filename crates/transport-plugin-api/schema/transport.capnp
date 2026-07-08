# The wire contract between ptj (the host) and an out-of-process transport
# plugin (a separate binary with its OWN Cargo.lock, spawned by the host and
# spoken to over the child's stdin/stdout via Cap'n Proto twoparty RPC).
#
# Plugins exist for transport stacks whose dependency trees cannot share the
# workspace lockfile (e.g. arti's and nym-sdk's libsqlite3-sys pins collide on
# links = "sqlite3"). The contract mirrors the in-process seam exactly:
# transport-core's channel traits moved onto the wire, one method per trait
# method, opaque bytes only. See contrib/design/transport-plugins.md.
#
# Error signaling: `handshake` returns an explicit HandshakeResult union so a
# version/kind rejection carries structured detail. `publish`/`collect` report
# failures as Cap'n Proto RPC exceptions (the transport-core Error is a plain
# human-readable string, which is exactly what an RPC exception carries).

@0xd28852d70a572e71;

struct Handshake {
  # Exchanged once, immediately after spawn, before any transport call. The
  # host sends its view; the plugin answers (via HandshakeResult.ok) with its
  # own, and the host verifies the protocol version and channel kind.

  protocolVersion @0 :UInt32;
  # The capnp-level protocol revision (transport-plugin-api's
  # PROTOCOL_VERSION), NOT the plugin's own release version. The host refuses
  # to drive a plugin whose answer does not match its own version exactly —
  # the scaffold has no compatibility window yet.

  channelKind @1 :ChannelKind;
  # Which collect shape the plugin serves (Transport vs
  # AttributableTransport). In the host's hello: the kind it intends to
  # request. In the plugin's answer: the kind it actually offers.

  config @2 :List(ConfigEntry);
  # Opaque key/value passthrough from the host CLI to the plugin (peer
  # addresses, credentials paths, ...). The host never interprets these; they
  # are the plugin's analogue of a transport crate's config struct.

  enum ChannelKind {
    anonymous @0;
    # collect yields bare opaque bytes (transport-core's AnonymousChannel).
    attributable @1;
    # collect yields (senderId, message) pairs (AttributableChannel).
  }

  struct ConfigEntry {
    key @0 :Text;
    value @1 :Text;
  }
}

struct Error {
  # A structured error for the explicit-result paths (handshake rejection).
  # Matches transport-core's stringly-typed Error: a human-readable message.
  message @0 :Text;
}

struct HandshakeResult {
  union {
    ok @0 :Handshake;
    # The plugin's own handshake: its protocol version, the channel kind it
    # serves, and (echoed) config it accepted.
    err @1 :Error;
    # The plugin refuses service (unsupported version, bad config, ...).
  }
}

struct AttributedMessage {
  # One collected message from an attributable plugin: the opaque
  # transport-supplied sender identity (transport-core's SenderId bytes)
  # paired with the opaque payload.
  senderId @0 :Data;
  message @1 :Data;
}

interface Plugin {
  # The bootstrap capability the plugin's vat exports over stdio. The host's
  # lifecycle: spawn child -> bootstrap -> handshake (version check) ->
  # request the transport capability matching the negotiated kind -> drive
  # publish/collect -> drop capabilities and close stdin to shut down.

  handshake @0 (hello :Handshake) -> (result :HandshakeResult);

  anonymous @1 () -> (transport :Transport);
  # Fails (RPC exception) if the plugin's channel kind is not anonymous.

  attributable @2 () -> (transport :AttributableTransport);
  # Fails (RPC exception) if the plugin's channel kind is not attributable.
}

interface Transport {
  # transport-core's Transport/AnonymousChannel seam on the wire: opaque
  # bytes only, no dedup/ordering (the lattice join lives in the host, and
  # is idempotent/commutative/associative, so duplicates cost nothing).

  publish @0 (message :Data) -> ();
  # Broadcast one opaque message to all participants.

  collect @1 () -> (messages :List(Data));
  # A fresh snapshot of every message currently known to the plugin,
  # including our own prior publishes. Polled repeatedly by the host.
}

interface AttributableTransport {
  # The attributable variant: identical contract, but collect pairs each
  # payload with the transport-supplied sender identity.

  publish @0 (message :Data) -> ();

  collect @1 () -> (messages :List(AttributedMessage));
}
