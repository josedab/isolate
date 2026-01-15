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
    check        Run all checks: format, lint, and test
    test         Run all tests with --all-features
    test-core    Run core tests only (faster)
    fmt          Format all code
    fmt-check    Check formatting without modifying files
    lint         Run clippy with -D warnings
    pre-commit   Full pre-push validation (fmt + lint + test)
    doctor       Verify development environment is set up correctly
    docs         Generate documentation
    build        Build all crates
    help         Show this help message

EXAMPLES:
    cargo xtask check       # Quick check before pushing
    cargo xtask doctor      # First-time setup verification
    cargo xtask test-core   # Fast feedback during development"
    );
}

// --- Task Implementations ---

fn run_check() -> Result<(), ()> {
    println!("🔍 Running all checks...\n");
    run_fmt(true)?;
    println!();
    run_lint()?;
    println!();
    run_test()?;
    println!("\n✅ All checks passed!");
    Ok(())
}

fn run_test() -> Result<(), ()> {
    println!("🧪 Running all tests...");
    cargo(&["test", "--all-features", "--workspace"])
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
    cargo(&["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
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

    // Check compilation
    print!("  Compilation:    ");
    match cargo_quiet(&["check", "--all-features", "--workspace"]) {
        Ok(()) => println!("✅ OK"),
        Err(()) => {
            println!("❌ FAILED");
            eprintln!("  Run `cargo check --all-features` for details.");
            return Err(());
        }
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

    println!("\n🏁 Environment check complete!");
    Ok(())
}

fn run_docs() -> Result<(), ()> {
    println!("📚 Generating documentation...");
    cargo(&["doc", "--no-deps", "--all-features", "--workspace"])
}

fn run_build() -> Result<(), ()> {
    println!("🔨 Building all crates...");
    cargo(&["build", "--all-features", "--workspace"])
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
