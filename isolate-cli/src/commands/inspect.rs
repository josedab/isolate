//! `isolate inspect` command — show module imports, exports, and memory requirements.

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use isolate_core::engine::WasmEngine;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct InspectArgs {
    /// Path to the WASM module
    pub module: std::path::PathBuf,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Suggest a SandboxConfig based on module requirements
    #[arg(long)]
    pub suggest_config: bool,
}

pub async fn inspect_command(args: InspectArgs, format: OutputFormat, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    let engine = WasmEngine::new()?;
    let module =
        engine.compile(&isolate_core::config::WasmModule::from_bytes(wasm_bytes.clone())?)?;

    let imports = module.required_imports();
    let exports = module.exported_functions();
    let memory = module.memory_requirements();

    if args.json || matches!(format, OutputFormat::Json) {
        let output = serde_json::json!({
            "file": args.module.display().to_string(),
            "size_bytes": wasm_bytes.len(),
            "imports": imports,
            "exports": exports,
            "memory": memory,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if !quiet {
        print_banner();
        println!("{}", "Module Inspection".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
    }

    // Imports
    println!("\n{} ({} total)", "Imports".yellow().bold(), imports.len());
    for imp in &imports {
        println!("  {} :: {} ({:?})", imp.module.dimmed(), imp.name, imp.kind);
    }

    // Exports
    println!("\n{} ({} total)", "Exports".green().bold(), exports.len());
    for exp in &exports {
        println!("  {} ({:?})", exp.name, exp.kind);
    }

    // Memory requirements
    if let Some(mem) = &memory {
        println!("\n{}", "Memory Requirements".blue().bold());
        println!("  Initial: {} pages ({} bytes)", mem.initial_pages, mem.initial_bytes);
        if let Some(max) = mem.maximum_pages {
            println!("  Maximum: {} pages ({} bytes)", max, mem.maximum_bytes.unwrap_or(0));
        } else {
            println!("  Maximum: {} (set memory_limit for safety)", "unbounded ⚠️".red());
        }
    } else {
        println!("\n{}", "Memory: no memory export declared".dimmed());
    }

    // Security warnings
    let has_unbounded_memory = memory.as_ref().is_some_and(|m| m.maximum_pages.is_none());
    let import_count = imports.len();
    if has_unbounded_memory || import_count > 20 {
        println!("\n{}", "Security Notes".red().bold());
        if has_unbounded_memory {
            println!("  ⚠️  Module has no memory maximum — always set a memory_limit");
        }
        if import_count > 20 {
            println!(
                "  ⚠️  Module has {} imports — review carefully for untrusted code",
                import_count
            );
        }
    }

    // Suggest config
    if args.suggest_config {
        println!("\n{}", "Suggested Configuration".magenta().bold());
        let mem_limit = memory.as_ref().and_then(|m| m.maximum_bytes).unwrap_or(128 * 1024 * 1024);
        println!("  memory_limit: {} bytes", mem_limit);
        println!("  fuel: 10_000_000 (adjust based on workload)");

        let needs_wasi = imports.iter().any(|i| {
            i.module.starts_with("wasi_snapshot_preview1") || i.module.starts_with("wasi:")
        });
        if needs_wasi {
            println!("  capabilities: stdout, stderr (WASI module detected)");
        }
    }

    Ok(())
}
