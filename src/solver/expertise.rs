//! Phase 2: expertise assignment.
//!
//! Each scout becomes expert on `k` teams, where `k` is the configured range
//! scaled up if needed so the roster can cover every team. Assignment is a
//! deterministic round robin: walk teams in id order and hand them out to scouts
//! in id order, cycling. This balances expert load across scouts and, as long as
//! `n_scouts * k >= n_teams`, guarantees every team has at least one expert. When
//! the roster is too small to cover all teams even at maximum `k`, the lowest id
//! teams are covered first and the coverage gap is reported later.

use crate::model::{ScoutId, TeamId};
use crate::solver::{experts_per_scout, Plan};

/// Assigns expert teams to every scout and builds the inverse index.
pub(crate) fn assign(plan: &mut Plan) {
    let n_scouts = plan.scouts.len();
    let n_teams = plan.n_teams;
    let k = experts_per_scout(n_teams, n_scouts, plan.experts_min, plan.experts_max);

    let mut experts: Vec<Vec<TeamId>> = vec![Vec::with_capacity(k); n_scouts];

    // Round robin: assignment slot s gets team (slot) for slot in 0..n_scouts*k,
    // mapping team index by modulo so every team is reached before any repeats.
    // Teams are visited in ascending id, scouts in ascending id, so the result
    // is fully determined by the counts.
    let total_slots = n_scouts * k;
    for slot in 0..total_slots {
        let scout = slot % n_scouts;
        let team = (slot % n_teams) as TeamId;
        if !experts[scout].contains(&team) {
            experts[scout].push(team);
        }
    }

    // A scout might have fewer than k distinct teams if n_teams < k for that
    // cycle alignment; top up with the next teams it does not yet hold, in id
    // order, so each scout reaches min(k, n_teams) experts deterministically.
    let target = k.min(n_teams);
    for (s, list) in experts.iter_mut().enumerate() {
        let mut t: TeamId = 0;
        while list.len() < target && (t as usize) < n_teams {
            if !list.contains(&t) {
                list.push(t);
            }
            t += 1;
        }
        list.sort_unstable();
        list.dedup();
        debug_assert!(list.len() <= target, "scout {s} over assigned experts");
    }

    // Build inverse index: experts per team, ascending scout id.
    let mut team_experts: Vec<Vec<ScoutId>> = vec![Vec::new(); n_teams];
    for (s, list) in experts.iter().enumerate() {
        for &t in list {
            team_experts[t as usize].push(s as ScoutId);
        }
    }

    plan.experts = experts;
    plan.team_experts = team_experts;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::{build_plan_for_test, SolveInput};

    fn plan_with(team_count: usize, matches_per_team: usize, scouts: usize) -> Plan {
        let sched = generate(SampleParams {
            team_count,
            matches_per_team,
            seed: 7,
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
        build_plan_for_test(&SolveInput {
            schedule: sched,
            config: cfg,
        })
        .unwrap()
    }

    #[test]
    fn every_team_has_an_expert_when_capacity_allows() {
        let mut plan = plan_with(24, 6, 8);
        assign(&mut plan);
        // 8 scouts, ceil(24/8)=3 experts each, 24 slots, covers all 24 teams.
        assert!(
            plan.team_experts.iter().all(|e| !e.is_empty()),
            "some team has no expert"
        );
    }

    #[test]
    fn experts_are_sorted_and_unique() {
        let mut plan = plan_with(30, 6, 10);
        assign(&mut plan);
        for list in &plan.experts {
            let mut sorted = list.clone();
            sorted.sort_unstable();
            assert_eq!(*list, sorted, "expert list not sorted");
            let mut dedup = sorted.clone();
            dedup.dedup();
            assert_eq!(dedup.len(), list.len(), "expert list has duplicates");
        }
    }

    #[test]
    fn deterministic_assignment() {
        let mut a = plan_with(36, 6, 9);
        let mut b = plan_with(36, 6, 9);
        assign(&mut a);
        assign(&mut b);
        assert_eq!(a.experts, b.experts);
        assert_eq!(a.team_experts, b.team_experts);
    }

    #[test]
    fn small_team_count_clamps() {
        // 6 teams, 6 scouts: each scout still gets up to its experts, capped at
        // the number of teams.
        let mut plan = plan_with(6, 6, 6);
        assign(&mut plan);
        assert!(plan.experts.iter().all(|e| !e.is_empty()));
        assert!(plan.experts.iter().all(|e| e.len() <= 6));
    }
}
