//! Configuration: global hyperparameters, per scout settings, and priority
//! weights, all loadable from TOML with sane defaults and validated with clear
//! errors.
//!
//! The TOML shape is intentionally flat and forgiving. Availability is expressed
//! either as match indices or as clock times that the loader maps onto match
//! columns using the parsed schedule (see `resolve` in `solver`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Errors produced while loading or validating configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The TOML text could not be parsed.
    Parse(String),
    /// A field held a value outside its valid range.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Parse(m) => write!(f, "config parse error: {m}"),
            ConfigError::Invalid(m) => write!(f, "config validation error: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Relative weights for the soft constraints, in the order they are relaxed.
///
/// A higher weight means the local search works harder to satisfy that
/// objective. The defaults follow the descending priority documented in the
/// charter: coverage first, then quotas, then mode fraction, then sparsity,
/// then break matching.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weights {
    pub coverage: f64,
    pub quotas: f64,
    pub qualitative: f64,
    pub sparsity: f64,
    pub break_match: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            coverage: 1000.0,
            quotas: 100.0,
            qualitative: 10.0,
            sparsity: 1.0,
            break_match: 0.5,
        }
    }
}

/// Global solver hyperparameters with defaults tuned for a typical event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Globals {
    /// Minimum number of teams each scout is expert on.
    pub experts_per_scout_min: u16,
    /// Maximum number of teams each scout is expert on before scaling.
    pub experts_per_scout_max: u16,
    /// Target fraction of a team's matches watched by one of its experts, in
    /// `0.0..=1.0`.
    pub primary_fraction: f64,
    /// Number of non expert team watch assignments each scout should receive.
    pub filler_quota: u16,
    /// Number of pit scouting assignments each scout should receive.
    pub pit_quota: u16,
    /// Target fraction of watch assignments tagged qualitative, in `0.0..=1.0`.
    pub qualitative_fraction: f64,
    /// Maximum consecutive active assignments before a forced break.
    pub max_consecutive: u16,
    /// Minimum length in match columns of a forced break.
    pub min_break_length: u16,
    /// Bounded local search iteration cap. Higher costs time, never correctness.
    pub repair_iterations: u32,
    /// Soft constraint weights.
    pub weights: Weights,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            experts_per_scout_min: 2,
            experts_per_scout_max: 3,
            primary_fraction: 0.8,
            filler_quota: 2,
            pit_quota: 2,
            qualitative_fraction: 0.2,
            max_consecutive: 4,
            min_break_length: 1,
            repair_iterations: 2000,
            weights: Weights::default(),
        }
    }
}

/// A clock time as `HH:MM` in 24 hour form, used to express availability against
/// match start times rather than column indices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockTime(pub String);

/// One endpoint of an availability window: either an explicit match column or a
/// clock time mapped onto the schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WindowBound {
    /// Match column index.
    Index(u32),
    /// Clock time string, mapped to the first match at or after it for arrival,
    /// or the last match at or before it for departure.
    Time(ClockTime),
}

/// Per scout configuration as written in TOML.
///
/// `arrive` defaults to the start of the event and `leave` to the end when
/// omitted. `own_pit` lists match columns the scout cannot scout. `sparsity` is
/// a preference for idle time in `0.0..=1.0`. `break_partner` names another
/// scout for aligned breaks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoutConfig {
    pub name: String,
    pub arrive: Option<WindowBound>,
    pub leave: Option<WindowBound>,
    pub own_pit: Vec<u32>,
    pub sparsity: f64,
    pub break_partner: Option<String>,
}

impl Default for ScoutConfig {
    fn default() -> Self {
        ScoutConfig {
            name: String::new(),
            arrive: None,
            leave: None,
            own_pit: Vec::new(),
            sparsity: 0.0,
            break_partner: None,
        }
    }
}

/// The full configuration document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub globals: Globals,
    pub scouts: Vec<ScoutConfig>,
}

impl Config {
    /// Parses a TOML document into a `Config`, then validates it.
    pub fn from_toml(text: &str) -> Result<Config, ConfigError> {
        let cfg: Config = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validates ranges and cross references, returning the first clear error.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let g = &self.globals;
        if g.experts_per_scout_min == 0 {
            return Err(ConfigError::Invalid(
                "globals.experts_per_scout_min must be at least 1".into(),
            ));
        }
        if g.experts_per_scout_max < g.experts_per_scout_min {
            return Err(ConfigError::Invalid(format!(
                "globals.experts_per_scout_max ({}) must be >= experts_per_scout_min ({})",
                g.experts_per_scout_max, g.experts_per_scout_min
            )));
        }
        check_fraction("globals.primary_fraction", g.primary_fraction)?;
        check_fraction("globals.qualitative_fraction", g.qualitative_fraction)?;
        if g.max_consecutive == 0 {
            return Err(ConfigError::Invalid(
                "globals.max_consecutive must be at least 1".into(),
            ));
        }
        if g.min_break_length == 0 {
            return Err(ConfigError::Invalid(
                "globals.min_break_length must be at least 1".into(),
            ));
        }
        check_weight("globals.weights.coverage", g.weights.coverage)?;
        check_weight("globals.weights.quotas", g.weights.quotas)?;
        check_weight("globals.weights.qualitative", g.weights.qualitative)?;
        check_weight("globals.weights.sparsity", g.weights.sparsity)?;
        check_weight("globals.weights.break_match", g.weights.break_match)?;

        for (i, s) in self.scouts.iter().enumerate() {
            if s.name.trim().is_empty() {
                return Err(ConfigError::Invalid(format!(
                    "scouts[{i}].name must not be empty"
                )));
            }
            check_fraction(&format!("scouts[{i}].sparsity"), s.sparsity)?;
        }

        // Names must be unique so that break partners resolve unambiguously.
        for i in 0..self.scouts.len() {
            for j in (i + 1)..self.scouts.len() {
                if self.scouts[i].name == self.scouts[j].name {
                    return Err(ConfigError::Invalid(format!(
                        "duplicate scout name '{}'",
                        self.scouts[i].name
                    )));
                }
            }
        }

        // Break partners must reference an existing scout.
        for (i, s) in self.scouts.iter().enumerate() {
            if let Some(partner) = &s.break_partner {
                if !self.scouts.iter().any(|o| &o.name == partner) {
                    return Err(ConfigError::Invalid(format!(
                        "scouts[{i}] break_partner '{partner}' names no known scout"
                    )));
                }
                if partner == &s.name {
                    return Err(ConfigError::Invalid(format!(
                        "scouts[{i}] break_partner cannot be the scout itself"
                    )));
                }
            }
        }

        Ok(())
    }
}

fn check_fraction(name: &str, v: f64) -> Result<(), ConfigError> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return Err(ConfigError::Invalid(format!(
            "{name} must be a finite value in 0.0..=1.0, got {v}"
        )));
    }
    Ok(())
}

fn check_weight(name: &str, v: f64) -> Result<(), ConfigError> {
    if !v.is_finite() || v < 0.0 {
        return Err(ConfigError::Invalid(format!(
            "{name} must be a finite non negative value, got {v}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().expect("defaults valid");
        assert_eq!(Globals::default().experts_per_scout_min, 2);
        assert_eq!(Globals::default().experts_per_scout_max, 3);
    }

    #[test]
    fn minimal_toml_parses_with_defaults() {
        let text = r#"
            [[scouts]]
            name = "Ada"
            [[scouts]]
            name = "Linus"
        "#;
        let cfg = Config::from_toml(text).expect("parses");
        assert_eq!(cfg.scouts.len(), 2);
        // Omitted globals fall back to defaults.
        assert_eq!(cfg.globals.pit_quota, 2);
    }

    #[test]
    fn override_globals_in_toml() {
        let text = r#"
            [globals]
            pit_quota = 5
            qualitative_fraction = 0.5

            [[scouts]]
            name = "Ada"
        "#;
        let cfg = Config::from_toml(text).expect("parses");
        assert_eq!(cfg.globals.pit_quota, 5);
        assert_eq!(cfg.globals.qualitative_fraction, 0.5);
        // Untouched fields keep defaults.
        assert_eq!(cfg.globals.filler_quota, 2);
    }

    #[test]
    fn out_of_range_fraction_is_rejected() {
        let text = r#"
            [globals]
            qualitative_fraction = 1.5
            [[scouts]]
            name = "Ada"
        "#;
        let err = Config::from_toml(text).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn experts_max_below_min_is_rejected() {
        let text = r#"
            [globals]
            experts_per_scout_min = 4
            experts_per_scout_max = 2
            [[scouts]]
            name = "Ada"
        "#;
        let err = Config::from_toml(text).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }

    #[test]
    fn empty_scout_name_is_rejected() {
        let text = r#"
            [[scouts]]
            name = ""
        "#;
        assert!(matches!(
            Config::from_toml(text),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn duplicate_names_rejected() {
        let text = r#"
            [[scouts]]
            name = "Ada"
            [[scouts]]
            name = "Ada"
        "#;
        assert!(matches!(
            Config::from_toml(text),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn unknown_break_partner_rejected() {
        let text = r#"
            [[scouts]]
            name = "Ada"
            break_partner = "Nobody"
        "#;
        assert!(matches!(
            Config::from_toml(text),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn self_break_partner_rejected() {
        let text = r#"
            [[scouts]]
            name = "Ada"
            break_partner = "Ada"
        "#;
        assert!(matches!(
            Config::from_toml(text),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn window_bound_parses_index_or_time() {
        let text = r#"
            [[scouts]]
            name = "Ada"
            arrive = 5
            leave = "16:30"
        "#;
        let cfg = Config::from_toml(text).expect("parses");
        assert_eq!(cfg.scouts[0].arrive, Some(WindowBound::Index(5)));
        assert_eq!(
            cfg.scouts[0].leave,
            Some(WindowBound::Time(ClockTime("16:30".into())))
        );
    }

    #[test]
    fn malformed_toml_is_parse_error() {
        let text = "this is not = valid = toml [[[";
        assert!(matches!(
            Config::from_toml(text),
            Err(ConfigError::Parse(_))
        ));
    }
}
