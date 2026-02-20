//! # GraphQL Schema Generation
//!
//! Automatically generate GraphQL schemas from WASM module exports.
//! Maps WASM function signatures to GraphQL query/mutation types with
//! type-safe argument marshaling and result deserialization.
//!
//! ## Example
//!
//! ```rust
//! use isolate_core::graphql_schema::{SchemaGenerator, WasmExport, WasmType};
//!
//! let exports = vec![
//!     WasmExport::function("add", vec![WasmType::I32, WasmType::I32], Some(WasmType::I32)),
//!     WasmExport::function("greet", vec![WasmType::String], Some(WasmType::String)),
//! ];
//!
//! let generator = SchemaGenerator::new();
//! let schema = generator.generate(&exports);
//! assert!(schema.sdl.contains("add"));
//! assert!(schema.sdl.contains("greet"));
//! ```

#![allow(missing_docs)]
mod generator;
mod types;

pub use generator::{GeneratedSchema, SchemaGenerator, SchemaGeneratorConfig};
pub use types::{GraphQLField, GraphQLType, WasmExport, WasmType};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_schema_generation() {
        let exports = vec![
            WasmExport::function("add", vec![WasmType::I32, WasmType::I32], Some(WasmType::I32)),
            WasmExport::function("echo", vec![WasmType::String], Some(WasmType::String)),
            WasmExport::function("reset", vec![], None),
        ];

        let gen = SchemaGenerator::new();
        let schema = gen.generate(&exports);

        // Queries for pure functions
        assert!(schema.sdl.contains("type Query"));
        // Mutations for side-effect functions
        assert!(schema.sdl.contains("type Mutation"));
        assert_eq!(schema.field_count(), 3);
    }

    #[test]
    fn test_schema_introspection() {
        let exports = vec![WasmExport::function("hello", vec![], Some(WasmType::String))];

        let gen = SchemaGenerator::new();
        let schema = gen.generate(&exports);

        let fields = schema.query_fields();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "hello");
    }
}
