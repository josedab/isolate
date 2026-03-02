//! `isolate estimate` command — dry-run a module to estimate resource usage.

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use isolate_core::capability::Capability;
use isolate_core::{Sandbox, SandboxConfig};
use std::time::Duration;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct EstimateArgs {
    /// Path to the WASM module
    pub module: std::path::PathBuf,

    /// Optional input file
    #[arg(long)]
    pub input: Option<std::path::PathBuf>,

    /// Number of estimation runs (more runs = better confidence)
    #[arg(long, default_value = "3")]
    pub runs: usize,
}

pub async fn estimate_command(args: EstimateArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    let input = if let Some(ref path) = args.input {
        std::fs::read(path).with_context(|| format!("Failed to read input: {}", path.display()))?
    } else {
        Vec::new()
    };

    let engine = std::sync::Arc::new(isolate_core::engine::WasmEngine::new()?);

    let mut durations = Vec::new();
    let mut fuel_values = Vec::new();
    let mut peak_memory_values = Vec::new();

    for _ in 0..args.runs.max(1) {
        let config = SandboxConfig::builder()
            .module(&wasm_bytes)?
            .fuel(100_000_000) // generous fuel for estimation
            .memory_limit(512 * 1024 * 1024) // 512MB
            .wall_time_limit(Duration::from_secs(60))
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .build()?;

        let mut sandbox = Sandbox::create_with_engine(config, engine.clone()).await?;
        let output = sandbox.run(&input).await?;

        durations.push(output.duration);
        fuel_values.push(output.resource_usage.fuel_consumed);
        peak_memory_values.push(output.resource_usage.peak_memory);
    }

    if durations.is_empty() {
        anyhow::bail!("All estimation runs failed. Check that the WASM module executes correctly.");
    }

    let count = durations.len() as u32;
    let avg_duration = durations.iter().sum::<Duration>() / count;
    let min_duration = durations.iter().min().copied().unwrap_or_default();
    let max_duration = durations.iter().max().copied().unwrap_or_default();

    let fuel_count = fuel_values.len().max(1) as u64;
    let avg_fuel: u64 = fuel_values.iter().sum::<u64>() / fuel_count;
    let max_fuel = fuel_values.iter().max().copied().unwrap_or(0);
    let recommended_fuel = max_fuel.saturating_mul(3) / 2; // 1.5x without float overflow

    let mem_count = peak_memory_values.len().max(1);
    let avg_memory: usize = peak_memory_values.iter().sum::<usize>() / mem_count;
    let max_memory = peak_memory_values.iter().max().copied().unwrap_or(0);
    let recommended_memory = max_memory
        .saturating_mul(3)
        .checked_div(2)
        .unwrap_or(max_memory)
        .checked_next_power_of_two()
        .unwrap_or(max_memory);

    if matches!(format, OutputFormat::Json) {
        let output = serde_json::json!({
            "runs": args.runs,
            "duration_ms": {
                "min": min_duration.as_secs_f64() * 1000.0,
                "avg": avg_duration.as_secs_f64() * 1000.0,
                "max": max_duration.as_secs_f64() * 1000.0,
            },
            "fuel": {
                "avg": avg_fuel,
                "max": max_fuel,
                "recommended": recommended_fuel,
            },
            "peak_memory_bytes": {
                "avg": avg_memory,
                "max": max_memory,
                "recommended": recommended_memory,
            },
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if !quiet {
        print_banner();
        println!("{}", "Resource Estimation".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
        println!("  Runs: {}", args.runs);
    }

    println!("\n{}", "Duration".yellow().bold());
    println!("  Min: {:.2}ms", min_duration.as_secs_f64() * 1000.0);
    println!("  Avg: {:.2}ms", avg_duration.as_secs_f64() * 1000.0);
    println!("  Max: {:.2}ms", max_duration.as_secs_f64() * 1000.0);

    println!("\n{}", "Fuel Consumption".green().bold());
    println!("  Avg: {}", avg_fuel);
    println!("  Max: {}", max_fuel);
    println!("  {} {}", "Recommended:".bold(), recommended_fuel);

    println!("\n{}", "Peak Memory".blue().bold());
    println!("  Avg: {} bytes", avg_memory);
    println!("  Max: {} bytes", max_memory);
    println!("  {} {} bytes", "Recommended:".bold(), recommended_memory);

    Ok(())
}
