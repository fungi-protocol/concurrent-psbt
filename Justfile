system := `nix eval --offline --raw --impure --expr builtins.currentSystem`
has_nom := `which nom 2>/dev/null || true`
nix_cmd := if has_nom != "" { "nom" } else { "nix" }

# Quick checks (fast dev cycle feedback)
check:
    {{ nix_cmd }} build --no-update-lock-file '.#checks.{{ system }}.quick'

# All flake checks
check-all:
    {{ if has_nom != "" { "nix flake check --no-update-lock-file --log-format internal-json -v 2>&1 | nom --json" } else { "nix flake check --no-update-lock-file" } }}

# Lint checks (formatting + source invariants)
lint:
    {{ nix_cmd }} build --no-update-lock-file '.#checks.{{ system }}.lint'
 
# Auto-format all files
fmt:
    nix fmt

# Run tests via cargo-nextest
test:
    cargo nextest run --no-tests=warn

# Run clippy lints
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Build nix coverage check
coverage:
    {{ nix_cmd }} build --no-update-lock-file '.#checks.{{ system }}.coverage'

# Scrub history: quick check + message hygiene for every commit
scrub:
    jj git export
    nix run --no-update-lock-file .#scrub-commit-history
