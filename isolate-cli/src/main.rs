//! Isolate CLI
//!
//! Command-line interface for the Isolate secure sandbox runtime.
//! Features beautiful terminal output, progress indicators, and interactive mode.

mod commands;
mod config;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use isolate_core::error::Error as IsolateError;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use commands::*;
use output::*;

/// Isolate - Secure Sandbox Runtime
#[derive(Parser, Debug)]
#[command(name = "isolate")]
#[command(about = "Execute WASM code securely in an isolated sandbox", long_about = None)]
#[command(version)]
#[command(after_help = "Examples:\n  \
  isolate run module.wasm --cap-stdout\n  \
  isolate run module.wasm --memory-limit 128M --fuel 1000000\n  \
  isolate benchmark module.wasm --iterations 100\n  \
  isolate info module.wasm\n")]
pub struct Cli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "warn", global = true)]
    log_level: String,

    /// Output format (text, json, pretty)
    #[arg(short = 'F', long, default_value = "pretty", global = true)]
    format: OutputFormat,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// Quiet mode - only output the result
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a WASM module in a secure sandbox
    Run(RunArgs),

    /// Validate a WASM module
    Validate(ValidateArgs),

    /// Show detailed information about a WASM module
    Info(InfoArgs),

    /// Analyze a WASM module and suggest security capabilities
    Analyze(AnalyzeArgs),

    /// Benchmark sandbox creation performance
    Benchmark(BenchmarkArgs),

    /// Interactively run with permission prompts
    Interactive(InteractiveArgs),

    /// Manage snapshots
    #[command(subcommand)]
    Snapshot(SnapshotCommands),

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Check installation and system requirements
    Doctor,

    /// Initialize a new Isolate project with example files
    Init(InitArgs),

    /// Inspect a WASM module: list imports, exports, memory requirements
    Inspect(InspectArgs),

    /// Estimate resource usage for a WASM module via dry run
    Estimate(EstimateArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install human-panic for nice error messages
    human_panic::setup_panic!();

    let cli = Cli::parse();

    // Configure colored output
    if cli.no_color {
        colored::control::set_override(false);
    }

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| cli.log_level.parse().unwrap_or_default());

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let result = match cli.command {
        Commands::Run(args) => run_command(args, cli.format, cli.quiet).await,
        Commands::Validate(args) => validate_command(args, cli.quiet),
        Commands::Info(args) => info_command(args, cli.quiet),
        Commands::Analyze(args) => analyze_command(args, cli.quiet),
        Commands::Benchmark(args) => benchmark_command(args, cli.quiet).await,
        Commands::Interactive(args) => interactive_command(args).await,
        Commands::Snapshot(cmd) => snapshot_command(cmd).await,
        Commands::Completions(args) => completions_command(args),
        Commands::Doctor => doctor_command(cli.quiet).await,
        Commands::Init(args) => init_command(args, cli.quiet),
        Commands::Inspect(args) => inspect_command(args, cli.format, cli.quiet).await,
        Commands::Estimate(args) => estimate_command(args, cli.format, cli.quiet).await,
    };

    if let Err(e) = &result {
        if !cli.quiet {
            eprintln!("\n{} {}", "Error:".red().bold(), e);

            // Show cause chain
            let mut cause = e.source();
            while let Some(c) = cause {
                eprintln!("  {} {}", "Caused by:".yellow(), c);
                cause = c.source();
            }

            // Show suggestion if this is an isolate error
            if let Some(isolate_err) = e.downcast_ref::<IsolateError>() {
                if let Some(suggestion) = isolate_err.suggestion() {
                    eprintln!();
                    eprintln!("  {} {}", "Suggestion:".cyan().bold(), suggestion);
                }
            }
        }
        std::process::exit(1);
    }

    result
}
