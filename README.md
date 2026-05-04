# Lattice-PSBT

A data structure that models PSBTs as a [semilattice](https://en.wikipedia.org/wiki/Semilattice), enabling non-conflicting fragments to merge deterministically for eventual consistency in collaborative transaction construction.

Note: This is a work in progress.

## Motivation

PSBT ([BIP-174](https://github.com/bitcoin/bips/blob/master/bip-0174.mediawiki) / [BIP-370](https://github.com/bitcoin/bips/blob/master/bip-0370.mediawiki)) is a data encoding format that packages everything a signer needs to produce a valid signatures (inputs, outputs, BIP-32 derivation paths, etc.) The standard also defines functional roles: a constructor that builds the base transaction, one or more signers that attach partial signatures, and a combiner that merges the fragments into a final transaction.

But PSBT itself is completely agnostic about semantics. It doesn’t express any notion of ordering, data dependency, or construction lifecycles. It’s a static container and not a model for how transactions evolve. Most protocols end up ignoring the "roles" defined in the specs.

This becomes a problem for collaborative transaction construction protocols where multiple parties cooperate in a privacy-preserving manner to construct a transaction without a central coordinator. Two concrete examples:

- Silent Payments: outputs depend on the full set of inputs, so all inputs must be known before an output key can even be derived.
- Coordinator-less CoinJoins: peers learn and merge transaction fragments incrementally, requiring deterministic merge and ordering semantics that still respect inner-transaction dependencies.

Without a shared model for how PSBTs are constructed and merged over time, each protocol is forced to reinvent ad-hoc solutions. This fragments the ecosystem and makes wallet interoperability much harder.

## The Problem

Current PSBT standards are insufficient for coordinator-less transaction construction protocols. The existing role model (constructor / signer / combiner) assumes a single party builds the transaction skeleton upfront. It provides no standard way to express:

- Ordering semantics: a canonical ordering that all peers can independently derive
- Merge semantics: deterministic rules for combining independently learned fragments
- Data dependencies: constraints that some fields (e.g. output keys) can only be finalized once other fields (e.g. the full input set) are known

For example: when "inputs finalized" PSBT is merged with a PSBT that includes new inputs, those inputs should be rejected.  

## The Solution

We propose relaxing the current PSBT model into something that encodes ordering, merge, and data-dependency semantics directly in the format.

Concretely, we model a PSBT as a state-based conflict-free replicated data type (CRDT) -- a [semilattice](https://en.wikipedia.org/wiki/Semilattice) -- where peers can merge updates as they learn new inputs, outputs, or other metadata. This ensures collaboration produces a coherent transaction state regardless of message ordering or duplication, without requiring a coordinator.

The open question is whether this requires an extension to BIP-370 or a new standard that more fundamentally relaxes the PSBT role model.

For more information on CRDTs please see this [comprehensive write up](https://inria.hal.science/file/index/docid/555588/filename/techreport.pdf)

## TODOs

CRDT correctness:
  * [ ] 
- [ ] Fix `BTreeMap::join`  currently non-commutative (last writer wins on key collision). Should return an error on conflicting values, consistent with how `Option<T>` join handles conflicts.
- [ ] Add property tests verifying the semilattice laws (commutativity, associativity, idempotency) across all `Join` impls. Without these, convergence is asserted by intuition rather than checked.
Protocol / features

- [ ] Convert to PSBTv0 alongside PSBTv2
- [ ] Allow for unordered internal inputs and outputs
- [ ] Joining on the typestate should revert state in some cases ([lightning dual funding](https://bitcoinops.org/en/topics/dual-funding/))

Infrastructure

- [ ] Minimal CI pipeline
