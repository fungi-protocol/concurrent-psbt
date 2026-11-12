//! Stateright model of replicas gossiping `JoinResult<V>` states.
//!
//! The replica state and its merge are the crate's own `JoinResult` and `Join` code, not a
//! transcription: the model only supplies the gossip protocol around them.
//!
//! Each replica starts with one `Ok` value, its contribution. A step has a replica pull a peer's
//! state and join it into its own. Alongside the state each replica tracks which contributions it
//! has seen, so that [`preservation`] can specify its state independently of the join (the
//! maximal contributions it has seen, under an order each value type states separately), and
//! [`convergence`] can state strong eventual consistency: replicas that have seen the same updates
//! hold equal states.

use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};

use concurrent_psbt::{Conflict, Join, JoinResult, PartialJoin};
use stateright::{Model, Property};

/// The shared carrier of the value types: `Absent`, `X` and `Y`, as in the TLA+ model, and `X1`,
/// a refinement of `X` that only the `tree` config uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Atom {
    Absent,
    X,
    X1,
    Y,
}

impl Atom {
    pub const ALL: [Atom; 3] = [Atom::Absent, Atom::X, Atom::Y];
    pub const TREE: [Atom; 4] = [Atom::Absent, Atom::X, Atom::X1, Atom::Y];
}

/// Debug-print a newtype as its contents, so counterexamples read `Ok(Y)`, not `Ok(Flat(Y))`.
macro_rules! transparent_debug {
    ($($newtype:ident),*) => {$(
        impl Debug for $newtype {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                Debug::fmt(&self.0, f)
            }
        }
    )*};
}

/// Flat `PartialJoin`: two values join only if they are equal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Flat(pub Atom);

impl PartialJoin for Flat {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self == other {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

/// Option-like `PartialJoin` (the `option` config) with a nontrivial least upper bound: `Absent`
/// is below `X` and `Y` (like `None` below `Some`), which are otherwise unrelated.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Optional(pub Atom);

impl PartialJoin for Optional {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        match (self.0, other.0) {
            (a, b) if a == b => Ok(self),
            (Atom::Absent, _) => Ok(other),
            (_, Atom::Absent) => Ok(self),
            _ => Err(Conflict::from_values([self, other])),
        }
    }
}

/// Tree-ordered `PartialJoin` (the `tree` config): `Absent` below everything, `X1` above `X`, and
/// `Y` incomparable to `X` and `X1`. A conflict `{X, Y}` must absorb `X1` as `{X1, Y}`,
/// refining one value while keeping the other.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tree(pub Atom);

impl PartialJoin for Tree {
    fn try_join(self, other: Self) -> JoinResult<Self> {
        if self.below(&other) {
            Ok(other)
        } else if other.below(&self) {
            Ok(self)
        } else {
            Err(Conflict::from_values([self, other]))
        }
    }
}

transparent_debug!(Flat, Optional, Tree);

/// What the model needs of a value type, on top of `PartialJoin`.
pub trait Value: PartialJoin + Clone + Debug + Eq + Hash + Send + Sync + 'static {
    /// The order `try_join` is meant to decide, stated separately so [`preservation`] does not
    /// trust the join it checks.
    fn below(&self, other: &Self) -> bool;
}

impl Value for Flat {
    fn below(&self, other: &Self) -> bool {
        self == other
    }
}

impl Value for Optional {
    fn below(&self, other: &Self) -> bool {
        self == other || self.0 == Atom::Absent
    }
}

impl Value for Tree {
    fn below(&self, other: &Self) -> bool {
        self == other || self.0 == Atom::Absent || (self.0 == Atom::X && other.0 == Atom::X1)
    }
}

/// A replica's `JoinResult<V>`, with the `Hash` that Stateright needs.
///
/// `Conflict` equality is set equality, so the hash must ignore element order too; `Conflict`
/// has no `Hash` of its own.
#[derive(Clone, PartialEq, Eq)]
pub struct Cell<V: PartialJoin>(pub JoinResult<V>);

impl<V: PartialJoin + Hash> Hash for Cell<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            Ok(v) => {
                0u8.hash(state);
                v.hash(state);
            }
            Err(conflict) => {
                1u8.hash(state);
                let mut elements: Vec<u64> = conflict
                    .into_iter()
                    .map(|v| {
                        let mut h = DefaultHasher::new();
                        v.hash(&mut h);
                        h.finish()
                    })
                    .collect();
                elements.sort_unstable();
                elements.hash(state);
            }
        }
    }
}

impl<V: Value> Debug for Cell<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Ok(v) => write!(f, "Ok({v:?})"),
            Err(conflict) => {
                f.write_str("Err{")?;
                for (i, v) in conflict.into_iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v:?}")?;
                }
                f.write_str("}")
            }
        }
    }
}

/// Every replica's contribution and state, and which replicas' contributions it has incorporated.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct State<V: PartialJoin> {
    pub contrib: Vec<V>,
    pub cells: Vec<Cell<V>>,
    pub seen: Vec<BTreeSet<usize>>,
}

impl<V: Value> Debug for State<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (r, (cell, seen)) in self.cells.iter().zip(&self.seen).enumerate() {
            if r > 0 {
                f.write_str(" | ")?;
            }
            let seen: Vec<String> = seen.iter().map(|s| format!("r{}", s + 1)).collect();
            write!(f, "r{} = {:?} seen {{{}}}", r + 1, cell, seen.join(","))?;
        }
        Ok(())
    }
}

/// Replica `to` pulls `from`'s state and joins it into its own.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pull {
    pub to: usize,
    pub from: usize,
}

impl Debug for Pull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "r{} := r{} join r{}",
            self.to + 1,
            self.to + 1,
            self.from + 1
        )
    }
}

/// `replicas` participants, each starting from one of `values`.
pub struct Gossip<V> {
    pub replicas: usize,
    pub values: Vec<V>,
}

impl<V: Value> Model for Gossip<V> {
    type State = State<V>;
    type Action = Pull;

    fn init_states(&self) -> Vec<State<V>> {
        // Every assignment of a value to each replica.
        let mut assignments: Vec<Vec<V>> = vec![vec![]];
        for _ in 0..self.replicas {
            assignments = assignments
                .into_iter()
                .flat_map(|prefix| {
                    self.values.iter().map(move |v| {
                        let mut next = prefix.clone();
                        next.push(v.clone());
                        next
                    })
                })
                .collect();
        }
        assignments
            .into_iter()
            .map(|values| State {
                cells: values.iter().cloned().map(|v| Cell(Ok(v))).collect(),
                contrib: values,
                seen: (0..self.replicas).map(|r| BTreeSet::from([r])).collect(),
            })
            .collect()
    }

    fn actions(&self, _state: &State<V>, actions: &mut Vec<Pull>) {
        for to in 0..self.replicas {
            for from in 0..self.replicas {
                if to != from {
                    actions.push(Pull { to, from });
                }
            }
        }
    }

    fn next_state(&self, last: &State<V>, Pull { to, from }: Pull) -> Option<State<V>> {
        let mut next = last.clone();
        // The crate's `impl Join for JoinResult<V>`.
        next.cells[to] = Cell(Join::join(
            last.cells[to].0.clone(),
            last.cells[from].0.clone(),
        ));
        next.seen[to] = &last.seen[to] | &last.seen[from];
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always("preservation", |_, state| preservation(state)),
            Property::always("convergence", |_, state| convergence(state)),
        ]
    }
}

/// Each replica holds the maximal contributions it has seen, `Ok` if there is one. This rules out
/// merges that converge by losing updates, and implies [`convergence`].
pub fn preservation<V: Value>(state: &State<V>) -> bool {
    state.seen.iter().zip(&state.cells).all(|(seen, cell)| {
        let seen: Vec<&V> = seen.iter().map(|&s| &state.contrib[s]).collect();
        let mut maximal: Vec<V> = Vec::new();
        for &v in &seen {
            let dominated = seen.iter().any(|&w| v != w && v.below(w));
            if !dominated && !maximal.contains(v) {
                maximal.push(v.clone());
            }
        }
        let expected = if maximal.len() == 1 {
            Ok(maximal.remove(0))
        } else {
            Err(Conflict::from_values(maximal))
        };
        cell.0 == expected
    })
}

/// Strong eventual consistency: replicas that have seen the same updates hold equal states.
pub fn convergence<V: Value>(state: &State<V>) -> bool {
    let n = state.cells.len();
    (0..n)
        .all(|r| (0..n).all(|s| state.seen[r] != state.seen[s] || state.cells[r] == state.cells[s]))
}

/// A named model configuration, as run by the `psbt-stateright` binary.
pub enum Config {
    /// `try_join` is `Ok` iff equal.
    Flat,
    /// `try_join` has `Absent` below `X` and `Y`.
    Option,
    /// `try_join` has `Absent` below everything and `X1` above `X`.
    Tree,
}

impl Config {
    pub fn parse(name: &str) -> Option<Config> {
        match name {
            "flat" => Some(Config::Flat),
            "option" => Some(Config::Option),
            "tree" => Some(Config::Tree),
            _ => None,
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Config::Flat => "flat",
            Config::Option => "option",
            Config::Tree => "tree",
        })
    }
}
