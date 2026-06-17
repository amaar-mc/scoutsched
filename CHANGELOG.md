# Changelog

All notable changes to this project are documented here. The format follows
Keep a Changelog, and the project uses semantic versioning.

## [Unreleased]

## [0.1.0] - 2026-06-17

Initial release.

### Added

- Deterministic scouting schedule solver: a constructive heuristic with bounded
  local search that always returns a complete, hard feasible grid. Phases for
  expertise assignment, primary coverage, filler and pit and mode tagging, break
  and sparsity enforcement, and coverage repair.
- Hard constraints enforced by construction: per scout availability windows, own
  pit duty blocks, one activity per scout per match, and one team per scout per
  match.
- Soft constraints relaxed by a configurable priority order with sane default
  weights: coverage, then pit and filler quotas, then qualitative fraction, then
  sparsity, then break matching. Every relaxation and coverage gap is reported.
- The Blue Alliance API v3 client for `GET /event/{event_key}/matches`, plus a
  local `--matches-file` path that shares the same parser so the tool runs with no
  network and no API key.
- A deterministic seeded sample data generator that produces balanced
  qualification schedules with six distinct teams per match, for testing without a
  key.
- TOML configuration with global hyperparameters, per scout settings, priority
  weights, and validation with clear error messages. Selected globals can be
  overridden by CLI flags.
- CSV, JSON, and human readable summary outputs, and a `--out-dir` mode that writes
  all three.
- A `gen-sample` CLI subcommand to write sample events.
- Unit tests per module and integration tests proving the solver always returns a
  complete hard feasible schedule across a large sweep of instances, and that
  identical input gives byte identical output.

### Notes

- Availability is expressed by match index. A clock time string is accepted in the
  config schema but is reserved for a future build; using a clock time currently
  returns a clear error rather than guessing a mapping.
- Publishing to crates.io is pending. The crate is available from GitHub.
