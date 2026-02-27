use serde::{Deserialize, Serialize};

/// WASM value types (subset relevant for GraphQL mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    String,
    Bytes,
    Bool,
    Void,
}

impl WasmType {
    /// Map a WASM type to the corresponding GraphQL type name.
    pub fn to_graphql(&self) -> &'static str {
        match self {
            WasmType::I32 => "Int",
            WasmType::I64 => "Int",
            WasmType::F32 => "Float",
            WasmType::F64 => "Float",
            WasmType::String => "String",
            WasmType::Bytes => "String",
            WasmType::Bool => "Boolean",
            WasmType::Void => "Boolean",
        }
    }

    /// Whether this type represents a scalar in GraphQL.
    pub fn is_scalar(&self) -> bool {
        true // all WASM primitive types map to GraphQL scalars
    }
}

/// An exported function from a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmExport {
    pub name: String,
    pub params: Vec<WasmParam>,
    pub return_type: Option<WasmType>,
    pub is_mutation: bool,
    pub description: Option<String>,
}

/// A parameter of an exported function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmParam {
    pub name: String,
    pub wasm_type: WasmType,
}

impl WasmExport {
    /// Create a function export with positional parameters.
    pub fn function(
        name: impl Into<String>,
        param_types: Vec<WasmType>,
        return_type: Option<WasmType>,
    ) -> Self {
        let params: Vec<WasmParam> = param_types
            .into_iter()
            .enumerate()
            .map(|(i, t)| WasmParam { name: format!("arg{}", i), wasm_type: t })
            .collect();

        let is_mutation = return_type.is_none();

        Self { name: name.into(), params, return_type, is_mutation, description: None }
    }

    /// Create a function export with named parameters.
    pub fn function_named(
        name: impl Into<String>,
        params: Vec<(impl Into<String>, WasmType)>,
        return_type: Option<WasmType>,
    ) -> Self {
        let params: Vec<WasmParam> =
            params.into_iter().map(|(n, t)| WasmParam { name: n.into(), wasm_type: t }).collect();

        Self { name: name.into(), params, return_type, is_mutation: false, description: None }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn as_mutation(mut self) -> Self {
        self.is_mutation = true;
        self
    }
}

/// A field in the GraphQL schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLField {
    pub name: String,
    pub arguments: Vec<GraphQLArgument>,
    pub return_type: GraphQLType,
    pub description: Option<String>,
    pub is_mutation: bool,
}

/// A GraphQL argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLArgument {
    pub name: String,
    pub graphql_type: GraphQLType,
    pub required: bool,
}

/// A GraphQL type reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQLType {
    pub name: String,
    pub non_null: bool,
    pub list: bool,
}

impl GraphQLType {
    pub fn scalar(name: &str) -> Self {
        Self { name: name.to_string(), non_null: true, list: false }
    }

    pub fn nullable(name: &str) -> Self {
        Self { name: name.to_string(), non_null: false, list: false }
    }

    /// Format as GraphQL SDL type reference.
    pub fn to_sdl(&self) -> String {
        let base = if self.list { format!("[{}]", self.name) } else { self.name.clone() };
        if self.non_null {
            format!("{base}!")
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_type_to_graphql() {
        assert_eq!(WasmType::I32.to_graphql(), "Int");
        assert_eq!(WasmType::F64.to_graphql(), "Float");
        assert_eq!(WasmType::String.to_graphql(), "String");
        assert_eq!(WasmType::Bool.to_graphql(), "Boolean");
    }

    #[test]
    fn test_wasm_export_function() {
        let f =
            WasmExport::function("add", vec![WasmType::I32, WasmType::I32], Some(WasmType::I32));
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "arg0");
        assert!(!f.is_mutation);
    }

    #[test]
    fn test_wasm_export_void_is_mutation() {
        let f = WasmExport::function("reset", vec![], None);
        assert!(f.is_mutation); // void return → mutation
    }

    #[test]
    fn test_wasm_export_named_params() {
        let f = WasmExport::function_named(
            "greet",
            vec![("name", WasmType::String), ("age", WasmType::I32)],
            Some(WasmType::String),
        );
        assert_eq!(f.params[0].name, "name");
        assert_eq!(f.params[1].name, "age");
    }

    #[test]
    fn test_graphql_type_sdl() {
        assert_eq!(GraphQLType::scalar("Int").to_sdl(), "Int!");
        assert_eq!(GraphQLType::nullable("String").to_sdl(), "String");
        let list = GraphQLType { name: "Int".into(), non_null: true, list: true };
        assert_eq!(list.to_sdl(), "[Int]!");
    }

    #[test]
    fn test_export_with_description() {
        let f = WasmExport::function("hello", vec![], Some(WasmType::String))
            .with_description("Says hello");
        assert_eq!(f.description, Some("Says hello".to_string()));
    }
}
