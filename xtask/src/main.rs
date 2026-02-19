//! xtask — workspace-native task runner for Isolate.
//!
//! This replaces the need for `just` or `make` by providing cross-platform
//! developer workflow commands using only `cargo`.
//!
//! Usage:
//!   cargo xtask check       # Run all checks (fmt, clippy, test)
//!   cargo xtask test        # Run all tests
//!   cargo xtask fmt         # Format code
//!   cargo xtask lint        # Run clippy lints
//!   cargo xtask pre-commit  # Full pre-push validation
//!   cargo xtask doctor      # Verify development environment

use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().map(|s| s.as_str()).unwrap_or("help");

    let result = match task {
        "check" => run_check(),
        "test" => run_test(),
        "test-core" => run_test_core(),
        "fmt" => run_fmt(false),
        "fmt-check" => run_fmt(true),
        "lint" => run_lint(),
        "pre-commit" => run_pre_commit(),
        "doctor" => run_doctor(),
        "docs" => run_docs(),
        "build" => run_build(),
        "install-hooks" => run_install_hooks(),
        "new-module" => {
            let name = args.get(1).map(|s| s.as_str());
            let feature = args.get(2).map(|s| s.as_str());
            run_new_module(name, feature)
        }
        "bump" => {
            let version = args.get(1).map(|s| s.as_str());
            run_bump(version)
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {other}");
            eprintln!("Run `cargo xtask help` for available commands.");
            Err(())
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn print_help() {
    println!(
        "\
cargo xtask — Developer workflow commands for Isolate

USAGE:
    cargo xtask <COMMAND>

COMMANDS:
    check          Run all checks: format, lint, and test (default members)
    test           Run tests (default members)
    test-core      Run core tests only (faster)
    fmt            Format all code
    fmt-check      Check formatting without modifying files
    lint           Run clippy with -D warnings (default members)
    pre-commit     Full pre-push validation (fmt + lint + test)
    doctor         Verify development environment is set up correctly
    docs           Generate documentation (default members)
    build          Build crates (default members)
    install-hooks  Install git pre-commit hooks
    new-module     Scaffold a new feature-gated module
    bump           Bump workspace version (e.g., cargo xtask bump 0.2.0)
    help           Show this help message

Use `cargo test --all-features --workspace` to include all crates (requires Python dev headers).

EXAMPLES:
    cargo xtask check                     # Quick check before pushing
    cargo xtask doctor                    # First-time setup verification
    cargo xtask test-core                 # Fast feedback during development
    cargo xtask new-module my_mod extras  # Create module gated on 'extras' feature"
    );
}

// --- Task Implementations ---

fn run_check() -> Result<(), ()> {
    println!("🔍 Running all checks...\n");
    run_fmt(true)?;
    println!();
    run_lint()?;
    println!();
    run_test_core()?;
    println!("\n✅ All checks passed!");
    Ok(())
}

fn run_test() -> Result<(), ()> {
    println!("🧪 Running tests (default members)...");
    cargo(&["test"])
}

fn run_test_core() -> Result<(), ()> {
    println!("🧪 Running core tests...");
    cargo(&["test", "--package", "isolate-core"])
}

fn run_fmt(check_only: bool) -> Result<(), ()> {
    if check_only {
        println!("📐 Checking formatting...");
        cargo(&["fmt", "--all", "--", "--check"])
    } else {
        println!("📐 Formatting code...");
        cargo(&["fmt", "--all"])
    }
}

fn run_lint() -> Result<(), ()> {
    println!("📎 Running clippy...");
    cargo(&["clippy", "--all-targets", "--", "-D", "warnings"])
}

fn run_pre_commit() -> Result<(), ()> {
    println!("🚀 Running pre-commit checks...\n");
    run_fmt(true)?;
    println!();
    run_lint()?;
    println!();
    run_test()?;
    println!("\n✅ Pre-commit checks passed! Ready to push.");
    Ok(())
}

fn run_doctor() -> Result<(), ()> {
    println!("🔍 Checking development environment...\n");

    // Check Rust toolchain
    print!("  Rust toolchain: ");
    match cmd("rustc", &["--version"]) {
        Ok(out) => println!("{}", out.trim()),
        Err(()) => {
            println!("NOT FOUND");
            eprintln!("  ❌ Install Rust via https://rustup.rs/");
            return Err(());
        }
    }

    print!("  Cargo version:  ");
    match cmd("cargo", &["--version"]) {
        Ok(out) => println!("{}", out.trim()),
        Err(()) => {
            println!("NOT FOUND");
            return Err(());
        }
    }

    // Check compilation (default members only — no Python required)
    print!("  Compilation:    ");
    match cargo_quiet(&["check"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => {
            println!("❌ FAILED");
            eprintln!("  Run `cargo check` for details.");
            return Err(());
        }
    }

    // Check full workspace compilation (optional, requires Python dev headers)
    print!("  Full workspace: ");
    match cargo_quiet(&["check", "--all-features", "--workspace"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => println!("⚠️  FAILED (optional — install python3-dev for full workspace)"),
    }

    // Check formatting
    print!("  Formatting:     ");
    match cargo_quiet(&["fmt", "--all", "--", "--check"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => println!("⚠️  Run `cargo xtask fmt` to fix"),
    }

    // Quick test
    print!("  Core tests:     ");
    match cargo_quiet(&["test", "--package", "isolate-core", "-q"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => {
            println!("❌ FAILED");
            eprintln!("  Run `cargo test --package isolate-core` for details.");
            return Err(());
        }
    }

    // Check optional tools
    println!();
    println!("  Optional tools:");
    print!("    just:         ");
    if which("just") {
        println!("✅ installed");
    } else {
        println!("— not installed (optional: cargo install just)");
    }
    print!("    cargo-audit:  ");
    if which("cargo-audit") {
        println!("✅ installed");
    } else {
        println!("— not installed (optional: cargo install cargo-audit)");
    }
    print!("    protoc:       ");
    if which("protoc") {
        println!("✅ installed");
    } else {
        println!("— not installed (required for isolate-server: brew install protobuf)");
    }
    print!("    cargo-watch:  ");
    if which("cargo-watch") {
        println!("✅ installed");
    } else {
        println!("— not installed (optional: cargo install cargo-watch)");
    }

    // Check and install pre-commit hooks
    println!();
    print!("  Pre-commit hook: ");
    let hook_path = std::path::Path::new(".git/hooks/pre-commit");
    if hook_path.exists() {
        println!("✅ installed");
    } else {
        println!("— not installed, installing...");
        if run_install_hooks().is_ok() {
            println!("    ✅ Hook installed successfully");
        } else {
            println!("    ⚠️  Failed to install hook (run `cargo xtask install-hooks` manually)");
        }
    }

    println!("\n🏁 Environment check complete!");
    Ok(())
}

fn run_docs() -> Result<(), ()> {
    println!("📚 Generating documentation...");
    cargo(&["doc", "--no-deps"])
}

fn run_build() -> Result<(), ()> {
    println!("🔨 Building crates...");
    cargo(&["build"])
}

fn run_install_hooks() -> Result<(), ()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    println!("🪝 Installing git pre-commit hook...");

    let hook_dir = std::path::Path::new(".git/hooks");
    if !hook_dir.exists() {
        eprintln!("Error: .git/hooks directory not found. Are you in a git repository?");
        return Err(());
    }

    let hook_path = hook_dir.join("pre-commit");
    let hook_script = r#"#!/usr/bin/env sh
# Installed by: cargo xtask install-hooks
set -e

echo "Running pre-commit checks..."
cargo xtask fmt-check
cargo xtask lint
echo "Pre-commit checks passed!"
"#;

    fs::write(&hook_path, hook_script)
        .map_err(|e| eprintln!("Failed to write hook: {e}"))?;
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755))
        .map_err(|e| eprintln!("Failed to set hook permissions: {e}"))?;

    println!("✅ Pre-commit hook installed at {}", hook_path.display());
    Ok(())
}

fn run_new_module(name: Option<&str>, feature: Option<&str>) -> Result<(), ()> {
    use std::fs;

    let name = match name {
        Some(n) => n,
        None => {
            eprintln!("Usage: cargo xtask new-module <name> [feature]");
            eprintln!("  name     Module name (snake_case, e.g. my_module)");
            eprintln!("  feature  Feature flag to gate the module (e.g. extras)");
            return Err(());
        }
    };

    // Validate module name
    if !name.chars().all(|c| c.is_ascii_lowercase() || c == '_') || name.is_empty() {
        eprintln!("Error: module name must be non-empty snake_case (lowercase + underscores).");
        return Err(());
    }

    let core_src = std::path::Path::new("isolate-core/src");
    let mod_dir = core_src.join(name);

    if mod_dir.exists() {
        eprintln!("Error: directory {} already exists.", mod_dir.display());
        return Err(());
    }

    println!("📦 Scaffolding module '{name}'...");

    // Create module directory
    fs::create_dir_all(&mod_dir)
        .map_err(|e| eprintln!("Failed to create directory: {e}"))?;

    // Write mod.rs
    let mod_rs = format!(
        r#"//! `{name}` module.
//!
//! This module provides functionality for the `{name}` feature.
//! Update this documentation with specific details about the module's
//! purpose, types, and usage examples.

#![allow(missing_docs)]

/// Returns the module identifier.
///
/// Replace this placeholder with the actual module functionality.
pub fn hello() -> &'static str {{
    "{name} module"
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn test_hello() {{
        assert_eq!(hello(), "{name} module");
    }}
}}
"#
    );
    fs::write(mod_dir.join("mod.rs"), mod_rs)
        .map_err(|e| eprintln!("Failed to write mod.rs: {e}"))?;

    println!("  ✅ Created {}/mod.rs", mod_dir.display());

    // Print instructions for manual steps
    println!();
    if let Some(feat) = feature {
        println!("  Next steps:");
        println!("  1. Add to isolate-core/src/lib.rs:");
        println!("     #[cfg(feature = \"{feat}\")]");
        println!("     #[allow(missing_docs)]");
        println!("     pub mod {name};");
        println!();
        println!("  2. If '{feat}' is a new feature, add to isolate-core/Cargo.toml:");
        println!("     [features]");
        println!("     {feat} = []");
    } else {
        println!("  Next steps:");
        println!("  1. Add to isolate-core/src/lib.rs:");
        println!("     pub mod {name};");
    }

    println!();
    println!("✅ Module '{name}' scaffolded successfully!");
    Ok(())
}

fn run_bump(version: Option<&str>) -> Result<(), ()> {
    use std::fs;

    let version = match version {
        Some(v) => v,
        None => {
            eprintln!("Usage: cargo xtask bump <VERSION>");
            eprintln!("  VERSION  Semantic version (e.g., 0.2.0)");
            return Err(());
        }
    };

    // Validate version format (basic semver: MAJOR.MINOR.PATCH with optional pre-release)
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 3
        || parts.iter().take(3).any(|p| {
            p.split('-')
                .next()
                .map_or(true, |n| n.parse::<u32>().is_err())
        })
    {
        eprintln!("Error: invalid version '{version}'. Expected semver format (e.g., 0.2.0)");
        return Err(());
    }

    let cargo_toml_path = "Cargo.toml";
    println!("📦 Bumping workspace version to {version}...\n");

    // Read root Cargo.toml
    let content = fs::read_to_string(cargo_toml_path)
        .map_err(|e| eprintln!("Failed to read {cargo_toml_path}: {e}"))?;

    // Replace version in [workspace.package]
    let mut found = false;
    let mut new_content = String::with_capacity(content.len());
    let mut in_workspace_package = false;

    for line in content.lines() {
        if line.trim() == "[workspace.package]" {
            in_workspace_package = true;
        } else if line.starts_with('[') && in_workspace_package {
            in_workspace_package = false;
        }

        if in_workspace_package && line.starts_with("version") {
            new_content.push_str(&format!("version = \"{version}\""));
            found = true;
        } else {
            new_content.push_str(line);
        }
        new_content.push('\n');
    }

    if !found {
        eprintln!("Error: could not find version in [workspace.package] in {cargo_toml_path}");
        return Err(());
    }

    fs::write(cargo_toml_path, &new_content)
        .map_err(|e| eprintln!("Failed to write {cargo_toml_path}: {e}"))?;
    println!("  ✅ Updated {cargo_toml_path}");

    // Verify workspace inherits correctly
    print!("  Verifying workspace inheritance... ");
    match cargo_quiet(&["check", "--workspace"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => {
            println!("❌ FAILED");
            eprintln!("  cargo check failed after version bump. Please check Cargo.toml files.");
            return Err(());
        }
    }

    println!("\n✅ Version bumped to {version}");
    println!("  Next steps:");
    println!("  1. Update CHANGELOG.md");
    println!("  2. Commit: git commit -am \"chore: bump version to {version}\"");
    println!("  3. Tag: git tag v{version}");
    Ok(())
}

// --- Helpers ---

fn cargo(args: &[&str]) -> Result<(), ()> {
    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|e| eprintln!("Failed to run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(())
    }
}

fn cargo_quiet(args: &[&str]) -> Result<(), ()> {
    let status = Command::new("cargo")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| eprintln!("Failed to run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(())
    }
}

fn cmd(program: &str, args: &[&str]) -> Result<String, ()> {
    let output = Command::new(program).args(args).output().map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(())
    }
}

fn which(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
