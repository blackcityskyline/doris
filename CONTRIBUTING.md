# Contributing

## Before a commit

```
make gate
```

which is `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test` and `cargo doc --no-deps`. All four have to be clean. The
`cargo doc` run is not ceremony: it catches doc links to methods that no
longer exist, which is worse than a broken link because the link claims
an API is there.

## Style

Rust 2021. `snake_case` for values, `PascalCase` for types, max 100
columns, no trailing whitespace. No `.unwrap()` outside tests -- use `?`.
A `match` is exhaustive or has a `_ =>`.

Comments say what the code cannot. "Why this rather than the obvious
alternative" earns its place; "what this line does" does not -- the line
does. A comment about a decision that has since been reversed is
worse than no comment, because the next reader cannot tell which part
of the paragraph is still true.

## Tests

A test pins a *place*, not a function. The decision usually lives one
level above the function the fix is in, and a test that calls the
function directly passes with the routing broken. Three of the four
holes found while writing this rule were exactly that.

Live probes of a real tracker are `#[ignore]`d and live in
`tests/*_live_tests.rs`:

```
cargo test --test nyaa_live_tests -- --ignored --nocapture
```

Use `--test-threads=1` when two browsers at once have been seen to kill
a session mid-test. Never let them run in CI.

Tests that touch process-wide state (`$HOME`, the file logger) need
`#[serial]` *and* a file of their own -- one process, one `HOME` to
change. A pair of such tests once shared a file with 36 others and
flaked under a full parallel run.

## Commit messages

Imperative mood, and explain *why* for anything non-mechanical. A diff
already says what changed.