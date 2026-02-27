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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_wasm_module() {
        let dir = tempfile::tempdir().unwrap();
        // Minimal valid WASI WASM module
        let wasm: &[u8] = &[
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60, 0x01, 0x7f,
            0x00, 0x60, 0x00, 0x00, 0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73,
            0x6e, 0x61, 0x70, 0x73, 0x68, 0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65,
            0x77, 0x31, 0x09, 0x70, 0x72, 0x6f, 0x63, 0x5f, 0x65, 0x78, 0x69, 0x74, 0x00, 0x00,
            0x03, 0x02, 0x01, 0x01, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d,
            0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74,
            0x00, 0x01, 0x0a, 0x08, 0x01, 0x06, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
        ];
        let path = dir.path().join("valid.wasm");
        std::fs::write(&path, wasm).unwrap();

        let args = ValidateArgs { module: path };
        assert!(validate_command(args, true).is_ok());
    }

    #[test]
    fn test_validate_nonexistent_file() {
        let args = ValidateArgs { module: PathBuf::from("/nonexistent/path/module.wasm") };
        assert!(validate_command(args, true).is_err());
    }

    #[test]
    fn test_validate_args_default() {
        let args = ValidateArgs { module: PathBuf::from("test.wasm") };
        assert_eq!(args.module, PathBuf::from("test.wasm"));
    }
}
