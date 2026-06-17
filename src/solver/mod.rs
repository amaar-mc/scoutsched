//! The deterministic scheduling solver.
//!
//! # Overview
//!
//! The solver turns a parsed qualification schedule and a scout roster into a
//! complete grid of cells, one per (scout, match) pair. It runs six phases:
//!
//! 1. Resolve config and build availability bitsets and a team to matches index.
//! 2. Expertise assignment: give each scout k expert teams, scaled so every team
//!    has at least one expert.
//! 3. Primary coverage: experts watch their teams' matches.
//! 4. Filler, pit, and mode tagging: spread baseline watches, place pit scouting
//!    when no expert team plays, and tag a minority of watches qualitative.
//! 5. Break and sparsity enforcement: bound consecutive work, honor preferences.
//! 6. Bounded deterministic local search repair: fix any remaining coverage gaps
//!    and improve soft preferences without breaking hard constraints.
//!
//! # Guarantees
//!
//! The returned `Schedule` is always complete (every cell assigned) and hard
//! feasible: no scout is ever assigned outside availability or during own pit
//! duty, no scout has two activities in one match (structural, one cell per
//! pair), and a watched team is one of the match's six teams. Soft constraints
//! are relaxed by documented priority when an instance is over constrained, and
//! every relaxation is reported.
//!
//! # Determinism
//!
//! There is no randomness. Every choice uses a fixed total order over indices
//! and stable sorts, so identical input yields byte identical output.

mod breaks;
mod coverage;
mod expertise;
mod fill;
mod repair;

use crate::config::{Config, Weights, WindowBound};
use crate::model::{Cell, Match, MatchBitset, MatchIndex, Scout, ScoutId, TeamId};
use crate::report::Report;
use crate::tba::ParsedSchedule;

/// Everything the solver needs: the parsed schedule, the resolved config, and
/// the FRC team numbers for reporting.
#[derive(Debug, Clone)]
pub struct SolveInput {
    pub schedule: ParsedSchedule,
    pub config: Config,
}

/// Errors that prevent the solver from even starting. These are structural
/// problems with the instance, not over constraint, which is always handled by
/// relaxation rather than failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveError {
    /// The roster was empty.
    NoScouts,
    /// A scout's resolved availability window was empty or inverted.
    BadWindow(String),
    /// A clock time could not be mapped because the schedule has no times.
    UnmappableTime(String),
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::NoScouts => write!(f, "no scouts configured"),
            SolveError::BadWindow(m) => write!(f, "bad availability window: {m}"),
            SolveError::UnmappableTime(m) => write!(f, "unmappable clock time: {m}"),
        }
    }
}

impl std::error::Error for SolveError {}

/// The completed schedule and its report.
#[derive(Debug, Clone)]
pub struct Schedule {
    /// `grid[scout][match]` is the assignment for that pair.
    pub grid: Vec<Vec<Cell>>,
    /// Scout names in id order, for labeling.
    pub scout_names: Vec<String>,
    /// Match numbers in column order, for labeling.
    pub match_numbers: Vec<u32>,
    /// FRC team numbers indexed by internal team id.
    pub team_numbers: Vec<u32>,
    /// The six FRC team numbers per match column, red first then blue, so
    /// consumers and tests can see who plays each match.
    pub match_teams: Vec<[u32; crate::model::MATCH_SIZE]>,
    /// What the solver achieved and what it relaxed.
    pub report: Report,
}

impl Schedule {
    /// Number of scouts (grid rows).
    pub fn scout_count(&self) -> usize {
        self.grid.len()
    }

    /// Number of matches (grid columns).
    pub fn match_count(&self) -> usize {
        self.match_numbers.len()
    }
}

/// Resolved, solver internal hyperparameters with availability folded into
/// bitsets. Built once at the start so phases never re read the TOML config.
pub(crate) struct Plan {
    /// Number of matches.
    pub n_matches: usize,
    /// The matches in column order.
    pub matches: Vec<Match>,
    /// Number of distinct teams.
    pub n_teams: usize,
    /// FRC team numbers indexed by internal team id, for reporting.
    pub team_numbers: Vec<u32>,
    /// Resolved scouts.
    pub scouts: Vec<Scout>,
    /// `available[scout]` has a set bit for each match the scout can work.
    pub available: Vec<MatchBitset>,
    /// `team_matches[team]` lists the match columns where the team plays.
    pub team_matches: Vec<Vec<MatchIndex>>,
    /// Expert teams per scout, ascending team id.
    pub experts: Vec<Vec<TeamId>>,
    /// Experts per team, ascending scout id (inverse of `experts`).
    pub team_experts: Vec<Vec<ScoutId>>,
    /// The working grid.
    pub grid: Vec<Vec<Cell>>,
    /// Hyperparameters copied from config for quick access.
    pub experts_min: u16,
    pub experts_max: u16,
    pub primary_fraction: f64,
    pub filler_quota: u16,
    pub pit_quota: u16,
    pub qualitative_fraction: f64,
    pub max_consecutive: u16,
    pub min_break_length: u16,
    pub repair_iterations: u32,
    pub weights: Weights,
}

impl Plan {
    /// Convenience: is this scout available for this match.
    pub fn is_available(&self, scout: usize, m: usize) -> bool {
        self.available[scout].get(m)
    }

    /// Count of a scout's active assignments (watch or pit).
    pub fn active_load(&self, scout: usize) -> usize {
        self.grid[scout].iter().filter(|c| c.is_active()).count()
    }

    /// Returns the scout currently watching `team` in match `m`, if any.
    pub fn watcher_of(&self, m: usize, team: TeamId) -> Option<ScoutId> {
        for (s, row) in self.grid.iter().enumerate() {
            if let Cell::Watch { team: t, .. } = row[m] {
                if t == team {
                    return Some(s as ScoutId);
                }
            }
        }
        None
    }
}

/// Solves an instance, always returning a complete hard feasible schedule.
pub fn solve(input: SolveInput) -> Result<Schedule, SolveError> {
    let mut plan = build_plan(&input)?;

    expertise::assign(&mut plan);
    coverage::assign(&mut plan);
    // Close coverage gaps with the free slack that still exists, before filler
    // and pit consume it. Coverage is the top priority, so it claims slack first.
    repair::run(&mut plan);
    fill::assign(&mut plan);
    // Breaks run last so that nothing reintroduces work into a rest column. Break
    // enforcement only ever converts redundant or pit cells, so it cannot reduce
    // coverage; this ordering is what makes coverage strictly dominate the break
    // and sparsity preferences in the documented priority.
    breaks::enforce(&mut plan);

    let report = crate::report::build(&plan);

    let scout_names = plan.scouts.iter().map(|s| s.name.clone()).collect();
    let match_numbers = plan.matches.iter().map(|m| m.match_number).collect();
    let team_numbers = input.schedule.team_numbers;
    let match_teams = plan
        .matches
        .iter()
        .map(|m| {
            let mut row = [0u32; crate::model::MATCH_SIZE];
            for (i, &t) in m.teams.iter().enumerate() {
                row[i] = team_numbers[t as usize];
            }
            row
        })
        .collect();

    Ok(Schedule {
        grid: plan.grid,
        scout_names,
        match_numbers,
        team_numbers,
        match_teams,
        report,
    })
}

/// Phase 1: resolve config into a `Plan`, building availability bitsets, the team
/// to matches index, and the initial grid (Unavailable outside windows and
/// during own pit duty, Break everywhere else).
fn build_plan(input: &SolveInput) -> Result<Plan, SolveError> {
    let cfg = &input.config;
    if cfg.scouts.is_empty() {
        return Err(SolveError::NoScouts);
    }
    let matches = input.schedule.matches.clone();
    let n_matches = matches.len();
    let n_teams = input.schedule.team_count();

    // Build the team to matches index in a single pass.
    let mut team_matches: Vec<Vec<MatchIndex>> = vec![Vec::new(); n_teams];
    for (mi, m) in matches.iter().enumerate() {
        for &t in &m.teams {
            team_matches[t as usize].push(mi as MatchIndex);
        }
    }

    // Resolve each scout: window bounds (index or clock time) into a half open
    // range, own pit blocks, then an availability bitset.
    let mut scouts: Vec<Scout> = Vec::with_capacity(cfg.scouts.len());
    let name_to_id: std::collections::HashMap<&str, ScoutId> = cfg
        .scouts
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i as ScoutId))
        .collect();

    for sc in &cfg.scouts {
        let arrive = resolve_bound(sc.arrive.as_ref(), 0, n_matches)?;
        let leave = resolve_bound(sc.leave.as_ref(), n_matches as u32, n_matches)?;
        if arrive >= leave {
            return Err(SolveError::BadWindow(format!(
                "scout '{}' arrives at {arrive} but leaves at {leave}",
                sc.name
            )));
        }
        let break_partner = sc
            .break_partner
            .as_ref()
            .and_then(|p| name_to_id.get(p.as_str()).copied());
        scouts.push(Scout {
            name: sc.name.clone(),
            arrive,
            leave,
            own_pit_blocks: sc.own_pit.clone(),
            sparsity: sc.sparsity,
            break_partner,
        });
    }

    // Availability bitset: set bits inside the window, cleared for own pit
    // blocks and for matches beyond the schedule length.
    let mut available: Vec<MatchBitset> = Vec::with_capacity(scouts.len());
    for s in &scouts {
        let mut b = MatchBitset::new(n_matches);
        let lo = s.arrive as usize;
        let hi = (s.leave as usize).min(n_matches);
        for m in lo..hi {
            b.set(m);
        }
        for &blk in &s.own_pit_blocks {
            let blk = blk as usize;
            if blk < n_matches {
                b.clear(blk);
            }
        }
        available.push(b);
    }

    // Initial grid: Unavailable where a scout cannot work, Break otherwise.
    let grid: Vec<Vec<Cell>> = available
        .iter()
        .map(|avail| {
            (0..n_matches)
                .map(|m| {
                    if avail.get(m) {
                        Cell::Break
                    } else {
                        Cell::Unavailable
                    }
                })
                .collect()
        })
        .collect();

    let g = &cfg.globals;
    Ok(Plan {
        n_matches,
        matches,
        n_teams,
        team_numbers: input.schedule.team_numbers.clone(),
        scouts,
        available,
        team_matches,
        experts: Vec::new(),
        team_experts: Vec::new(),
        grid,
        experts_min: g.experts_per_scout_min,
        experts_max: g.experts_per_scout_max,
        primary_fraction: g.primary_fraction,
        filler_quota: g.filler_quota,
        pit_quota: g.pit_quota,
        qualitative_fraction: g.qualitative_fraction,
        max_consecutive: g.max_consecutive,
        min_break_length: g.min_break_length,
        repair_iterations: g.repair_iterations,
        weights: g.weights,
    })
}

/// Maps a window bound to a match column. `Index` is clamped to the schedule.
/// `Time` is not yet supported by the sample path and returns an error so the
/// limitation is explicit rather than silent.
fn resolve_bound(
    bound: Option<&WindowBound>,
    default: u32,
    n_matches: usize,
) -> Result<MatchIndex, SolveError> {
    match bound {
        None => Ok(default.min(n_matches as u32)),
        Some(WindowBound::Index(i)) => Ok((*i).min(n_matches as u32)),
        Some(WindowBound::Time(t)) => Err(SolveError::UnmappableTime(format!(
            "clock time '{}' cannot be mapped: this build maps availability by match index only",
            t.0
        ))),
    }
}

/// Shared helper: choose the qualitative target count for a watch list size.
/// Rounds to nearest so small lists still get representation when the fraction
/// is non zero.
pub(crate) fn qualitative_target(total: usize, fraction: f64) -> usize {
    ((total as f64) * fraction).round() as usize
}

/// Shared helper: the configured number of experts per scout, scaled up when
/// there are more teams than the roster can cover at the minimum so that every
/// team can have at least one expert. Capped at the team count.
pub(crate) fn experts_per_scout(n_teams: usize, n_scouts: usize, min: u16, max: u16) -> usize {
    debug_assert!(n_scouts > 0);
    let min = min as usize;
    let max = max as usize;
    // Coverage needs at least ceil(n_teams / n_scouts) experts each. Start from
    // the configured range, then scale up to that need so the roster can cover
    // every team. The result never drops below 1 and never exceeds n_teams.
    let needed = n_teams.div_ceil(n_scouts);
    let mut k = needed.clamp(min, max);
    if needed > max {
        k = needed;
    }
    k.clamp(1, n_teams.max(1))
}

/// Convenience constructor used by tests and the binary.
pub fn solve_from_parts(schedule: ParsedSchedule, config: Config) -> Result<Schedule, SolveError> {
    solve(SolveInput { schedule, config })
}

/// Test only access to phase 1 so phase modules can build a `Plan` directly.
#[cfg(test)]
pub(crate) fn build_plan_for_test(input: &SolveInput) -> Result<Plan, SolveError> {
    build_plan(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};

    fn tiny_config(n: usize) -> Config {
        Config {
            globals: Globals::default(),
            scouts: (0..n)
                .map(|i| ScoutConfig {
                    name: format!("S{i}"),
                    ..ScoutConfig::default()
                })
                .collect(),
        }
    }

    #[test]
    fn build_plan_sets_availability_and_index() {
        let sched = generate(SampleParams {
            team_count: 18,
            matches_per_team: 6,
            seed: 1,
        })
        .unwrap();
        let cfg = tiny_config(4);
        let plan = build_plan(&SolveInput {
            schedule: sched.clone(),
            config: cfg,
        })
        .unwrap();
        assert_eq!(plan.n_matches, sched.matches.len());
        assert_eq!(plan.team_matches.len(), 18);
        // Every team plays 6 matches.
        assert!(plan.team_matches.iter().all(|v| v.len() == 6));
        // Full availability for default windows.
        assert!(plan.available.iter().all(|b| b.count() == plan.n_matches));
    }

    #[test]
    fn no_scouts_errors() {
        let sched = generate(SampleParams {
            team_count: 12,
            matches_per_team: 6,
            seed: 1,
        })
        .unwrap();
        let err = solve_from_parts(sched, tiny_config(0)).unwrap_err();
        assert_eq!(err, SolveError::NoScouts);
    }

    #[test]
    fn experts_per_scout_scales_for_coverage() {
        // 30 teams, 6 scouts, min 2 max 3: ceil(30/6)=5 > max, scale to 5.
        assert_eq!(experts_per_scout(30, 6, 2, 3), 5);
        // 12 teams, 6 scouts: ceil=2, within range.
        assert_eq!(experts_per_scout(12, 6, 2, 3), 2);
        // few teams clamp to team count.
        assert_eq!(experts_per_scout(1, 6, 2, 3), 1);
    }

    #[test]
    fn solve_returns_complete_grid() {
        let sched = generate(SampleParams {
            team_count: 24,
            matches_per_team: 9,
            seed: 3,
        })
        .unwrap();
        let s = solve_from_parts(sched, tiny_config(8)).unwrap();
        assert_eq!(s.scout_count(), 8);
        assert_eq!(s.match_count(), 36);
        // Every cell is assigned (no panics, full rectangular grid).
        assert!(s.grid.iter().all(|r| r.len() == 36));
    }
}
