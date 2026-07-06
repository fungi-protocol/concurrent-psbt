# Integration Tests

Flake checks demonstrating the motivation for concurrent-psbt and
verifying that `ptj` handles the scenarios Bitcoin Core cannot.

## `joinpsbt-gap`: Bitcoin Core's limitations

Shows two fundamental gaps in Bitcoin Core's PSBT merging:

### Gap 1: Output duplication (`joinpsbts`)

Two PSBTs with disjoint inputs but the same output destination.
`joinpsbts` duplicates the output, silently creating an overspend.

```mermaid
graph LR
    A["PSBT A<br/>input: coin₁<br/>output: 1 BTC → addr"] --> J["joinpsbts(A, B)"]
    B["PSBT B<br/>input: coin₂<br/>output: 1 BTC → addr"] --> J
    J --> R["Result<br/>inputs: coin₁, coin₂<br/>outputs: <b>2×</b> 1 BTC → addr"]
    style R fill:#f88,stroke:#a00
```

### Gap 2: Rejection of concurrent contributions (`combinepsbt`)

Two PSBTs that spend the same input but have different output sets.
`combinepsbt` rejects them because the unsigned transactions differ,
even though the intent is to merge both parties' outputs.

```mermaid
graph LR
    A["Alice's PSBT<br/>input: coin₁<br/>output: → alice_addr"] --> C["combinepsbt(A, B)"]
    B["Bob's PSBT<br/>input: coin₁<br/>output: → alice_addr, → bob_addr"] --> C
    C --> R["❌ REJECTED<br/>unsigned transactions differ"]
    style R fill:#f88,stroke:#a00
```

The script then runs `ptj` positive controls for the same protocol gaps:
`ptj join A A` keeps one input and one output, and joining two same-input
constructor contributions keeps the input once while unioning distinct outputs.

## `sneakernet-lattice`: Structural consequences

Demonstrates what happens in a realistic multi-party scenario where
copies propagate redundantly via sneakernet (USB sticks, email, etc.).

### Scenario: Three-party coinjoin

```mermaid
sequenceDiagram
    participant A as Alice
    participant B as Bob
    participant C as Carol

    Note over A,C: Each party has a funded UTXO

    rect rgb(230, 245, 255)
    Note over A,C: Phase 1: Create contributions
    A->>A: createpsbt(input_A, output_A)
    B->>B: createpsbt(input_B, output_B)
    C->>C: createpsbt(input_C, output_C)
    end

    rect rgb(255, 240, 230)
    Note over A,C: Phase 2: Sneakernet exchange (unordered)
    A-->>B: USB with PSBT_A
    B-->>A: USB with PSBT_B
    B-->>C: USB with PSBT_B
    C-->>B: USB with PSBT_C
    A-->>C: USB with PSBT_A
    C-->>A: USB with PSBT_C
    end

    rect rgb(230, 255, 230)
    Note over A,C: Phase 3: Each party joins locally
    A->>A: joinpsbts(A, B, C) → ???
    B->>B: joinpsbts(B, A, C) → ???
    C->>C: joinpsbts(C, A, B) → ???
    end
```

With `joinpsbts`, the results depend on who already merged what.
Overlapping inputs cause rejection. Duplicate outputs cause overspend.
The same script then runs the three-party scenario through `ptj` and checks
that all paths converge after deterministic sorting. The sorted BIP 370 PSBTs
are exported to BIP 174 before asking Bitcoin Core to decode, sign, finalize,
or broadcast them.

### Lattice property violations

```mermaid
graph BT
    A[PSBT_A] --> AB["A + B"]
    B[PSBT_B] --> AB
    B --> BC["B + C"]
    C[PSBT_C] --> BC
    A --> AC["A + C"]
    C --> AC

    AB --> ABC_1["(A+B) + C"]
    C --> ABC_1
    A --> ABC_2["A + (B+C)"]
    BC --> ABC_2
    AC --> ABC_3["(A+C) + B"]
    B --> ABC_3

    ABC_1 -. "should equal" .-> GOAL
    ABC_2 -. "should equal" .-> GOAL
    ABC_3 -. "should equal" .-> GOAL

    GOAL["A ⊔ B ⊔ C"]

    style GOAL fill:#8f8,stroke:#0a0

    ABC_1 --> IDEM["ABC + ABC<br/>= ABC?"]
    ABC_1 --> REDUND["ABC + A<br/>= ABC?"]

    style IDEM fill:#ff8,stroke:#aa0
    style REDUND fill:#ff8,stroke:#aa0
```

| Property | `joinpsbts` | `ptj join` |
|----------|-------------|------------|
| Idempotent: `join(X, X) = X` | ❌ rejects (overlapping inputs) | ✅ |
| Commutative: `join(A, B) = join(B, A)` | ⚠️ modulo shuffling | ✅ |
| Associative: `join(join(A,B), C) = join(A, join(B,C))` | ❌ rejects | ✅ |
| Absorbing: `join(ABC, A) = ABC` | ❌ rejects | ✅ |
| Gossip-safe: `join(AB, BC, AC) = ABC` | ❌ rejects | ✅ |

## `ptj-sneakernet`: Verification

Runs the same three-party scenario using `ptj create` and `ptj join`,
verifying that all merge paths produce identical BIP 370 PSBTs after sorting
with the same seed. Each sorted PSBT is then exported to BIP 174 for Bitcoin
Core content checks and signing.

```mermaid
graph BT
    A["ptj create<br/>--input A --output A_dest"] --> AB["ptj join A B"]
    B["ptj create<br/>--input B --output B_dest"] --> AB
    B --> BC["ptj join B C"]
    C["ptj create<br/>--input C --output C_dest"] --> BC
    A --> AC["ptj join A C"]
    C --> AC

    AB --> ABC1["ptj join AB C"]
    C --> ABC1
    A --> ABC2["ptj join A BC"]
    BC --> ABC2
    AC --> ABC3["ptj join AC B"]
    B --> ABC3

    ABC1 --> SORT1["ptj sort --seed S"]
    ABC2 --> SORT2["ptj sort --seed S"]
    ABC3 --> SORT3["ptj sort --seed S"]

    SORT1 --> EQ["All identical ✅"]
    SORT2 --> EQ
    SORT3 --> EQ

    EQ --> CORE["ptj export-bip174<br/>Bitcoin Core decode/sign/finalize"]

    style EQ fill:#8f8,stroke:#0a0
```

Additionally verified:

- `ptj join ABC ABC` = ABC (idempotent)
- `ptj join ABC A` = ABC (redundant copy absorbed)
- `ptj join AB BC AC` = ABC (gossip merge)
- BIP 174 export decoded by Bitcoin Core: 3 inputs, 3 outputs, 29.997 BTC of outputs on every path
- BIP 174 export signs, finalizes, broadcasts, and mines on regtest
