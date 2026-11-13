//! `psbt-stateright <flat|option|tree>... [--replicas N]`: exhaustively model-check each
//! configuration and exit non-zero, printing the counterexamples, if any violates `preservation`
//! or `convergence`.

use std::process::ExitCode;
use std::time::Instant;

use psbt_stateright::{Atom, Config, Flat, Gossip, Optional, Tree, Value};
use stateright::{Checker, Model};

/// Check `model` breadth-first. Returns `true` if every invariant holds in every reachable state.
fn check<V: Value>(name: &Config, model: Gossip<V>) -> bool {
    println!(
        "config {name}: {} replicas, values {:?}",
        model.replicas, model.values
    );
    let start = Instant::now();
    // One thread, so that BFS finds a shortest counterexample deterministically.
    let checker = model.checker().threads(1).spawn_bfs().join();
    println!(
        "explored {} unique states ({} generated), depth {}, in {:.2?}",
        checker.unique_state_count(),
        checker.state_count(),
        checker.max_depth(),
        start.elapsed(),
    );
    let mut holds = true;
    for property in ["preservation", "convergence"] {
        match checker.discovery(property) {
            None => println!("Property \"{property}\" holds in all reachable states"),
            Some(path) => {
                holds = false;
                let steps = path.into_vec();
                println!(
                    "Property \"{property}\" violated after {} steps:",
                    steps.len() - 1
                );
                let mut previous = None;
                for (i, (state, action)) in steps.iter().enumerate() {
                    match previous {
                        None => println!("  init"),
                        Some(action) => println!("  step {i}: {action:?}"),
                    }
                    println!("    {state:?}");
                    previous = action.as_ref();
                }
            }
        }
    }
    holds
}

fn main() -> ExitCode {
    let mut configs = Vec::new();
    let mut replicas = 3;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--replicas" {
            replicas = args
                .next()
                .and_then(|n| n.parse().ok())
                .expect("--replicas takes a number");
        } else {
            configs.push(Config::parse(&arg).expect("config must be `flat`, `option` or `tree`"));
        }
    }

    let mut ok = true;
    for config in &configs {
        ok &= match config {
            Config::Flat => check(
                config,
                Gossip {
                    replicas,
                    values: Atom::ALL.map(Flat).to_vec(),
                },
            ),
            Config::Option => check(
                config,
                Gossip {
                    replicas,
                    values: Atom::ALL.map(Optional).to_vec(),
                },
            ),
            Config::Tree => check(
                config,
                Gossip {
                    replicas,
                    values: Atom::TREE.map(Tree).to_vec(),
                },
            ),
        };
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
