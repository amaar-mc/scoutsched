# Architecture

scoutsched turns a Blue Alliance qualification schedule and a scout roster into a
complete scouting grid. This document describes the data model, the deterministic
algorithm, the complexity of the underlying problem, and what the heuristic
guarantees versus what it approximates.

## Data model

Entities are small integers so the solver can use them as array indices and pack
state into bitsets.

- A `TeamId` is a dense index assigned by ascending FRC team number over the
  event. The original team number is kept in a side table for output.
- A `ScoutId` is the roster index.
- A `MatchIndex` is the match column in time order, starting at zero.
- A `Match` is a time ordered column of six team slots, three red then three blue.
- A `Cell` is the assignment for one (scout, match) pair: `Watch { team, mode }`,
  `Pit { team }`, `Break`, or `Unavailable`. Because there is exactly one cell per
  pair, the no double booking and one team per scout per match constraints are
  structural rather than checked.
- Availability is a `MatchBitset`, a fixed length bitset over match columns. The
  solver also builds a team to matches index so it never scans the whole schedule
  to find where a team plays.

The grid is a `Vec<Vec<Cell>>` indexed `[scout][match]`. It starts as `Unavailable`
wherever a scout cannot work (outside the arrive and leave window, or during own
pit duty) and `Break` everywhere else. Phases only ever overwrite `Break` cells, so
the hard availability and own pit constraints can never be violated by
construction.

## The algorithm

scoutsched is a deterministic constructive heuristic followed by bounded local
search. There is no randomness anywhere in the solver. The only seeded randomness
in the project is the sample data generator, which is not part of solving.

The phases run in this order.

1. Parse and resolve. Build availability bitsets, the team to matches index, and
   the initial grid.
2. Expertise assignment. Give each scout `k` expert teams by a round robin over
   teams in id order, where `k` is the configured range scaled up if needed so the
   roster can cover every team. As long as `scouts * k >= teams`, every team has at
   least one expert.
3. Primary coverage. For each match in column order and each of its teams, if the
   team is not yet watched, assign the lightest loaded available expert, ties
   broken by ascending scout id.
4. Coverage repair. A first pass of the bounded local search closes any coverage
   gap using the free slack that still exists, before filler and pit consume it.
   Coverage is the top priority, so it claims slack first.
5. Filler, pit, and mode tagging. Give each scout up to `filler_quota` non expert
   watches for a baseline perspective, preferring still uncovered team matches.
   Place up to `pit_quota` pit assignments in matches where none of the scout's
   expert teams play. Tag an evenly spread minority of each scout's watches
   qualitative to hit the configured fraction.
6. Break and sparsity enforcement. Break up runs longer than `max_consecutive`,
   trim load for scouts with a sparsity preference, and align break partners.
   Every break either converts a redundant or pit cell, or hands a unique watch
   off to a free scout first, so coverage is never reduced. This phase runs last
   so nothing reintroduces work into a rest column, which is what makes coverage
   strictly dominate the break and sparsity preferences.

All choices use a fixed total order over indices and stable sorts, so identical
input yields byte identical output. The integration tests assert this directly
across many instances.

### Determinism and tie breaking

Every phase resolves ties by ascending index. The team id assignment is by
ascending FRC team number, the expert round robin is by ascending team then scout
id, coverage picks the lightest loaded expert with ties by scout id, and the
report sorts relaxations by descending soft constraint weight with ties by a stable
kind string. There is no hash iteration over unordered collections in any decision
path.

### Complexity of the algorithm

Let s be the number of scouts, m the number of matches, and t the number of teams.
The constructive phases are near linear in the grid size s times m, with small
factors for scanning a match's six teams or a team's experts. The local search and
break enforcement are bounded by fixed iteration caps, and each pass is polynomial
in s and m. The solver avoids quadratic blowups by using the team to matches index
and bitset availability rather than rescanning the schedule. In practice an event
sized instance, tens of teams and a dozen scouts over scores of matches, solves in
well under a second.

## Why this is a hard problem

The decision version of scout scheduling is NP-hard. It generalizes two classic NP
problems at once and sits in the nurse rostering and crew scheduling family, which
is NP-hard in general.

### Reduction sketch from set cover

Consider the coverage objective in isolation with availability constraints. We are
given team matches that must each be observed, and scouts who, through their
availability windows and expertise, can each observe only certain subsets of those
team matches in a fixed time budget. Deciding whether a bounded number of scouts
can observe every required team match is an instance of set cover.

Given a set cover instance with universe U and a family of subsets S over U, build a
scouting instance with one team match per element of U and one candidate scout per
subset in S, where a scout can cover exactly the team matches corresponding to its
subset. A scouting assignment that observes all team matches using at most k scouts
exists if and only if the set cover instance has a cover of size k. Set cover is NP
complete, so the coverage decision problem is NP-hard, and so is the full problem
that contains it.

### The interval scheduling and rostering angle

The availability windows, the maximum consecutive work rule, and the minimum break
length make each scout's row a sequence with forbidden regions and bounded run
lengths. Choosing which scout covers which team match while respecting these per
scout sequence constraints is exactly the structure of nurse rostering and crew
scheduling, where shifts must be assigned subject to availability, consecutive
shift limits, and rest rules. Those problems are NP-hard, and adding the coverage
requirement on top does not make them easier.

### Why a deterministic heuristic rather than an exact solver

An exact solver, for example an integer program or a constraint solver, could find
an optimal assignment, but three properties of this setting argue against it for
the shipped tool.

- The tool must always return a complete schedule a scouting lead can use, even
  when the instance is over constrained. An exact solver reports infeasibility;
  the lead still needs a grid. A constructive heuristic always produces one and
  reports which soft targets it relaxed.
- The hard constraints in this domain are simple enough to satisfy by construction.
  Availability and own pit duty are handled by initializing those cells as
  unavailable and never overwriting them, and no double booking is structural. The
  hard part is the soft objectives, which are preferences, not feasibility.
- Determinism and zero heavy dependencies are project goals. A constructive
  heuristic with fixed tie breaking is trivially reproducible and needs no solver
  library. An exact solver would add a large dependency and, unless carefully
  configured, can return different optima across versions or platforms.

## Guarantees

What the heuristic guarantees on every input, proven by the integration sweep over
hundreds of instances:

- The schedule is complete. Every (scout, match) cell holds a valid assignment.
- The schedule is hard feasible. No scout is assigned outside its availability
  window or during own pit duty, no scout has two activities in one match, and a
  watched team always plays that match.
- The solver never fails on an over constrained instance. It relaxes soft
  constraints by the documented priority and reports every relaxation and coverage
  gap.

What the heuristic approximates rather than optimizes:

- Soft preference optimality. The constructive plus local search approach finds a
  good assignment of coverage, quotas, mode fractions, sparsity, and break
  matching, but does not prove it optimal. The priority weights steer which soft
  objectives are favored when they conflict.

## The soft constraint priority

When an instance cannot satisfy every soft objective, they are relaxed lowest
priority first. The descending order, configurable by weight with sane defaults, is:

1. Coverage. Every team has an expert, and every team match is watched by an
   available scout.
2. Pit scouting quota and filler quota.
3. Qualitative fraction target.
4. Per scout sparsity and density preference.
5. Break matching. Pairs of scouts who requested aligned breaks get them.

The report names each relaxed objective with a detail string and lists any
coverage gaps, so the relaxation is always visible.

## The Blue Alliance integration

`tba.rs` fetches `GET /event/{event_key}/matches` with the `X-TBA-Auth-Key` header,
keeps only qualification matches (`comp_level` equal to `qm`), sorts them by match
number, and parses the alliance team keys into dense team ids. The exact same
parser reads a local JSON file through `--matches-file`, so the tool and every test
run with no network and no real key.
