use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use comfy_table::{presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, Table};
use isolate_core::SandboxConfig;
use std::path::PathBuf;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct InfoArgs {
    /// Path to the WASM module
    pub module: PathBuf,

    /// Show exports
    #[arg(long)]
    pub exports: bool,

    /// Show imports
    #[arg(long)]
    pub imports: bool,
}

#[derive(Parser, Debug)]
pub struct AnalyzeArgs {
    /// Path to the WASM module
    pub module: PathBuf,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

pub fn info_command(args: InfoArgs, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Validate
    let _config = SandboxConfig::builder().module(&wasm_bytes)?;

    if quiet {
        println!("{}", isolate_core::config::ModuleHash::from_bytes(&wasm_bytes));
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

pub fn analyze_command(args: AnalyzeArgs, quiet: bool) -> Result<()> {
    use isolate_core::policy_gen::ModuleAnalyzer;

    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    let analyzer = ModuleAnalyzer::new();
    let report = analyzer.analyze(&wasm_bytes);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if !quiet {
        print_banner();
    }

    println!("{}", "Security Analysis Report".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());

    let risk_str = report.overall_risk.to_string();
    println!(
        "  {} {}",
        "Risk Level:".bold(),
        match risk_str.as_str() {
            "low" => risk_str.green(),
            "medium" => risk_str.yellow(),
            "high" | "critical" => risk_str.red(),
            _ => risk_str.normal(),
        }
    );
    println!("  {} {}", "Module Size:".bold(), format_bytes(report.module_size));
    println!();

    if !report.suggested_capabilities.is_empty() {
        println!("{}", "Suggested Capabilities".cyan().bold());
        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec![
            Cell::new("Capability").add_attribute(Attribute::Bold),
            Cell::new("Confidence").add_attribute(Attribute::Bold),
            Cell::new("Risk").add_attribute(Attribute::Bold),
            Cell::new("Reason").add_attribute(Attribute::Bold),
        ]);
        for cap in &report.suggested_capabilities {
            table.add_row(vec![
                Cell::new(&cap.capability).fg(Color::Green),
                Cell::new(format!("{:.0}%", cap.confidence * 100.0)),
                Cell::new(cap.risk.to_string()),
                Cell::new(&cap.reason),
            ]);
        }
        println!("{}", table);
    }

    if !report.security_concerns.is_empty() {
        println!("\n{}", "Security Concerns".red().bold());
        let mut table = Table::new();
        table.load_preset(UTF8_FULL_CONDENSED);
        table.set_header(vec![
            Cell::new("Risk").add_attribute(Attribute::Bold),
            Cell::new("Description").add_attribute(Attribute::Bold),
            Cell::new("Mitigation").add_attribute(Attribute::Bold),
        ]);
        for concern in &report.security_concerns {
            let risk_text = concern.risk.to_string();
            let risk_cell = match risk_text.as_str() {
                "high" | "critical" => Cell::new(&risk_text).fg(Color::Red),
                "medium" => Cell::new(&risk_text).fg(Color::Yellow),
                _ => Cell::new(&risk_text).fg(Color::Green),
            };
            table.add_row(vec![
                risk_cell,
                Cell::new(&concern.description),
                Cell::new(&concern.mitigation),
            ]);
        }
        println!("{}", table);
    }

    if !report.imports.is_empty() {
        println!("\n  {} {}", "Imports detected:".dimmed(), report.imports.len());
        for imp in &report.imports {
            let wasi_tag = if imp.is_wasi { " (WASI)" } else { "" };
            println!("    {} {}.{}{}", "•".dimmed(), imp.module, imp.name, wasi_tag.dimmed());
        }
    }

    println!("\n  {}", report.summary.dimmed());

    Ok(())
}
