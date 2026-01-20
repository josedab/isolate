use serde::{Deserialize, Serialize};
use std::fmt;

/// Error type for policy parsing failures.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parse error at {}:{}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A complete policy document containing one or more sandbox policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDocument {
    pub policies: Vec<SandboxPolicy>,
}

/// A single sandbox policy definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub name: String,
    pub resource: Option<ResourceBlock>,
    pub capability: Option<CapabilityBlock>,
    pub network: Option<NetworkBlock>,
}

/// Resource limits block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceBlock {
    pub memory_limit: Option<String>,
    pub fuel: Option<u64>,
    pub timeout: Option<String>,
    pub max_io_bytes: Option<u64>,
}

/// Capability permissions block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityBlock {
    pub allow_stdout: Option<bool>,
    pub allow_stderr: Option<bool>,
    pub allow_stdin: Option<bool>,
    pub allow_env: Option<bool>,
    pub allow_clock: Option<bool>,
    pub allow_random: Option<bool>,
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
}

/// Network policy block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkBlock {
    pub allow_dns: Option<bool>,
    pub allow_http: Vec<String>,
    pub allow_tcp: Vec<String>,
    pub deny_all: Option<bool>,
}

/// Parser for the policy DSL.
pub struct PolicyParser;

impl PolicyParser {
    /// Parse a policy document from the DSL string.
    pub fn parse(input: &str) -> Result<PolicyDocument, ParseError> {
        let mut policies = Vec::new();
        let mut chars = input.chars().peekable();
        let mut line = 1usize;
        let mut col = 1usize;

        loop {
            skip_whitespace_and_comments(&mut chars, &mut line, &mut col);
            if chars.peek().is_none() {
                break;
            }

            let token = read_identifier(&mut chars, &mut col);
            if token.is_empty() {
                // Skip unexpected characters
                chars.next();
                col += 1;
                continue;
            }

            if token == "sandbox" {
                let policy = parse_sandbox_policy(&mut chars, &mut line, &mut col)?;
                policies.push(policy);
            } else {
                return Err(ParseError {
                    message: format!("expected 'sandbox', found '{token}'"),
                    line,
                    col,
                });
            }
        }

        Ok(PolicyDocument { policies })
    }
}

fn skip_whitespace_and_comments(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) {
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' => {
                chars.next();
                *col += 1;
            }
            '\n' => {
                chars.next();
                *line += 1;
                *col = 1;
            }
            '#' => {
                // Skip comment line
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' {
                        *line += 1;
                        *col = 1;
                        break;
                    }
                }
            }
            _ => break,
        }
    }
}

fn read_identifier(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    col: &mut usize,
) -> String {
    let mut ident = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            ident.push(c);
            chars.next();
            *col += 1;
        } else {
            break;
        }
    }
    ident
}

fn read_string(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &usize,
    col: &mut usize,
) -> Result<String, ParseError> {
    // Consume opening quote
    match chars.next() {
        Some('"') => *col += 1,
        _ => {
            return Err(ParseError {
                message: "expected '\"'".into(),
                line: *line,
                col: *col,
            })
        }
    }

    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') => {
                *col += 1;
                return Ok(s);
            }
            Some(c) => {
                s.push(c);
                *col += 1;
            }
            None => {
                return Err(ParseError {
                    message: "unterminated string".into(),
                    line: *line,
                    col: *col,
                })
            }
        }
    }
}

fn read_number(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    col: &mut usize,
) -> u64 {
    let mut n = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '_' {
            if c != '_' {
                n.push(c);
            }
            chars.next();
            *col += 1;
        } else {
            break;
        }
    }
    n.parse::<u64>().unwrap_or(0)
}

fn expect_char(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    expected: char,
    line: &usize,
    col: &mut usize,
) -> Result<(), ParseError> {
    match chars.next() {
        Some(c) if c == expected => {
            *col += 1;
            Ok(())
        }
        Some(c) => Err(ParseError {
            message: format!("expected '{expected}', found '{c}'"),
            line: *line,
            col: *col,
        }),
        None => Err(ParseError {
            message: format!("expected '{expected}', found EOF"),
            line: *line,
            col: *col,
        }),
    }
}

fn read_string_list(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) -> Result<Vec<String>, ParseError> {
    expect_char(chars, '[', line, col)?;
    let mut items = Vec::new();
    loop {
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&']') {
            chars.next();
            *col += 1;
            return Ok(items);
        }
        let s = read_string(chars, line, col)?;
        items.push(s);
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&',') {
            chars.next();
            *col += 1;
        }
    }
}

fn read_bool(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    col: &mut usize,
) -> bool {
    let ident = read_identifier(chars, col);
    ident == "true"
}

fn parse_sandbox_policy(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) -> Result<SandboxPolicy, ParseError> {
    skip_whitespace_and_comments(chars, line, col);
    let name = read_string(chars, line, col)?;
    skip_whitespace_and_comments(chars, line, col);
    expect_char(chars, '{', line, col)?;

    let mut policy = SandboxPolicy {
        name,
        resource: None,
        capability: None,
        network: None,
    };

    loop {
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&'}') {
            chars.next();
            *col += 1;
            return Ok(policy);
        }

        let block_name = read_identifier(chars, col);
        skip_whitespace_and_comments(chars, line, col);
        expect_char(chars, '{', line, col)?;

        match block_name.as_str() {
            "resource" => policy.resource = Some(parse_resource_block(chars, line, col)?),
            "capability" => policy.capability = Some(parse_capability_block(chars, line, col)?),
            "network" => policy.network = Some(parse_network_block(chars, line, col)?),
            _ => {
                return Err(ParseError {
                    message: format!("unknown block '{block_name}'"),
                    line: *line,
                    col: *col,
                })
            }
        }
    }
}

fn parse_resource_block(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) -> Result<ResourceBlock, ParseError> {
    let mut block = ResourceBlock::default();
    loop {
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&'}') {
            chars.next();
            *col += 1;
            return Ok(block);
        }
        let key = read_identifier(chars, col);
        skip_whitespace_and_comments(chars, line, col);
        expect_char(chars, '=', line, col)?;
        skip_whitespace_and_comments(chars, line, col);

        match key.as_str() {
            "memory_limit" | "memory-limit" => block.memory_limit = Some(read_string(chars, line, col)?),
            "fuel" => block.fuel = Some(read_number(chars, col)),
            "timeout" => block.timeout = Some(read_string(chars, line, col)?),
            "max_io_bytes" | "max-io-bytes" => block.max_io_bytes = Some(read_number(chars, col)),
            _ => {
                return Err(ParseError {
                    message: format!("unknown resource key '{key}'"),
                    line: *line,
                    col: *col,
                })
            }
        }
    }
}

fn parse_capability_block(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) -> Result<CapabilityBlock, ParseError> {
    let mut block = CapabilityBlock::default();
    loop {
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&'}') {
            chars.next();
            *col += 1;
            return Ok(block);
        }
        let key = read_identifier(chars, col);
        skip_whitespace_and_comments(chars, line, col);
        expect_char(chars, '=', line, col)?;
        skip_whitespace_and_comments(chars, line, col);

        match key.as_str() {
            "allow_stdout" | "allow-stdout" => block.allow_stdout = Some(read_bool(chars, col)),
            "allow_stderr" | "allow-stderr" => block.allow_stderr = Some(read_bool(chars, col)),
            "allow_stdin" | "allow-stdin" => block.allow_stdin = Some(read_bool(chars, col)),
            "allow_env" | "allow-env" => block.allow_env = Some(read_bool(chars, col)),
            "allow_clock" | "allow-clock" => block.allow_clock = Some(read_bool(chars, col)),
            "allow_random" | "allow-random" => block.allow_random = Some(read_bool(chars, col)),
            "fs_read" | "fs-read" => block.fs_read = read_string_list(chars, line, col)?,
            "fs_write" | "fs-write" => block.fs_write = read_string_list(chars, line, col)?,
            _ => {
                return Err(ParseError {
                    message: format!("unknown capability key '{key}'"),
                    line: *line,
                    col: *col,
                })
            }
        }
    }
}

fn parse_network_block(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    line: &mut usize,
    col: &mut usize,
) -> Result<NetworkBlock, ParseError> {
    let mut block = NetworkBlock::default();
    loop {
        skip_whitespace_and_comments(chars, line, col);
        if chars.peek() == Some(&'}') {
            chars.next();
            *col += 1;
            return Ok(block);
        }
        let key = read_identifier(chars, col);
        skip_whitespace_and_comments(chars, line, col);
        expect_char(chars, '=', line, col)?;
        skip_whitespace_and_comments(chars, line, col);

        match key.as_str() {
            "allow_dns" | "allow-dns" => block.allow_dns = Some(read_bool(chars, col)),
            "allow_http" | "allow-http" => block.allow_http = read_string_list(chars, line, col)?,
            "allow_tcp" | "allow-tcp" => block.allow_tcp = read_string_list(chars, line, col)?,
            "deny_all" | "deny-all" => block.deny_all = Some(read_bool(chars, col)),
            _ => {
                return Err(ParseError {
                    message: format!("unknown network key '{key}'"),
                    line: *line,
                    col: *col,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let input = r#"sandbox "test" {}"#;
        let doc = PolicyParser::parse(input).unwrap();
        assert_eq!(doc.policies.len(), 1);
        assert_eq!(doc.policies[0].name, "test");
    }

    #[test]
    fn test_parse_resource_block() {
        let input = r#"
            sandbox "svc" {
                resource {
                    memory_limit = "128MB"
                    fuel = 1000000
                    timeout = "30s"
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let res = doc.policies[0].resource.as_ref().unwrap();
        assert_eq!(res.memory_limit.as_deref(), Some("128MB"));
        assert_eq!(res.fuel, Some(1_000_000));
        assert_eq!(res.timeout.as_deref(), Some("30s"));
    }

    #[test]
    fn test_parse_capability_block() {
        let input = r#"
            sandbox "app" {
                capability {
                    allow_stdout = true
                    allow_stderr = false
                    fs_read = ["/data", "/config"]
                    fs_write = ["/tmp"]
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let cap = doc.policies[0].capability.as_ref().unwrap();
        assert_eq!(cap.allow_stdout, Some(true));
        assert_eq!(cap.allow_stderr, Some(false));
        assert_eq!(cap.fs_read, vec!["/data", "/config"]);
        assert_eq!(cap.fs_write, vec!["/tmp"]);
    }

    #[test]
    fn test_parse_network_block() {
        let input = r#"
            sandbox "net" {
                network {
                    allow_dns = true
                    allow_http = ["api.example.com", "cdn.example.com"]
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let net = doc.policies[0].network.as_ref().unwrap();
        assert_eq!(net.allow_dns, Some(true));
        assert_eq!(net.allow_http.len(), 2);
    }

    #[test]
    fn test_parse_multiple_policies() {
        let input = r#"
            sandbox "a" {}
            sandbox "b" {}
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        assert_eq!(doc.policies.len(), 2);
    }

    #[test]
    fn test_parse_with_comments() {
        let input = r#"
            # This is a policy for the API handler
            sandbox "api" {
                resource {
                    # 256MB memory limit
                    memory_limit = "256MB"
                    fuel = 500000
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        assert_eq!(doc.policies[0].name, "api");
    }

    #[test]
    fn test_parse_error_unknown_block() {
        let input = r#"sandbox "x" { unknown { } }"#;
        let result = PolicyParser::parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_display() {
        let err = ParseError {
            message: "test error".into(),
            line: 5,
            col: 10,
        };
        assert_eq!(format!("{err}"), "parse error at 5:10: test error");
    }

    #[test]
    fn test_parse_full_policy() {
        let input = r#"
            sandbox "complete" {
                resource {
                    memory_limit = "512MB"
                    fuel = 5000000
                    timeout = "120s"
                    max_io_bytes = 10485760
                }
                capability {
                    allow_stdout = true
                    allow_stderr = true
                    allow_stdin = false
                    allow_env = true
                    allow_clock = true
                    allow_random = true
                    fs_read = ["/data", "/config", "/certs"]
                    fs_write = ["/tmp", "/output"]
                }
                network {
                    allow_dns = true
                    allow_http = ["api.internal.com"]
                    allow_tcp = ["db.internal.com:5432"]
                    deny_all = false
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let p = &doc.policies[0];
        assert_eq!(p.name, "complete");
        assert!(p.resource.is_some());
        assert!(p.capability.is_some());
        assert!(p.network.is_some());
    }
}
