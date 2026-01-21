//! Crate version information.

/// Returns the current version of isolate-core.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns version information as a formatted string.
pub fn version_info() -> String {
    format!("isolate-core v{}", version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_not_empty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn test_version_info_format() {
        let info = version_info();
        assert!(info.starts_with("isolate-core v"));
    }
}
