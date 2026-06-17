//! Phase 3: primary coverage.
//!
//! For each match in column order, each of its six teams should be watched by
//! one of that team's experts. The policy is deterministic:
//!
//! - Visit matches in ascending column index, teams in slot order.
//! - For a team not yet watched in that match, consider its experts who are
//!   available for the match and not already busy that column. Choose the expert
//!   with the lightest current active load, breaking ties by ascending scout id.
//! - If no expert is free (all busy, unavailable, or the team has no expert),
//!   leave the team for the fill and repair phases, which may assign a non expert
//!   watcher. Any team match left unwatched after repair is a reported coverage
//!   gap.
//!
//! This honors the primary fraction by saturating expert coverage first; the
//! fraction itself is a soft target measured in the report, and over assignment
//! beyond it is harmless because broad coverage is the top priority.

use crate::model::{Cell, ScoutId, WatchMode};
use crate::solver::Plan;

/// Assigns expert watchers across all team matches.
pub(crate) fn assign(plan: &mut Plan) {
    let n_matches = plan.n_matches;

    for m in 0..n_matches {
        // Teams in this match, in slot order, deduplicated defensively.
        let teams = plan.matches[m].teams;
        for &team in teams.iter() {
            // Skip if already watched this column (an earlier slot or duplicate).
            if plan.watcher_of(m, team).is_some() {
                continue;
            }
            if let Some(scout) = pick_expert(plan, m, team) {
                plan.grid[scout as usize][m] = Cell::Watch {
                    team,
                    mode: WatchMode::Quantitative,
                };
            }
        }
    }
}

/// Chooses the best available expert for a team in a match, or None.
///
/// "Available" means inside the scout's window and not on own pit duty, and the
/// cell is currently a Break (free). Among candidates the lightest active load
/// wins, ties broken by ascending scout id for determinism.
fn pick_expert(plan: &Plan, m: usize, team: crate::model::TeamId) -> Option<ScoutId> {
    let experts = &plan.team_experts[team as usize];
    let mut best: Option<(usize, ScoutId)> = None;
    for &s in experts {
        let su = s as usize;
        if !plan.is_available(su, m) {
            continue;
        }
        if plan.grid[su][m] != Cell::Break {
            continue;
        }
        let load = plan.active_load(su);
        match best {
            None => best = Some((load, s)),
            Some((bl, bs)) => {
                if load < bl || (load == bl && s < bs) {
                    best = Some((load, s));
                }
            }
        }
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::{build_plan_for_test, expertise, SolveInput};

    fn prepared(team_count: usize, matches_per_team: usize, scouts: usize, seed: u64) -> Plan {
        let sched = generate(SampleParams {
            team_count,
            matches_per_team,
            seed,
        })
        .unwrap();
        let cfg = Config {
            globals: Globals::default(),
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
        plan
    }

    #[test]
    fn no_scout_double_booked_after_coverage() {
        let mut plan = prepared(24, 9, 10, 1);
        assign(&mut plan);
        // Each cell is a single Cell, so double booking is structurally
        // impossible; assert each active cell watches a team that actually
        // plays that match.
        for m in 0..plan.n_matches {
            for s in 0..plan.scouts.len() {
                if let Cell::Watch { team, .. } = plan.grid[s][m] {
                    assert!(
                        plan.matches[m].contains(team),
                        "scout {s} watches team {team} not in match {m}"
                    );
                }
            }
        }
    }

    #[test]
    fn experts_only_watch_inside_availability() {
        // Restrict one scout's window and verify it is never assigned outside.
        let sched = generate(SampleParams {
            team_count: 18,
            matches_per_team: 8,
            seed: 2,
        })
        .unwrap();
        let mut scouts: Vec<ScoutConfig> = (0..6)
            .map(|i| ScoutConfig {
                name: format!("S{i}"),
                ..ScoutConfig::default()
            })
            .collect();
        scouts[0].arrive = Some(crate::config::WindowBound::Index(5));
        scouts[0].leave = Some(crate::config::WindowBound::Index(10));
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
        assign(&mut plan);
        for m in 0..plan.n_matches {
            if !(5..10).contains(&m) {
                assert_eq!(
                    plan.grid[0][m],
                    Cell::Unavailable,
                    "scout 0 active outside window at {m}"
                );
            }
        }
    }

    #[test]
    fn coverage_is_high_when_capacity_allows() {
        let mut plan = prepared(24, 12, 12, 3);
        assign(&mut plan);
        let mut watched = 0usize;
        let mut total = 0usize;
        for m in 0..plan.n_matches {
            for &team in plan.matches[m].teams.iter() {
                total += 1;
                if plan.watcher_of(m, team).is_some() {
                    watched += 1;
                }
            }
        }
        // With ample scouts, expert coverage alone should be substantial.
        assert!(
            watched * 2 >= total,
            "expert coverage unexpectedly low: {watched}/{total}"
        );
    }
}
