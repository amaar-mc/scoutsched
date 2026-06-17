//! Integration tests for the core guarantee: across many varied instances the
//! solver always returns a complete, hard feasible schedule, and identical input
//! always produces byte identical output.
//!
//! These tests build instances with the deterministic sample generator (no real
//! Blue Alliance key needed) and a range of rosters and constraint sets, then
//! assert the hard constraints directly on the returned grid.

use scoutsched::config::{Config, Globals, ScoutConfig, WindowBound};
use scoutsched::model::Cell;
use scoutsched::output;
use scoutsched::sample::{generate, SampleParams};
use scoutsched::solver::{solve_from_parts, Schedule};

/// Reconstructs the availability window and own pit blocks for a scout from its
/// config, mirroring the solver's resolution, so the test can assert that no
/// active assignment falls outside availability.
fn window_of(sc: &ScoutConfig, n_matches: usize) -> (usize, usize, Vec<usize>) {
    let arrive = match &sc.arrive {
        Some(WindowBound::Index(i)) => (*i as usize).min(n_matches),
        _ => 0,
    };
    let leave = match &sc.leave {
        Some(WindowBound::Index(i)) => (*i as usize).min(n_matches),
        _ => n_matches,
    };
    let blocks: Vec<usize> = sc.own_pit.iter().map(|b| *b as usize).collect();
    (arrive, leave, blocks)
}

/// Asserts every hard constraint on a solved schedule against its config.
fn assert_hard_feasible(s: &Schedule, cfg: &Config) {
    let n = s.match_count();

    // The grid is complete and rectangular: one row per scout, one cell per
    // match, every cell a valid variant (guaranteed by the type, checked for
    // shape here).
    assert_eq!(s.grid.len(), cfg.scouts.len(), "row count mismatch");
    for row in &s.grid {
        assert_eq!(row.len(), n, "row width mismatch");
    }

    for (si, sc) in cfg.scouts.iter().enumerate() {
        let (arrive, leave, blocks) = window_of(sc, n);
        for m in 0..n {
            let cell = s.grid[si][m];

            // Hard: no activity outside the availability window.
            let in_window = m >= arrive && m < leave;
            if !in_window {
                assert!(
                    !cell.is_active(),
                    "scout {si} active at match {m} outside window {arrive}..{leave}"
                );
            }

            // Hard: no activity during own pit duty.
            if blocks.contains(&m) {
                assert!(
                    !cell.is_active(),
                    "scout {si} active during own pit block at match {m}"
                );
            }

            // Hard: a watched team must actually play that match; a pit scouted
            // team must not play that match.
            match cell {
                Cell::Watch { team, .. } => {
                    let num = s.team_numbers[team as usize];
                    assert!(
                        s.match_teams[m].contains(&num),
                        "scout {si} watches team {num} not in match {m}"
                    );
                }
                Cell::Pit { team } => {
                    let num = s.team_numbers[team as usize];
                    assert!(
                        !s.match_teams[m].contains(&num),
                        "scout {si} pit scouts team {num} while it plays match {m}"
                    );
                }
                Cell::Break | Cell::Unavailable => {}
            }
        }
    }

    // Hard: no scout has two activities in one match. This is structural since a
    // cell is a single variant, but we confirm the grid shape supports it: each
    // (scout, match) maps to exactly one cell, which the rectangular shape check
    // above already guarantees. One team per scout per match follows because a
    // cell names at most one team.
}

/// Builds a uniform roster of `n` scouts with default settings.
fn roster(n: usize) -> Vec<ScoutConfig> {
    (0..n)
        .map(|i| ScoutConfig {
            name: format!("scout{i}"),
            ..ScoutConfig::default()
        })
        .collect()
}

#[test]
fn always_hard_feasible_over_a_matrix_of_instances() {
    // A broad sweep of team counts, matches per team, scout counts, and seeds.
    let team_counts = [12usize, 18, 24, 30, 36, 48];
    let mpts = [6usize, 9, 12];
    let scout_counts = [3usize, 5, 8, 12, 20];
    let seeds = [1u64, 7, 42, 100];

    let mut instances = 0;
    for &tc in &team_counts {
        for &mpt in &mpts {
            // The product must tile into whole matches.
            if (tc * mpt) % 6 != 0 {
                continue;
            }
            for &sc in &scout_counts {
                for &seed in &seeds {
                    let sched = generate(SampleParams {
                        team_count: tc,
                        matches_per_team: mpt,
                        seed,
                    })
                    .expect("sample feasible");
                    let cfg = Config {
                        globals: Globals::default(),
                        scouts: roster(sc),
                    };
                    let solved =
                        solve_from_parts(sched, cfg.clone()).expect("solver returns a schedule");
                    assert_hard_feasible(&solved, &cfg);
                    instances += 1;
                }
            }
        }
    }
    // Make sure the sweep actually exercised many instances.
    assert!(instances >= 200, "expected a large sweep, ran {instances}");
}

#[test]
fn hard_feasible_with_tight_availability_and_own_pit() {
    // Constraint heavy instances: narrow windows, own pit blocks, partners.
    let configs = vec![tight_config_a(), tight_config_b(), tight_config_c()];
    let seeds = [3u64, 11, 99];

    for cfg in &configs {
        for &seed in &seeds {
            let sched = generate(SampleParams {
                team_count: 30,
                matches_per_team: 12,
                seed,
            })
            .unwrap();
            let solved = solve_from_parts(sched, cfg.clone()).expect("solver returns schedule");
            assert_hard_feasible(&solved, cfg);
        }
    }
}

fn tight_config_a() -> Config {
    let mut scouts = roster(6);
    // Half the team arrives late, half leaves early: only partial overlap.
    scouts[0].arrive = Some(WindowBound::Index(20));
    scouts[1].arrive = Some(WindowBound::Index(15));
    scouts[2].leave = Some(WindowBound::Index(40));
    scouts[3].leave = Some(WindowBound::Index(30));
    scouts[4].own_pit = vec![0, 1, 2, 3, 4, 5];
    Config {
        globals: Globals::default(),
        scouts,
    }
}

fn tight_config_b() -> Config {
    let mut scouts = roster(4);
    // A very small roster for 30 teams forces heavy relaxation but must stay
    // hard feasible.
    scouts[0].own_pit = vec![10, 11, 12];
    scouts[1].arrive = Some(WindowBound::Index(5));
    scouts[1].leave = Some(WindowBound::Index(55));
    Config {
        globals: Globals {
            pit_quota: 4,
            filler_quota: 5,
            qualitative_fraction: 0.4,
            max_consecutive: 3,
            ..Globals::default()
        },
        scouts,
    }
}

fn tight_config_c() -> Config {
    let mut scouts = roster(10);
    scouts[0].break_partner = Some("scout1".into());
    scouts[1].break_partner = Some("scout0".into());
    scouts[2].sparsity = 0.8;
    scouts[3].sparsity = 0.5;
    scouts[4].arrive = Some(WindowBound::Index(8));
    scouts[5].leave = Some(WindowBound::Index(50));
    scouts[6].own_pit = vec![20, 21, 22, 23];
    Config {
        globals: Globals {
            max_consecutive: 5,
            min_break_length: 2,
            ..Globals::default()
        },
        scouts,
    }
}

#[test]
fn identical_input_gives_byte_identical_output() {
    let seeds = [1u64, 50, 123];
    for &seed in &seeds {
        let make = || {
            let sched = generate(SampleParams {
                team_count: 32,
                matches_per_team: 12,
                seed,
            })
            .unwrap();
            let cfg = Config {
                globals: Globals::default(),
                scouts: roster(9),
            };
            solve_from_parts(sched, cfg).unwrap()
        };
        let a = make();
        let b = make();
        // Byte identical across all three output formats.
        assert_eq!(output::to_csv(&a), output::to_csv(&b), "csv differs");
        assert_eq!(output::to_json(&a), output::to_json(&b), "json differs");
        assert_eq!(
            output::to_summary(&a),
            output::to_summary(&b),
            "summary differs"
        );
    }
}

#[test]
fn coverage_and_quotas_met_when_capacity_is_ample() {
    // Many scouts relative to teams: full coverage and the qualitative target
    // should be met, with no coverage relaxation.
    let sched = generate(SampleParams {
        team_count: 18,
        matches_per_team: 12,
        seed: 5,
    })
    .unwrap();
    let cfg = Config {
        globals: Globals {
            qualitative_fraction: 0.2,
            ..Globals::default()
        },
        scouts: roster(18),
    };
    let solved = solve_from_parts(sched, cfg.clone()).unwrap();
    assert_hard_feasible(&solved, &cfg);
    assert!(solved.report.full_coverage(), "expected full coverage");
    assert!(
        solved
            .report
            .relaxations
            .iter()
            .all(|r| !r.kind.starts_with("coverage")),
        "unexpected coverage relaxation: {:?}",
        solved.report.relaxations
    );
}

#[test]
fn degrades_gracefully_when_over_constrained() {
    // Three scouts cannot cover 48 teams of expert duty. The solver must still
    // return a complete hard feasible schedule and report the relaxations rather
    // than failing.
    let sched = generate(SampleParams {
        team_count: 48,
        matches_per_team: 9,
        seed: 8,
    })
    .unwrap();
    let cfg = Config {
        globals: Globals::default(),
        scouts: roster(3),
    };
    let solved = solve_from_parts(sched, cfg.clone()).unwrap();
    assert_hard_feasible(&solved, &cfg);
    // Over constraint surfaces as reported relaxations, not an error.
    assert!(
        !solved.report.relaxations.is_empty(),
        "expected relaxations for an over constrained instance"
    );
}

#[test]
fn single_scout_instance_is_feasible() {
    // The degenerate roster of one scout: still complete and hard feasible.
    let sched = generate(SampleParams {
        team_count: 12,
        matches_per_team: 6,
        seed: 2,
    })
    .unwrap();
    let cfg = Config {
        globals: Globals::default(),
        scouts: roster(1),
    };
    let solved = solve_from_parts(sched, cfg.clone()).unwrap();
    assert_hard_feasible(&solved, &cfg);
}
