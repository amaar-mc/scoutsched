//! Deterministic sample schedule generator.
//!
//! This is the only module that uses randomness, and it is fully seeded: the
//! same seed and parameters always produce the same schedule. It exists so the
//! solver can be exercised over many realistic instances without a real The Blue
//! Alliance key. The output is the same JSON shape as the live API, so it flows
//! through `tba::parse_matches_json` unchanged.
//!
//! A realistic FRC qualification schedule has every team play the same number of
//! matches, six distinct teams per match, and matches spaced through the day.
//! The generator enforces distinct teams per match and balanced appearances.

use crate::model::{Match, TeamId, MATCH_SIZE};
use crate::tba::ParsedSchedule;

/// SplitMix64: a tiny, well distributed seeded PRNG. Chosen because it needs no
/// dependency and is deterministic across platforms, which is exactly what the
/// generator requires.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Seeds the generator. Any seed is valid.
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    /// Returns the next 64 bit value and advances the state.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Returns a uniformly distributed value in `0..bound` using Lemire style
    /// rejection to avoid modulo bias. `bound` must be non zero.
    pub fn below(&mut self, bound: usize) -> usize {
        debug_assert!(bound > 0);
        let bound = bound as u64;
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let v = self.next_u64();
            if v < zone {
                return (v % bound) as usize;
            }
        }
    }

    /// Fisher Yates shuffle in place, deterministic for a given state.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// Errors from sample generation, all due to infeasible parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleError {
    Infeasible(String),
}

impl std::fmt::Display for SampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SampleError::Infeasible(m) => write!(f, "sample generation infeasible: {m}"),
        }
    }
}

impl std::error::Error for SampleError {}

/// Parameters for a synthetic event.
#[derive(Debug, Clone, Copy)]
pub struct SampleParams {
    pub team_count: usize,
    pub matches_per_team: usize,
    pub seed: u64,
}

/// Generates a balanced qualification schedule as a `ParsedSchedule`.
///
/// Every team plays exactly `matches_per_team` matches, each match has six
/// distinct teams, and `team_count * matches_per_team` must be a multiple of six
/// so the appearances tile into whole matches. Team numbers are assigned as
/// `1..=team_count` to look like real FRC numbers.
pub fn generate(params: SampleParams) -> Result<ParsedSchedule, SampleError> {
    let SampleParams {
        team_count,
        matches_per_team,
        seed,
    } = params;

    if team_count < MATCH_SIZE {
        return Err(SampleError::Infeasible(format!(
            "team_count {team_count} must be at least {MATCH_SIZE}"
        )));
    }
    if matches_per_team == 0 {
        return Err(SampleError::Infeasible(
            "matches_per_team must be at least 1".into(),
        ));
    }
    let total_slots = team_count * matches_per_team;
    if total_slots % MATCH_SIZE != 0 {
        return Err(SampleError::Infeasible(format!(
            "team_count * matches_per_team ({total_slots}) must be a multiple of {MATCH_SIZE}"
        )));
    }
    let num_matches = total_slots / MATCH_SIZE;

    let mut rng = SplitMix64::new(seed);
    let teams = assign_matches(team_count, matches_per_team, num_matches, &mut rng)?;

    let matches: Vec<Match> = teams
        .into_iter()
        .enumerate()
        .map(|(i, slots)| Match {
            match_number: (i + 1) as u32,
            teams: slots,
        })
        .collect();

    let team_numbers: Vec<u32> = (1..=team_count as u32).collect();
    Ok(ParsedSchedule {
        matches,
        team_numbers,
    })
}

/// Builds the match team arrays with a construction that cannot dead end.
///
/// The appearances are dealt round robin across matches: the j-th copy of team t
/// goes to match `(t * matches_per_team + j) mod num_matches`. Because
/// `matches_per_team <= num_matches` (guaranteed since `team_count >= 6`), a
/// team's `matches_per_team` copies land in distinct matches, so no team ever
/// appears twice in one match. Dealing evenly also gives each match exactly six
/// teams. To make the result look like a real schedule rather than a rotation,
/// the team labels are permuted by a seeded shuffle and the six slots within each
/// match are shuffled; both preserve the distinctness and balance invariants.
fn assign_matches(
    team_count: usize,
    matches_per_team: usize,
    num_matches: usize,
    rng: &mut SplitMix64,
) -> Result<Vec<[TeamId; MATCH_SIZE]>, SampleError> {
    // A permutation of team ids so the schedule is not a visible rotation.
    let mut labels: Vec<TeamId> = (0..team_count as TeamId).collect();
    rng.shuffle(&mut labels);

    // Deal appearances into per match lists. Each list ends with exactly six
    // distinct teams by the construction argument above.
    let mut lists: Vec<Vec<TeamId>> = vec![Vec::with_capacity(MATCH_SIZE); num_matches];
    for (t, &label) in labels.iter().enumerate() {
        for j in 0..matches_per_team {
            let m = (t * matches_per_team + j) % num_matches;
            lists[m].push(label);
        }
    }

    let mut matches: Vec<[TeamId; MATCH_SIZE]> = Vec::with_capacity(num_matches);
    for list in lists.iter_mut() {
        debug_assert_eq!(
            list.len(),
            MATCH_SIZE,
            "uneven deal: construction invariant broken"
        );
        // Defensive distinctness check; an assert keeps the invariant explicit.
        debug_assert!(
            {
                let mut seen = list.clone();
                seen.sort_unstable();
                seen.dedup();
                seen.len() == list.len()
            },
            "duplicate team dealt into a match"
        );
        rng.shuffle(list);
        let mut slots = [0u16; MATCH_SIZE];
        slots.copy_from_slice(&list[..MATCH_SIZE]);
        matches.push(slots);
    }

    Ok(matches)
}

/// Serializes a `ParsedSchedule` into The Blue Alliance event matches JSON shape,
/// so the generated event can be saved and reused via `--matches-file`.
pub fn to_tba_json(sched: &ParsedSchedule) -> String {
    let mut objs = Vec::with_capacity(sched.matches.len());
    for m in &sched.matches {
        let red: Vec<String> = m.teams[0..3]
            .iter()
            .map(|id| format!("frc{}", sched.team_numbers[*id as usize]))
            .collect();
        let blue: Vec<String> = m.teams[3..6]
            .iter()
            .map(|id| format!("frc{}", sched.team_numbers[*id as usize]))
            .collect();
        objs.push(serde_json::json!({
            "comp_level": "qm",
            "match_number": m.match_number,
            "set_number": 1,
            "alliances": {
                "red": {"team_keys": red},
                "blue": {"team_keys": blue}
            }
        }));
    }
    serde_json::to_string_pretty(&objs).expect("schedule json serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn splitmix_is_deterministic() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_respects_bound() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..10_000 {
            assert!(rng.below(6) < 6);
        }
    }

    #[test]
    fn generate_is_balanced_and_distinct() {
        let params = SampleParams {
            team_count: 36,
            matches_per_team: 10,
            seed: 1,
        };
        let sched = generate(params).expect("feasible");
        assert_eq!(sched.matches.len(), 36 * 10 / 6);
        assert_eq!(sched.team_count(), 36);

        // Every match has six distinct teams.
        for m in &sched.matches {
            let set: HashSet<TeamId> = m.teams.iter().copied().collect();
            assert_eq!(
                set.len(),
                MATCH_SIZE,
                "match {} has a repeat",
                m.match_number
            );
        }

        // Every team plays exactly matches_per_team times.
        let mut counts = vec![0usize; 36];
        for m in &sched.matches {
            for t in m.teams {
                counts[t as usize] += 1;
            }
        }
        assert!(counts.iter().all(|&c| c == 10), "appearances unbalanced");
    }

    #[test]
    fn generate_is_deterministic_for_seed() {
        let p = SampleParams {
            team_count: 24,
            matches_per_team: 12,
            seed: 99,
        };
        let a = generate(p).expect("ok");
        let b = generate(p).expect("ok");
        assert_eq!(a.matches, b.matches);
        assert_eq!(to_tba_json(&a), to_tba_json(&b));
    }

    #[test]
    fn different_seeds_differ() {
        let p1 = SampleParams {
            team_count: 24,
            matches_per_team: 12,
            seed: 1,
        };
        let p2 = SampleParams { seed: 2, ..p1 };
        assert_ne!(generate(p1).unwrap().matches, generate(p2).unwrap().matches);
    }

    #[test]
    fn rejects_non_multiple_of_six() {
        let p = SampleParams {
            team_count: 10,
            matches_per_team: 1,
            seed: 0,
        };
        assert!(matches!(generate(p), Err(SampleError::Infeasible(_))));
    }

    #[test]
    fn rejects_too_few_teams() {
        let p = SampleParams {
            team_count: 5,
            matches_per_team: 6,
            seed: 0,
        };
        assert!(matches!(generate(p), Err(SampleError::Infeasible(_))));
    }

    #[test]
    fn roundtrips_through_tba_parser() {
        let p = SampleParams {
            team_count: 30,
            matches_per_team: 8,
            seed: 5,
        };
        let sched = generate(p).expect("ok");
        let json = to_tba_json(&sched);
        let reparsed = crate::tba::parse_matches_json(&json).expect("reparses");
        assert_eq!(reparsed.matches.len(), sched.matches.len());
        assert_eq!(reparsed.team_numbers, sched.team_numbers);
    }
}
