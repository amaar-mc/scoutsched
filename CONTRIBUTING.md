# Contributing

Thanks for your interest in scoutsched. Contributions are welcome.

## Getting started

You need a recent stable Rust toolchain. Then:

```sh
git clone https://github.com/amaar-mc/scoutsched
cd scoutsched
cargo test
```

## Before you open a pull request

All of these must pass, in this order. CI runs the same checks.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Guidelines

- The solver is deterministic. Do not introduce randomness into the solving path.
  Any tie breaking must use a fixed total order over indices or a stable sort, so
  identical input keeps producing byte identical output. The determinism test must
  stay green.
- Hard constraints must never be violated. Availability, own pit duty, no double
  booking, and one team per scout per match are guaranteed by construction; keep
  them that way. The feasibility sweep in `tests/` must stay green.
- Over constrained instances must still return a complete schedule. Handle scarcity
  by relaxing soft constraints in the documented priority order and reporting it,
  never by failing.
- A bug fix starts with a failing test that reproduces the bug, then the fix.
- Prefer small, pure functions and explicit types. No default parameter values in
  the public domain types.
- No em dash characters in code, comments, docs, or commit messages. Use commas,
  periods, colons, semicolons, or hyphens.

## Commit messages

Use the form `type(scope): description`, for example
`fix(solver): keep coverage when forcing a break`. Keep the subject in the
imperative mood.

## Reporting bugs and requesting features

Open an issue using the templates under `.github/ISSUE_TEMPLATE`. For a solver bug,
include the config and the matches JSON (or the `gen-sample` parameters) that
reproduce it, so the result can be checked deterministically.
