use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect};
use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::path::PathBuf;
use std::time::Instant;

use crate::output::*;

#[derive(Parser, Debug)]
pub struct InteractiveArgs {
    /// Path to the WASM module
    pub module: PathBuf,
}

pub async fn interactive_command(args: InteractiveArgs) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    print_banner();

    println!("{}", "Interactive Mode".cyan().bold());
    println!("{}", "─".repeat(50).dimmed());
    println!("Module: {}\n", args.module.display().to_string().cyan());

    let wasm_bytes = std::fs::read(&args.module)
        .with_context(|| format!("Failed to read module: {}", args.module.display()))?;

    // Validate first
    let _ = SandboxConfig::builder().module(&wasm_bytes).context("Invalid WASM module")?;

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

    println!("\n  {} {} capabilities selected\n", "✓".green(), selections.len());

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
    let config =
        SandboxConfig::builder().module(&wasm_bytes)?.capabilities(capabilities.clone()).build()?;

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
        println!("  {} Exited with code {}", "✗".red().bold(), output.exit_code.to_string().red());
    }

    println!(
        "  {} Creation: {} | Execution: {}",
        "⏱".dimmed(),
        format_duration(creation_time).cyan(),
        format_duration(output.duration).cyan()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_index_mapping() {
        let mut capabilities = Vec::new();
        let selections = vec![0, 1, 3, 4];

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

        // 4 selections but index 3 adds 2 capabilities
        assert_eq!(capabilities.len(), 5);
    }

    #[test]
    fn test_empty_capability_selection() {
        let mut capabilities = Vec::new();
        let selections: Vec<usize> = vec![];

        for idx in &selections {
            if *idx == 0 {
                capabilities.push(Capability::stdout());
            }
        }

        assert!(capabilities.is_empty());
    }

    #[test]
    fn test_interactive_args() {
        use clap::Parser;
        let args = InteractiveArgs::try_parse_from(["interactive", "module.wasm"]).unwrap();
        assert_eq!(args.module, PathBuf::from("module.wasm"));
    }
}
