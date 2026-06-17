# Charter

## Purpose

Give FRC teams a correct, deterministic tool that turns a Blue Alliance
qualification schedule and a scout roster into a complete scouting assignment
grid: who watches which team in each match, who is pit scouting, who is on break,
with quantitative and qualitative tagging. The output is something a scouting lead
can print and hand out, and something a dashboard can consume as JSON.

## Scope

- Build a complete, hard feasible scouting grid from a parsed qualification
  schedule and a TOML roster.
- Per team expert assignments, primary coverage, filler watches, pit scouting,
  and quantitative or qualitative mode tagging.
- Hard constraints on availability, own pit duty, and one activity per scout per
  match, never violated.
- Soft constraints relaxed by a documented priority when an instance is over
  constrained, with a report of what was relaxed.
- A Blue Alliance client and a local file path, so the tool runs with or without a
  real API key.
- A deterministic sample data generator for testing without a key.

## Non-goals

- A live scouting data store or a match analytics engine. scoutsched plans who
  scouts what; it does not record or analyze the resulting scouting data.
- A general purpose constraint solver. It does one family of scheduling well with a
  deterministic heuristic, not an exact optimizer.
- Randomized output. The solver is deterministic. The only seeded randomness is in
  the sample generator, which is not part of solving.
- A heavy dependency footprint. The crate uses a small set of well known crates and
  nothing more.

## Principles

- Always return a usable schedule. Even an over constrained instance yields a
  complete, hard feasible grid plus an honest report of relaxations.
- Hard constraints are structural where possible. Availability and own pit duty are
  enforced by construction, not by post hoc checking.
- Determinism is a feature. Identical input gives byte identical output, which the
  tests assert.
- Strong typing and small integer ids. Bitsets and indices keep the solver fast and
  the state explicit.
- Honest reporting. Coverage gaps and relaxed soft targets are surfaced, never
  hidden.

## Audience

FRC scouting leads and software students building scouting systems, plus anyone
interested in a clean, deterministic take on a rostering style scheduling problem.
