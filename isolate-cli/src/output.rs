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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_megabytes() {
        assert_eq!(parse_size("10M").unwrap(), 10 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_gigabytes() {
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_kilobytes() {
        assert_eq!(parse_size("512K").unwrap(), 512 * 1024);
    }

    #[test]
    fn test_parse_size_bytes_no_suffix() {
        assert_eq!(parse_size("4096").unwrap(), 4096);
    }

    #[test]
    fn test_parse_size_with_whitespace() {
        assert_eq!(parse_size("  10M  ").unwrap(), 10 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_lowercase() {
        assert_eq!(parse_size("10m").unwrap(), 10 * 1024 * 1024);
    }

    #[test]
    fn test_parse_size_empty_returns_error() {
        assert!(parse_size("").is_err());
    }

    #[test]
    fn test_parse_size_non_numeric_returns_error() {
        assert!(parse_size("abcM").is_err());
    }

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 bytes");
    }

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(512), "512 bytes");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        let result = format_bytes(2048);
        assert!(result.contains("KB"));
    }

    #[test]
    fn test_format_bytes_megabytes() {
        let result = format_bytes(10 * 1024 * 1024);
        assert!(result.contains("MB"));
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        let result = format_bytes(2 * 1024 * 1024 * 1024);
        assert!(result.contains("GB"));
    }

    #[test]
    fn test_format_duration_microseconds() {
        let d = Duration::from_nanos(500_000); // 0.5ms → 500µs
        let result = format_duration(d);
        assert!(result.contains("µs"));
    }

    #[test]
    fn test_format_duration_milliseconds() {
        let d = Duration::from_millis(42);
        let result = format_duration(d);
        assert!(result.contains("ms"));
    }

    #[test]
    fn test_format_duration_seconds() {
        let d = Duration::from_secs(2);
        let result = format_duration(d);
        assert!(result.contains("s"));
        assert!(!result.contains("ms"));
    }

    #[test]
    fn test_format_number_small() {
        assert_eq!(format_number(42), "42");
    }

    #[test]
    fn test_format_number_thousands() {
        assert_eq!(format_number(1_000), "1,000");
    }

    #[test]
    fn test_format_number_millions() {
        assert_eq!(format_number(1_000_000), "1,000,000");
    }

    #[test]
    fn test_format_number_zero() {
        assert_eq!(format_number(0), "0");
    }

    #[test]
    fn test_format_number_max() {
        let result = format_number(u64::MAX);
        assert!(result.contains(","));
        // u64::MAX = 18,446,744,073,709,551,615
        assert!(result.starts_with("18,446,744,073,709,551,615"));
    }
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
