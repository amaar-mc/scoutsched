//! The Blue Alliance API v3 client and match JSON parsing.
//!
//! Two entry points share one parser: `fetch_event_matches` performs a blocking
//! HTTP GET against the live API, and `parse_matches_json` reads the same JSON
//! shape from a local file so the tool and its tests run with no network and no
//! real key. Both produce a `ParsedSchedule` of qualification matches in match
//! number order, plus the dense team id table used everywhere downstream.

use crate::model::{Match, TeamId, MATCH_SIZE};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

const TBA_BASE: &str = "https://www.thebluealliance.com/api/v3";

/// Errors from fetching or parsing The Blue Alliance match data.
#[derive(Debug)]
pub enum TbaError {
    /// Network or HTTP transport failure.
    Http(String),
    /// The response body or file was not valid JSON in the expected shape.
    Parse(String),
    /// The data was well formed JSON but not a usable schedule.
    Schedule(String),
    /// A local matches file could not be read.
    Io(String),
}

impl fmt::Display for TbaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TbaError::Http(m) => write!(f, "TBA http error: {m}"),
            TbaError::Parse(m) => write!(f, "TBA parse error: {m}"),
            TbaError::Schedule(m) => write!(f, "schedule error: {m}"),
            TbaError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for TbaError {}

/// One alliance inside a The Blue Alliance match object.
#[derive(Debug, Clone, Deserialize)]
pub struct TbaAlliance {
    pub team_keys: Vec<String>,
}

/// Both alliances of a match.
#[derive(Debug, Clone, Deserialize)]
pub struct TbaAlliances {
    pub red: TbaAlliance,
    pub blue: TbaAlliance,
}

/// The subset of a The Blue Alliance match object that scoutsched consumes.
#[derive(Debug, Clone, Deserialize)]
pub struct TbaMatch {
    pub comp_level: String,
    pub match_number: u32,
    #[serde(default)]
    pub set_number: u32,
    pub alliances: TbaAlliances,
}

/// A parsed qualification schedule: matches in match number order and the table
/// that maps each internal `TeamId` back to its FRC team number.
#[derive(Debug, Clone)]
pub struct ParsedSchedule {
    pub matches: Vec<Match>,
    /// `team_numbers[id as usize]` is the FRC team number for that internal id.
    pub team_numbers: Vec<u32>,
}

impl ParsedSchedule {
    /// Number of distinct teams appearing in the schedule.
    pub fn team_count(&self) -> usize {
        self.team_numbers.len()
    }
}

/// Parses a `frcNNNN` team key into its numeric team number.
fn parse_team_key(key: &str) -> Result<u32, TbaError> {
    let digits = key
        .strip_prefix("frc")
        .ok_or_else(|| TbaError::Parse(format!("team key '{key}' does not start with 'frc'")))?;
    digits
        .parse::<u32>()
        .map_err(|_| TbaError::Parse(format!("team key '{key}' has no numeric team number")))
}

/// Converts raw The Blue Alliance match objects into a `ParsedSchedule`.
///
/// Only qualification matches (`comp_level == "qm"`) are kept, and they are
/// sorted by match number. Internal team ids are assigned by ascending FRC team
/// number so that the same schedule always yields the same id table, which is
/// part of the determinism guarantee.
pub fn schedule_from_tba(raw: Vec<TbaMatch>) -> Result<ParsedSchedule, TbaError> {
    let mut quals: Vec<TbaMatch> = raw.into_iter().filter(|m| m.comp_level == "qm").collect();
    if quals.is_empty() {
        return Err(TbaError::Schedule(
            "no qualification matches (comp_level 'qm') found".into(),
        ));
    }
    quals.sort_by_key(|m| m.match_number);

    // First pass: collect distinct team numbers in sorted order for a stable id
    // assignment that does not depend on match traversal order.
    let mut number_to_id: BTreeMap<u32, TeamId> = BTreeMap::new();
    for m in &quals {
        for key in m
            .alliances
            .red
            .team_keys
            .iter()
            .chain(&m.alliances.blue.team_keys)
        {
            let num = parse_team_key(key)?;
            number_to_id.entry(num).or_insert(0);
        }
    }
    let mut team_numbers: Vec<u32> = number_to_id.keys().copied().collect();
    team_numbers.sort_unstable();
    if team_numbers.len() > u16::MAX as usize {
        return Err(TbaError::Schedule(format!(
            "too many teams ({}), exceeds supported maximum",
            team_numbers.len()
        )));
    }
    for (id, num) in team_numbers.iter().enumerate() {
        number_to_id.insert(*num, id as TeamId);
    }

    // Second pass: build matches with dense ids.
    let mut matches = Vec::with_capacity(quals.len());
    for m in &quals {
        let reds = &m.alliances.red.team_keys;
        let blues = &m.alliances.blue.team_keys;
        if reds.len() != 3 || blues.len() != 3 {
            return Err(TbaError::Schedule(format!(
                "qualification match {} does not have three teams per alliance",
                m.match_number
            )));
        }
        let mut teams = [0u16; MATCH_SIZE];
        for (slot, key) in reds.iter().chain(blues.iter()).enumerate() {
            let num = parse_team_key(key)?;
            teams[slot] = number_to_id[&num];
        }
        matches.push(Match {
            match_number: m.match_number,
            teams,
        });
    }

    Ok(ParsedSchedule {
        matches,
        team_numbers,
    })
}

/// Parses match JSON text in The Blue Alliance event matches shape.
///
/// Accepts the top level array returned by `GET /event/{key}/matches`. This is
/// the path used by `--matches-file` and by every integration test, so no real
/// API key is ever needed to exercise the solver.
pub fn parse_matches_json(text: &str) -> Result<ParsedSchedule, TbaError> {
    let raw: Vec<TbaMatch> =
        serde_json::from_str(text).map_err(|e| TbaError::Parse(e.to_string()))?;
    schedule_from_tba(raw)
}

/// Reads and parses a local matches JSON file.
pub fn parse_matches_file(path: &str) -> Result<ParsedSchedule, TbaError> {
    let text = std::fs::read_to_string(path).map_err(|e| TbaError::Io(e.to_string()))?;
    parse_matches_json(&text)
}

/// Fetches qualification matches for an event from the live API.
///
/// `event_key` is a value like `2024svr`. The key is sent in the
/// `X-TBA-Auth-Key` header. The response is parsed by the same code path as
/// local files.
pub fn fetch_event_matches(event_key: &str, api_key: &str) -> Result<ParsedSchedule, TbaError> {
    let url = format!("{TBA_BASE}/event/{event_key}/matches");
    let resp = ureq::get(&url)
        .set("X-TBA-Auth-Key", api_key)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| TbaError::Http(e.to_string()))?;
    let body = resp
        .into_string()
        .map_err(|e| TbaError::Http(e.to_string()))?;
    parse_matches_json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
        {"comp_level":"qm","match_number":2,"set_number":1,
         "alliances":{"red":{"team_keys":["frc10","frc20","frc30"]},
                      "blue":{"team_keys":["frc40","frc50","frc60"]}}},
        {"comp_level":"qm","match_number":1,"set_number":1,
         "alliances":{"red":{"team_keys":["frc60","frc50","frc40"]},
                      "blue":{"team_keys":["frc30","frc20","frc10"]}}},
        {"comp_level":"qf","match_number":1,"set_number":1,
         "alliances":{"red":{"team_keys":["frc10","frc20","frc30"]},
                      "blue":{"team_keys":["frc40","frc50","frc60"]}}}
    ]"#;

    #[test]
    fn parses_and_sorts_quals_only() {
        let sched = parse_matches_json(SAMPLE).expect("parses");
        // Only the two qm matches survive, sorted by match number.
        assert_eq!(sched.matches.len(), 2);
        assert_eq!(sched.matches[0].match_number, 1);
        assert_eq!(sched.matches[1].match_number, 2);
    }

    #[test]
    fn team_ids_are_dense_and_sorted_by_number() {
        let sched = parse_matches_json(SAMPLE).expect("parses");
        assert_eq!(sched.team_numbers, vec![10, 20, 30, 40, 50, 60]);
        // Match 1 red is [60,50,40] -> ids [5,4,3].
        assert_eq!(sched.matches[0].teams, [5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn rejects_bad_team_key() {
        let text = r#"[{"comp_level":"qm","match_number":1,"set_number":1,
            "alliances":{"red":{"team_keys":["254","frc20","frc30"]},
                         "blue":{"team_keys":["frc40","frc50","frc60"]}}}]"#;
        assert!(matches!(parse_matches_json(text), Err(TbaError::Parse(_))));
    }

    #[test]
    fn rejects_wrong_alliance_size() {
        let text = r#"[{"comp_level":"qm","match_number":1,"set_number":1,
            "alliances":{"red":{"team_keys":["frc10","frc20"]},
                         "blue":{"team_keys":["frc40","frc50","frc60"]}}}]"#;
        assert!(matches!(
            parse_matches_json(text),
            Err(TbaError::Schedule(_))
        ));
    }

    #[test]
    fn rejects_no_quals() {
        let text = r#"[{"comp_level":"f","match_number":1,"set_number":1,
            "alliances":{"red":{"team_keys":["frc10","frc20","frc30"]},
                         "blue":{"team_keys":["frc40","frc50","frc60"]}}}]"#;
        assert!(matches!(
            parse_matches_json(text),
            Err(TbaError::Schedule(_))
        ));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            parse_matches_json("not json"),
            Err(TbaError::Parse(_))
        ));
    }
}
