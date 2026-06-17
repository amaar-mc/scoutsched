//! Command line interface.
//!
//! The default action solves a schedule from either a local matches file or the
//! live Blue Alliance API, using a TOML config for the scout roster and
//! hyperparameters, with flags that override selected globals. A `gen-sample`
//! subcommand writes a deterministic synthetic event so the tool can be tried
//! and tested without a real API key.

use crate::config::Config;
use crate::output;
use crate::sample::{generate, to_tba_json, SampleParams};
use crate::solver::solve_from_parts;
use crate::tba::{fetch_event_matches, parse_matches_file, ParsedSchedule};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Top level CLI.
#[derive(Debug, Parser)]
#[command(
    name = "scoutsched",
    version,
    about = "Deterministic FRC scouting schedule generator",
    long_about = "Builds a complete, hard feasible scouting grid from a Blue Alliance \
qualification schedule and a scout roster: per team expert assignments, breaks, \
pit scouting, and quantitative or qualitative tagging. The solver is deterministic; \
identical input always produces identical output."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub solve: SolveArgs,
}

/// Subcommands. When none is given, the solve arguments are used.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Generate a deterministic sample event as Blue Alliance style JSON.
    GenSample(GenSampleArgs),
    /// Solve a schedule (the default action; also available explicitly).
    Solve(SolveArgs),
}

/// Which artifact to print to stdout when no output directory is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Csv,
    Json,
    Summary,
}

/// Arguments for solving a schedule.
#[derive(Debug, Clone, Parser)]
pub struct SolveArgs {
    /// Path to the TOML config with the scout roster and hyperparameters.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Local Blue Alliance matches JSON file. When set, no network or key is
    /// used. Takes precedence over an event code.
    #[arg(long)]
    pub matches_file: Option<PathBuf>,

    /// Blue Alliance event code, for example 2024svr. Requires an API key.
    #[arg(long)]
    pub event: Option<String>,

    /// Blue Alliance API key. Falls back to the TBA_API_KEY environment
    /// variable. Only needed when fetching by event code.
    #[arg(long, env = "TBA_API_KEY", hide_env_values = true)]
    pub tba_key: Option<String>,

    /// Output format when writing to stdout.
    #[arg(long, value_enum, default_value_t = Format::Summary)]
    pub format: Format,

    /// Directory to write csv, json, and summary files into. When set, all
    /// three are written and stdout stays quiet except for a confirmation.
    #[arg(short, long)]
    pub out_dir: Option<PathBuf>,

    /// Override the pit scouting quota per scout.
    #[arg(long)]
    pub pit_quota: Option<u16>,

    /// Override the filler watch quota per scout.
    #[arg(long)]
    pub filler_quota: Option<u16>,

    /// Override the qualitative fraction target, in 0.0 to 1.0.
    #[arg(long)]
    pub qualitative_fraction: Option<f64>,

    /// Override the maximum consecutive active assignments before a break.
    #[arg(long)]
    pub max_consecutive: Option<u16>,
}

impl Default for SolveArgs {
    fn default() -> Self {
        SolveArgs {
            config: None,
            matches_file: None,
            event: None,
            tba_key: None,
            format: Format::Summary,
            out_dir: None,
            pit_quota: None,
            filler_quota: None,
            qualitative_fraction: None,
            max_consecutive: None,
        }
    }
}

/// Arguments for the sample generator.
#[derive(Debug, Clone, Parser)]
pub struct GenSampleArgs {
    /// Number of teams at the synthetic event.
    #[arg(long, default_value_t = 36)]
    pub teams: usize,

    /// Matches each team plays.
    #[arg(long, default_value_t = 10)]
    pub matches_per_team: usize,

    /// Seed for the deterministic generator.
    #[arg(long, default_value_t = 1)]
    pub seed: u64,

    /// File to write the JSON into. Writes to stdout when omitted.
    #[arg(short, long)]
    pub out: Option<PathBuf>,
}

/// The error type surfaced to `main`.
#[derive(Debug)]
pub enum CliError {
    /// A required argument was missing or contradictory.
    Usage(String),
    /// Wrapped lower level error with context.
    Failed(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(m) => write!(f, "{m}"),
            CliError::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Runs the CLI to completion, returning a process friendly result.
pub fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Some(Command::GenSample(args)) => run_gen_sample(args),
        Some(Command::Solve(args)) => run_solve(args),
        None => run_solve(cli.solve),
    }
}

/// Loads the schedule from a file or the live API per the arguments.
fn load_schedule(args: &SolveArgs) -> Result<ParsedSchedule, CliError> {
    if let Some(path) = &args.matches_file {
        let p = path.to_string_lossy();
        return parse_matches_file(&p).map_err(|e| CliError::Failed(format!("{e}")));
    }
    let event = args.event.as_ref().ok_or_else(|| {
        CliError::Usage(
            "provide --matches-file for local data, or --event with a Blue Alliance key".into(),
        )
    })?;
    let key = args.tba_key.as_ref().ok_or_else(|| {
        CliError::Usage(
            "fetching by --event needs a key via --tba-key or the TBA_API_KEY env var".into(),
        )
    })?;
    fetch_event_matches(event, key).map_err(|e| CliError::Failed(format!("{e}")))
}

/// Loads and override merges the config.
fn load_config(args: &SolveArgs) -> Result<Config, CliError> {
    let mut cfg = match &args.config {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| CliError::Failed(format!("reading config: {e}")))?;
            Config::from_toml(&text).map_err(|e| CliError::Failed(format!("{e}")))?
        }
        None => {
            return Err(CliError::Usage(
                "a --config TOML file with at least one scout is required".into(),
            ))
        }
    };

    // Apply flag overrides onto globals.
    if let Some(v) = args.pit_quota {
        cfg.globals.pit_quota = v;
    }
    if let Some(v) = args.filler_quota {
        cfg.globals.filler_quota = v;
    }
    if let Some(v) = args.qualitative_fraction {
        cfg.globals.qualitative_fraction = v;
    }
    if let Some(v) = args.max_consecutive {
        cfg.globals.max_consecutive = v;
    }

    // Revalidate after overrides so flag values get the same clear errors.
    cfg.validate()
        .map_err(|e| CliError::Failed(format!("{e}")))?;
    Ok(cfg)
}

/// The solve action.
fn run_solve(args: SolveArgs) -> Result<(), CliError> {
    let schedule = load_schedule(&args)?;
    let config = load_config(&args)?;
    let solved = solve_from_parts(schedule, config)
        .map_err(|e| CliError::Failed(format!("solver error: {e}")))?;

    match &args.out_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .map_err(|e| CliError::Failed(format!("creating out dir: {e}")))?;
            write_file(dir, "schedule.csv", &output::to_csv(&solved))?;
            write_file(dir, "schedule.json", &output::to_json(&solved))?;
            write_file(dir, "summary.txt", &output::to_summary(&solved))?;
            println!(
                "wrote schedule.csv, schedule.json, summary.txt to {}",
                dir.display()
            );
        }
        None => {
            let text = match args.format {
                Format::Csv => output::to_csv(&solved),
                Format::Json => output::to_json(&solved),
                Format::Summary => output::to_summary(&solved),
            };
            print!("{text}");
        }
    }
    Ok(())
}

/// Writes one output file into a directory.
fn write_file(dir: &std::path::Path, name: &str, contents: &str) -> Result<(), CliError> {
    let path = dir.join(name);
    std::fs::write(&path, contents)
        .map_err(|e| CliError::Failed(format!("writing {}: {e}", path.display())))
}

/// The sample generator action.
fn run_gen_sample(args: GenSampleArgs) -> Result<(), CliError> {
    let sched = generate(SampleParams {
        team_count: args.teams,
        matches_per_team: args.matches_per_team,
        seed: args.seed,
    })
    .map_err(|e| CliError::Failed(format!("{e}")))?;
    let json = to_tba_json(&sched);
    match &args.out {
        Some(path) => {
            std::fs::write(path, &json)
                .map_err(|e| CliError::Failed(format!("writing {}: {e}", path.display())))?;
            println!(
                "wrote {} matches for {} teams to {}",
                sched.matches.len(),
                sched.team_count(),
                path.display()
            );
        }
        None => println!("{json}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_solve_flags() {
        let cli = Cli::try_parse_from([
            "scoutsched",
            "--matches-file",
            "m.json",
            "--config",
            "c.toml",
            "--pit-quota",
            "4",
            "--format",
            "json",
        ])
        .expect("parses");
        assert_eq!(cli.solve.pit_quota, Some(4));
        assert_eq!(cli.solve.format, Format::Json);
        assert_eq!(cli.solve.matches_file, Some(PathBuf::from("m.json")));
    }

    #[test]
    fn parses_gen_sample_subcommand() {
        let cli = Cli::try_parse_from(["scoutsched", "gen-sample", "--teams", "40", "--seed", "7"])
            .expect("parses");
        match cli.command {
            Some(Command::GenSample(a)) => {
                assert_eq!(a.teams, 40);
                assert_eq!(a.seed, 7);
                assert_eq!(a.matches_per_team, 10);
            }
            _ => panic!("expected gen-sample"),
        }
    }

    #[test]
    fn solve_without_source_is_usage_error() {
        let args = SolveArgs {
            config: Some(PathBuf::from("c.toml")),
            ..SolveArgs::default()
        };
        let err = load_schedule(&args).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn solve_without_config_is_usage_error() {
        let args = SolveArgs {
            matches_file: Some(PathBuf::from("m.json")),
            ..SolveArgs::default()
        };
        let err = load_config(&args).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }
}
