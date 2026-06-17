//! Phase 6: bounded deterministic local search repair.
//!
//! Earlier phases can leave coverage gaps: a team match with no watcher, because
//! every expert was busy or a break was forced over a sole watcher. Repair fixes
//! what it can with moves that always preserve the hard constraints, runs for at
//! most `repair_iterations` passes, and stops early when a pass makes no change,
//! which keeps it deterministic and bounded.
//!
//! Two move types, tried in this fixed order each pass:
//!
//! 1. Fill: for an uncovered team match, assign an available scout who is free
//!    that column (currently a Break) to watch the team. This strictly improves
//!    coverage and can never violate a hard constraint, since it only writes a
//!    Watch into an available Break cell.
//! 2. Reassign: for an uncovered team match where no scout is free, move a scout
//!    who is watching an already redundant team that column onto the uncovered
//!    team. The vacated team stays covered by its other watcher, so coverage is
//!    monotonically non decreasing.
//!
//! All scans are in ascending index order with stable tie breaks, so the repair
//! is a deterministic function of its input.

use crate::model::{Cell, ScoutId, TeamId, WatchMode};
use crate::solver::Plan;

/// Runs the bounded repair loop.
pub(crate) fn run(plan: &mut Plan) {
    let max_iters = plan.repair_iterations;
    for _ in 0..max_iters {
        let changed = one_pass(plan);
        if !changed {
            break;
        }
    }
}

/// One repair pass. Returns true if any move was applied.
fn one_pass(plan: &mut Plan) -> bool {
    let mut changed = false;
    let n = plan.n_matches;

    for m in 0..n {
        // Distinct teams in this match in ascending id, so the scan is stable.
        let mut teams: Vec<TeamId> = plan.matches[m].teams.to_vec();
        teams.sort_unstable();
        teams.dedup();

        for team in teams {
            if plan.watcher_of(m, team).is_some() {
                continue;
            }
            // Move 1: a free, available scout watches the team.
            if let Some(s) = free_scout_for(plan, m) {
                plan.grid[s as usize][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Quantitative,
                };
                changed = true;
                continue;
            }
            // Move 2: reassign a scout from a redundant team to this team.
            if let Some(s) = reassignable_scout_for(plan, m) {
                plan.grid[s as usize][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Quantitative,
                };
                changed = true;
            }
        }
    }

    changed
}

/// Available scout for match `m` whose cell is a free Break, choosing the
/// lightest current active load so coverage recovery also balances the roster.
/// Ties break by ascending scout id for determinism.
fn free_scout_for(plan: &Plan, m: usize) -> Option<ScoutId> {
    let mut best: Option<(usize, ScoutId)> = None;
    for s in 0..plan.scouts.len() {
        if plan.is_available(s, m) && plan.grid[s][m] == Cell::Break {
            let load = plan.active_load(s);
            match best {
                None => best = Some((load, s as ScoutId)),
                Some((bl, _)) if load < bl => best = Some((load, s as ScoutId)),
                _ => {}
            }
        }
    }
    best.map(|(_, s)| s)
}

/// Lowest id scout in match `m` currently watching a team that is covered by at
/// least one other scout, so moving them off it keeps that team covered.
fn reassignable_scout_for(plan: &Plan, m: usize) -> Option<ScoutId> {
    for s in 0..plan.scouts.len() {
        if let Cell::Watch { team, .. } = plan.grid[s][m] {
            let others = (0..plan.scouts.len()).filter(|&o| {
                o != s && matches!(plan.grid[o][m], Cell::Watch { team: t, .. } if t == team)
            });
            if others.count() >= 1 {
                return Some(s as ScoutId);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::{breaks, build_plan_for_test, coverage, expertise, fill, SolveInput};

    fn full_pipeline(team_count: usize, mpt: usize, scouts: usize, g: Globals, seed: u64) -> Plan {
        let sched = generate(SampleParams {
            team_count,
            matches_per_team: mpt,
            seed,
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
        fill::assign(&mut plan);
        breaks::enforce(&mut plan);
        plan
    }

    fn coverage_count(plan: &Plan) -> usize {
        let mut c = 0;
        for m in 0..plan.n_matches {
            for &team in plan.matches[m].teams.iter() {
                if plan.watcher_of(m, team).is_some() {
                    c += 1;
                }
            }
        }
        c
    }

    #[test]
    fn repair_never_decreases_coverage() {
        let mut plan = full_pipeline(24, 12, 8, Globals::default(), 1);
        let before = coverage_count(&plan);
        run(&mut plan);
        let after = coverage_count(&plan);
        assert!(after >= before, "coverage decreased: {before} -> {after}");
    }

    #[test]
    fn repair_fills_gaps_when_capacity_exists() {
        // Ample scouts relative to teams: repair should reach full coverage.
        let mut plan = full_pipeline(12, 12, 12, Globals::default(), 2);
        run(&mut plan);
        let total: usize = (0..plan.n_matches)
            .map(|m| {
                let mut t = plan.matches[m].teams.to_vec();
                t.sort_unstable();
                t.dedup();
                t.len()
            })
            .sum();
        assert_eq!(coverage_count(&plan), total, "expected full coverage");
    }

    #[test]
    fn repair_preserves_hard_constraints() {
        let mut scouts: Vec<ScoutConfig> = (0..8)
            .map(|i| ScoutConfig {
                name: format!("S{i}"),
                ..ScoutConfig::default()
            })
            .collect();
        scouts[0].arrive = Some(crate::config::WindowBound::Index(10));
        scouts[2].own_pit = vec![1, 2, 3];
        let sched = generate(SampleParams {
            team_count: 24,
            matches_per_team: 10,
            seed: 3,
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
        fill::assign(&mut plan);
        breaks::enforce(&mut plan);
        run(&mut plan);

        // Scout 0 never active before match 10.
        for m in 0..10 {
            assert!(!plan.grid[0][m].is_active());
        }
        // Scout 2 never active during own pit blocks, and a watched team plays.
        for &blk in &[1usize, 2, 3] {
            assert!(!plan.grid[2][blk].is_active());
        }
        for m in 0..plan.n_matches {
            for s in 0..plan.scouts.len() {
                if let Cell::Watch { team, .. } = plan.grid[s][m] {
                    assert!(plan.matches[m].contains(team));
                }
            }
        }
    }

    #[test]
    fn repair_is_deterministic() {
        let mut a = full_pipeline(30, 10, 9, Globals::default(), 5);
        let mut b = full_pipeline(30, 10, 9, Globals::default(), 5);
        run(&mut a);
        run(&mut b);
        assert_eq!(a.grid, b.grid);
    }

    #[test]
    fn repair_terminates_at_iteration_cap() {
        // A tiny cap still terminates and leaves a valid grid.
        let g = Globals {
            repair_iterations: 1,
            ..Globals::default()
        };
        let mut plan = full_pipeline(24, 10, 8, g, 7);
        run(&mut plan);
        // No panic, grid intact.
        assert!(plan.grid.iter().all(|r| r.len() == plan.n_matches));
    }
}
