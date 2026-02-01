//! JSON Schema validation for LLM function parameters.
//!
//! Provides basic JSON Schema validation to ensure function call arguments
//! conform to declared parameter schemas before sandbox execution.

use serde::{Deserialize, Serialize};

/// Errors that can occur during schema validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaError {
    /// Value type does not match the expected schema type.
    TypeMismatch {
        /// Expected JSON Schema type.
        expected: String,
        /// Actual type found.
        got: String,
    },
    /// A required field is missing.
    MissingRequired {
        /// Name of the missing field.
        field: String,
    },
    /// A value is invalid for the given path.
    InvalidValue {
        /// JSON path to the invalid value.
        path: String,
        /// Reason the value is invalid.
        reason: String,
    },
    /// An array exceeds the maximum allowed length.
    ArrayLengthExceeded {
        /// Maximum allowed length.
        max: usize,
        /// Actual length.
        actual: usize,
    },
    /// An unknown property was found in strict mode.
    UnknownProperty {
        /// Name of the unknown property.
        name: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaError::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {}, got {}", expected, got)
            }
            SchemaError::MissingRequired { field } => {
                write!(f, "missing required field: {}", field)
            }
            SchemaError::InvalidValue { path, reason } => {
                write!(f, "invalid value at {}: {}", path, reason)
            }
            SchemaError::ArrayLengthExceeded { max, actual } => {
                write!(f, "array length {} exceeds max {}", actual, max)
            }
            SchemaError::UnknownProperty { name } => {
                write!(f, "unknown property: {}", name)
            }
        }
    }
}

/// A JSON Schema wrapper with convenience methods for parameter validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    schema: serde_json::Value,
}

impl ParameterSchema {
    /// Create a `ParameterSchema` from a raw JSON Schema value.
    pub fn from_json_schema(schema: serde_json::Value) -> Self {
        Self { schema }
    }

    /// Get the list of required field names.
    pub fn required_fields(&self) -> Vec<String> {
        self.schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    /// Check whether a field is declared in the schema's properties.
    pub fn has_field(&self, name: &str) -> bool {
        self.schema
            .get("properties")
            .and_then(|v| v.as_object())
            .map(|props| props.contains_key(name))
            .unwrap_or(false)
    }

    /// Get the underlying JSON Schema value.
    pub fn schema(&self) -> &serde_json::Value {
        &self.schema
    }
}

/// Validates JSON values against JSON Schema definitions.
#[derive(Debug, Clone)]
pub struct SchemaValidator;

impl SchemaValidator {
    /// Create a new `SchemaValidator`.
    pub fn new() -> Self {
        Self
    }

    /// Validate a JSON value against a [`ParameterSchema`].
    pub fn validate(
        &self,
        schema: &ParameterSchema,
        value: &serde_json::Value,
    ) -> std::result::Result<(), Vec<SchemaError>> {
        let mut errors = Vec::new();
        self.validate_value(schema.schema(), value, "", &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_value(
        &self,
        schema: &serde_json::Value,
        value: &serde_json::Value,
        path: &str,
        errors: &mut Vec<SchemaError>,
    ) {
        // Check the declared type
        if let Some(type_str) = schema.get("type").and_then(|v| v.as_str()) {
            if !self.type_matches(type_str, value) {
                errors.push(SchemaError::TypeMismatch {
                    expected: type_str.to_string(),
                    got: json_type_name(value).to_string(),
                });
                return;
            }
        }

        match value {
            serde_json::Value::Object(map) => {
                // Validate required fields
                if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
                    for req in required {
                        if let Some(field) = req.as_str() {
                            if !map.contains_key(field) {
                                errors.push(SchemaError::MissingRequired {
                                    field: field.to_string(),
                                });
                            }
                        }
                    }
                }

                // Validate individual properties
                if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
                    for (key, val) in map {
                        if let Some(prop_schema) = properties.get(key) {
                            let child_path = if path.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", path, key)
                            };
                            self.validate_value(prop_schema, val, &child_path, errors);
                        }
                    }

                    // Check for unknown properties when additionalProperties is false
                    if schema.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
                        for key in map.keys() {
                            if !properties.contains_key(key) {
                                errors.push(SchemaError::UnknownProperty { name: key.clone() });
                            }
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                // Validate maxItems
                if let Some(max) = schema.get("maxItems").and_then(|v| v.as_u64()) {
                    let max = max as usize;
                    if arr.len() > max {
                        errors.push(SchemaError::ArrayLengthExceeded { max, actual: arr.len() });
                    }
                }

                // Validate items schema
                if let Some(items_schema) = schema.get("items") {
                    for (i, item) in arr.iter().enumerate() {
                        let child_path = format!("{}[{}]", path, i);
                        self.validate_value(items_schema, item, &child_path, errors);
                    }
                }
            }
            serde_json::Value::String(s) => {
                // Validate enum
                if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array()) {
                    let allowed: Vec<&str> =
                        enum_values.iter().filter_map(|v| v.as_str()).collect();
                    if !allowed.contains(&s.as_str()) {
                        errors.push(SchemaError::InvalidValue {
                            path: path.to_string(),
                            reason: format!("value '{}' not in allowed values: {:?}", s, allowed),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn type_matches(&self, expected: &str, value: &serde_json::Value) -> bool {
        match expected {
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => true,
        }
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Return a human-readable type name for a JSON value.
fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn weather_schema() -> ParameterSchema {
        ParameterSchema::from_json_schema(json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" },
                "units": { "type": "string", "enum": ["celsius", "fahrenheit"] }
            },
            "required": ["city"]
        }))
    }

    #[test]
    fn test_valid_object() {
        let schema = weather_schema();
        let validator = SchemaValidator::new();
        let value = json!({"city": "London", "units": "celsius"});
        assert!(validator.validate(&schema, &value).is_ok());
    }

    #[test]
    fn test_missing_required_field() {
        let schema = weather_schema();
        let validator = SchemaValidator::new();
        let value = json!({"units": "celsius"});
        let errs = validator.validate(&schema, &value).unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            SchemaError::MissingRequired { field } if field == "city"
        )));
    }

    #[test]
    fn test_type_mismatch() {
        let schema = weather_schema();
        let validator = SchemaValidator::new();
        // Top-level should be object, not array
        let value = json!([1, 2, 3]);
        let errs = validator.validate(&schema, &value).unwrap_err();
        assert!(errs.iter().any(
            |e| matches!(e, SchemaError::TypeMismatch { expected, .. } if expected == "object")
        ));
    }

    #[test]
    fn test_enum_validation() {
        let schema = weather_schema();
        let validator = SchemaValidator::new();
        let value = json!({"city": "London", "units": "kelvin"});
        let errs = validator.validate(&schema, &value).unwrap_err();
        assert!(errs.iter().any(|e| matches!(e, SchemaError::InvalidValue { .. })));
    }

    #[test]
    fn test_unknown_property_strict() {
        let schema = ParameterSchema::from_json_schema(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "additionalProperties": false
        }));
        let validator = SchemaValidator::new();
        let value = json!({"name": "test", "extra": 123});
        let errs = validator.validate(&schema, &value).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, SchemaError::UnknownProperty { name } if name == "extra")));
    }

    #[test]
    fn test_array_max_items() {
        let schema = ParameterSchema::from_json_schema(json!({
            "type": "array",
            "items": { "type": "integer" },
            "maxItems": 3
        }));
        let validator = SchemaValidator::new();

        let ok = json!([1, 2, 3]);
        assert!(validator.validate(&schema, &ok).is_ok());

        let too_long = json!([1, 2, 3, 4]);
        let errs = validator.validate(&schema, &too_long).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, SchemaError::ArrayLengthExceeded { max: 3, actual: 4 })));
    }

    #[test]
    fn test_array_item_type_validation() {
        let schema = ParameterSchema::from_json_schema(json!({
            "type": "array",
            "items": { "type": "string" }
        }));
        let validator = SchemaValidator::new();
        let value = json!(["hello", 42]);
        let errs = validator.validate(&schema, &value).unwrap_err();
        assert!(errs.iter().any(
            |e| matches!(e, SchemaError::TypeMismatch { expected, .. } if expected == "string")
        ));
    }

    #[test]
    fn test_parameter_schema_required_fields() {
        let schema = weather_schema();
        assert_eq!(schema.required_fields(), vec!["city".to_string()]);
    }

    #[test]
    fn test_parameter_schema_has_field() {
        let schema = weather_schema();
        assert!(schema.has_field("city"));
        assert!(schema.has_field("units"));
        assert!(!schema.has_field("country"));
    }

    #[test]
    fn test_nested_object_validation() {
        let schema = ParameterSchema::from_json_schema(json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "object",
                    "properties": {
                        "street": { "type": "string" },
                        "zip": { "type": "string" }
                    },
                    "required": ["street"]
                }
            },
            "required": ["address"]
        }));
        let validator = SchemaValidator::new();

        // Valid nested object
        let valid = json!({"address": {"street": "123 Main St", "zip": "12345"}});
        assert!(validator.validate(&schema, &valid).is_ok());

        // Missing nested required field
        let invalid = json!({"address": {"zip": "12345"}});
        let errs = validator.validate(&schema, &invalid).unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            SchemaError::MissingRequired { field } if field == "street"
        )));
    }

    #[test]
    fn test_schema_error_display() {
        let err = SchemaError::TypeMismatch {
            expected: "string".to_string(),
            got: "integer".to_string(),
        };
        assert_eq!(err.to_string(), "type mismatch: expected string, got integer");

        let err = SchemaError::MissingRequired { field: "name".to_string() };
        assert_eq!(err.to_string(), "missing required field: name");
    }
}
