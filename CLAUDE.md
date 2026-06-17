# scoutsched

Deterministic FRC scouting schedule generator. A Rust library crate plus a CLI
binary, both named `scoutsched`. Given a Blue Alliance qualification schedule and a
scout roster, it builds a complete, hard feasible scouting grid: per team expert
assignments, primary coverage, filler watches, pit scouting, breaks, and
quantitative or qualitative tagging.

## Commands

- Build: `cargo build` (release: `cargo build --release`)
- Test: `cargo test` (unit tests per module plus integration tests in `tests/`)
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt --all` (check with `cargo fmt --all -- --check`)
- Run on sample data: `cargo run -- gen-sample --teams 36 --matches-per-team 12 --seed 1 --out matches.json`
  then `cargo run -- --matches-file matches.json --config examples/config.toml`

## Architecture

Modules under `src/`:

- `model.rs` core types: `TeamId`, `ScoutId`, `Match`, `Cell`, `Scout`, and the
  `MatchBitset` used for availability.
- `config.rs` the TOML config: global hyperparameters, per scout settings, priority
  weights, and validation with clear errors.
- `tba.rs` the Blue Alliance v3 client and the shared match JSON parser used by both
  the live fetch and `--matches-file`.
- `sample.rs` the deterministic seeded sample generator. The only randomness in the
  project; never used by the solver.
- `solver/` the deterministic engine. `mod.rs` holds the orchestrator, the `Plan`
  working state, and config resolution. The phases are `expertise.rs`,
  `coverage.rs`, `fill.rs`, `breaks.rs`, and `repair.rs`.
- `report.rs` computes coverage, per scout load, and the priority ordered list of
  relaxed soft constraints.
- `output.rs` renders CSV, JSON, and a human readable summary.
- `cli.rs` the clap based command line, including the `gen-sample` subcommand.
- `lib.rs` and `main.rs` the library surface and the thin binary shell.

See `docs/architecture.md` for the algorithm phases, the complexity argument, and
the guarantees.

## Conventions

- Rust edition 2021. Strong typing, small integer ids, bitsets for availability.
- No randomness in the solver. Fixed total tie breaking by indices and stable
  sorts. Identical input gives byte identical output.
- Hard constraints are structural where possible: availability and own pit duty are
  initialized as unavailable cells that phases never overwrite, and no double
  booking follows from one cell per (scout, match) pair.
- The solver always returns a complete, hard feasible schedule. Over constraint is
  handled by relaxing soft constraints in priority order and reporting it, never by
  failing.
- No default parameter values in the public domain types; construct them
  explicitly.

## Soft constraint priority

Relaxed lowest priority first: coverage, then pit and filler quotas, then the
qualitative fraction, then sparsity, then break matching. The order is configurable
by weight with sane defaults.

## Testing rules

- Every module has unit tests. Integration tests in `tests/` prove the core
  guarantees: across a large sweep of team counts, scout counts, and constraint
  sets the solver always returns a complete hard feasible schedule, and identical
  input gives byte identical output.
- A bug fix starts with a failing test.

## Release

- Gates that must pass: `cargo fmt --all -- --check`, then
  `cargo clippy --all-targets -- -D warnings`, then `cargo test`, then
  `cargo build --release`.
- Semantic versioning. Update `CHANGELOG.md`. Tag `vX.Y.Z` and cut a GitHub
  release. Publishing to crates.io is pending.

## Style

- No em dash characters in code, comments, docs, or commit messages. Use commas,
  periods, colons, semicolons, or hyphens.
- Comments explain non obvious reasoning only.
- Never add a Co-authored-by trailer or mention AI assistance in commits.
