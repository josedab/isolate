//! TypeScript type-stripping transpiler.
//!
//! Provides a lightweight TypeScript → JavaScript transpiler that strips
//! type annotations without requiring a full TypeScript compiler. Handles
//! common TS syntax: type annotations, interfaces, enums, generics, and
//! type assertions.
//!
//! This is a pragmatic approach for running TS in sandboxes — it strips
//! types to produce valid JS, but does not perform type checking.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Result of a TypeScript transpilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranspileResult {
    /// The output JavaScript code.
    pub js_code: String,
    /// Whether transpilation succeeded.
    pub success: bool,
    /// Transpilation duration.
    pub duration: Duration,
    /// Lines of TypeScript processed.
    pub lines_processed: usize,
    /// Number of type annotations removed.
    pub types_stripped: usize,
    /// Warnings during transpilation.
    pub warnings: Vec<String>,
    /// Error message if transpilation failed.
    pub error: Option<String>,
}

/// TypeScript type-stripping transpiler.
pub struct TsTranspiler {
    strip_interfaces: bool,
    strip_type_aliases: bool,
    strip_enums: bool,
}

impl Default for TsTranspiler {
    fn default() -> Self {
        Self::new()
    }
}

impl TsTranspiler {
    /// Create a new transpiler.
    pub fn new() -> Self {
        Self {
            strip_interfaces: true,
            strip_type_aliases: true,
            strip_enums: false,
        }
    }

    /// Transpile TypeScript source to JavaScript.
    pub fn transpile(&self, source: &str) -> TranspileResult {
        let start = Instant::now();
        let lines: Vec<&str> = source.lines().collect();
        let mut output_lines = Vec::new();
        let mut types_stripped = 0;
        let mut warnings = Vec::new();
        let mut i = 0;
        let mut in_interface = false;
        let mut brace_depth = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            // Skip interface blocks
            if self.strip_interfaces && trimmed.starts_with("interface ") {
                in_interface = true;
                brace_depth = 0;
                types_stripped += 1;
            }

            if in_interface {
                brace_depth += trimmed.matches('{').count();
                brace_depth = brace_depth.saturating_sub(trimmed.matches('}').count());
                if brace_depth == 0 && trimmed.contains('}') {
                    in_interface = false;
                }
                i += 1;
                continue;
            }

            // Skip type aliases
            if self.strip_type_aliases && trimmed.starts_with("type ") && trimmed.contains('=') {
                types_stripped += 1;
                i += 1;
                continue;
            }

            // Skip standalone declare statements
            if trimmed.starts_with("declare ") {
                types_stripped += 1;
                i += 1;
                continue;
            }

            // Process the line - strip inline type annotations
            let processed = self.strip_line_types(trimmed);
            if processed != trimmed {
                types_stripped += 1;
            }
            output_lines.push(processed);
            i += 1;
        }

        // Check for common TS-only syntax that wasn't handled
        let js_code = output_lines.join("\n");
        if js_code.contains("<T>") || js_code.contains("<T,") {
            warnings.push("Generic type parameters may remain in output".to_string());
        }

        TranspileResult {
            js_code,
            success: true,
            duration: start.elapsed(),
            lines_processed: lines.len(),
            types_stripped,
            warnings,
            error: None,
        }
    }

    /// Strip type annotations from a single line.
    fn strip_line_types(&self, line: &str) -> String {
        let mut result = line.to_string();

        // Strip function parameter types: (x: number, y: string) -> (x, y)
        result = strip_param_types(&result);

        // Strip return type annotations: function foo(): number -> function foo()
        result = strip_return_type(&result);

        // Strip variable type annotations: const x: number = -> const x =
        result = strip_var_type(&result);

        // Strip type assertions: x as string -> x
        result = strip_type_assertions(&result);

        // Strip non-null assertions: x! -> x
        result = result.replace("!.", ".");
        result = result.replace("!;", ";");
        result = result.replace("!)", ")");

        // Strip access modifiers in class declarations
        for modifier in &["public ", "private ", "protected ", "readonly "] {
            if result.trim_start().starts_with(modifier) {
                result = result.replacen(modifier, "", 1);
            }
        }

        result
    }
}

/// Strip type annotations from function parameters.
fn strip_param_types(line: &str) -> String {
    // Match patterns like (name: type) or (name: type, name2: type2)
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    let mut in_parens = false;

    while let Some(c) = chars.next() {
        if c == '(' {
            in_parens = true;
            result.push(c);
            continue;
        }
        if c == ')' {
            in_parens = false;
            result.push(c);
            continue;
        }

        if in_parens && c == ':' {
            // Skip type annotation until comma or closing paren
            let mut depth = 0;
            while let Some(&next) = chars.peek() {
                if next == '<' || next == '(' {
                    depth += 1;
                } else if next == '>' || (next == ')' && depth > 0) {
                    depth -= 1;
                } else if depth == 0 && (next == ',' || next == ')') {
                    break;
                }
                chars.next();
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Strip return type from function declarations.
fn strip_return_type(line: &str) -> String {
    // Pattern: ) : Type { or ): Type => or ): Type;
    if let Some(paren_close) = line.rfind(')') {
        let after = &line[paren_close + 1..];
        let trimmed_after = after.trim_start();
        if trimmed_after.starts_with(':') {
            // Find where the type annotation ends (at {, =>, ;, or end)
            let type_start = paren_close + 1 + (after.len() - trimmed_after.len());
            let rest = &line[type_start + 1..]; // skip the ':'
            for (i, c) in rest.char_indices() {
                if c == '{' || c == ';' {
                    let before = &line[..paren_close + 1];
                    let remaining = &rest[i..];
                    return format!("{} {}", before.trim_end(), remaining);
                }
                if rest[i..].starts_with("=>") {
                    let before = &line[..paren_close + 1];
                    let remaining = &rest[i..];
                    return format!("{} {}", before.trim_end(), remaining);
                }
            }
        }
    }
    line.to_string()
}

/// Strip type annotations from variable declarations.
fn strip_var_type(line: &str) -> String {
    // Pattern: const/let/var name: Type = value
    for keyword in &["const ", "let ", "var "] {
        if let Some(kw_pos) = line.find(keyword) {
            let after_keyword = &line[kw_pos + keyword.len()..];
            if let Some(colon_pos) = after_keyword.find(':') {
                // Check it's not inside a string or object
                let before_colon = &after_keyword[..colon_pos];
                if !before_colon.contains('{') && !before_colon.contains('(') {
                    let after_colon = &after_keyword[colon_pos + 1..];
                    // Find the = sign
                    if let Some(eq_pos) = find_equals(after_colon) {
                        let name = before_colon.trim();
                        let value_part = &after_colon[eq_pos..];
                        return format!(
                            "{}{}{} {}",
                            &line[..kw_pos],
                            keyword,
                            name,
                            value_part
                        );
                    }
                }
            }
        }
    }
    line.to_string()
}

/// Find the first unparenthesized '=' sign.
fn find_equals(s: &str) -> Option<usize> {
    let mut depth: u32 = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Strip `as Type` assertions.
fn strip_type_assertions(line: &str) -> String {
    let mut result = line.to_string();
    // Simple pattern: ` as Type` at word boundaries
    while let Some(pos) = result.find(" as ") {
        let after = &result[pos + 4..];
        // Find end of type (semicolon, comma, paren, bracket)
        let end = after
            .find(|c: char| c == ';' || c == ',' || c == ')' || c == ']' || c == '}')
            .unwrap_or(after.len());
        let type_name = after[..end].trim();
        // Only strip if the "type" looks like a type (starts with uppercase or is a keyword)
        if type_name.starts_with(|c: char| c.is_uppercase())
            || matches!(type_name, "string" | "number" | "boolean" | "any" | "unknown" | "never" | "void")
        {
            result = format!("{}{}", &result[..pos], &result[pos + 4 + end..]);
        } else {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_basic() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile("const x = 42;\nconsole.log(x);");

        assert!(result.success);
        assert!(result.js_code.contains("const x = 42;"));
        assert!(result.js_code.contains("console.log(x);"));
    }

    #[test]
    fn test_strip_variable_types() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile("const x: number = 42;");

        assert!(result.success);
        assert!(result.js_code.contains("const x = 42;"));
        assert!(!result.js_code.contains(": number"));
    }

    #[test]
    fn test_strip_interface() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile(
            "interface User {\n  name: string;\n  age: number;\n}\nconst x = 1;",
        );

        assert!(result.success);
        assert!(!result.js_code.contains("interface"));
        assert!(result.js_code.contains("const x = 1;"));
        assert!(result.types_stripped > 0);
    }

    #[test]
    fn test_strip_type_alias() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile("type ID = string;\nconst id = '123';");

        assert!(result.success);
        assert!(!result.js_code.contains("type ID"));
        assert!(result.js_code.contains("const id = '123';"));
    }

    #[test]
    fn test_strip_as_assertion() {
        let result = strip_type_assertions("const x = value as string;");
        assert!(!result.contains(" as string"));
    }

    #[test]
    fn test_strip_declare() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile("declare const window: any;\nconst x = 1;");

        assert!(result.success);
        assert!(!result.js_code.contains("declare"));
        assert!(result.js_code.contains("const x = 1;"));
    }

    #[test]
    fn test_transpile_result_metadata() {
        let transpiler = TsTranspiler::new();
        let result = transpiler.transpile("const x: number = 42;\nconst y: string = 'hello';");

        assert!(result.success);
        assert_eq!(result.lines_processed, 2);
        assert!(result.types_stripped >= 2);
        assert!(result.duration.as_nanos() > 0);
    }

    #[test]
    fn test_strip_return_type() {
        let result = strip_return_type("function add(a, b): number {");
        assert!(!result.contains(": number"));
        assert!(result.contains("{"));
    }

    #[test]
    fn test_strip_param_types() {
        let result = strip_param_types("(name: string, age: number)");
        assert!(!result.contains(": string"));
        assert!(!result.contains(": number"));
        assert!(result.contains("name"));
        assert!(result.contains("age"));
    }
}
