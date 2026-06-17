//! scoutsched: a deterministic FRC scouting schedule generator.
//!
//! Given a Blue Alliance qualification schedule and a scout roster, scoutsched
//! builds a complete, hard feasible scouting grid: who watches which team in
//! each match, who is pit scouting, who is on break, with quantitative and
//! qualitative tagging. The solver is a deterministic constructive heuristic
//! followed by bounded local search. It contains no randomness and always
//! returns a complete schedule that satisfies every hard constraint, relaxing
//! soft constraints by documented priority when an instance is over constrained.
//!
//! See `docs/architecture.md` for the algorithm and complexity discussion.

pub mod cli;
pub mod config;
pub mod model;
pub mod output;
pub mod report;
pub mod sample;
pub mod solver;
pub mod tba;

pub use config::{Config, ConfigError};
pub use model::{Cell, Match, MatchBitset, Scout, WatchMode};
pub use solver::{solve, Schedule, SolveInput};
