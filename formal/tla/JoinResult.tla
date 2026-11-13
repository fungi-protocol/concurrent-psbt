---------------------------- MODULE JoinResult ----------------------------
(***************************************************************************)
(* Replicas gossiping `JoinResult<V>` states, mirroring                    *)
(* crates/concurrent-psbt/src/lattice/partial.rs.                          *)
(*                                                                         *)
(* `Conflict<V>` is modelled as a set because its `PartialEq` is set       *)
(* equality; insertion order is unobservable through `==`. Like           *)
(* `Conflict::insert`, `Err` keeps only the maximal values.                *)
(*                                                                         *)
(* `Preservation` specifies each replica's state independently of `Join`: *)
(* the maximal contributions among those it has seen, `Ok` if there is    *)
(* one. It rules out merges that converge by losing updates, and implies  *)
(* `Convergence`, strong eventual consistency: two replicas that have      *)
(* received the same set of updates (in whatever order) hold equal states. *)
(***************************************************************************)
EXTENDS FiniteSets

CONSTANTS
    Replicas,   \* participants, each contributing one value
    Values,     \* the PartialJoin carrier type V
    Absent,     \* a distinguished element of Values (used by "option" mode)
    Mode        \* which `PartialJoin::try_join` to model

ASSUME Absent \in Values
ASSUME Mode \in {"flat", "option"}

(* The order `PartialJoin::try_join` decides.                             *)
(*  - "flat": only equal values join (the `Foo` impl in partial.rs tests). *)
(*  - "option": `Absent` is below everything else, otherwise flat, as the  *)
(*    `try_join` docs allow.                                               *)
Below(a, b) == a = b \/ (Mode = "option" /\ a = Absent)

Max(c) == {x \in c : \A y \in c : Below(x, y) => x = y}

Ok(v)  == [ok |-> TRUE,  v |-> v,      c |-> {}]
Err(c) == [ok |-> FALSE, v |-> Absent, c |-> Max(c)]

(* `PartialJoin::try_join`: the greater of two comparable values. *)
TryJoin(a, b) ==
    IF Below(a, b) THEN Ok(b)
    ELSE IF Below(b, a) THEN Ok(a)
    ELSE Err({a, b})

(* `impl Join for JoinResult<V>`, arm for arm. *)
Join(a, b) ==
    CASE  a.ok /\  b.ok -> TryJoin(a.v, b.v)
      [] ~a.ok /\ ~b.ok -> Err(a.c \cup b.c)
      [] ~a.ok /\  b.ok -> Err(a.c \cup {b.v})
      []  a.ok /\ ~b.ok -> Err({a.v} \cup b.c)

VARIABLES contrib, state, seen
vars == <<contrib, state, seen>>

Init ==
    /\ contrib \in [Replicas -> Values]
    /\ state = [r \in Replicas |-> Ok(contrib[r])]
    /\ seen = [r \in Replicas |-> {r}]

(* Replica `r` pulls `s`'s state and joins it into its own. *)
Sync(r, s) ==
    /\ r /= s
    /\ state' = [state EXCEPT ![r] = Join(state[r], state[s])]
    /\ seen'  = [seen  EXCEPT ![r] = seen[r] \cup seen[s]]
    /\ UNCHANGED contrib

Next == \E r, s \in Replicas : Sync(r, s)

Spec == Init /\ [][Next]_vars

TypeOK ==
    \A r \in Replicas :
        \/ state[r].ok /\ state[r].v \in Values
        \/ ~state[r].ok /\ state[r].c \subseteq Values /\ state[r].c /= {}

(* What a replica that has seen the contributions of `S` must hold. *)
Denote(S) ==
    LET m == Max({contrib[s] : s \in S})
    IN  IF Cardinality(m) = 1 THEN Ok(CHOOSE v \in m : TRUE) ELSE Err(m)

Preservation == \A r \in Replicas : state[r] = Denote(seen[r])

Convergence ==
    \A r, s \in Replicas : seen[r] = seen[s] => state[r] = state[s]
====
