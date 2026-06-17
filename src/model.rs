//! Core data model for scoutsched.
//!
//! All entity identifiers are small integers so that the solver can use them as
//! array indices and pack availability into bitsets. A `TeamId` is an internal
//! dense index assigned in first-seen order over the qualification schedule; the
//! original FRC team number is preserved separately for output.

use serde::{Deserialize, Serialize};

/// Internal dense index for a team. Range is the number of distinct teams at the
/// event, which never approaches `u16::MAX`.
pub type TeamId = u16;

/// Internal dense index for a scout. Range is the roster size.
pub type ScoutId = u16;

/// Index of a match column in time order, starting at zero.
pub type MatchIndex = u32;

/// The two alliance colors. Each FRC qualification match has three teams per
/// alliance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Alliance {
    Red,
    Blue,
}

/// Number of team slots per alliance in an FRC qualification match.
pub const ALLIANCE_SIZE: usize = 3;

/// Number of team slots in a full match (both alliances).
pub const MATCH_SIZE: usize = ALLIANCE_SIZE * 2;

/// A single qualification match as a time ordered column of six team slots.
///
/// `teams` holds the six participating teams as internal ids: the first three
/// are the red alliance, the last three are the blue alliance. `match_number` is
/// the FRC qualification match number used for labeling and ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub match_number: u32,
    pub teams: [TeamId; MATCH_SIZE],
}

impl Match {
    /// Returns the alliance that a slot index belongs to.
    pub fn alliance_of_slot(slot: usize) -> Alliance {
        if slot < ALLIANCE_SIZE {
            Alliance::Red
        } else {
            Alliance::Blue
        }
    }

    /// Returns true when the given team plays in this match.
    pub fn contains(&self, team: TeamId) -> bool {
        self.teams.contains(&team)
    }
}

/// How a scout observes a team during a watched match.
///
/// Quantitative is structured counting (cycles, scores, penalties). Qualitative
/// is open observation (strategy, driver skill, failures). A configurable
/// minority of watch assignments are tagged qualitative so that every scout
/// contributes some narrative perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WatchMode {
    Quantitative,
    Qualitative,
}

/// What a scout is doing during one match column.
///
/// Exactly one assignment exists per (scout, match) cell, which makes the hard
/// no-double-booking constraint structural rather than something to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// Watching one team from the stands, with a counting mode.
    Watch { team: TeamId, mode: WatchMode },
    /// Pit scouting a team during a match in which that team does not play.
    Pit { team: TeamId },
    /// A scheduled rest. Counts against the scout's load as idle time.
    Break,
    /// Outside the scout's availability or during own-pit duty. Never an
    /// activity, and never counted as a relaxable break.
    Unavailable,
}

impl Cell {
    /// Returns true when this cell occupies the scout with real work.
    pub fn is_active(&self) -> bool {
        matches!(self, Cell::Watch { .. } | Cell::Pit { .. })
    }

    /// Returns the team this cell concerns, if any.
    pub fn team(&self) -> Option<TeamId> {
        match self {
            Cell::Watch { team, .. } => Some(*team),
            Cell::Pit { team } => Some(*team),
            Cell::Break | Cell::Unavailable => None,
        }
    }
}

/// A scout and the constraints that govern their schedule.
///
/// `arrive` and `leave` define a half open availability window over match
/// columns: the scout can be assigned work for matches in `arrive..leave`.
/// `own_pit_blocks` are match columns during which the scout is running their
/// own team's pit and cannot scout. `sparsity` in `0.0..=1.0` is a preference
/// for idle time, where higher means the scout prefers a lighter load.
#[derive(Debug, Clone)]
pub struct Scout {
    pub name: String,
    pub arrive: MatchIndex,
    pub leave: MatchIndex,
    pub own_pit_blocks: Vec<MatchIndex>,
    pub sparsity: f64,
    /// Index of a partner scout who requested aligned breaks, if any.
    pub break_partner: Option<ScoutId>,
}

/// A fixed length bitset over match columns, used for availability and
/// occupancy. Words are `u64`, so a 256 match event needs four words. Bit `i`
/// of word `w` represents match column `w * 64 + i`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchBitset {
    words: Vec<u64>,
    len: usize,
}

impl MatchBitset {
    /// Creates an all zero bitset that can hold `len` match columns.
    pub fn new(len: usize) -> Self {
        let words = len.div_ceil(64);
        MatchBitset {
            words: vec![0u64; words],
            len,
        }
    }

    /// Creates an all ones bitset over `len` columns.
    pub fn all_set(len: usize) -> Self {
        let mut b = MatchBitset::new(len);
        for i in 0..len {
            b.set(i);
        }
        b
    }

    /// Number of columns this bitset addresses.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true when the bitset addresses zero columns.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sets the bit for a column.
    pub fn set(&mut self, idx: usize) {
        debug_assert!(idx < self.len);
        self.words[idx / 64] |= 1u64 << (idx % 64);
    }

    /// Clears the bit for a column.
    pub fn clear(&mut self, idx: usize) {
        debug_assert!(idx < self.len);
        self.words[idx / 64] &= !(1u64 << (idx % 64));
    }

    /// Returns true when the column bit is set.
    pub fn get(&self, idx: usize) -> bool {
        debug_assert!(idx < self.len);
        (self.words[idx / 64] >> (idx % 64)) & 1 == 1
    }

    /// Count of set bits.
    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Iterates set columns in ascending order.
    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len).filter(move |&i| self.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_to_alliance_split_is_three_and_three() {
        assert_eq!(Match::alliance_of_slot(0), Alliance::Red);
        assert_eq!(Match::alliance_of_slot(2), Alliance::Red);
        assert_eq!(Match::alliance_of_slot(3), Alliance::Blue);
        assert_eq!(Match::alliance_of_slot(5), Alliance::Blue);
    }

    #[test]
    fn cell_active_and_team_accessors() {
        let w = Cell::Watch {
            team: 7,
            mode: WatchMode::Quantitative,
        };
        let p = Cell::Pit { team: 9 };
        assert!(w.is_active());
        assert!(p.is_active());
        assert!(!Cell::Break.is_active());
        assert!(!Cell::Unavailable.is_active());
        assert_eq!(w.team(), Some(7));
        assert_eq!(p.team(), Some(9));
        assert_eq!(Cell::Break.team(), None);
    }

    #[test]
    fn bitset_set_clear_count_iter() {
        let mut b = MatchBitset::new(130);
        assert_eq!(b.len(), 130);
        assert_eq!(b.count(), 0);
        b.set(0);
        b.set(64);
        b.set(129);
        assert!(b.get(0));
        assert!(b.get(64));
        assert!(b.get(129));
        assert!(!b.get(1));
        assert_eq!(b.count(), 3);
        assert_eq!(b.iter_set().collect::<Vec<_>>(), vec![0, 64, 129]);
        b.clear(64);
        assert!(!b.get(64));
        assert_eq!(b.count(), 2);
    }

    #[test]
    fn bitset_all_set_is_full() {
        let b = MatchBitset::all_set(70);
        assert_eq!(b.count(), 70);
        assert!(b.get(69));
    }
}
