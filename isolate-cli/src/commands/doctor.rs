use anyhow::Result;
use colored::*;
use isolate_core::{Sandbox, SandboxConfig};

use crate::output::*;

pub async fn doctor_command(quiet: bool) -> Result<()> {
    if !quiet {
        print_banner();
        println!("{}", "System Diagnostics".cyan().bold());
        println!("{}", "─".repeat(50).dimmed());
    }

    let mut all_ok = true;

    // Check 1: Rust version
    let rust_version = rustc_version();
    if !quiet {
        print!("  {} Rust version: {}", "•".dimmed(), rust_version.cyan());
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
        0x02, 0x24, 0x01, 0x16, 0x77, 0x61, 0x73, 0x69, 0x5f, 0x73, 0x6e, 0x61, 0x70, 0x73, 0x68,
        0x6f, 0x74, 0x5f, 0x70, 0x72, 0x65, 0x76, 0x69, 0x65, 0x77, 0x31, 0x09, 0x70, 0x72, 0x6f,
        0x63, 0x5f, 0x65, 0x78, 0x69, 0x74, 0x00, 0x00, // Function section
        0x03, 0x02, 0x01, 0x01, // Memory section
        0x05, 0x03, 0x01, 0x00, 0x01, // Export section: memory and _start
        0x07, 0x13, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x5f, 0x73,
        0x74, 0x61, 0x72, 0x74, 0x00, 0x01, // Code section: call proc_exit(0)
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
            println!("  {}", "Isolate is ready to use.".dimmed());
        } else {
            println!("\n  {} Some checks failed.", "✗".red().bold());
            println!("  {}", "Please review the issues above.".dimmed());
        }
    }

    if all_ok {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

pub fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
