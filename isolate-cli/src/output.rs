use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub const BANNER: &str = r#"
  ___          _       _
 |_ _|___  ___| | __ _| |_ ___
  | |/ __|/ _ \ |/ _` | __/ _ \
  | |\__ \ (_) | | (_| | ||  __/
 |___|___/\___/|_|\__,_|\__\___|
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Pretty,
}

pub fn parse_size(s: &str) -> Result<usize> {
    use anyhow::Context;
    let s = s.trim().to_uppercase();
    let (num, multiplier) = if s.ends_with('G') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('K') {
        (&s[..s.len() - 1], 1024)
    } else {
        (s.as_str(), 1)
    };
    let num: usize = num.parse().context("Invalid size number")?;
    Ok(num * multiplier)
}

pub fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms >= 1000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else if ms >= 1.0 {
        format!("{:.2}ms", ms)
    } else {
        format!("{:.2}µs", ms * 1000.0)
    }
}

pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

pub fn print_banner() {
    println!("{}", BANNER.cyan().bold());
    println!("  {}  v{}\n", "Secure Sandbox Runtime".dimmed(), env!("CARGO_PKG_VERSION"));
}

pub fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}").unwrap().tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}
