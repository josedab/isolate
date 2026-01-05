//! Isolate CLI
//!
//! Command-line interface for the Isolate secure sandbox runtime.
//! Features beautiful terminal output, progress indicators, and interactive mode.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, Table};
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const BANNER: &str = r#"
  ___          _       _
 |_ _|___  ___| | __ _| |_ ___
  | |/ __|/ _ \ |/ _` | __/ _ \
  | |\__ \ (_) | | (_| | ||  __/
 |___|___/\___/|_|\__,_|\__\___|
"#;

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
struct Cli {
    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "warn", global = true)]
    log_level: String,

    /// Output format (text, json, pretty)
    #[arg(short, long, default_value = "pretty", global = true)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Pretty,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a WASM module in a secure sandbox
    Run(RunArgs),

    /// Validate a WASM module
    Validate(ValidateArgs),

    /// Show detailed information about a WASM module
    Info(InfoArgs),

    /// Benchmark sandbox creation performance
    Benchmark(BenchmarkArgs),

    /// Interactively run with permission prompts
    Interactive(InteractiveArgs),

    /// Manage snapshots
    #[command(subcommand)]
    Snapshot(SnapshotCommands),
}

#[derive(Subcommand, Debug)]
enum SnapshotCommands {
    /// List stored snapshots
    List,
    /// Delete a snapshot
    Delete { id: String },
    /// Show snapshot info
    Info { id: String },
}

#[derive(Parser, Debug)]
struct RunArgs {
    /// Path to the WASM module
    module: PathBuf,

    /// Memory limit (e.g., 128M, 1G)
    #[arg(short, long, default_value = "256M")]
    memory_limit: String,

    /// Fuel limit (instruction count)
    #[arg(short, long)]
    fuel: Option<u64>,

    /// Wall-clock timeout in seconds
    #[arg(short, long, default_value = "60")]
    timeout: u64,

    /// CPU time limit in seconds
    #[arg(long)]
    cpu_time: Option<u64>,

    /// Grant stdout capability
    #[arg(long)]
    cap_stdout: bool,

    /// Grant stderr capability
    #[arg(long)]
    cap_stderr: bool,

    /// Grant stdin capability
    #[arg(long)]
    cap_stdin: bool,

    /// Grant all stdio capabilities
    #[arg(long)]
    cap_stdio: bool,

    /// Grant filesystem read capability (path)
    #[arg(long)]
    cap_fs_read: Vec<PathBuf>,

    /// Grant filesystem write capability (path)
    #[arg(long)]
    cap_fs_write: Vec<PathBuf>,

    /// Grant HTTP capability (host pattern)
    #[arg(long)]
    cap_http: Vec<String>,

    /// Grant DNS resolution capability
    #[arg(long)]
    cap_dns: bool,

    /// Grant system clock capability
    #[arg(long)]
    cap_time: bool,

    /// Grant random number capability
    #[arg(long)]
    cap_random: bool,

    /// Environment variable to pass (KEY=VALUE)
    #[arg(short, long)]
    env: Vec<String>,

    /// Arguments to pass to the module
    #[arg(last = true)]
    args: Vec<String>,

    /// Entry point function
    #[arg(long, default_value = "_start")]
    entry: String,

    /// Read input from stdin
    #[arg(long)]
    stdin: bool,

    /// Show resource usage after execution
    #[arg(long)]
    show_stats: bool,
}

#[derive(Parser, Debug)]
struct ValidateArgs {
    /// Path to the WASM module
    module: PathBuf,
}

#[derive(Parser, Debug)]
struct InfoArgs {
    /// Path to the WASM module
    module: PathBuf,

    /// Show exports
    #[arg(long)]
    exports: bool,

    /// Show imports
    #[arg(long)]
    imports: bool,
}

#[derive(Parser, Debug)]
struct BenchmarkArgs {
    /// Path to the WASM module
    module: PathBuf,

    /// Number of iterations
    #[arg(short, long, default_value = "100")]
    iterations: usize,

    /// Warm up iterations
    #[arg(long, default_value = "10")]
    warmup: usize,

    /// Include execution in benchmark
    #[arg(long)]
    include_run: bool,
}

#[derive(Parser, Debug)]
struct InteractiveArgs {
    /// Path to the WASM module
    module: PathBuf,
}

fn parse_size(s: &str) -> Result<usize> {
    let s = s.trim().to_uppercase();
    let (num, multiplier) = if s.ends_with('G') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('K') {
        (&s[..s.len() - 1], 1024)
    } else {
        (s.as_str(), 1)
    };
    let num: usize = num.parse().context("Invalid size number")?;
    Ok(num * multiplier)
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{:.2}ms", ms)
    } else {
        format!("{:.2}µs", ms * 1000.0)
    }
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn print_banner() {
    println!("{}", BANNER.cyan().bold());
    println!(
        "  {}  v{}\n",
        "Secure Sandbox Runtime".dimmed(),
        env!("CARGO_PKG_VERSION")
    );
}

fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
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
        Commands::Benchmark(args) => benchmark_command(args, cli.quiet).await,
        Commands::Interactive(args) => interactive_command(args).await,
        Commands::Snapshot(cmd) => snapshot_command(cmd).await,
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
        }
        std::process::exit(1);
    }

    result
}

async fn run_command(args: RunArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    // Read the WASM module
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Build capabilities
    let mut capabilities = Vec::new();

    if args.cap_stdout || args.cap_stdio {
        capabilities.push(Capability::stdout());
    }
    if args.cap_stderr || args.cap_stdio {
        capabilities.push(Capability::stderr());
    }
    if args.cap_stdin || args.cap_stdio {
        capabilities.push(Capability::stdin());
    }
    for path in &args.cap_fs_read {
        capabilities.push(Capability::filesystem_read(path));
    }
    for path in &args.cap_fs_write {
        capabilities.push(Capability::filesystem_write(path));
    }
    if !args.cap_http.is_empty() {
        capabilities.push(Capability::http_client(args.cap_http.clone()));
    }
    if args.cap_dns {
        capabilities.push(Capability::dns_resolve());
    }
    if args.cap_time {
        capabilities.push(Capability::system_clock());
        capabilities.push(Capability::monotonic_clock());
    }
    if args.cap_random {
        capabilities.push(Capability::secure_random());
    }

    // Parse environment variables
    let mut env_vars = std::collections::HashMap::new();
    for env_str in &args.env {
        let parts: Vec<_> = env_str.splitn(2, '=').collect();
        if parts.len() == 2 {
            env_vars.insert(parts[0].to_string(), parts[1].to_string());
        }
    }

    // Build configuration
    let memory_limit = parse_size(&args.memory_limit)?;

    let mut builder = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(memory_limit)
        .wall_time_limit(Duration::from_secs(args.timeout))
        .capabilities(capabilities.clone())
        .envs(env_vars)
        .args(args.args.clone().into_iter())
        .entry_point(args.entry.clone());

    if let Some(fuel) = args.fuel {
        builder = builder.fuel(fuel);
    }
    if let Some(cpu_time) = args.cpu_time {
        builder = builder.cpu_time_limit(Duration::from_secs(cpu_time));
    }

    let config = builder.build()?;

    // Show spinner while creating sandbox
    let spinner = if !quiet && format == OutputFormat::Pretty {
        Some(create_spinner("Creating sandbox..."))
    } else {
        None
    };

    let creation_start = Instant::now();
    let mut sandbox = Sandbox::create(config).await?;
    let creation_time = creation_start.elapsed();

    if let Some(sp) = &spinner {
        sp.set_message("Executing...");
    }

    // Read input if requested
    let input = if args.stdin {
        let mut buffer = Vec::new();
        std::io::stdin().read_to_end(&mut buffer)?;
        buffer
    } else {
        Vec::new()
    };

    let output = sandbox.run(&input).await?;

    if let Some(sp) = spinner {
        sp.finish_and_clear();
    }

    // Output results
    match format {
        OutputFormat::Json => {
            let result = serde_json::json!({
                "exit_code": output.exit_code,
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
                "duration_ms": output.duration.as_secs_f64() * 1000.0,
                "creation_time_ms": creation_time.as_secs_f64() * 1000.0,
                "resource_usage": {
                    "peak_memory": output.resource_usage.peak_memory,
                    "fuel_consumed": output.resource_usage.fuel_consumed,
                    "cpu_time_ms": output.resource_usage.cpu_time.as_secs_f64() * 1000.0,
                },
                "capabilities_granted": capabilities.len(),
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            // Simple text output
            if !output.stdout.is_empty() {
                std::io::Write::write_all(&mut std::io::stdout(), &output.stdout)?;
            }
            if !output.stderr.is_empty() {
                std::io::Write::write_all(&mut std::io::stderr(), &output.stderr)?;
            }
        }
        OutputFormat::Pretty => {
            // Write stdout/stderr first
            if !output.stdout.is_empty() {
                std::io::Write::write_all(&mut std::io::stdout(), &output.stdout)?;
            }
            if !output.stderr.is_empty() {
                std::io::Write::write_all(&mut std::io::stderr(), &output.stderr)?;
            }

            if !quiet && (args.show_stats || output.exit_code != 0) {
                println!();
                println!("{}", "─".repeat(50).dimmed());

                // Status line
                if output.exit_code == 0 {
                    println!("  {} Execution completed successfully", "✓".green().bold());
                } else {
                    println!(
                        "  {} Exited with code {}",
                        "✗".red().bold(),
                        output.exit_code.to_string().red()
                    );
                }

                // Stats table
                let mut table = Table::new();
                table.load_preset(UTF8_FULL_CONDENSED);
                table.set_header(vec![
                    Cell::new("Metric").add_attribute(Attribute::Bold),
                    Cell::new("Value").add_attribute(Attribute::Bold),
                ]);

                table.add_row(vec![
                    Cell::new("Creation Time"),
                    Cell::new(format_duration(creation_time)).fg(Color::Cyan),
                ]);
                table.add_row(vec![
                    Cell::new("Execution Time"),
                    Cell::new(format_duration(output.duration)).fg(Color::Cyan),
                ]);
                table.add_row(vec![
                    Cell::new("Peak Memory"),
                    Cell::new(format_bytes(output.resource_usage.peak_memory)).fg(Color::Yellow),
                ]);
                if output.resource_usage.fuel_consumed > 0 {
                    table.add_row(vec![
                        Cell::new("Fuel Consumed"),
                        Cell::new(format_number(output.resource_usage.fuel_consumed))
                            .fg(Color::Magenta),
                    ]);
                }
                table.add_row(vec![
                    Cell::new("Capabilities"),
                    Cell::new(format!("{} granted", capabilities.len())).fg(Color::Green),
                ]);

                println!("{}", table);
            }
        }
    }

    std::process::exit(output.exit_code);
}

fn validate_command(args: ValidateArgs, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    let spinner = if !quiet {
        Some(create_spinner("Validating module..."))
    } else {
        None
    };

    // Try to create a config (which validates the module)
    match SandboxConfig::builder().module(&wasm_bytes) {
        Ok(_) => {
            if let Some(sp) = spinner {
                sp.finish_with_message("Module is valid WASM ✓".green().to_string());
            } else if !quiet {
                println!("{} Module is valid WASM", "✓".green());
            }
            Ok(())
        }
        Err(e) => {
            if let Some(sp) = spinner {
                sp.finish_with_message(format!("{} Invalid module", "✗".red()));
            }
            eprintln!("{} {}", "Error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}

fn info_command(args: InfoArgs, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Validate
    let _config = SandboxConfig::builder().module(&wasm_bytes)?;

    if quiet {
        println!(
            "{}",
            isolate_core::config::ModuleHash::from_bytes(&wasm_bytes)
        );
        return Ok(());
    }

    print_banner();

    println!("{}", "Module Information".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);

    table.add_row(vec![
        Cell::new("File").add_attribute(Attribute::Bold),
        Cell::new(args.module.display().to_string()),
    ]);
    table.add_row(vec![
        Cell::new("Size").add_attribute(Attribute::Bold),
        Cell::new(format_bytes(wasm_bytes.len())),
    ]);
    table.add_row(vec![
        Cell::new("Hash").add_attribute(Attribute::Bold),
        Cell::new(isolate_core::config::ModuleHash::from_bytes(&wasm_bytes).to_string())
            .fg(Color::Cyan),
    ]);

    if wasm_bytes.len() >= 8 {
        let version =
            u32::from_le_bytes([wasm_bytes[4], wasm_bytes[5], wasm_bytes[6], wasm_bytes[7]]);
        table.add_row(vec![
            Cell::new("WASM Version").add_attribute(Attribute::Bold),
            Cell::new(version.to_string()),
        ]);
    }

    println!("{}", table);

    Ok(())
}

async fn benchmark_command(args: BenchmarkArgs, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    if !quiet {
        print_banner();
        println!("{}", "Sandbox Benchmark".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
        println!("Module: {}", args.module.display().to_string().cyan());
        println!(
            "Warmup: {} iterations | Benchmark: {} iterations\n",
            args.warmup.to_string().yellow(),
            args.iterations.to_string().yellow()
        );
    }

    // Warmup with progress bar
    let warmup_pb = if !quiet {
        let pb = ProgressBar::new(args.warmup as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{prefix:.bold.dim} [{bar:40.cyan/blue}] {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("━━╺"),
        );
        pb.set_prefix("Warmup");
        Some(pb)
    } else {
        None
    };

    for _ in 0..args.warmup {
        let config = SandboxConfig::builder().module(&wasm_bytes)?.build()?;
        let _sandbox = Sandbox::create(config).await?;
        if let Some(pb) = &warmup_pb {
            pb.inc(1);
        }
    }
    if let Some(pb) = warmup_pb {
        pb.finish_with_message("done".green().to_string());
    }

    // Benchmark with progress bar
    let bench_pb = if !quiet {
        let pb = ProgressBar::new(args.iterations as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{prefix:.bold.dim} [{bar:40.green/blue}] {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("━━╺"),
        );
        pb.set_prefix("Benchmark");
        Some(pb)
    } else {
        None
    };

    let mut durations = Vec::with_capacity(args.iterations);
    for _ in 0..args.iterations {
        let start = std::time::Instant::now();
        let config = SandboxConfig::builder().module(&wasm_bytes)?.build()?;
        let _sandbox = Sandbox::create(config).await?;
        durations.push(start.elapsed());
        if let Some(pb) = &bench_pb {
            pb.inc(1);
        }
    }
    if let Some(pb) = bench_pb {
        pb.finish_with_message("done".green().to_string());
    }

    // Calculate statistics
    durations.sort();

    let sum: Duration = durations.iter().sum();
    let avg = sum / args.iterations as u32;
    let min = durations.first().unwrap();
    let max = durations.last().unwrap();
    let p50 = &durations[args.iterations / 2];
    let p95 = &durations[args.iterations * 95 / 100];
    let p99 = &durations[args.iterations * 99 / 100];

    if quiet {
        println!(
            "{}",
            serde_json::json!({
                "min_ms": min.as_secs_f64() * 1000.0,
                "avg_ms": avg.as_secs_f64() * 1000.0,
                "max_ms": max.as_secs_f64() * 1000.0,
                "p50_ms": p50.as_secs_f64() * 1000.0,
                "p95_ms": p95.as_secs_f64() * 1000.0,
                "p99_ms": p99.as_secs_f64() * 1000.0,
            })
        );
    } else {
        println!("\n{}", "Results".green().bold());
        println!("{}", "─".repeat(50).dimmed());

        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec![
            Cell::new("Percentile").add_attribute(Attribute::Bold),
            Cell::new("Time").add_attribute(Attribute::Bold),
        ]);

        table.add_row(vec![
            Cell::new("Min"),
            Cell::new(format_duration(*min)).fg(Color::Green),
        ]);
        table.add_row(vec![
            Cell::new("p50 (Median)"),
            Cell::new(format_duration(*p50)).fg(Color::Cyan),
        ]);
        table.add_row(vec![
            Cell::new("Average"),
            Cell::new(format_duration(avg)).fg(Color::Yellow),
        ]);
        table.add_row(vec![
            Cell::new("p95"),
            Cell::new(format_duration(*p95)).fg(Color::Yellow),
        ]);
        table.add_row(vec![
            Cell::new("p99"),
            Cell::new(format_duration(*p99)).fg(Color::Red),
        ]);
        table.add_row(vec![
            Cell::new("Max"),
            Cell::new(format_duration(*max)).fg(Color::Red),
        ]);

        println!("{}", table);

        // Calculate and display performance rating
        let p50_ms = p50.as_secs_f64() * 1000.0;
        let rating = if p50_ms < 5.0 {
            "Excellent".green().bold()
        } else if p50_ms < 10.0 {
            "Good".cyan()
        } else if p50_ms < 50.0 {
            "Average".yellow()
        } else {
            "Slow".red()
        };

        println!("\n  Performance Rating: {}", rating);
        if p50_ms < 5.0 {
            println!("  {} Sub-5ms cold start achieved!", "⚡".yellow());
        }
    }

    Ok(())
}

async fn interactive_command(args: InteractiveArgs) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    print_banner();

    println!("{}", "Interactive Mode".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());
    println!("Module: {}\n", args.module.display().to_string().cyan());

    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Validate first
    let _ = SandboxConfig::builder()
        .module(&wasm_bytes)
        .context("Invalid WASM module")?;

    println!("  {} Module validated successfully\n", "✓".green().bold());

    // Ask about capabilities
    println!("{}", "Select capabilities to grant:".yellow().bold());

    let capability_options = vec![
        "stdout - Write to standard output",
        "stderr - Write to standard error",
        "stdin - Read from standard input",
        "time - Access system clock",
        "random - Secure random numbers",
        "dns - DNS resolution",
    ];

    let selections = MultiSelect::with_theme(&ColorfulTheme::default())
        .with_prompt("Use space to select, enter to confirm")
        .items(&capability_options)
        .defaults(&[true, true, false, false, false, false])
        .interact()?;

    let mut capabilities = Vec::new();
    for idx in &selections {
        match *idx {
            0 => capabilities.push(Capability::stdout()),
            1 => capabilities.push(Capability::stderr()),
            2 => capabilities.push(Capability::stdin()),
            3 => {
                capabilities.push(Capability::system_clock());
                capabilities.push(Capability::monotonic_clock());
            }
            4 => capabilities.push(Capability::secure_random()),
            5 => capabilities.push(Capability::dns_resolve()),
            _ => {}
        }
    }

    println!(
        "\n  {} {} capabilities selected\n",
        "✓".green(),
        selections.len()
    );

    // Confirm execution
    if !Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Execute with these capabilities?")
        .default(true)
        .interact()?
    {
        println!("\n{}", "Cancelled.".yellow());
        return Ok(());
    }

    println!();

    // Build and run
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .capabilities(capabilities.clone())
        .build()?;

    let spinner = create_spinner("Creating sandbox...");
    let creation_start = Instant::now();
    let mut sandbox = Sandbox::create(config).await?;
    let creation_time = creation_start.elapsed();

    spinner.set_message("Executing...");
    let output = sandbox.run(&[]).await?;
    spinner.finish_and_clear();

    // Output results
    println!("{}", "─".repeat(50).dimmed());

    if !output.stdout.is_empty() {
        println!("{}", "Output:".cyan().bold());
        std::io::Write::write_all(&mut std::io::stdout(), &output.stdout)?;
        println!();
    }

    if !output.stderr.is_empty() {
        println!("{}", "Errors:".red().bold());
        std::io::Write::write_all(&mut std::io::stderr(), &output.stderr)?;
        println!();
    }

    println!("{}", "─".repeat(50).dimmed());

    if output.exit_code == 0 {
        println!("  {} Completed successfully", "✓".green().bold());
    } else {
        println!(
            "  {} Exited with code {}",
            "✗".red().bold(),
            output.exit_code.to_string().red()
        );
    }

    println!(
        "  {} Creation: {} | Execution: {}",
        "⏱".dimmed(),
        format_duration(creation_time).cyan(),
        format_duration(output.duration).cyan()
    );

    Ok(())
}

async fn snapshot_command(cmd: SnapshotCommands) -> Result<()> {
    match cmd {
        SnapshotCommands::List => {
            println!("{}", "Snapshot Management".cyan().bold());
            println!("{}", "─".repeat(50).dimmed());
            println!(
                "{}",
                "No snapshots stored (feature under development)".dimmed()
            );
            Ok(())
        }
        SnapshotCommands::Delete { id } => {
            println!("Would delete snapshot: {}", id.yellow());
            Ok(())
        }
        SnapshotCommands::Info { id } => {
            println!("Would show info for snapshot: {}", id.yellow());
            Ok(())
        }
    }
}
