use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use isolate_core::SandboxConfig;
use std::path::PathBuf;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Path to the WASM module
    pub module: PathBuf,
}

pub fn validate_command(args: ValidateArgs, quiet: bool) -> Result<()> {
    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    let spinner = if !quiet { Some(create_spinner("Validating module...")) } else { None };

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
