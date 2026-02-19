use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, Table};
use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use crate::config::*;
use crate::output::*;

#[derive(Parser, Debug)]
pub struct RunArgs {
    /// Path to the WASM module
    pub module: PathBuf,

    /// Memory limit (e.g., 128M, 1G)
    #[arg(short, long, default_value = "256M")]
    pub memory_limit: String,

    /// Fuel limit (instruction count)
    #[arg(short, long)]
    pub fuel: Option<u64>,

    /// Wall-clock timeout in seconds
    #[arg(short, long, default_value = "60")]
    pub timeout: u64,

    /// CPU time limit in seconds
    #[arg(long)]
    pub cpu_time: Option<u64>,

    /// Grant stdout capability
    #[arg(long)]
    pub cap_stdout: bool,

    /// Grant stderr capability
    #[arg(long)]
    pub cap_stderr: bool,

    /// Grant stdin capability
    #[arg(long)]
    pub cap_stdin: bool,

    /// Grant all stdio capabilities
    #[arg(long)]
    pub cap_stdio: bool,

    /// Grant filesystem read capability (path)
    #[arg(long)]
    pub cap_fs_read: Vec<PathBuf>,

    /// Grant filesystem write capability (path)
    #[arg(long)]
    pub cap_fs_write: Vec<PathBuf>,

    /// Grant HTTP capability (host pattern)
    #[arg(long)]
    pub cap_http: Vec<String>,

    /// Grant DNS resolution capability
    #[arg(long)]
    pub cap_dns: bool,

    /// Grant system clock capability
    #[arg(long)]
    pub cap_time: bool,

    /// Grant random number capability
    #[arg(long)]
    pub cap_random: bool,

    /// Environment variable to pass (KEY=VALUE)
    #[arg(short, long)]
    pub env: Vec<String>,

    /// Arguments to pass to the module
    #[arg(last = true)]
    pub args: Vec<String>,

    /// Entry point function
    #[arg(long, default_value = "_start")]
    pub entry: String,

    /// Read input from stdin
    #[arg(long)]
    pub stdin: bool,

    /// Show resource usage after execution
    #[arg(long)]
    pub show_stats: bool,

    /// Watch for file changes and re-execute
    #[arg(short, long)]
    pub watch: bool,

    /// Debounce delay for watch mode in milliseconds
    #[arg(long, default_value = "500")]
    pub watch_delay: u64,
}

pub async fn run_command(args: RunArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    if args.watch {
        run_watch_mode(args, format, quiet).await
    } else {
        let exit_code = run_once(&args, format, quiet).await?;
        std::process::exit(exit_code);
    }
}

pub async fn run_watch_mode(args: RunArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let module_path = args
        .module
        .canonicalize()
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
    let mut debouncer = new_debouncer(Duration::from_millis(args.watch_delay), tx)
        .context("Failed to create file watcher")?;

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
    })
    .ok();

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
                    e.path == module_path || e.path.file_name() == module_path.file_name()
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

pub async fn run_once(args: &RunArgs, format: OutputFormat, quiet: bool) -> Result<i32> {
    // Load project config if available
    let project_config = load_project_config();
    let sandbox_defaults = project_config.as_ref().and_then(|c| c.sandbox.as_ref());

    // Read the WASM module
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Build capabilities - CLI args override config file
    let mut capabilities = Vec::new();
    let caps_config = sandbox_defaults.and_then(|s| s.capabilities.as_ref());

    // stdout
    let use_stdout =
        args.cap_stdout || args.cap_stdio || caps_config.and_then(|c| c.stdout).unwrap_or(false);
    if use_stdout {
        capabilities.push(Capability::stdout());
    }

    // stderr
    let use_stderr =
        args.cap_stderr || args.cap_stdio || caps_config.and_then(|c| c.stderr).unwrap_or(false);
    if use_stderr {
        capabilities.push(Capability::stderr());
    }

    // stdin
    let use_stdin =
        args.cap_stdin || args.cap_stdio || caps_config.and_then(|c| c.stdin).unwrap_or(false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_args_defaults() {
        use clap::Parser;
        let args = RunArgs::try_parse_from(["run", "module.wasm"]).unwrap();
        assert_eq!(args.memory_limit, "256M");
        assert_eq!(args.timeout, 60);
        assert_eq!(args.entry, "_start");
        assert!(!args.cap_stdout);
        assert!(!args.cap_stderr);
        assert!(!args.cap_stdin);
        assert!(!args.cap_stdio);
        assert!(!args.watch);
        assert_eq!(args.watch_delay, 500);
    }

    #[test]
    fn test_run_args_cap_stdio_flag() {
        use clap::Parser;
        let args = RunArgs::try_parse_from(["run", "module.wasm", "--cap-stdio"]).unwrap();
        assert!(args.cap_stdio);
    }

    #[test]
    fn test_run_args_memory_limit() {
        use clap::Parser;
        let args = RunArgs::try_parse_from(["run", "module.wasm", "-m", "512M"]).unwrap();
        assert_eq!(args.memory_limit, "512M");
    }

    #[test]
    fn test_run_args_fuel() {
        use clap::Parser;
        let args = RunArgs::try_parse_from(["run", "module.wasm", "-f", "1000000"]).unwrap();
        assert_eq!(args.fuel, Some(1000000));
    }

    #[test]
    fn test_run_args_env_vars() {
        use clap::Parser;
        let args = RunArgs::try_parse_from([
            "run",
            "module.wasm",
            "-e",
            "KEY1=value1",
            "-e",
            "KEY2=value2",
        ])
        .unwrap();
        assert_eq!(args.env.len(), 2);
        assert_eq!(args.env[0], "KEY1=value1");
    }

    #[test]
    fn test_run_args_fs_capabilities() {
        use clap::Parser;
        let args = RunArgs::try_parse_from([
            "run",
            "module.wasm",
            "--cap-fs-read",
            "/data",
            "--cap-fs-write",
            "/tmp",
        ])
        .unwrap();
        assert_eq!(args.cap_fs_read.len(), 1);
        assert_eq!(args.cap_fs_write.len(), 1);
    }

    #[test]
    fn test_run_args_http_hosts() {
        use clap::Parser;
        let args = RunArgs::try_parse_from([
            "run",
            "module.wasm",
            "--cap-http",
            "api.example.com",
            "--cap-http",
            "cdn.example.com",
        ])
        .unwrap();
        assert_eq!(args.cap_http.len(), 2);
    }

    #[test]
    fn test_env_var_parsing_key_value() {
        let env_str = "API_KEY=secret123";
        let parts: Vec<_> = env_str.splitn(2, '=').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "API_KEY");
        assert_eq!(parts[1], "secret123");
    }

    #[test]
    fn test_env_var_parsing_value_with_equals() {
        let env_str = "CONFIG=key=value";
        let parts: Vec<_> = env_str.splitn(2, '=').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "CONFIG");
        assert_eq!(parts[1], "key=value");
    }

    #[test]
    fn test_env_var_expansion_pattern() {
        let value = "${HOME}";
        let is_expansion = value.starts_with("${") && value.ends_with("}");
        assert!(is_expansion);
        let var_name = &value[2..value.len() - 1];
        assert_eq!(var_name, "HOME");
    }

    #[test]
    fn test_env_var_non_expansion() {
        let value = "plain_value";
        let is_expansion = value.starts_with("${") && value.ends_with("}");
        assert!(!is_expansion);
    }
}
