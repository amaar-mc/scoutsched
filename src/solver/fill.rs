//! Phase 4: filler watches, pit scouting, and mode tagging.
//!
//! After primary coverage, three things remain:
//!
//! - Pit scouting: each scout should get `pit_quota` pit assignments, and a pit
//!   assignment is only legal in a match where none of the scout's expert teams
//!   play (so it never competes with their stand duty). The team pit scouted is
//!   one of the scout's experts, which makes pit notes complement stand notes.
//! - Filler watches: each scout should watch `filler_quota` non expert teams for
//!   a baseline cross perspective. Filler preferentially fills team matches that
//!   are still uncovered, improving the top priority coverage objective for free.
//! - Mode tagging: a `qualitative_fraction` minority of each scout's watches are
//!   tagged qualitative, spread evenly across that scout's watches so no scout is
//!   all one mode.
//!
//! Every placement only ever overwrites a Break cell inside availability, so the
//! hard constraints stay satisfied. Order is fixed for determinism: pit, then
//! filler, then tagging, each iterating scouts and matches in ascending index.

use crate::model::{Cell, MatchIndex, TeamId, WatchMode};
use crate::solver::{qualitative_target, Plan};

/// Runs pit assignment, filler assignment, then mode tagging.
pub(crate) fn assign(plan: &mut Plan) {
    assign_pit(plan);
    assign_filler(plan);
    tag_modes(plan);
}

/// Returns true when none of the scout's expert teams play in match `m`.
fn no_expert_plays(plan: &Plan, scout: usize, m: usize) -> bool {
    let experts = &plan.experts[scout];
    !plan.matches[m].teams.iter().any(|t| experts.contains(t))
}

/// Phase 4a: place pit scouting in expert free matches, up to the quota.
fn assign_pit(plan: &mut Plan) {
    let n_matches = plan.n_matches;
    for scout in 0..plan.scouts.len() {
        let quota = plan.pit_quota as usize;
        if quota == 0 || plan.experts[scout].is_empty() {
            continue;
        }
        let mut placed = 0usize;
        // Pit scout the scout's experts in round robin so each gets visited.
        let experts = plan.experts[scout].clone();
        let mut ei = 0usize;
        for m in 0..n_matches {
            if placed >= quota {
                break;
            }
            if !free_here(plan, scout, m) {
                continue;
            }
            if !no_expert_plays(plan, scout, m) {
                continue;
            }
            let team = experts[ei % experts.len()];
            ei += 1;
            plan.grid[scout][m] = Cell::Pit { team };
            placed += 1;
        }
    }
}

/// Phase 4b: assign filler watches, preferring uncovered team matches.
fn assign_filler(plan: &mut Plan) {
    let n_matches = plan.n_matches;
    for scout in 0..plan.scouts.len() {
        let quota = plan.filler_quota as usize;
        if quota == 0 {
            continue;
        }
        let mut placed = 0usize;

        // First pass: fill uncovered team matches the scout is not expert on.
        for m in 0..n_matches {
            if placed >= quota {
                break;
            }
            if !free_here(plan, scout, m) {
                continue;
            }
            if let Some(team) = uncovered_non_expert_team(plan, scout, m) {
                plan.grid[scout][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Quantitative,
                };
                placed += 1;
            }
        }

        // Second pass: if quota remains, watch any non expert team in a match,
        // even if already covered, for the baseline perspective.
        for m in 0..n_matches {
            if placed >= quota {
                break;
            }
            if !free_here(plan, scout, m) {
                continue;
            }
            if let Some(team) = any_non_expert_team(plan, scout, m) {
                plan.grid[scout][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Quantitative,
                };
                placed += 1;
            }
        }
    }
}

/// Cell is available and currently a free Break.
fn free_here(plan: &Plan, scout: usize, m: usize) -> bool {
    plan.is_available(scout, m) && plan.grid[scout][m] == Cell::Break
}

/// Lowest id team in match `m` that the scout is not expert on and that no one
/// watches yet.
fn uncovered_non_expert_team(plan: &Plan, scout: usize, m: usize) -> Option<TeamId> {
    let experts = &plan.experts[scout];
    let mut teams: Vec<TeamId> = plan.matches[m].teams.to_vec();
    teams.sort_unstable();
    teams.dedup();
    teams
        .into_iter()
        .find(|t| !experts.contains(t) && plan.watcher_of(m, *t).is_none())
}

/// Lowest id team in match `m` the scout is not expert on, covered or not.
fn any_non_expert_team(plan: &Plan, scout: usize, m: usize) -> Option<TeamId> {
    let experts = &plan.experts[scout];
    let mut teams: Vec<TeamId> = plan.matches[m].teams.to_vec();
    teams.sort_unstable();
    teams.dedup();
    teams.into_iter().find(|t| !experts.contains(t))
}

/// Phase 4c: tag a minority of each scout's watches qualitative, spread evenly.
///
/// For each scout, collect its watch columns in ascending order, compute the
/// qualitative target from the fraction, then pick evenly spaced watches to tag.
/// Even spacing means a scout with eight watches and a target of two gets the
/// first and the fifth tagged, not two adjacent ones.
fn tag_modes(plan: &mut Plan) {
    let fraction = plan.qualitative_fraction;
    if fraction <= 0.0 {
        return;
    }
    for scout in 0..plan.scouts.len() {
        let watches: Vec<MatchIndex> = (0..plan.n_matches)
            .filter(|&m| matches!(plan.grid[scout][m], Cell::Watch { .. }))
            .map(|m| m as MatchIndex)
            .collect();
        let target = qualitative_target(watches.len(), fraction);
        if target == 0 || watches.is_empty() {
            continue;
        }
        // Evenly spaced indices into the watch list: floor(i * len / target).
        for i in 0..target {
            let idx = (i * watches.len()) / target;
            let m = watches[idx] as usize;
            if let Cell::Watch { team, .. } = plan.grid[scout][m] {
                plan.grid[scout][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Qualitative,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::{build_plan_for_test, coverage, expertise, SolveInput};

    fn prepared(team_count: usize, mpt: usize, scouts: usize, g: Globals) -> Plan {
        let sched = generate(SampleParams {
            team_count,
            matches_per_team: mpt,
            seed: 11,
        })
        .unwrap();
        let cfg = Config {
            globals: g,
            scouts: (0..scouts)
                .map(|i| ScoutConfig {
                    name: format!("S{i}"),
                    ..ScoutConfig::default()
                })
                .collect(),
        };
        let mut plan = build_plan_for_test(&SolveInput {
            schedule: sched,
            config: cfg,
        })
        .unwrap();
        expertise::assign(&mut plan);
        coverage::assign(&mut plan);
        plan
    }

    #[test]
    fn pit_only_when_no_expert_plays() {
        let mut plan = prepared(24, 10, 8, Globals::default());
        assign(&mut plan);
        for scout in 0..plan.scouts.len() {
            for m in 0..plan.n_matches {
                if let Cell::Pit { .. } = plan.grid[scout][m] {
                    assert!(
                        no_expert_plays(&plan, scout, m),
                        "pit scheduled while an expert team plays (scout {scout}, match {m})"
                    );
                }
            }
        }
    }

    #[test]
    fn pit_respects_quota() {
        let g = Globals {
            pit_quota: 3,
            ..Globals::default()
        };
        let mut plan = prepared(24, 12, 8, g);
        assign(&mut plan);
        for scout in 0..plan.scouts.len() {
            let pits = plan.grid[scout]
                .iter()
                .filter(|c| matches!(c, Cell::Pit { .. }))
                .count();
            assert!(pits <= 3, "scout {scout} exceeded pit quota: {pits}");
        }
    }

    #[test]
    fn qualitative_fraction_is_a_minority() {
        let g = Globals {
            qualitative_fraction: 0.25,
            ..Globals::default()
        };
        let mut plan = prepared(24, 12, 8, g);
        assign(&mut plan);
        let mut qual = 0usize;
        let mut total = 0usize;
        for row in &plan.grid {
            for c in row {
                if let Cell::Watch { mode, .. } = c {
                    total += 1;
                    if *mode == WatchMode::Qualitative {
                        qual += 1;
                    }
                }
            }
        }
        assert!(total > 0);
        // A minority overall.
        assert!(
            qual * 2 <= total,
            "qualitative is not a minority: {qual}/{total}"
        );
        // But non zero given a positive fraction and enough watches.
        assert!(qual > 0, "no qualitative tags applied");
    }

    #[test]
    fn fill_only_overwrites_breaks_inside_availability() {
        let mut scouts: Vec<ScoutConfig> = (0..6)
            .map(|i| ScoutConfig {
                name: format!("S{i}"),
                ..ScoutConfig::default()
            })
            .collect();
        scouts[1].own_pit = vec![3, 4, 5];
        let sched = generate(SampleParams {
            team_count: 18,
            matches_per_team: 8,
            seed: 4,
        })
        .unwrap();
        let cfg = Config {
            globals: Globals::default(),
            scouts,
        };
        let mut plan = build_plan_for_test(&SolveInput {
            schedule: sched,
            config: cfg,
        })
        .unwrap();
        expertise::assign(&mut plan);
        coverage::assign(&mut plan);
        assign(&mut plan);
        for &blk in &[3usize, 4, 5] {
            assert_eq!(
                plan.grid[1][blk],
                Cell::Unavailable,
                "own pit block was overwritten"
            );
        }
    }

    #[test]
    fn deterministic() {
        let mut a = prepared(30, 10, 9, Globals::default());
        let mut b = prepared(30, 10, 9, Globals::default());
        assign(&mut a);
        assign(&mut b);
        assert_eq!(a.grid, b.grid);
    }
}
