//! Solver report: what was achieved and what was relaxed.
//!
//! The report is computed from the final grid and the plan's targets. It records
//! coverage, per scout load, pit assignments, the qualitative fraction achieved,
//! and a list of relaxations in priority order. A relaxation is recorded whenever
//! an achieved value falls short of its target, which is exactly the information
//! a scouting lead needs to decide whether to add scouts or loosen a quota.

use crate::model::{Cell, WatchMode};
use crate::solver::{qualitative_target, Plan};
use serde::Serialize;

/// A single relaxed soft constraint, named in priority order, with a human
/// readable detail string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relaxation {
    /// Stable machine name, for example "coverage" or "pit_quota".
    pub kind: String,
    /// Human readable explanation of the shortfall.
    pub detail: String,
}

/// Per scout load summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScoutLoad {
    pub name: String,
    pub watches: usize,
    pub pits: usize,
    pub breaks: usize,
    pub qualitative: usize,
}

/// One uncovered team match, for the coverage gap list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoverageGap {
    pub match_number: u32,
    pub team_number: u32,
}

/// The full solver report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Team matches watched divided by total team matches.
    pub coverage_fraction: f64,
    /// Total team matches (distinct team appearances over all matches).
    pub total_team_matches: usize,
    /// Team matches with at least one watcher.
    pub watched_team_matches: usize,
    /// Team matches left with no watcher.
    pub coverage_gaps: Vec<CoverageGap>,
    /// Number of teams that have at least one expert assigned.
    pub teams_with_expert: usize,
    /// Total number of teams.
    pub total_teams: usize,
    /// Fraction of all team matches watched specifically by one of that team's
    /// experts (the primary coverage objective), aggregated over teams.
    pub primary_fraction: f64,
    /// Target primary fraction from config, for comparison.
    pub primary_target: f64,
    /// Per scout load.
    pub scout_loads: Vec<ScoutLoad>,
    /// Qualitative watches divided by total watches.
    pub qualitative_fraction: f64,
    /// Target qualitative fraction from config, for comparison.
    pub qualitative_target: f64,
    /// Soft constraints relaxed, in priority order.
    pub relaxations: Vec<Relaxation>,
}

impl Report {
    /// True when every team match is watched.
    pub fn full_coverage(&self) -> bool {
        self.coverage_gaps.is_empty()
    }
}

/// Counts columns in a scout row that sit beyond position `limit` within a run
/// of consecutive active cells, that is the number of forced consecutive matches
/// over the allowed maximum.
fn count_over_limit(row: &[Cell], limit: usize) -> usize {
    let mut over = 0usize;
    let mut run = 0usize;
    for c in row {
        if c.is_active() {
            run += 1;
            if run > limit {
                over += 1;
            }
        } else {
            run = 0;
        }
    }
    over
}

/// Builds the report from the final plan.
pub(crate) fn build(plan: &Plan) -> Report {
    let n = plan.n_matches;

    // Coverage over distinct team matches.
    let mut total_tm = 0usize;
    let mut watched_tm = 0usize;
    let mut gaps: Vec<CoverageGap> = Vec::new();
    for m in 0..n {
        let mut teams = plan.matches[m].teams.to_vec();
        teams.sort_unstable();
        teams.dedup();
        for team in teams {
            total_tm += 1;
            if plan.watcher_of(m, team).is_some() {
                watched_tm += 1;
            } else {
                gaps.push(CoverageGap {
                    match_number: plan.matches[m].match_number,
                    team_number: plan.team_numbers[team as usize],
                });
            }
        }
    }

    let coverage_fraction = if total_tm == 0 {
        1.0
    } else {
        watched_tm as f64 / total_tm as f64
    };

    let teams_with_expert = plan.team_experts.iter().filter(|e| !e.is_empty()).count();

    // Primary coverage: over each team's own matches (via the team to matches
    // index), how many are watched by one of that team's experts. This measures
    // the primary coverage objective distinctly from overall coverage, which can
    // be met by filler watchers.
    let mut primary_watched = 0usize;
    let mut primary_total = 0usize;
    for team in 0..plan.n_teams {
        let experts = &plan.team_experts[team];
        for &m in &plan.team_matches[team] {
            primary_total += 1;
            if let Some(w) = plan.watcher_of(m as usize, team as crate::model::TeamId) {
                if experts.contains(&w) {
                    primary_watched += 1;
                }
            }
        }
    }
    let primary_fraction = if primary_total == 0 {
        1.0
    } else {
        primary_watched as f64 / primary_total as f64
    };

    // Per scout loads.
    let mut scout_loads = Vec::with_capacity(plan.scouts.len());
    let mut total_watches = 0usize;
    let mut total_qual = 0usize;
    for (s, scout) in plan.scouts.iter().enumerate() {
        let mut watches = 0;
        let mut pits = 0;
        let mut breaks = 0;
        let mut qual = 0;
        for c in &plan.grid[s] {
            match c {
                Cell::Watch { mode, .. } => {
                    watches += 1;
                    if *mode == WatchMode::Qualitative {
                        qual += 1;
                    }
                }
                Cell::Pit { .. } => pits += 1,
                Cell::Break => breaks += 1,
                Cell::Unavailable => {}
            }
        }
        total_watches += watches;
        total_qual += qual;
        scout_loads.push(ScoutLoad {
            name: scout.name.clone(),
            watches,
            pits,
            breaks,
            qualitative: qual,
        });
    }

    let qualitative_fraction = if total_watches == 0 {
        0.0
    } else {
        total_qual as f64 / total_watches as f64
    };

    // Collect relaxations, each paired with the configured weight of the soft
    // constraint it belongs to. The list is then sorted by descending weight so
    // the report presents shortfalls in the same priority order the solver uses
    // to decide what to relax. This is where the configurable weights take
    // effect on the output.
    let w = &plan.weights;
    let mut weighted: Vec<(f64, Relaxation)> = Vec::new();

    // Coverage family (highest priority).
    if teams_with_expert < plan.n_teams {
        weighted.push((
            w.coverage,
            Relaxation {
                kind: "coverage_expert".into(),
                detail: format!(
                    "{} of {} teams have no assigned expert (roster too small for the team count)",
                    plan.n_teams - teams_with_expert,
                    plan.n_teams
                ),
            },
        ));
    }
    if !gaps.is_empty() {
        weighted.push((
            w.coverage,
            Relaxation {
                kind: "coverage".into(),
                detail: format!("{} of {} team matches left unwatched", gaps.len(), total_tm),
            },
        ));
    }
    // Primary coverage shortfall sits just below full coverage in the family.
    if primary_fraction + 1e-9 < plan.primary_fraction {
        weighted.push((
            w.coverage - 1.0,
            Relaxation {
                kind: "primary_coverage".into(),
                detail: format!(
                    "expert watched {:.1}% of team matches, below the {:.1}% primary target",
                    primary_fraction * 100.0,
                    plan.primary_fraction * 100.0
                ),
            },
        ));
    }

    // Quotas.
    let pit_short = (0..plan.scouts.len())
        .filter(|&s| scout_loads[s].pits < plan.pit_quota as usize)
        .count();
    if plan.pit_quota > 0 && pit_short > 0 {
        weighted.push((
            w.quotas,
            Relaxation {
                kind: "pit_quota".into(),
                detail: format!(
                    "{pit_short} scouts received fewer than the pit quota of {} (no expert free match available)",
                    plan.pit_quota
                ),
            },
        ));
    }
    let filler_short = (0..plan.scouts.len())
        .filter(|&s| scout_loads[s].watches < plan.filler_quota as usize)
        .count();
    if plan.filler_quota > 0 && filler_short > 0 {
        weighted.push((
            w.quotas - 1.0,
            Relaxation {
                kind: "filler_quota".into(),
                detail: format!(
                    "{filler_short} scouts received fewer total watches than the filler quota of {}",
                    plan.filler_quota
                ),
            },
        ));
    }

    // Qualitative fraction.
    let qual_target_total = qualitative_target(total_watches, plan.qualitative_fraction);
    if total_qual < qual_target_total {
        weighted.push((
            w.qualitative,
            Relaxation {
                kind: "qualitative_fraction".into(),
                detail: format!(
                    "qualitative tags {total_qual} fell short of target {qual_target_total} ({:.0}% of {total_watches} watches)",
                    plan.qualitative_fraction * 100.0
                ),
            },
        ));
    }

    // Break preference (lowest priority): count columns where a scout works
    // beyond the consecutive limit. These remain only when full coverage leaves
    // no rest slack, so they are the first thing to relax.
    let max_run = plan.max_consecutive as usize;
    let over_limit: usize = (0..plan.scouts.len())
        .map(|s| count_over_limit(&plan.grid[s], max_run))
        .sum();
    if over_limit > 0 {
        weighted.push((
            w.sparsity,
            Relaxation {
                kind: "max_consecutive".into(),
                detail: format!(
                    "{over_limit} columns exceed the consecutive limit of {max_run} (full coverage left no rest slack)"
                ),
            },
        ));
    }

    // Descending weight, then stable by kind for full determinism.
    weighted.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.kind.cmp(&b.1.kind))
    });
    let relaxations: Vec<Relaxation> = weighted.into_iter().map(|(_, r)| r).collect();

    Report {
        coverage_fraction,
        total_team_matches: total_tm,
        watched_team_matches: watched_tm,
        coverage_gaps: gaps,
        teams_with_expert,
        total_teams: plan.n_teams,
        primary_fraction,
        primary_target: plan.primary_fraction,
        scout_loads,
        qualitative_fraction,
        qualitative_target: plan.qualitative_fraction,
        relaxations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::solve_from_parts;

    fn run(team_count: usize, mpt: usize, scouts: usize, g: Globals, seed: u64) -> Report {
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
        solve_from_parts(sched, cfg).unwrap().report
    }

    #[test]
    fn ample_scouts_reach_full_coverage() {
        let r = run(12, 12, 12, Globals::default(), 1);
        assert!(r.full_coverage(), "gaps: {:?}", r.coverage_gaps);
        assert_eq!(r.coverage_fraction, 1.0);
        assert!(r.relaxations.iter().all(|x| x.kind != "coverage"));
    }

    #[test]
    fn too_few_scouts_reports_relaxation_not_failure() {
        // Two scouts cannot cover 36 teams of expert duty; report says so.
        let r = run(36, 6, 2, Globals::default(), 2);
        assert!(r.teams_with_expert <= 36);
        // Either expert coverage or watch coverage is relaxed.
        assert!(
            r.relaxations.iter().any(|x| x.kind.starts_with("coverage")),
            "expected a coverage relaxation, got {:?}",
            r.relaxations
        );
    }

    #[test]
    fn loads_sum_consistently() {
        let r = run(24, 10, 8, Globals::default(), 3);
        for load in &r.scout_loads {
            assert!(load.qualitative <= load.watches);
        }
    }

    #[test]
    fn coverage_fraction_in_unit_interval() {
        let r = run(30, 8, 5, Globals::default(), 4);
        assert!((0.0..=1.0).contains(&r.coverage_fraction));
        assert!((0.0..=1.0).contains(&r.qualitative_fraction));
    }
}
