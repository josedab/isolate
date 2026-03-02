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

    /// Also validate a sandbox configuration file (JSON or YAML)
    #[arg(short, long)]
    pub config: Option<PathBuf>,
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
        }
        Err(e) => {
            if let Some(sp) = spinner {
                sp.finish_with_message(format!("{} Invalid module", "✗".red()));
            }
            eprintln!("{} {}", "Error:".red().bold(), e);
            std::process::exit(1);
        }
    }

    // Validate config file if provided
    if let Some(config_path) = &args.config {
        let spinner = if !quiet { Some(create_spinner("Validating config file...")) } else { None };

        match validate_config_file(config_path) {
            Ok(summary) => {
                if let Some(sp) = spinner {
                    sp.finish_with_message("Config file is valid ✓".green().to_string());
                }
                if !quiet {
                    println!("  {} {}", "Capabilities:".dimmed(), summary.capabilities);
                    if let Some(mem) = &summary.heap_max {
                        println!("  {} {}", "Heap max:".dimmed(), mem);
                    }
                    if let Some(timeout) = &summary.timeout {
                        println!("  {} {}", "Timeout:".dimmed(), timeout);
                    }
                }
            }
            Err(e) => {
                if let Some(sp) = spinner {
                    sp.finish_with_message(format!("{} Invalid config file", "✗".red()));
                }
                anyhow::bail!("Config validation failed: {}", e);
            }
        }
    }

    Ok(())
}

struct ConfigSummary {
    capabilities: usize,
    heap_max: Option<String>,
    timeout: Option<String>,
}

fn validate_config_file(path: &PathBuf) -> Result<ConfigSummary> {
    use isolate_core::config::ConfigFile;

    let cfg = ConfigFile::from_file(path)
        .with_context(|| format!("Failed to load config: {}", path.display()))?;

    cfg.validate().with_context(|| "Config file contains invalid values")?;

    Ok(ConfigSummary {
        capabilities: cfg.to_capabilities().len(),
        heap_max: cfg.resources.memory.as_ref().and_then(|m| m.heap_max.clone()),
        timeout: cfg.resources.timeout.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60, 0x01, 0x7f, 0x00,
        0x60, 0x00, 0x00, 0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61,
        0x70, 0x73, 0x68, 0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65, 0x77, 0x31, 0x09,
        0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01,
        0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79,
        0x02, 0x00, 0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x08, 0x01, 0x06,
        0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
    ];

    #[test]
    fn test_validate_valid_wasm_module() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.wasm");
        std::fs::write(&path, MINIMAL_WASM).unwrap();

        let args = ValidateArgs { module: path, config: None };
        assert!(validate_command(args, true).is_ok());
    }

    #[test]
    fn test_validate_nonexistent_file() {
        let args =
            ValidateArgs { module: PathBuf::from("/nonexistent/path/module.wasm"), config: None };
        assert!(validate_command(args, true).is_err());
    }

    #[test]
    fn test_validate_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("valid.wasm");
        std::fs::write(&wasm_path, MINIMAL_WASM).unwrap();

        let config_path = dir.path().join("config.json");
        std::fs::write(
            &config_path,
            r#"{ "capabilities": { "stdout": true }, "resources": { "timeout": "30s" } }"#,
        )
        .unwrap();

        let args = ValidateArgs { module: wasm_path, config: Some(config_path) };
        assert!(validate_command(args, true).is_ok());
    }

    #[test]
    fn test_validate_with_invalid_config() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("valid.wasm");
        std::fs::write(&wasm_path, MINIMAL_WASM).unwrap();

        let config_path = dir.path().join("bad.json");
        std::fs::write(&config_path, "not json at all").unwrap();

        let args = ValidateArgs { module: wasm_path, config: Some(config_path) };
        assert!(validate_command(args, true).is_err());
    }

    #[test]
    fn test_validate_config_with_bad_size() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("valid.wasm");
        std::fs::write(&wasm_path, MINIMAL_WASM).unwrap();

        let config_path = dir.path().join("bad_size.json");
        std::fs::write(
            &config_path,
            r#"{ "resources": { "memory": { "heap_max": "notasize" } } }"#,
        )
        .unwrap();

        let args = ValidateArgs { module: wasm_path, config: Some(config_path) };
        assert!(validate_command(args, true).is_err());
    }
}
