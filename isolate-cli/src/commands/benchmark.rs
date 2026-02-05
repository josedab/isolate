use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, Table};
use indicatif::{ProgressBar, ProgressStyle};
use isolate_core::{Sandbox, SandboxConfig};
use std::path::PathBuf;
use std::time::Duration;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct BenchmarkArgs {
    /// Path to the WASM module
    pub module: PathBuf,

    /// Number of iterations
    #[arg(short, long, default_value = "100")]
    pub iterations: usize,

    /// Warm up iterations
    #[arg(long, default_value = "10")]
    pub warmup: usize,

    /// Include execution in benchmark
    #[arg(long)]
    pub include_run: bool,
}

pub async fn benchmark_command(args: BenchmarkArgs, quiet: bool) -> Result<()> {
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

        table.add_row(vec![Cell::new("Min"), Cell::new(format_duration(*min)).fg(Color::Green)]);
        table.add_row(vec![
            Cell::new("p50 (Median)"),
            Cell::new(format_duration(*p50)).fg(Color::Cyan),
        ]);
        table
            .add_row(vec![Cell::new("Average"), Cell::new(format_duration(avg)).fg(Color::Yellow)]);
        table.add_row(vec![Cell::new("p95"), Cell::new(format_duration(*p95)).fg(Color::Yellow)]);
        table.add_row(vec![Cell::new("p99"), Cell::new(format_duration(*p99)).fg(Color::Red)]);
        table.add_row(vec![Cell::new("Max"), Cell::new(format_duration(*max)).fg(Color::Red)]);

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
