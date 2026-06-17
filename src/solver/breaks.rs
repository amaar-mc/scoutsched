//! Phase 5: break and sparsity enforcement.
//!
//! Three soft objectives are handled here, all below coverage in priority:
//!
//! - Max consecutive: no scout should work more than `max_consecutive` active
//!   cells in a row. When a run is too long, a cell is converted to a Break for
//!   at least `min_break_length` columns. To avoid creating a coverage gap when
//!   it can be helped, the cell chosen to become a break is preferentially one
//!   whose team is also watched by someone else in that match, or a filler or
//!   pit cell, before a sole expert watch.
//! - Sparsity: a scout with a higher sparsity preference is trimmed toward a
//!   lighter load by converting some redundant active cells to Break. Redundant
//!   means the team is still covered by another scout, so coverage is preserved.
//! - Break matching: two scouts who named each other as break partners have
//!   their break columns nudged toward alignment where a free Break can be
//!   created without dropping unique coverage.
//!
//! Because every conversion only turns an active cell into a Break (never the
//! reverse here, and never touches Unavailable), the hard constraints are
//! untouched. Coverage lost here is a candidate for the repair phase to recover.

use crate::model::{Cell, ScoutId};
use crate::solver::Plan;

/// Enforces max consecutive, applies sparsity trimming, then aligns partner
/// breaks.
pub(crate) fn enforce(plan: &mut Plan) {
    // Hand offs during max consecutive enforcement move work onto previously
    // idle scouts, which can create new long runs for them. Iterate to a bounded
    // fixed point so recipients are re checked. Each pass strictly increases the
    // number of break cells (a value bounded by the grid size), so the loop
    // always terminates; the cap is a safety bound, not a tuning knob.
    let cap = plan.n_matches + plan.scouts.len() + 1;
    for _ in 0..cap {
        if !enforce_max_consecutive(plan) {
            break;
        }
    }
    apply_sparsity(plan);
    align_partner_breaks(plan);
}

/// Returns true when converting `(scout, m)` to a Break would not orphan its
/// team, that is the cell is not the only one watching that team in the match.
/// Pit cells are always safe to convert.
fn safe_to_break(plan: &Plan, scout: usize, m: usize) -> bool {
    match plan.grid[scout][m] {
        Cell::Watch { team, .. } => (0..plan.scouts.len()).any(|other| {
            other != scout
                && matches!(plan.grid[other][m], Cell::Watch { team: t, .. } if t == team)
        }),
        Cell::Pit { .. } => true,
        Cell::Break | Cell::Unavailable => false,
    }
}

/// Length of the contiguous active run that would result for `scout` if column
/// `m` became active, counting active columns immediately before and after `m`.
fn merged_run_at(plan: &Plan, scout: usize, m: usize) -> usize {
    let n = plan.n_matches;
    let mut len = 1usize;
    let mut j = m;
    while j > 0 && plan.grid[scout][j - 1].is_active() {
        len += 1;
        j -= 1;
    }
    let mut k = m + 1;
    while k < n && plan.grid[scout][k].is_active() {
        len += 1;
        k += 1;
    }
    len
}

/// Recipient scout for a hand off at match `m`: available and currently free.
/// Among free scouts the one whose resulting merged run is shortest is chosen,
/// so the hand off spreads work and avoids creating long runs; ties break by
/// lightest load, then by ascending id. Recipients that would stay within
/// `max_consecutive` are always preferred over those that would exceed it. The
/// outer enforcement loop is bounded by a fixed iteration cap, which guarantees
/// termination regardless of the choice here.
fn free_scout(plan: &Plan, m: usize, except: usize) -> Option<usize> {
    let max = plan.max_consecutive as usize;
    let mut best: Option<(bool, usize, usize, usize)> = None;
    for s in 0..plan.scouts.len() {
        if s != except && plan.is_available(s, m) && plan.grid[s][m] == Cell::Break {
            let merged = merged_run_at(plan, s, m);
            let over = merged > max;
            let load = plan.active_load(s);
            let cand = (over, merged, load, s);
            match best {
                None => best = Some(cand),
                // Prefer not over limit, then shorter merged run, then lighter
                // load, then lower id. Tuple order with `over` as bool gives that
                // since false sorts before true.
                Some(b) if cand < b => best = Some(cand),
                _ => {}
            }
        }
    }
    best.map(|(_, _, _, s)| s)
}

/// Frees `(scout, m)` to a Break while preserving coverage.
///
/// A pit cell or a redundant watch is converted directly. A unique watch is
/// first handed off to a free scout in the same column so the team stays
/// covered, then the original is broken. Returns true on success; false means
/// the cell is a unique watch with no free scout to take it, so breaking it would
/// drop coverage and is refused (coverage outranks the break preference).
fn make_break(plan: &mut Plan, scout: usize, m: usize) -> bool {
    match plan.grid[scout][m] {
        Cell::Pit { .. } => {
            plan.grid[scout][m] = Cell::Break;
            true
        }
        Cell::Watch { team, mode } => {
            if safe_to_break(plan, scout, m) {
                plan.grid[scout][m] = Cell::Break;
                return true;
            }
            if let Some(other) = free_scout(plan, m, scout) {
                plan.grid[other][m] = Cell::Watch { team, mode };
                plan.grid[scout][m] = Cell::Break;
                return true;
            }
            false
        }
        Cell::Break | Cell::Unavailable => false,
    }
}

/// Walks each scout's row once and breaks up runs longer than `max_consecutive`.
///
/// When a run exceeds the limit, a break is inserted at the offending cell using
/// `make_break`, which hands off any unique coverage to a free scout first, so
/// coverage is never reduced. If the cell cannot be freed (a unique watch with no
/// free scout in that column), the run is genuinely irreducible without dropping
/// coverage, so it is accepted and the counter resets; this is the over
/// constrained case where the lower priority break preference yields to the
/// higher priority coverage objective. Returns true when at least one break was
/// inserted, so the caller can iterate to a fixed point.
fn enforce_max_consecutive(plan: &mut Plan) -> bool {
    let max = plan.max_consecutive as usize;
    let min_break = plan.min_break_length.max(1) as usize;
    let n = plan.n_matches;
    let mut changed = false;

    for scout in 0..plan.scouts.len() {
        let mut run = 0usize;
        let mut m = 0usize;
        while m < n {
            if plan.grid[scout][m].is_active() {
                run += 1;
                if run > max {
                    // Try to insert a break starting at this cell, handing off
                    // coverage where needed, for up to min_break columns.
                    let mut inserted = 0usize;
                    let mut k = m;
                    while k < n && inserted < min_break {
                        if plan.grid[scout][k].is_active() && make_break(plan, scout, k) {
                            inserted += 1;
                        } else if plan.grid[scout][k].is_active() {
                            // Could not free this cell without dropping coverage;
                            // stop trying to extend the break here.
                            break;
                        }
                        k += 1;
                    }
                    if inserted > 0 {
                        changed = true;
                        run = 0;
                        m = k;
                        continue;
                    } else {
                        // Irreducible at this position: accept the long run and
                        // reset so we do not retry every following cell.
                        run = 0;
                    }
                }
            } else {
                run = 0;
            }
            m += 1;
        }
    }
    changed
}

/// Trims redundant active cells for scouts with a sparsity preference.
///
/// The number of cells trimmed scales with the preference: a sparsity of 1.0
/// trims all redundant cells, 0.0 trims none. Only cells that are safe to break
/// (team still covered, or pit) are eligible, so coverage is never reduced.
fn apply_sparsity(plan: &mut Plan) {
    let n = plan.n_matches;
    for scout in 0..plan.scouts.len() {
        let pref = plan.scouts[scout].sparsity;
        if pref <= 0.0 {
            continue;
        }
        let redundant: Vec<usize> = (0..n)
            .filter(|&m| plan.grid[scout][m].is_active() && safe_to_break(plan, scout, m))
            .collect();
        let to_trim = ((redundant.len() as f64) * pref).round() as usize;
        for &m in redundant.iter().take(to_trim) {
            // Recheck: an earlier trim in this loop could have made this cell the
            // sole watcher of its team.
            if safe_to_break(plan, scout, m) {
                plan.grid[scout][m] = Cell::Break;
            }
        }
    }
}

/// Nudges break partners toward shared break columns.
///
/// For each unordered partner pair, find columns where one is on a Break and the
/// other has a redundant active cell, and convert the latter to a Break so both
/// rest together. Only acts when it costs no unique coverage.
fn align_partner_breaks(plan: &mut Plan) {
    let n = plan.n_matches;
    let pairs = partner_pairs(plan);
    for (a, b) in pairs {
        let (a, b) = (a as usize, b as usize);
        for m in 0..n {
            let a_break = matches!(plan.grid[a][m], Cell::Break);
            let b_break = matches!(plan.grid[b][m], Cell::Break);
            if a_break && !b_break && safe_to_break(plan, b, m) {
                plan.grid[b][m] = Cell::Break;
            } else if b_break && !a_break && safe_to_break(plan, a, m) {
                plan.grid[a][m] = Cell::Break;
            }
        }
    }
}

/// Collects unique unordered break partner pairs `(lo, hi)` with `lo < hi`, only
/// when both scouts name each other, so alignment is mutual.
fn partner_pairs(plan: &Plan) -> Vec<(ScoutId, ScoutId)> {
    let mut out = Vec::new();
    for (s, scout) in plan.scouts.iter().enumerate() {
        if let Some(p) = scout.break_partner {
            let pu = p as usize;
            if pu < plan.scouts.len()
                && plan.scouts[pu].break_partner == Some(s as ScoutId)
                && (s as ScoutId) < p
            {
                out.push((s as ScoutId, p));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::{build_plan_for_test, coverage, expertise, fill, SolveInput};

    fn prepared(scouts: Vec<ScoutConfig>, g: Globals, seed: u64) -> Plan {
        let sched = generate(SampleParams {
            team_count: 24,
            matches_per_team: 12,
            seed,
        })
        .unwrap();
        let cfg = Config { globals: g, scouts };
        let mut plan = build_plan_for_test(&SolveInput {
            schedule: sched,
            config: cfg,
        })
        .unwrap();
        expertise::assign(&mut plan);
        coverage::assign(&mut plan);
        fill::assign(&mut plan);
        plan
    }

    fn named(n: usize) -> Vec<ScoutConfig> {
        (0..n)
            .map(|i| ScoutConfig {
                name: format!("S{i}"),
                ..ScoutConfig::default()
            })
            .collect()
    }

    fn max_run(row: &[Cell]) -> usize {
        let mut best = 0;
        let mut run = 0;
        for c in row {
            if c.is_active() {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
        best
    }

    #[test]
    fn max_consecutive_reduces_runs_without_dropping_coverage() {
        // Many scouts relative to teams gives redundancy, so the safe break
        // logic has cells it can convert. With 16 scouts and 24 teams every team
        // match has spare watchers, so runs can be cut to the limit.
        let g = Globals {
            max_consecutive: 3,
            min_break_length: 1,
            ..Globals::default()
        };
        let mut plan = prepared(named(16), g, 1);
        let covered_before = covered_team_matches(&plan);
        enforce(&mut plan);
        let covered_after = covered_team_matches(&plan);

        // Coverage is never reduced by break enforcement.
        assert!(
            covered_before.iter().all(|tm| covered_after.contains(tm)),
            "break enforcement dropped coverage"
        );
        // With ample redundancy, every run is cut to the limit.
        for (s, row) in plan.grid.iter().enumerate() {
            assert!(
                max_run(row) <= 3,
                "scout {s} has a run of {} exceeding the limit with redundancy available",
                max_run(row)
            );
        }
    }

    #[test]
    fn max_consecutive_never_drops_coverage_even_when_tight() {
        // Few scouts: runs may be irreducible, but coverage must still hold.
        let g = Globals {
            max_consecutive: 2,
            min_break_length: 1,
            ..Globals::default()
        };
        let mut plan = prepared(named(6), g, 2);
        let covered_before = covered_team_matches(&plan);
        enforce(&mut plan);
        let covered_after = covered_team_matches(&plan);
        assert_eq!(
            covered_before, covered_after,
            "break enforcement changed coverage in a tight instance"
        );
    }

    #[test]
    fn sparsity_reduces_load_without_dropping_unique_coverage() {
        let mut scouts = named(8);
        scouts[0].sparsity = 1.0;
        let g = Globals::default();
        let mut plan = prepared(scouts, g, 2);

        // Test sparsity trimming in isolation: max consecutive enforcement may
        // legitimately drop coverage (the repair phase recovers it), but the
        // sparsity step alone must never reduce coverage because it only trims
        // redundant cells.
        let covered_before = covered_team_matches(&plan);
        let load_before = plan.active_load(0);
        apply_sparsity(&mut plan);
        let load_after = plan.active_load(0);
        let covered_after = covered_team_matches(&plan);

        assert!(
            load_after <= load_before,
            "sparsity should not increase load"
        );
        assert!(
            covered_before.iter().all(|tm| covered_after.contains(tm)),
            "sparsity dropped unique coverage"
        );
    }

    fn covered_team_matches(plan: &Plan) -> std::collections::HashSet<(usize, u16)> {
        let mut set = std::collections::HashSet::new();
        for m in 0..plan.n_matches {
            for &team in plan.matches[m].teams.iter() {
                if plan.watcher_of(m, team).is_some() {
                    set.insert((m, team));
                }
            }
        }
        set
    }

    #[test]
    fn partner_breaks_are_mutual_only() {
        let mut scouts = named(4);
        scouts[0].break_partner = Some("S1".into());
        let g = Globals::default();
        let plan = prepared(scouts, g, 3);
        assert!(
            partner_pairs(&plan).is_empty(),
            "non mutual partner formed a pair"
        );
    }

    #[test]
    fn partner_breaks_pair_when_mutual() {
        let mut scouts = named(4);
        scouts[0].break_partner = Some("S1".into());
        scouts[1].break_partner = Some("S0".into());
        let g = Globals::default();
        let plan = prepared(scouts, g, 3);
        assert_eq!(partner_pairs(&plan), vec![(0u16, 1u16)]);
    }

    #[test]
    fn deterministic() {
        let mut a = prepared(named(7), Globals::default(), 5);
        let mut b = prepared(named(7), Globals::default(), 5);
        enforce(&mut a);
        enforce(&mut b);
        assert_eq!(a.grid, b.grid);
    }
}
