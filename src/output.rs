//! Output rendering: CSV grid, structured JSON, and a human readable summary.
//!
//! All three are pure functions of a `Schedule`. The CSV is the operational
//! artifact a scouting lead prints; the JSON is for programmatic consumers; the
//! summary is the at a glance report of coverage, load, and relaxations.

use crate::model::{Cell, WatchMode};
use crate::solver::Schedule;
use serde::Serialize;

/// Renders the schedule as CSV.
///
/// The first row labels each match column as `Qm{number}`. Each subsequent row
/// is one scout: the first field is the scout name, then one field per match. A
/// watch cell is the team number, with a trailing `*` for qualitative. A pit
/// cell is `PIT:{team}`. Break and unavailable cells are empty, which keeps the
/// grid readable at a glance. A legend is appended as comment rows.
pub fn to_csv(s: &Schedule) -> String {
    let mut out = String::new();

    // Header row.
    out.push_str("scout");
    for &mn in &s.match_numbers {
        out.push(',');
        out.push_str(&format!("Qm{mn}"));
    }
    out.push('\n');

    // One row per scout.
    for (si, name) in s.scout_names.iter().enumerate() {
        out.push_str(&csv_escape(name));
        for mi in 0..s.match_count() {
            out.push(',');
            out.push_str(&cell_label(&s.grid[si][mi], &s.team_numbers));
        }
        out.push('\n');
    }

    // Legend as trailing comment lines, prefixed with '#'.
    out.push_str("# legend\n");
    out.push_str("# 1680   watch team 1680, quantitative\n");
    out.push_str("# 1680*  watch team 1680, qualitative\n");
    out.push_str("# PIT:254 pit scout team 254\n");
    out.push_str("# (empty) break or unavailable\n");

    out
}

/// The textual label for a single cell, used by the CSV renderer.
fn cell_label(cell: &Cell, team_numbers: &[u32]) -> String {
    match cell {
        Cell::Watch { team, mode } => {
            let num = team_numbers[*team as usize];
            match mode {
                WatchMode::Quantitative => num.to_string(),
                WatchMode::Qualitative => format!("{num}*"),
            }
        }
        Cell::Pit { team } => format!("PIT:{}", team_numbers[*team as usize]),
        Cell::Break | Cell::Unavailable => String::new(),
    }
}

/// Quotes a CSV field if it contains a comma, quote, or newline.
fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n']) {
        let escaped = field.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        field.to_string()
    }
}

/// One scout's row in the JSON output.
#[derive(Debug, Clone, Serialize)]
struct JsonScoutRow {
    name: String,
    cells: Vec<JsonCell>,
}

/// One cell in the JSON output, a tagged shape that is easy to consume.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JsonCell {
    Watch { team: u32, mode: String },
    Pit { team: u32 },
    Break,
    Unavailable,
}

/// The full JSON document.
#[derive(Debug, Clone, Serialize)]
struct JsonSchedule<'a> {
    match_numbers: &'a [u32],
    match_teams: &'a [[u32; crate::model::MATCH_SIZE]],
    scouts: Vec<JsonScoutRow>,
    report: &'a crate::report::Report,
}

/// Renders the schedule as pretty printed JSON for programmatic use.
pub fn to_json(s: &Schedule) -> String {
    let scouts: Vec<JsonScoutRow> = s
        .scout_names
        .iter()
        .enumerate()
        .map(|(si, name)| JsonScoutRow {
            name: name.clone(),
            cells: (0..s.match_count())
                .map(|mi| json_cell(&s.grid[si][mi], &s.team_numbers))
                .collect(),
        })
        .collect();

    let doc = JsonSchedule {
        match_numbers: &s.match_numbers,
        match_teams: &s.match_teams,
        scouts,
        report: &s.report,
    };
    serde_json::to_string_pretty(&doc).expect("schedule json serializes")
}

fn json_cell(cell: &Cell, team_numbers: &[u32]) -> JsonCell {
    match cell {
        Cell::Watch { team, mode } => JsonCell::Watch {
            team: team_numbers[*team as usize],
            mode: match mode {
                WatchMode::Quantitative => "quantitative".into(),
                WatchMode::Qualitative => "qualitative".into(),
            },
        },
        Cell::Pit { team } => JsonCell::Pit {
            team: team_numbers[*team as usize],
        },
        Cell::Break => JsonCell::Break,
        Cell::Unavailable => JsonCell::Unavailable,
    }
}

/// Renders the human readable summary report.
pub fn to_summary(s: &Schedule) -> String {
    let r = &s.report;
    let mut out = String::new();

    out.push_str("scoutsched summary\n");
    out.push_str("==================\n\n");

    out.push_str(&format!(
        "matches: {}    scouts: {}    teams: {}\n",
        s.match_count(),
        s.scout_count(),
        r.total_teams
    ));
    out.push_str(&format!(
        "coverage: {}/{} team matches watched ({:.1}%)\n",
        r.watched_team_matches,
        r.total_team_matches,
        r.coverage_fraction * 100.0
    ));
    out.push_str(&format!(
        "experts: {}/{} teams have an assigned expert\n",
        r.teams_with_expert, r.total_teams
    ));
    out.push_str(&format!(
        "primary coverage: {:.1}% by experts (target {:.1}%)\n",
        r.primary_fraction * 100.0,
        r.primary_target * 100.0
    ));
    out.push_str(&format!(
        "qualitative: {:.1}% of watches (target {:.1}%)\n\n",
        r.qualitative_fraction * 100.0,
        r.qualitative_target * 100.0
    ));

    out.push_str("per scout load (watch / pit / break / qualitative):\n");
    for load in &r.scout_loads {
        out.push_str(&format!(
            "  {:<16} {:>3} / {:>3} / {:>3} / {:>3}\n",
            load.name, load.watches, load.pits, load.breaks, load.qualitative
        ));
    }
    out.push('\n');

    if r.relaxations.is_empty() {
        out.push_str("relaxations: none, all soft targets met\n");
    } else {
        out.push_str("relaxations (soft constraints loosened, highest priority first):\n");
        for relax in &r.relaxations {
            out.push_str(&format!("  [{}] {}\n", relax.kind, relax.detail));
        }
    }

    if !r.coverage_gaps.is_empty() {
        out.push_str(&format!("\ncoverage gaps ({}):\n", r.coverage_gaps.len()));
        for gap in &r.coverage_gaps {
            out.push_str(&format!(
                "  match {} team {} unwatched\n",
                gap.match_number, gap.team_number
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Globals, ScoutConfig};
    use crate::sample::{generate, SampleParams};
    use crate::solver::solve_from_parts;

    fn schedule(team_count: usize, mpt: usize, scouts: usize, seed: u64) -> Schedule {
        let sched = generate(SampleParams {
            team_count,
            matches_per_team: mpt,
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
        solve_from_parts(sched, cfg).unwrap()
    }

    #[test]
    fn csv_has_header_and_one_row_per_scout() {
        let s = schedule(24, 9, 6, 1);
        let csv = to_csv(&s);
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].starts_with("scout,Qm"));
        // Header + 6 scout rows + legend lines.
        let scout_rows = lines
            .iter()
            .filter(|l| l.starts_with("S") && !l.starts_with("# "))
            .count();
        assert_eq!(scout_rows, 6);
        // Each scout row has the right number of fields.
        let fields = lines[1].split(',').count();
        assert_eq!(fields, 1 + s.match_count());
    }

    #[test]
    fn csv_cell_labels_are_correct() {
        // Construct a tiny known schedule and check labels directly.
        assert_eq!(
            cell_label(
                &Cell::Watch {
                    team: 0,
                    mode: WatchMode::Quantitative
                },
                &[1680]
            ),
            "1680"
        );
        assert_eq!(
            cell_label(
                &Cell::Watch {
                    team: 0,
                    mode: WatchMode::Qualitative
                },
                &[1680]
            ),
            "1680*"
        );
        assert_eq!(cell_label(&Cell::Pit { team: 1 }, &[1680, 254]), "PIT:254");
        assert_eq!(cell_label(&Cell::Break, &[1680]), "");
        assert_eq!(cell_label(&Cell::Unavailable, &[1680]), "");
    }

    #[test]
    fn json_parses_back_and_has_expected_shape() {
        let s = schedule(24, 9, 6, 2);
        let json = to_json(&s);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert!(value["match_numbers"].is_array());
        assert_eq!(value["scouts"].as_array().unwrap().len(), 6);
        assert!(value["report"]["coverage_fraction"].is_number());
        // A cell has a kind tag.
        let first_cell = &value["scouts"][0]["cells"][0];
        assert!(first_cell["kind"].is_string());
    }

    #[test]
    fn summary_mentions_coverage_and_loads() {
        let s = schedule(24, 9, 6, 3);
        let summary = to_summary(&s);
        assert!(summary.contains("coverage:"));
        assert!(summary.contains("per scout load"));
        assert!(summary.contains("relaxations"));
    }

    #[test]
    fn outputs_are_deterministic() {
        let a = schedule(30, 10, 8, 5);
        let b = schedule(30, 10, 8, 5);
        assert_eq!(to_csv(&a), to_csv(&b));
        assert_eq!(to_json(&a), to_json(&b));
        assert_eq!(to_summary(&a), to_summary(&b));
    }

    #[test]
    fn csv_escapes_commas_in_names() {
        assert_eq!(csv_escape("Doe, Jane"), "\"Doe, Jane\"");
        assert_eq!(csv_escape("plain"), "plain");
    }
}
