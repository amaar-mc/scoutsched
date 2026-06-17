# Examples

This directory holds a ready to run example.

- `config.toml` is a fully documented configuration for eight scouts.
- `sample_matches.json` is a deterministic twelve team event in The Blue Alliance
  event matches JSON shape, generated with
  `scoutsched gen-sample --teams 12 --matches-per-team 9 --seed 3`.

## Run it

From the repository root, after `cargo build --release`:

```sh
# Human readable summary.
./target/release/scoutsched \
  --matches-file examples/sample_matches.json \
  --config examples/config.toml \
  --format summary

# The CSV grid.
./target/release/scoutsched \
  --matches-file examples/sample_matches.json \
  --config examples/config.toml \
  --format csv

# Write all three artifacts into a directory.
./target/release/scoutsched \
  --matches-file examples/sample_matches.json \
  --config examples/config.toml \
  --out-dir out/
```

## Generate your own sample event

```sh
# 36 teams, each playing 12 matches, written to matches.json.
./target/release/scoutsched gen-sample \
  --teams 36 --matches-per-team 12 --seed 1 --out matches.json
```

The product of teams and matches per team must be a multiple of six, since each
match has six team slots. The generator is deterministic: the same seed and
parameters always produce the same event.

## Use a real Blue Alliance event

```sh
export TBA_API_KEY=your_read_api_key
./target/release/scoutsched \
  --event 2024svr \
  --config examples/config.toml \
  --out-dir out/
```
