# scoutsched

A deterministic FRC scouting schedule generator. Given a Blue Alliance
qualification schedule and a scout roster, scoutsched builds a complete scouting
grid: who watches which team in each match, who is pit scouting, who is on break,
with quantitative and qualitative tagging. It is a Rust library crate plus a CLI
binary, both named `scoutsched`.

The solver is deterministic. There is no randomness and no machine learning.
Identical input always produces byte identical output, which the tests assert. The
only seeded randomness in the project is the sample data generator, which is not
part of solving.

## What it does

For a qualification schedule and a roster you describe in a small TOML file,
scoutsched produces:

- Per team expert assignments. Each scout becomes the expert on a few teams and
  primarily watches their matches. The number of experts per scout scales up
  automatically when there are more teams than the roster can otherwise cover, so
  every team gets an expert.
- Primary coverage. Every team in every match is watched by an available scout,
  preferring that team's expert.
- Filler watches. Each scout watches a few non expert teams for a baseline cross
  perspective.
- Pit scouting. Each scout gets pit scouting slots, placed only in matches where
  none of their expert teams play.
- Quantitative and qualitative tagging. A configurable minority of watches are
  tagged qualitative, spread across each scout.
- Breaks. Runs longer than a configurable limit are broken up, with a minimum break
  length and per scout sparsity preferences, and optional aligned breaks for
  partner scouts.

It always returns a complete, hard feasible schedule. When an instance is over
constrained it relaxes soft constraints by a documented priority and reports
exactly what it relaxed and any coverage gaps.

## The model

- A match is a time ordered column of six team slots, three red then three blue.
- A grid cell for a (scout, match) pair is one of: watch a team in a counting mode
  (quantitative or qualitative), pit scout a team, take a break, or unavailable.
- Each scout has an availability window, optional own pit duty blocks during which
  they cannot scout, a sparsity preference, and an optional break partner.

Hard constraints, never violated:

- Per scout availability windows.
- Per scout own pit duty blocks.
- No double booking: at most one activity per scout per match.
- One team per scout per match.

Soft constraints, relaxed lowest priority first when over constrained:

1. Coverage: every team has an expert, every team match is watched.
2. Pit scouting quota and filler quota.
3. Qualitative fraction target.
4. Per scout sparsity and density preference.
5. Break matching for partner scouts.

The priority order is configurable through weights, with sane defaults.

## Complexity

The decision version of this problem is NP-hard. It generalizes set cover (the
coverage objective with availability) and the interval and rest constraints of
nurse rostering and crew scheduling. scoutsched therefore uses a deterministic
constructive heuristic followed by bounded local search rather than an exact
solver. The heuristic guarantees a complete, hard feasible schedule on every input
and reports which soft objectives it relaxed; it approximates soft preference
optimality rather than proving it. See `docs/architecture.md` for the reduction
sketch and the full discussion.

## Install and build

scoutsched is not yet published to crates.io (pending). Build from source with a
recent stable Rust toolchain:

```sh
git clone https://github.com/amaar-mc/scoutsched
cd scoutsched
cargo build --release
# the binary is at target/release/scoutsched
```

Run the tests and gates:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Usage

### With sample data, no API key

Generate a deterministic sample event, then solve it. This is the fastest way to
try the tool and is exactly how the tests run.

```sh
# Generate a 12 team event where each team plays 9 matches.
scoutsched gen-sample --teams 12 --matches-per-team 9 --seed 3 --out matches.json

# Solve it against a roster and print the human readable summary.
scoutsched --matches-file matches.json --config examples/config.toml --format summary

# Or write all three artifacts (CSV, JSON, summary) into a directory.
scoutsched --matches-file matches.json --config examples/config.toml --out-dir out/
```

### With a real Blue Alliance event

Provide an event code and an API key. The key can come from the `--tba-key` flag or
the `TBA_API_KEY` environment variable. Get a key from your Blue Alliance account
page.

```sh
export TBA_API_KEY=your_read_api_key
scoutsched --event 2024svr --config examples/config.toml --format csv > schedule.csv
```

### Output formats

- `--format csv` prints the grid. Rows are scouts, columns are matches.
- `--format json` prints a structured document for programmatic use, including the
  per match team rosters and the full report.
- `--format summary` prints coverage, per scout load, and the relaxations.
- `--out-dir DIR` writes `schedule.csv`, `schedule.json`, and `summary.txt`.

### Flag overrides

Selected globals can be overridden on the command line without editing the config:
`--pit-quota`, `--filler-quota`, `--qualitative-fraction`, and `--max-consecutive`.

## Config format

A TOML file with an optional `[globals]` table and one `[[scouts]]` entry per scout.
Every field has a default, so the minimum is just a list of scout names.

```toml
[globals]
# Experts per scout, scaled up automatically if needed for coverage.
experts_per_scout_min = 2
experts_per_scout_max = 3
# Target fraction of a team's matches watched by one of its experts.
primary_fraction = 0.8
# Non expert watches and pit scouting slots per scout.
filler_quota = 1
pit_quota = 2
# Minority of watches tagged qualitative.
qualitative_fraction = 0.25
# Break rules.
max_consecutive = 4
min_break_length = 1
# Bounded local search iteration cap. Higher costs time, never correctness.
repair_iterations = 2000

# Soft constraint weights, higher means favored when objectives conflict.
[globals.weights]
coverage = 1000.0
quotas = 100.0
qualitative = 10.0
sparsity = 1.0
break_match = 0.5

[[scouts]]
name = "Ada"

[[scouts]]
name = "Grace"
# Availability as match indices. A clock time string is reserved for a future
# build; use match indices for now.
arrive = 5
leave = 40

[[scouts]]
name = "Linus"
# Match columns during which this scout runs their own team's pit.
own_pit = [10, 11, 12]
# Preference for idle time, 0.0 to 1.0.
sparsity = 0.5

[[scouts]]
name = "Katherine"
break_partner = "Dorothy"

[[scouts]]
name = "Dorothy"
break_partner = "Katherine"
```

The config is validated with clear error messages: fractions must be in range,
expert bounds must be ordered, scout names must be unique, and a break partner must
name another scout who names them back.

## Example output

Solving a 12 team event with 8 scouts (`examples/config.toml`) gives full coverage
with breaks and pit scouting where there is slack. CSV, first few columns:

```
scout,Qm1,Qm2,Qm3,Qm4,Qm5,Qm6,Qm7,Qm8,Qm9
Ada,9*,9,9,9,12,9,9,9,9*
Grace,2*,2,2,2*,5*,2,2,2*,2
Linus,PIT:3,PIT:11,2*,,9*,,,,
Margaret,12*,12,12,12*,6,12,12,12*,12
Dennis,5*,5,5,5,8,5,5,5,5*
Barbara,6*,6,6,6*,,6,6,6*,6
Katherine,PIT:3,PIT:7,2*,,2,,,,
Dorothy,8*,8,8,8*,,8,8,8*,8
```

A cell is the team number being watched. A trailing `*` marks a qualitative watch.
`PIT:3` means pit scouting team 3. An empty cell is a break or unavailable.

The summary report:

```
scoutsched summary
==================

matches: 18    scouts: 8    teams: 12
coverage: 108/108 team matches watched (100.0%)
experts: 12/12 teams have an assigned expert
primary coverage: 77.8% by experts (target 80.0%)
qualitative: 27.4% of watches (target 25.0%)

per scout load (watch / pit / break / qualitative):
  Ada               16 /   0 /   2 /   3
  Grace             17 /   0 /   1 /   5
  Linus             11 /   2 /   5 /   4
  Margaret          16 /   0 /   2 /   5
  Dennis            15 /   0 /   3 /   3
  Barbara           14 /   2 /   2 /   4
  Katherine         10 /   2 /   6 /   3
  Dorothy           14 /   0 /   4 /   4

relaxations (soft constraints loosened, highest priority first):
  [primary_coverage] expert watched 77.8% of team matches, below the 80.0% primary target
  [pit_quota] 5 scouts received fewer than the pit quota of 2 (no expert free match available)
  [max_consecutive] 41 columns exceed the consecutive limit of 4 (full coverage left no rest slack)
```

The relaxations are honest: with full coverage as the top priority, when there is
not enough rest slack to also satisfy the pit quota and the consecutive limit for
every scout, those lower priority targets give way and the report says so.

## Project layout

```
src/
  model.rs      core types and the availability bitset
  config.rs     TOML config, validation, priority weights
  tba.rs        Blue Alliance v3 client and the shared match parser
  sample.rs     deterministic seeded sample generator
  solver/       the deterministic engine
    mod.rs      orchestrator, plan state, config resolution
    expertise.rs, coverage.rs, fill.rs, breaks.rs, repair.rs
  report.rs     coverage, load, and relaxation reporting
  output.rs     CSV, JSON, and summary rendering
  cli.rs        the command line, including gen-sample
  lib.rs, main.rs
tests/          integration tests over sample data
examples/       sample config, sample matches, usage notes
docs/           charter and architecture
```

## License

MIT. See `LICENSE`.
