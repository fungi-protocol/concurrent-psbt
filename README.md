# concurrent-psbt

Concurrency friendly PSBT merging for collaborative transaction construction.

See [spec](https://github.com/nothingmuch/multiparty-protocol-docs/blob/psbt/psbt.md) for details.

## Check fixtures

Each check fixture is a deliberately bad commit whose message names the Nix
check expected to reject it:

```text
[EXPECT-FAIL: cargo-shear] Add unused dependency
```

The `check-fixtures` branch registers fixtures as the non-first parents of its
merge commit. CI replays each fixture onto the revision under test and verifies
that its named check fails.

```console
nix run .#validate-check-fixtures
```

Recreate the registry merge with every fixture branch as a direct parent:

```console
git switch --detach <registry-base>
git merge --no-ff -s ours --no-edit fixture/cargo-shear fixture/another-check
git branch -f check-fixtures HEAD
git switch check-fixtures
git push --force-with-lease origin check-fixtures
```
