# Contributing

Thank you for looking. Billfold is small and deliberately narrow — it reads, shows and
explains; it never changes anything. Contributions that keep it that way are welcome.

## What will not be merged

- Anything that writes to, moves or deletes a file the module looks at, even with a
  confirmation.
- Anything that opens a network connection: no update check, no telemetry, no remote probe.
- A claim "from memory": each rule, path or meaning needs its source (documentation page,
  source file, or an observation marked as such).
- A score, a grade or a colour that judges: the module states facts, the user judges.
- Showing a secret in clear anywhere, including in debug output.

## Running

`BILLFOLD_FIXTURE_OUT=/tmp/fx cargo test -p billfold-core --test scan write_fixture` writes the
test home to disk; `billfold --home /tmp/fx` and `billfold-app --home /tmp/fx` run on it.

```bash
cargo test --workspace
cargo clippy --all-targets -- -D warnings
packaging/build.sh deb          # needs cargo-deb
```

## The tower

Billfold is a module of the [Blackberrylabs](https://github.com/blackberrylabs/blackberrylabs)
control tower. A new kind of finding needs its place on the four common severities
(`Kind::severity` in `crates/core/src/model.rs`) and a line in the CHANGELOG saying so.
