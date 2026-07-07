# Security Considerations

## Threat Model

**Adversarial participants.** A malicious peer can waste other
participants' time by joining a session and never signing, or by
contributing an input they don't control. The protocol is fail-safe:
nothing broadcasts without all `SIGHASH_ALL` signatures. But the
time-waste attack is real. Mitigations: session timeouts, reputation
via long-term nostr identity, small-value initial transactions to
build trust.

**Bridge metadata leakage.** A participant bridging two transports
(e.g. Tor and nostr) can correlate identities across networks. The
lattice properties make bridging algebraically free but not
privacy-free. Participants should understand that their bridge peer
has a privileged metadata position.

**Liveness.** If any participant goes offline before signing, the
transaction cannot be broadcast (`SIGHASH_ALL` requires all
signatures). The confirmation protocol terminates only when all
net-receivers confirm. A missing confirmation blocks all net-senders.
This is inherent to all-parties-must-sign schemes. Mitigation:
timeouts, session expiry, the ability to reconstruct a session
without the missing participant.

**Dust outputs.** A malicious participant could add a dust output to
a known address, potentially linking other participants' inputs to
an identity. The `MAX_TRANSACTION_WEIGHT` and minimum output value
policies (enforced at the PSBT level) provide some protection, but
participants should review the merged PSBT before signing.

**Input validity.** A participant who contributes an input they don't
own (or that has already been spent) wastes everyone's time: the
final transaction will be invalid. The protocol cannot prevent this
without a full node check, which is outside the IO-free core.
Wallets performing the IO layer should validate inputs before
signing.
