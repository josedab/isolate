use super::types::{GraphQLArgument, GraphQLField, GraphQLType, WasmExport};
use serde::{Deserialize, Serialize};

/// Configuration for the schema generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaGeneratorConfig {
    /// Include descriptions in the schema.
    pub include_descriptions: bool,
    /// Generate input types for complex arguments.
    pub generate_input_types: bool,
    /// Add __typename to all types.
    pub include_typename: bool,
}

impl Default for SchemaGeneratorConfig {
    fn default() -> Self {
        Self {
            include_descriptions: true,
            generate_input_types: true,
            include_typename: false,
        }
    }
}

/// A generated GraphQL schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSchema {
    /// The complete SDL (Schema Definition Language) string.
    pub sdl: String,
    /// Query fields.
    pub queries: Vec<GraphQLField>,
    /// Mutation fields.
    pub mutations: Vec<GraphQLField>,
}

impl GeneratedSchema {
    /// Total number of fields across queries and mutations.
    pub fn field_count(&self) -> usize {
        self.queries.len() + self.mutations.len()
    }

    /// Get all query fields.
    pub fn query_fields(&self) -> &[GraphQLField] {
        &self.queries
    }

    /// Get all mutation fields.
    pub fn mutation_fields(&self) -> &[GraphQLField] {
        &self.mutations
    }

    /// Find a field by name.
    pub fn find_field(&self, name: &str) -> Option<&GraphQLField> {
        self.queries
            .iter()
            .chain(self.mutations.iter())
            .find(|f| f.name == name)
    }
}

/// Generator that transforms WASM exports into GraphQL schemas.
pub struct SchemaGenerator {
    config: SchemaGeneratorConfig,
}

impl SchemaGenerator {
    pub fn new() -> Self {
        Self {
            config: SchemaGeneratorConfig::default(),
        }
    }

    pub fn with_config(config: SchemaGeneratorConfig) -> Self {
        Self { config }
    }

    /// Generate a GraphQL schema from WASM module exports.
    pub fn generate(&self, exports: &[WasmExport]) -> GeneratedSchema {
        let mut queries = Vec::new();
        let mut mutations = Vec::new();

        for export in exports {
            let field = self.export_to_field(export);
            if export.is_mutation {
                mutations.push(field);
            } else {
                queries.push(field);
            }
        }

        let sdl = self.build_sdl(&queries, &mutations);

        GeneratedSchema {
            sdl,
            queries,
            mutations,
        }
    }

    fn export_to_field(&self, export: &WasmExport) -> GraphQLField {
        let arguments: Vec<GraphQLArgument> = export
            .params
            .iter()
            .map(|p| GraphQLArgument {
                name: p.name.clone(),
                graphql_type: GraphQLType::scalar(p.wasm_type.to_graphql()),
                required: true,
            })
            .collect();

        let return_type = match &export.return_type {
            Some(t) => GraphQLType::scalar(t.to_graphql()),
            None => GraphQLType::scalar("Boolean"),
        };

        GraphQLField {
            name: export.name.clone(),
            arguments,
            return_type,
            description: export.description.clone(),
            is_mutation: export.is_mutation,
        }
    }

    fn build_sdl(&self, queries: &[GraphQLField], mutations: &[GraphQLField]) -> String {
        let mut sdl = String::new();

        // Schema definition
        sdl.push_str("schema {\n");
        if !queries.is_empty() {
            sdl.push_str("  query: Query\n");
        }
        if !mutations.is_empty() {
            sdl.push_str("  mutation: Mutation\n");
        }
        sdl.push_str("}\n\n");

        // Query type
        if !queries.is_empty() {
            sdl.push_str("type Query {\n");
            for field in queries {
                self.write_field(&mut sdl, field);
            }
            sdl.push_str("}\n\n");
        }

        // Mutation type
        if !mutations.is_empty() {
            sdl.push_str("type Mutation {\n");
            for field in mutations {
                self.write_field(&mut sdl, field);
            }
            sdl.push_str("}\n");
        }

        sdl
    }

    fn write_field(&self, sdl: &mut String, field: &GraphQLField) {
        // Description
        if self.config.include_descriptions {
            if let Some(ref desc) = field.description {
                sdl.push_str(&format!("  \"\"\"{}\"\"\"\n", desc));
            }
        }

        // Field with arguments
        sdl.push_str("  ");
        sdl.push_str(&field.name);

        if !field.arguments.is_empty() {
            sdl.push('(');
            let args: Vec<String> = field
                .arguments
                .iter()
                .map(|a| {
                    let type_str = a.graphql_type.to_sdl();
                    format!("{}: {}", a.name, type_str)
                })
                .collect();
            sdl.push_str(&args.join(", "));
            sdl.push(')');
        }

        sdl.push_str(": ");
        sdl.push_str(&field.return_type.to_sdl());
        sdl.push('\n');
    }
}

impl Default for SchemaGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql_schema::types::{WasmExport, WasmType};

    #[test]
    fn test_generate_empty() {
        let gen = SchemaGenerator::new();
        let schema = gen.generate(&[]);
        assert!(schema.sdl.contains("schema"));
        assert_eq!(schema.field_count(), 0);
    }

    #[test]
    fn test_generate_query_field() {
        let gen = SchemaGenerator::new();
        let exports = vec![WasmExport::function(
            "add",
            vec![WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )];
        let schema = gen.generate(&exports);

        assert!(schema.sdl.contains("type Query"));
        assert!(schema.sdl.contains("add(arg0: Int!, arg1: Int!): Int!"));
        assert_eq!(schema.queries.len(), 1);
    }

    #[test]
    fn test_generate_mutation_field() {
        let gen = SchemaGenerator::new();
        let exports = vec![WasmExport::function("reset", vec![], None)];
        let schema = gen.generate(&exports);

        assert!(schema.sdl.contains("type Mutation"));
        assert!(schema.sdl.contains("reset: Boolean!"));
        assert_eq!(schema.mutations.len(), 1);
    }

    #[test]
    fn test_generate_with_description() {
        let gen = SchemaGenerator::new();
        let exports = vec![WasmExport::function("hello", vec![], Some(WasmType::String))
            .with_description("Returns a greeting")];
        let schema = gen.generate(&exports);

        assert!(schema.sdl.contains("\"\"\"Returns a greeting\"\"\""));
    }

    #[test]
    fn test_generate_mixed_types() {
        let gen = SchemaGenerator::new();
        let exports = vec![WasmExport::function_named(
            "compute",
            vec![
                ("x", WasmType::F64),
                ("y", WasmType::F64),
                ("label", WasmType::String),
            ],
            Some(WasmType::F64),
        )];
        let schema = gen.generate(&exports);

        assert!(schema.sdl.contains("x: Float!"));
        assert!(schema.sdl.contains("y: Float!"));
        assert!(schema.sdl.contains("label: String!"));
        assert!(schema.sdl.contains(": Float!"));
    }

    #[test]
    fn test_find_field() {
        let gen = SchemaGenerator::new();
        let exports = vec![
            WasmExport::function("a", vec![], Some(WasmType::I32)),
            WasmExport::function("b", vec![], None),
        ];
        let schema = gen.generate(&exports);

        assert!(schema.find_field("a").is_some());
        assert!(schema.find_field("b").is_some());
        assert!(schema.find_field("c").is_none());
    }

    #[test]
    fn test_schema_sdl_structure() {
        let gen = SchemaGenerator::new();
        let exports = vec![
            WasmExport::function("get_value", vec![], Some(WasmType::I32)),
            WasmExport::function("set_value", vec![WasmType::I32], None),
        ];
        let schema = gen.generate(&exports);

        // Verify structure
        assert!(schema.sdl.contains("schema {"));
        assert!(schema.sdl.contains("query: Query"));
        assert!(schema.sdl.contains("mutation: Mutation"));
        assert!(schema.sdl.contains("get_value: Int!"));
        assert!(schema.sdl.contains("set_value(arg0: Int!): Boolean!"));
    }

    #[test]
    fn test_no_description_config() {
        let gen = SchemaGenerator::with_config(SchemaGeneratorConfig {
            include_descriptions: false,
            ..Default::default()
        });
        let exports = vec![WasmExport::function("f", vec![], Some(WasmType::I32))
            .with_description("this should not appear")];
        let schema = gen.generate(&exports);

        assert!(!schema.sdl.contains("this should not appear"));
    }

    #[test]
    fn test_bool_type_mapping() {
        let gen = SchemaGenerator::new();
        let exports = vec![WasmExport::function_named(
            "is_valid",
            vec![("flag", WasmType::Bool)],
            Some(WasmType::Bool),
        )];
        let schema = gen.generate(&exports);
        assert!(schema.sdl.contains("flag: Boolean!"));
        assert!(schema.sdl.contains(": Boolean!"));
    }
}
