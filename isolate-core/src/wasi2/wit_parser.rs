//! WIT text parser for reading `.wit` interface definition files.
//!
//! Parses a subset of the WIT format sufficient for Isolate's component
//! composition workflows. Supports packages, interfaces, records, enums,
//! variants, flags, and function definitions.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::wasi2::wit_parser::WitParser;
//!
//! let wit_text = r#"
//!     package myapp:api;
//!     interface greeter {
//!         greet: func(name: string) -> string;
//!     }
//! "#;
//!
//! let parser = WitParser::new();
//! let doc = parser.parse(wit_text).unwrap();
//! assert_eq!(doc.interfaces.len(), 1);
//! ```

use super::wit::{WitFunction, WitInterface, WitType, WitTypeKind};
use std::collections::HashMap;

/// Parsed WIT document containing a package declaration and interfaces.
#[derive(Debug, Clone)]
pub struct WitDocument {
    /// Package name (e.g., "myapp:api").
    pub package: String,
    /// Parsed interfaces.
    pub interfaces: Vec<WitInterface>,
}

/// Error during WIT parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum WitParseError {
    #[error("Expected {expected} at line {line}, got: {actual}")]
    Unexpected { expected: String, actual: String, line: usize },
    #[error("Unterminated block starting at line {line}")]
    UnterminatedBlock { line: usize },
    #[error("Missing package declaration")]
    MissingPackage,
    #[error("Invalid syntax at line {line}: {message}")]
    InvalidSyntax { line: usize, message: String },
}

/// Parser for WIT text format.
pub struct WitParser {
    _strict: bool,
}

impl Default for WitParser {
    fn default() -> Self {
        Self::new()
    }
}

impl WitParser {
    /// Create a new WIT parser.
    pub fn new() -> Self {
        Self { _strict: false }
    }

    /// Parse a WIT text document into a structured representation.
    pub fn parse(&self, input: &str) -> Result<WitDocument, WitParseError> {
        let lines: Vec<&str> = input.lines().collect();
        let mut package = None;
        let mut interfaces = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed.is_empty() || trimmed.starts_with("//") {
                i += 1;
                continue;
            }

            if trimmed.starts_with("package ") {
                let pkg =
                    trimmed.trim_start_matches("package ").trim_end_matches(';').trim().to_string();
                package = Some(pkg);
                i += 1;
                continue;
            }

            if trimmed.starts_with("interface ") {
                let (iface, end) = self.parse_interface(&lines, i, package.as_deref())?;
                interfaces.push(iface);
                i = end + 1;
                continue;
            }

            i += 1;
        }

        let package = package.unwrap_or_else(|| "unknown:package".to_string());
        Ok(WitDocument { package, interfaces })
    }

    fn parse_interface(
        &self,
        lines: &[&str],
        start: usize,
        package: Option<&str>,
    ) -> Result<(WitInterface, usize), WitParseError> {
        let header = lines[start].trim();
        let name = header.trim_start_matches("interface ").trim_end_matches('{').trim().to_string();

        let pkg = package.unwrap_or("unknown:package").to_string();

        let mut iface = WitInterface::new(&name, format!("{}:{}", pkg, name));

        let mut i = start + 1;
        let mut docs: Vec<String> = Vec::new();

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed == "}" {
                return Ok((iface, i));
            }

            if trimmed.is_empty() {
                i += 1;
                continue;
            }

            if trimmed.starts_with("///") {
                docs.push(trimmed.trim_start_matches("///").trim().to_string());
                i += 1;
                continue;
            }

            if trimmed.starts_with("//") {
                i += 1;
                continue;
            }

            // Type definitions
            if trimmed.starts_with("record ") {
                let (ty, end) = self.parse_record(lines, i, &docs)?;
                iface.types.push(ty);
                i = end + 1;
                docs.clear();
                continue;
            }
            if trimmed.starts_with("enum ") {
                let (ty, end) = self.parse_enum(lines, i, &docs)?;
                iface.types.push(ty);
                i = end + 1;
                docs.clear();
                continue;
            }
            if trimmed.starts_with("variant ") {
                let (ty, end) = self.parse_variant(lines, i, &docs)?;
                iface.types.push(ty);
                i = end + 1;
                docs.clear();
                continue;
            }
            if trimmed.starts_with("flags ") {
                let (ty, end) = self.parse_flags(lines, i, &docs)?;
                iface.types.push(ty);
                i = end + 1;
                docs.clear();
                continue;
            }
            if trimmed.starts_with("type ") {
                let ty = self.parse_alias(trimmed, &docs)?;
                iface.types.push(ty);
                i += 1;
                docs.clear();
                continue;
            }

            // Function definitions (name: func(...) -> ...)
            if trimmed.contains(": func(") {
                let func = self.parse_function(trimmed, &docs)?;
                iface.functions.push(func);
                i += 1;
                docs.clear();
                continue;
            }

            i += 1;
            docs.clear();
        }

        Err(WitParseError::UnterminatedBlock { line: start + 1 })
    }

    fn parse_record(
        &self,
        lines: &[&str],
        start: usize,
        docs: &[String],
    ) -> Result<(WitType, usize), WitParseError> {
        let header = lines[start].trim();
        let name = header.trim_start_matches("record ").trim_end_matches('{').trim().to_string();

        let mut fields: Vec<(String, String)> = Vec::new();
        let mut i = start + 1;

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed == "}" {
                let mut ty = WitType::new(name, WitTypeKind::Record(fields));
                if !docs.is_empty() {
                    ty = ty.with_docs(docs.join("\n"));
                }
                return Ok((ty, i));
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                let field = trimmed.trim_end_matches(',');
                if let Some((fname, ftype)) = field.split_once(':') {
                    fields.push((fname.trim().to_string(), ftype.trim().to_string()));
                }
            }
            i += 1;
        }

        Err(WitParseError::UnterminatedBlock { line: start + 1 })
    }

    fn parse_enum(
        &self,
        lines: &[&str],
        start: usize,
        docs: &[String],
    ) -> Result<(WitType, usize), WitParseError> {
        let header = lines[start].trim();
        let name = header.trim_start_matches("enum ").trim_end_matches('{').trim().to_string();

        let mut values: Vec<String> = Vec::new();
        let mut i = start + 1;

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed == "}" {
                let mut ty = WitType::new(name, WitTypeKind::Enum(values));
                if !docs.is_empty() {
                    ty = ty.with_docs(docs.join("\n"));
                }
                return Ok((ty, i));
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                let val = trimmed.trim_end_matches(',').trim().to_string();
                if !val.is_empty() {
                    values.push(val);
                }
            }
            i += 1;
        }

        Err(WitParseError::UnterminatedBlock { line: start + 1 })
    }

    fn parse_variant(
        &self,
        lines: &[&str],
        start: usize,
        docs: &[String],
    ) -> Result<(WitType, usize), WitParseError> {
        let header = lines[start].trim();
        let name = header.trim_start_matches("variant ").trim_end_matches('{').trim().to_string();

        let mut cases: Vec<(String, Option<String>)> = Vec::new();
        let mut i = start + 1;

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed == "}" {
                let mut ty = WitType::new(name, WitTypeKind::Variant(cases));
                if !docs.is_empty() {
                    ty = ty.with_docs(docs.join("\n"));
                }
                return Ok((ty, i));
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                let case = trimmed.trim_end_matches(',');
                if let Some(paren_start) = case.find('(') {
                    let case_name = case[..paren_start].trim().to_string();
                    let payload = case[paren_start + 1..].trim_end_matches(')').trim().to_string();
                    cases.push((case_name, Some(payload)));
                } else {
                    cases.push((case.trim().to_string(), None));
                }
            }
            i += 1;
        }

        Err(WitParseError::UnterminatedBlock { line: start + 1 })
    }

    fn parse_flags(
        &self,
        lines: &[&str],
        start: usize,
        docs: &[String],
    ) -> Result<(WitType, usize), WitParseError> {
        let header = lines[start].trim();
        let name = header.trim_start_matches("flags ").trim_end_matches('{').trim().to_string();

        let mut flags: Vec<String> = Vec::new();
        let mut i = start + 1;

        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed == "}" {
                let mut ty = WitType::new(name, WitTypeKind::Flags(flags));
                if !docs.is_empty() {
                    ty = ty.with_docs(docs.join("\n"));
                }
                return Ok((ty, i));
            }
            if !trimmed.is_empty() && !trimmed.starts_with("//") {
                let flag = trimmed.trim_end_matches(',').trim().to_string();
                if !flag.is_empty() {
                    flags.push(flag);
                }
            }
            i += 1;
        }

        Err(WitParseError::UnterminatedBlock { line: start + 1 })
    }

    fn parse_alias(&self, line: &str, docs: &[String]) -> Result<WitType, WitParseError> {
        // type foo = bar;
        let body = line.trim_start_matches("type ").trim_end_matches(';');
        if let Some((name, target)) = body.split_once('=') {
            let mut ty = WitType::new(name.trim(), WitTypeKind::Alias(target.trim().to_string()));
            if !docs.is_empty() {
                ty = ty.with_docs(docs.join("\n"));
            }
            Ok(ty)
        } else {
            Err(WitParseError::InvalidSyntax {
                line: 0,
                message: format!("Invalid type alias: {}", line),
            })
        }
    }

    fn parse_function(&self, line: &str, docs: &[String]) -> Result<WitFunction, WitParseError> {
        // name: func(param1: type1, param2: type2) -> result;
        let (name, rest) = line.split_once(':').ok_or_else(|| WitParseError::InvalidSyntax {
            line: 0,
            message: format!("Invalid function: {}", line),
        })?;

        let name = name.trim().to_string();
        let rest = rest.trim().trim_start_matches("func").trim_end_matches(';').trim();

        let mut func = WitFunction::new(&name);
        if !docs.is_empty() {
            func = func.with_docs(docs.join("\n"));
        }

        // Parse parameters
        if let Some(paren_start) = rest.find('(') {
            if let Some(paren_end) = rest.find(')') {
                let params_str = &rest[paren_start + 1..paren_end];
                if !params_str.trim().is_empty() {
                    for param in split_params(params_str) {
                        let param = param.trim();
                        if let Some((pname, ptype)) = param.split_once(':') {
                            func = func.with_param(pname.trim(), ptype.trim());
                        }
                    }
                }

                // Parse return type
                let after_params = &rest[paren_end + 1..];
                if let Some(arrow) = after_params.find("->") {
                    let result_type = after_params[arrow + 2..].trim();
                    if !result_type.is_empty() {
                        func = func.with_result(result_type);
                    }
                }
            }
        }

        Ok(func)
    }
}

/// Split parameter list respecting nested angle brackets and parentheses.
fn split_params(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                result.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    if start < s.len() {
        result.push(&s[start..]);
    }
    result
}

/// Parse multiple WIT documents from a map of filename → content.
pub fn parse_wit_bundle(
    files: &HashMap<String, String>,
) -> Result<Vec<WitDocument>, WitParseError> {
    let parser = WitParser::new();
    let mut docs = Vec::new();
    for (_name, content) in files {
        docs.push(parser.parse(content)?);
    }
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_interface() {
        let input = r#"
            package myapp:api;

            interface greeter {
                greet: func(name: string) -> string;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        assert_eq!(doc.package, "myapp:api");
        assert_eq!(doc.interfaces.len(), 1);
        assert_eq!(doc.interfaces[0].name, "greeter");
        assert_eq!(doc.interfaces[0].functions.len(), 1);
        assert_eq!(doc.interfaces[0].functions[0].name, "greet");
        assert_eq!(doc.interfaces[0].functions[0].params.len(), 1);
        assert_eq!(doc.interfaces[0].functions[0].results, Some("string".to_string()));
    }

    #[test]
    fn test_parse_interface_with_record() {
        let input = r#"
            package test:types;

            interface data {
                record point {
                    x: f64,
                    y: f64,
                }

                distance: func(a: point, b: point) -> f64;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        assert_eq!(doc.interfaces[0].types.len(), 1);
        assert_eq!(doc.interfaces[0].types[0].name, "point");
        let WitTypeKind::Record(fields) = &doc.interfaces[0].types[0].kind else {
            unreachable!("Expected record type");
        };
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].0, "x");
        assert_eq!(fields[0].1, "f64");
    }

    #[test]
    fn test_parse_interface_with_enum() {
        let input = r#"
            package test:types;

            interface shapes {
                enum color {
                    red,
                    green,
                    blue,
                }
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        assert_eq!(doc.interfaces[0].types.len(), 1);
        let WitTypeKind::Enum(values) = &doc.interfaces[0].types[0].kind else {
            unreachable!("Expected enum type");
        };
        assert_eq!(values, &["red", "green", "blue"]);
    }

    #[test]
    fn test_parse_interface_with_variant() {
        let input = r#"
            package test:types;

            interface results {
                variant result-val {
                    ok(string),
                    err(u32),
                    none,
                }
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let WitTypeKind::Variant(cases) = &doc.interfaces[0].types[0].kind else {
            unreachable!("Expected variant type");
        };
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0].0, "ok");
        assert_eq!(cases[0].1, Some("string".to_string()));
        assert_eq!(cases[2].0, "none");
        assert_eq!(cases[2].1, None);
    }

    #[test]
    fn test_parse_interface_with_flags() {
        let input = r#"
            package test:types;

            interface perms {
                flags permissions {
                    read,
                    write,
                    execute,
                }
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let WitTypeKind::Flags(flags) = &doc.interfaces[0].types[0].kind else {
            unreachable!("Expected flags type");
        };
        assert_eq!(flags, &["read", "write", "execute"]);
    }

    #[test]
    fn test_parse_type_alias() {
        let input = r#"
            package test:types;

            interface aliases {
                type buffer = list<u8>;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let WitTypeKind::Alias(target) = &doc.interfaces[0].types[0].kind else {
            unreachable!("Expected alias type");
        };
        assert_eq!(target, "list<u8>");
    }

    #[test]
    fn test_parse_multi_param_function() {
        let input = r#"
            package test:api;

            interface math {
                add: func(a: s32, b: s32) -> s32;
                noop: func();
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let funcs = &doc.interfaces[0].functions;
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].params.len(), 2);
        assert_eq!(funcs[1].params.len(), 0);
        assert!(funcs[1].results.is_none());
    }

    #[test]
    fn test_parse_with_docs() {
        let input = r#"
            package test:api;

            interface documented {
                /// Adds two numbers together.
                add: func(a: s32, b: s32) -> s32;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let func = &doc.interfaces[0].functions[0];
        assert!(func.docs.is_some());
        assert!(func.docs.as_ref().unwrap().contains("Adds two numbers"));
    }

    #[test]
    fn test_parse_multiple_interfaces() {
        let input = r#"
            package myapp:api;

            interface auth {
                login: func(user: string, pass: string) -> bool;
            }

            interface data {
                get: func(key: string) -> option<string>;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        assert_eq!(doc.interfaces.len(), 2);
        assert_eq!(doc.interfaces[0].name, "auth");
        assert_eq!(doc.interfaces[1].name, "data");
    }

    #[test]
    fn test_parse_complex_return_type() {
        let input = r#"
            package test:api;

            interface store {
                get: func(key: string) -> result<string, u32>;
            }
        "#;

        let parser = WitParser::new();
        let doc = parser.parse(input).unwrap();

        let func = &doc.interfaces[0].functions[0];
        assert_eq!(func.results, Some("result<string, u32>".to_string()));
    }

    #[test]
    fn test_parse_unterminated_block() {
        let input = r#"
            package test:api;

            interface broken {
                get: func() -> string;
        "#;

        let parser = WitParser::new();
        let result = parser.parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_wit_bundle() {
        let mut files = HashMap::new();
        files.insert(
            "api.wit".to_string(),
            "package a:b;\ninterface api {\n  run: func();\n}".to_string(),
        );
        files.insert(
            "types.wit".to_string(),
            "package a:b;\ninterface types {\n  type id = u64;\n}".to_string(),
        );

        let docs = parse_wit_bundle(&files).unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn test_split_params_simple() {
        let params = split_params("a: u32, b: string");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_split_params_nested() {
        let params = split_params("a: result<string, u32>, b: list<u8>");
        assert_eq!(params.len(), 2);
        assert!(params[0].contains("result<string, u32>"));
    }
}
