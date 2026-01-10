//! Isolate CLI
//!
//! Command-line interface for the Isolate secure sandbox runtime.
//! Features beautiful terminal output, progress indicators, and interactive mode.

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use colored::*;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, Table};
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect};
use indicatif::{ProgressBar, ProgressStyle};
use isolate_core::{capability::Capability, error::Error as IsolateError, Sandbox, SandboxConfig};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Configuration file structures for .isolate.toml
#[derive(Debug, Deserialize, Default)]
struct ProjectConfig {
    project: Option<ProjectInfo>,
    sandbox: Option<SandboxDefaults>,
    modules: Option<Vec<ModuleConfig>>,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectInfo {
    name: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SandboxDefaults {
    memory_limit: Option<String>,
    timeout: Option<u64>,
    fuel: Option<u64>,
    cpu_time: Option<u64>,
    entry_point: Option<String>,
    capabilities: Option<CapabilitiesConfig>,
    env: Option<std::collections::HashMap<String, String>>,
    args: Option<ArgsConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct CapabilitiesConfig {
    stdout: Option<bool>,
    stderr: Option<bool>,
    stdin: Option<bool>,
    time: Option<bool>,
    random: Option<bool>,
    dns: Option<bool>,
    fs: Option<FsCapabilities>,
    http: Option<HttpCapabilities>,
}

#[derive(Debug, Deserialize, Default)]
struct FsCapabilities {
    read: Option<Vec<String>>,
    write: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct HttpCapabilities {
    hosts: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct ArgsConfig {
    values: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ModuleConfig {
    name: String,
    path: String,
    memory_limit: Option<String>,
    timeout: Option<u64>,
    fuel: Option<u64>,
}

/// Load project configuration from .isolate.toml
fn load_project_config() -> Option<ProjectConfig> {
    // Search for config file in current directory and parents
    let mut current_dir = std::env::current_dir().ok()?;

    loop {
        let config_path = current_dir.join(".isolate.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).ok()?;
            return toml::from_str(&content).ok();
        }

        if !current_dir.pop() {
            break;
        }
    }

    None
}

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

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Check installation and system requirements
    Doctor,

    /// Initialize a new Isolate project with example files
    Init(InitArgs),
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
struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    shell: Shell,
}

#[derive(Parser, Debug)]
struct InitArgs {
    /// Project directory (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Project name (defaults to directory name)
    #[arg(short, long)]
    name: Option<String>,

    /// Include example WASM modules
    #[arg(long, default_value = "true")]
    examples: bool,

    /// Overwrite existing files
    #[arg(long)]
    force: bool,
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

    /// Watch for file changes and re-execute
    #[arg(short, long)]
    watch: bool,

    /// Debounce delay for watch mode in milliseconds
    #[arg(long, default_value = "500")]
    watch_delay: u64,
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
        Commands::Completions(args) => completions_command(args),
        Commands::Doctor => doctor_command(cli.quiet).await,
        Commands::Init(args) => init_command(args, cli.quiet),
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

async fn run_command(args: RunArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    if args.watch {
        run_watch_mode(args, format, quiet).await
    } else {
        let exit_code = run_once(&args, format, quiet).await?;
        std::process::exit(exit_code);
    }
}

async fn run_watch_mode(args: RunArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let module_path = args.module.canonicalize()
        .with_context(|| format!("Failed to find module: {}", args.module.display()))?;

    if !quiet {
        println!("{}", "─".repeat(50).dimmed());
        println!(
            "  {} Watch mode enabled for {}",
            "👁".cyan(),
            module_path.display().to_string().cyan()
        );
        println!("  {} Press Ctrl+C to stop\n", "ℹ".dimmed());
    }

    // Set up file watcher
    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(
        Duration::from_millis(args.watch_delay),
        tx,
    ).context("Failed to create file watcher")?;

    // Watch the module file's parent directory
    let watch_dir = module_path.parent().unwrap_or(&module_path);
    debouncer
        .watcher()
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .context("Failed to watch directory")?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).ok();

    // Initial run
    let mut run_count = 1u32;
    if !quiet {
        println!("{} Run #{}", "▶".green().bold(), run_count);
    }
    let _ = run_once(&args, format, quiet).await;

    // Watch loop
    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(events)) => {
                // Check if our module was modified
                let module_changed = events.iter().any(|e| {
                    e.path == module_path ||
                    e.path.file_name() == module_path.file_name()
                });

                if module_changed {
                    run_count += 1;
                    if !quiet {
                        println!("\n{}", "─".repeat(50).dimmed());
                        println!(
                            "  {} File changed, re-executing... (Run #{})",
                            "↻".yellow().bold(),
                            run_count
                        );
                        println!("{}", "─".repeat(50).dimmed());
                    }
                    let _ = run_once(&args, format, quiet).await;
                }
            }
            Ok(Err(e)) => {
                if !quiet {
                    eprintln!("{} Watch error: {:?}", "⚠".yellow(), e);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Normal timeout, continue watching
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }

    if !quiet {
        println!("\n{} Watch mode stopped.", "■".red());
    }

    Ok(())
}

async fn run_once(args: &RunArgs, format: OutputFormat, quiet: bool) -> Result<i32> {
    // Load project config if available
    let project_config = load_project_config();
    let sandbox_defaults = project_config
        .as_ref()
        .and_then(|c| c.sandbox.as_ref());

    // Read the WASM module
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Build capabilities - CLI args override config file
    let mut capabilities = Vec::new();
    let caps_config = sandbox_defaults.and_then(|s| s.capabilities.as_ref());

    // stdout
    let use_stdout = args.cap_stdout
        || args.cap_stdio
        || caps_config.and_then(|c| c.stdout).unwrap_or(false);
    if use_stdout {
        capabilities.push(Capability::stdout());
    }

    // stderr
    let use_stderr = args.cap_stderr
        || args.cap_stdio
        || caps_config.and_then(|c| c.stderr).unwrap_or(false);
    if use_stderr {
        capabilities.push(Capability::stderr());
    }

    // stdin
    let use_stdin = args.cap_stdin
        || args.cap_stdio
        || caps_config.and_then(|c| c.stdin).unwrap_or(false);
    if use_stdin {
        capabilities.push(Capability::stdin());
    }

    // filesystem read - combine CLI and config
    for path in &args.cap_fs_read {
        capabilities.push(Capability::filesystem_read(path));
    }
    if let Some(fs_caps) = caps_config.and_then(|c| c.fs.as_ref()) {
        if let Some(read_paths) = &fs_caps.read {
            for path in read_paths {
                capabilities.push(Capability::filesystem_read(PathBuf::from(path)));
            }
        }
    }

    // filesystem write - combine CLI and config
    for path in &args.cap_fs_write {
        capabilities.push(Capability::filesystem_write(path));
    }
    if let Some(fs_caps) = caps_config.and_then(|c| c.fs.as_ref()) {
        if let Some(write_paths) = &fs_caps.write {
            for path in write_paths {
                capabilities.push(Capability::filesystem_write(PathBuf::from(path)));
            }
        }
    }

    // HTTP - combine CLI and config
    let mut http_hosts = args.cap_http.clone();
    if let Some(http_caps) = caps_config.and_then(|c| c.http.as_ref()) {
        if let Some(hosts) = &http_caps.hosts {
            http_hosts.extend(hosts.clone());
        }
    }
    if !http_hosts.is_empty() {
        capabilities.push(Capability::http_client(http_hosts));
    }

    // dns
    let use_dns = args.cap_dns || caps_config.and_then(|c| c.dns).unwrap_or(false);
    if use_dns {
        capabilities.push(Capability::dns_resolve());
    }

    // time
    let use_time = args.cap_time || caps_config.and_then(|c| c.time).unwrap_or(false);
    if use_time {
        capabilities.push(Capability::system_clock());
        capabilities.push(Capability::monotonic_clock());
    }

    // random
    let use_random = args.cap_random || caps_config.and_then(|c| c.random).unwrap_or(false);
    if use_random {
        capabilities.push(Capability::secure_random());
    }

    // Parse environment variables - config first, CLI overrides
    let mut env_vars = std::collections::HashMap::new();
    if let Some(config_env) = sandbox_defaults.and_then(|s| s.env.as_ref()) {
        for (key, value) in config_env {
            // Support environment variable expansion: ${VAR_NAME}
            let expanded = if value.starts_with("${") && value.ends_with("}") {
                let var_name = &value[2..value.len() - 1];
                std::env::var(var_name).unwrap_or_default()
            } else {
                value.clone()
            };
            env_vars.insert(key.clone(), expanded);
        }
    }
    for env_str in &args.env {
        let parts: Vec<_> = env_str.splitn(2, '=').collect();
        if parts.len() == 2 {
            env_vars.insert(parts[0].to_string(), parts[1].to_string());
        }
    }

    // Build configuration - use config defaults where CLI args use defaults
    let memory_limit_str = if args.memory_limit == "256M" {
        sandbox_defaults
            .and_then(|s| s.memory_limit.as_ref())
            .cloned()
            .unwrap_or_else(|| args.memory_limit.clone())
    } else {
        args.memory_limit.clone()
    };
    let memory_limit = parse_size(&memory_limit_str)?;

    // Timeout - use config default if CLI is at default
    let timeout = if args.timeout == 60 {
        sandbox_defaults.and_then(|s| s.timeout).unwrap_or(args.timeout)
    } else {
        args.timeout
    };

    // Entry point - use config default if CLI is at default
    let entry = if args.entry == "_start" {
        sandbox_defaults
            .and_then(|s| s.entry_point.as_ref())
            .cloned()
            .unwrap_or_else(|| args.entry.clone())
    } else {
        args.entry.clone()
    };

    // Fuel - CLI overrides config
    let fuel = args.fuel.or_else(|| sandbox_defaults.and_then(|s| s.fuel));

    // CPU time - CLI overrides config
    let cpu_time = args.cpu_time.or_else(|| sandbox_defaults.and_then(|s| s.cpu_time));

    // Args - combine config and CLI
    let mut run_args = Vec::new();
    if let Some(config_args) = sandbox_defaults.and_then(|s| s.args.as_ref()) {
        if let Some(values) = &config_args.values {
            run_args.extend(values.clone());
        }
    }
    run_args.extend(args.args.clone());

    let mut builder = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(memory_limit)
        .wall_time_limit(Duration::from_secs(timeout))
        .capabilities(capabilities.clone())
        .envs(env_vars)
        .args(run_args.into_iter())
        .entry_point(entry);

    if let Some(fuel) = fuel {
        builder = builder.fuel(fuel);
    }
    if let Some(cpu_time) = cpu_time {
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

    Ok(output.exit_code)
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

fn completions_command(args: CompletionsArgs) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    generate(args.shell, &mut cmd, bin_name, &mut std::io::stdout());
    Ok(())
}

fn init_command(args: InitArgs, quiet: bool) -> Result<()> {
    use std::fs;
    use std::io::Write;

    let project_dir = args.path.canonicalize().unwrap_or_else(|_| args.path.clone());
    let project_name = args
        .name
        .unwrap_or_else(|| {
            project_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "isolate-project".to_string())
        });

    if !quiet {
        print_banner();
        println!("{}", "Initialize Isolate Project".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
        println!("  Project: {}", project_name.cyan());
        println!("  Directory: {}\n", project_dir.display().to_string().dimmed());
    }

    // Create project directory if it doesn't exist
    if !project_dir.exists() {
        fs::create_dir_all(&project_dir)?;
    }

    // Check for existing config
    let config_path = project_dir.join(".isolate.toml");
    if config_path.exists() && !args.force {
        anyhow::bail!(
            "Project already initialized (found .isolate.toml). Use --force to overwrite."
        );
    }

    // Create .isolate.toml config file
    let config_content = format!(
        r#"# Isolate Project Configuration
# Generated by: isolate init
# Documentation: https://github.com/josedab/isolate

[project]
name = "{}"
version = "0.1.0"

# Default sandbox configuration
[sandbox]
# Memory limit (supports K, M, G suffixes)
memory_limit = "256M"

# Wall-clock timeout in seconds
timeout = 60

# CPU fuel limit (instruction count, 0 = unlimited)
# fuel = 10000000

# CPU time limit in seconds (0 = unlimited)
# cpu_time = 30

# Entry point function
entry_point = "_start"

# Default capabilities to grant
[sandbox.capabilities]
stdout = true
stderr = true
stdin = false
time = false
random = false
dns = false

# Filesystem capabilities (uncomment to enable)
# [sandbox.capabilities.fs]
# read = ["/data"]
# write = ["/tmp"]

# HTTP capabilities (uncomment to enable)
# [sandbox.capabilities.http]
# hosts = ["*.example.com", "api.github.com"]

# Environment variables to pass
[sandbox.env]
# API_KEY = "${{API_KEY}}"

# Command-line arguments
# [sandbox.args]
# values = ["--verbose"]

# Multiple module configurations
# [[modules]]
# name = "main"
# path = "modules/main.wasm"
#
# [[modules]]
# name = "worker"
# path = "modules/worker.wasm"
# memory_limit = "128M"
"#,
        project_name
    );

    let mut config_file = fs::File::create(&config_path)?;
    config_file.write_all(config_content.as_bytes())?;

    if !quiet {
        println!("  {} Created .isolate.toml", "✓".green());
    }

    // Create examples directory with sample WASM modules
    if args.examples {
        let examples_dir = project_dir.join("examples");
        fs::create_dir_all(&examples_dir)?;

        // Minimal valid WASI WASM module that calls proc_exit(0)
        let hello_wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, // WASM magic
            0x01, 0x00, 0x00, 0x00, // Version 1
            // Type section
            0x01, 0x08, 0x02, 0x60, 0x01, 0x7f, 0x00, 0x60, 0x00, 0x00,
            // Import section: wasi_snapshot_preview1.proc_exit
            0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61,
            0x70, 0x73, 0x68, 0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65,
            0x77, 0x31, 0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74,
            0x00, 0x00,
            // Function section
            0x03, 0x02, 0x01, 0x01,
            // Memory section
            0x05, 0x03, 0x01, 0x00, 0x01,
            // Export section: memory and _start
            0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
            0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01,
            // Code section: call proc_exit(0)
            0x0a, 0x08, 0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
        ];

        fs::write(examples_dir.join("hello.wasm"), hello_wasm)?;

        if !quiet {
            println!("  {} Created examples/hello.wasm", "✓".green());
        }

        // Create a README for examples
        let examples_readme = r#"# Example WASM Modules

This directory contains example WebAssembly modules for use with Isolate.

## hello.wasm

A minimal WASI module that exits with code 0.

```bash
isolate run examples/hello.wasm --cap-stdout
```

## Building Your Own Modules

You can compile WASM modules from various languages:

### Rust
```bash
cargo build --target wasm32-wasip1 --release
```

### C/C++ (with WASI SDK)
```bash
$WASI_SDK/bin/clang --sysroot=$WASI_SDK/share/wasi-sysroot -o output.wasm input.c
```

### AssemblyScript
```bash
npx asc input.ts -o output.wasm --runtime stub
```

### Go (TinyGo)
```bash
tinygo build -o output.wasm -target wasi input.go
```
"#;
        fs::write(examples_dir.join("README.md"), examples_readme)?;

        if !quiet {
            println!("  {} Created examples/README.md", "✓".green());
        }
    }

    // Create .gitignore
    let gitignore_path = project_dir.join(".gitignore");
    if !gitignore_path.exists() || args.force {
        let gitignore_content = r#"# Isolate project ignores
*.wasm.cache
.isolate-snapshots/
"#;
        fs::write(&gitignore_path, gitignore_content)?;

        if !quiet {
            println!("  {} Created .gitignore", "✓".green());
        }
    }

    if !quiet {
        println!("{}", "─".repeat(50).dimmed());
        println!("\n  {} Project initialized successfully!", "✓".green().bold());
        println!("\n  {}", "Next steps:".yellow().bold());
        println!("    1. Add your WASM modules to the project");
        println!("    2. Configure capabilities in .isolate.toml");
        println!("    3. Run with: {}", "isolate run <module.wasm>".cyan());
        println!("\n  Try the example:");
        println!(
            "    {}",
            "isolate run examples/hello.wasm --cap-stdout".cyan()
        );
    }

    Ok(())
}

async fn doctor_command(quiet: bool) -> Result<()> {
    if !quiet {
        print_banner();
        println!("{}", "System Diagnostics".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
    }

    let mut all_ok = true;

    // Check 1: Rust version
    let rust_version = rustc_version();
    if !quiet {
        print!(
            "  {} Rust version: {}",
            "•".dimmed(),
            rust_version.cyan()
        );
        println!(" {}", "✓".green());
    }

    // Check 2: Wasmtime availability (we have it if we compiled)
    if !quiet {
        print!("  {} Wasmtime: {}", "•".dimmed(), "27.x".cyan());
        println!(" {}", "✓".green());
    }

    // Check 3: Self-test with embedded minimal WASM module
    // Minimal valid WASI WASM module that calls proc_exit(0)
    let minimal_wasm: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // WASM magic
        0x01, 0x00, 0x00, 0x00, // Version 1
        // Type section
        0x01, 0x08, 0x02, 0x60, 0x01, 0x7f, 0x00, 0x60, 0x00, 0x00,
        // Import section: wasi_snapshot_preview1.proc_exit
        0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61,
        0x70, 0x73, 0x68, 0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65,
        0x77, 0x31, 0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74,
        0x00, 0x00,
        // Function section
        0x03, 0x02, 0x01, 0x01,
        // Memory section
        0x05, 0x03, 0x01, 0x00, 0x01,
        // Export section: memory and _start
        0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00,
        0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01,
        // Code section: call proc_exit(0)
        0x0a, 0x08, 0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
    ];

    if !quiet {
        print!("  {} Self-test: ", "•".dimmed());
    }

    match SandboxConfig::builder().module(minimal_wasm) {
        Ok(builder) => match builder.build() {
            Ok(config) => match Sandbox::create(config).await {
                Ok(mut sandbox) => match sandbox.run(&[]).await {
                    Ok(output) => {
                        if output.exit_code == 0 {
                            if !quiet {
                                println!("{}", "Sandbox execution OK ✓".green());
                            }
                        } else {
                            if !quiet {
                                println!(
                                    "{}",
                                    format!("Unexpected exit code: {}", output.exit_code).yellow()
                                );
                            }
                            all_ok = false;
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            println!("{}", format!("Execution failed: {}", e).red());
                        }
                        all_ok = false;
                    }
                },
                Err(e) => {
                    if !quiet {
                        println!("{}", format!("Sandbox creation failed: {}", e).red());
                    }
                    all_ok = false;
                }
            },
            Err(e) => {
                if !quiet {
                    println!("{}", format!("Config build failed: {}", e).red());
                }
                all_ok = false;
            }
        },
        Err(e) => {
            if !quiet {
                println!("{}", format!("Module validation failed: {}", e).red());
            }
            all_ok = false;
        }
    }

    // Check 4: Temp directory access
    if !quiet {
        print!("  {} Temp directory: ", "•".dimmed());
    }
    match std::env::temp_dir().canonicalize() {
        Ok(temp) => {
            if !quiet {
                println!("{} {}", temp.display().to_string().cyan(), "✓".green());
            }
        }
        Err(_) => {
            if !quiet {
                println!("{}", "Not accessible".red());
            }
            all_ok = false;
        }
    }

    // Summary
    if !quiet {
        println!("{}", "─".repeat(50).dimmed());
        if all_ok {
            println!("\n  {} All checks passed!", "✓".green().bold());
            println!(
                "  {}",
                "Isolate is ready to use.".dimmed()
            );
        } else {
            println!("\n  {} Some checks failed.", "✗".red().bold());
            println!(
                "  {}",
                "Please review the issues above.".dimmed()
            );
        }
    }

    if all_ok {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
