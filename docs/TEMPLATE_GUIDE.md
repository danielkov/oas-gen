# Template Development Guide

This guide walks you through creating a new code generator template for OAS-Gen2.

## Overview

A template is a Rust crate that implements the `Generator` trait to produce code files for a specific programming language from the GenIR (language-agnostic intermediate representation).

## Quick Start

### 1. Create the Template Crate

```bash
cd templates/
cargo new --lib your-language
cd your-language
```

### 2. Basic Cargo.toml Setup

```toml
[package]
name = "your-language"
version = "0.1.0"
edition = "2024"

[dependencies]
codegen = { path = "../../codegen" }
ast = { path = "../../ast" }
handlebars = "5.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### 3. Implement the Generator

```rust
// templates/your-language/src/lib.rs
use codegen::{Generator, GenIr, Config, VirtualFS, Result};

pub struct YourLanguageGenerator;

impl Generator for YourLanguageGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        
        // Your generation logic here
        generate_types(ir, &mut vfs, config)?;
        generate_services(ir, &mut vfs, config)?;
        generate_package_files(&mut vfs)?;
        
        Ok(vfs)
    }
    
    fn language(&self) -> &str {
        "your-language"
    }
}
```

### 4. Register the Generator

**Add to `generate/Cargo.toml`:**
```toml
[dependencies]
your-language = { path = "../templates/your-language", optional = true }

[features]
default = ["typescript", "your-language"]
your-language = ["dep:your-language"]
```

**Add to `generate/src/lib.rs`:**
```rust
#[cfg(feature = "your-language")]
use your_language::YourLanguageGenerator;

impl GeneratorRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        
        #[cfg(feature = "typescript")]
        registry.register(Box::new(TypeScriptGenerator));
        
        #[cfg(feature = "your-language")]
        registry.register(Box::new(YourLanguageGenerator));
        
        registry
    }
}
```

**Add to workspace root `Cargo.toml`:**
```toml
[workspace]
members = [
    # ... existing members
    "templates/your-language",
]
```

## Understanding GenIR

### Core Types

The GenIR consists of types and services:

```rust
pub struct GenIr {
    pub types: Vec<TypeDef>,      // All type definitions
    pub services: Vec<ServiceDef>, // Grouped operations
}
```

### TypeDef Variants

```rust
pub enum TypeDef {
    Struct(StructDef),
    Enum(EnumDef),
    TypeAlias(TypeAliasDef),
    Primitive(PrimitiveDef),
}
```

#### StructDef
```rust
pub struct StructDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub description: Option<String>,
}

pub struct FieldDef {
    pub name: String,
    pub type_ref: TypeRef,
    pub required: bool,
    pub description: Option<String>,
}
```

#### EnumDef
```rust
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
    pub underlying_type: EnumType, // String or Integer
    pub description: Option<String>,
}
```

#### TypeRef
```rust
pub enum TypeRef {
    Named(String),           // Reference to another type
    Array(Box<TypeRef>),     // Array of type
    Optional(Box<TypeRef>),  // Nullable/optional type
    Primitive(PrimitiveType),// Built-in type
}

pub enum PrimitiveType {
    String,
    Integer,
    Float,
    Boolean,
    Date,
    DateTime,
    Binary,
    Object,
    Any,
}
```

### ServiceDef

```rust
pub struct ServiceDef {
    pub name: String,
    pub operations: Vec<OperationDef>,
    pub description: Option<String>,
}
```

### OperationDef

```rust
pub struct OperationDef {
    pub id: String,
    pub method: HttpMethod,
    pub path: String,
    pub parameters: Vec<ParameterDef>,
    pub request_body: Option<TypeRef>,
    pub responses: BTreeMap<u16, ResponseDef>,
    pub summary: Option<String>,
    pub description: Option<String>,
}

pub struct ParameterDef {
    pub name: String,
    pub location: ParameterLocation, // Path, Query, Header, Cookie
    pub type_ref: TypeRef,
    pub required: bool,
    pub description: Option<String>,
}

pub struct ResponseDef {
    pub status_code: u16,
    pub type_ref: Option<TypeRef>,
    pub description: Option<String>,
}
```

## Generation Strategy

### 1. Type Generation

Iterate through `ir.types` and generate type definitions:

```rust
fn generate_types(ir: &GenIr, vfs: &mut VirtualFS, config: &Config) -> Result<()> {
    for type_def in &ir.types {
        match type_def {
            TypeDef::Struct(s) => generate_struct(s, vfs, config)?,
            TypeDef::Enum(e) => generate_enum(e, vfs, config)?,
            TypeDef::TypeAlias(a) => generate_alias(a, vfs, config)?,
            TypeDef::Primitive(_) => {} // Usually skip primitives
        }
    }
    Ok(())
}
```

### 2. Service Generation

Iterate through `ir.services` and generate API client/service code:

```rust
fn generate_services(ir: &GenIr, vfs: &mut VirtualFS, config: &Config) -> Result<()> {
    for service in &ir.services {
        generate_service(service, vfs, config)?;
    }
    Ok(())
}

fn generate_service(service: &ServiceDef, vfs: &mut VirtualFS, config: &Config) -> Result<()> {
    for operation in &service.operations {
        // Generate method for each operation
    }
    Ok(())
}
```

### 3. Supporting Files

Generate package configuration, README, etc.:

```rust
fn generate_package_files(vfs: &mut VirtualFS) -> Result<()> {
    vfs.write_file("package.json", generate_package_json())?;
    vfs.write_file("README.md", generate_readme())?;
    Ok(())
}
```

## Using VirtualFS

The VirtualFS is an in-memory file system:

```rust
let mut vfs = VirtualFS::new();

// Write a file
vfs.write_file("src/types.ts", "export interface User { ... }")?;

// Write nested files
vfs.write_file("src/services/users.ts", "export class UsersService { ... }")?;

// All directories are created automatically
```

## Template Rendering with Handlebars

### Setup

```rust
use handlebars::Handlebars;
use serde::Serialize;

#[derive(Serialize)]
struct TypesContext {
    types: Vec<TypeData>,
}

#[derive(Serialize)]
struct TypeData {
    name: String,
    fields: Vec<FieldData>,
}
```

### Create Templates

```rust
const TYPES_TEMPLATE: &str = r#"
{{#each types}}
export interface {{name}} {
  {{#each fields}}
  {{name}}{{#unless required}}?{{/unless}}: {{type_ref}};
  {{/each}}
}
{{/each}}
"#;
```

### Render

```rust
fn generate_types_file(types: &[StructDef], vfs: &mut VirtualFS) -> Result<()> {
    let mut handlebars = Handlebars::new();
    handlebars.register_template_string("types", TYPES_TEMPLATE)?;
    
    let context = TypesContext {
        types: types.iter().map(|t| convert_to_template_data(t)).collect(),
    };
    
    let rendered = handlebars.render("types", &context)?;
    vfs.write_file("src/types.ts", rendered)?;
    
    Ok(())
}
```

## Type Mapping

Map GenIR `PrimitiveType` to your target language:

```rust
fn map_primitive_type(prim: &PrimitiveType) -> &'static str {
    match prim {
        PrimitiveType::String => "string",
        PrimitiveType::Integer => "number",
        PrimitiveType::Float => "number",
        PrimitiveType::Boolean => "boolean",
        PrimitiveType::Date => "Date",
        PrimitiveType::DateTime => "Date",
        PrimitiveType::Binary => "Blob",
        PrimitiveType::Object => "any",
        PrimitiveType::Any => "any",
    }
}
```

Map `TypeRef` recursively:

```rust
fn map_type_ref(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Array(inner) => format!("{}[]", map_type_ref(inner)),
        TypeRef::Optional(inner) => format!("{} | null", map_type_ref(inner)),
        TypeRef::Primitive(prim) => map_primitive_type(prim).to_string(),
    }
}
```

## Handling Configuration

Use the `Config` parameter:

```rust
fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
    let include_docs = config.include_docs;
    let output_dir = &config.output_dir;
    let service_style = config.service_style;
    
    // Use configuration
}
```

### ServiceStyle

```rust
match config.service_style {
    ServiceStyle::PerService => {
        // Generate one file per service
    }
    ServiceStyle::SingleClient => {
        // Generate one file with all operations
    }
    ServiceStyle::ByTag => {
        // Group by tags
    }
}
```

## Testing Your Template

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generates_struct() {
        let struct_def = StructDef {
            name: "User".to_string(),
            fields: vec![
                FieldDef {
                    name: "id".to_string(),
                    type_ref: TypeRef::Primitive(PrimitiveType::Integer),
                    required: true,
                    description: None,
                }
            ],
            description: None,
        };
        
        let result = generate_struct(&struct_def);
        assert!(result.contains("User"));
        assert!(result.contains("id"));
    }
}
```

### Integration Tests

```rust
// templates/your-language/tests/integration_test.rs
use your_language::YourLanguageGenerator;
use codegen::{Generator, Config};

#[test]
fn test_full_generation() {
    let spec = load_test_spec();
    let ir: GenIr = spec.into();
    let config = Config::default();
    let generator = YourLanguageGenerator;
    
    let vfs = generator.generate(&ir, &config).unwrap();
    
    assert!(vfs.contains("src/types.ts"));
    assert!(vfs.contains("src/services/pets.ts"));
}
```

### Manual Testing

```bash
cargo build
cargo run --bin oas-gen -- examples/petstore.json -t your-language -o test-output -v
```

Check the output:
```bash
ls -R test-output/
cat test-output/src/types.ts
```

Verify generated code compiles in target language:
```bash
cd test-output
# Run language-specific compiler/linter
```

## Best Practices

### 1. Follow Target Language Conventions

- Use proper naming conventions (camelCase, snake_case, PascalCase)
- Follow formatting standards
- Add language-specific linting config

### 2. Handle All TypeRef Variants

```rust
fn map_type_ref(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Array(inner) => format!("Array<{}>", map_type_ref(inner)),
        TypeRef::Optional(inner) => format!("{} | null", map_type_ref(inner)),
        TypeRef::Primitive(prim) => map_primitive(prim),
    }
}
```

### 3. Include Documentation

```rust
if config.include_docs {
    if let Some(desc) = &struct_def.description {
        output.push_str(&format!("/**\n * {}\n */\n", desc));
    }
}
```

### 4. Generate Package Files

Create working package configuration:
- `package.json` (Node.js)
- `setup.py` (Python)
- `Cargo.toml` (Rust)
- `go.mod` (Go)
- etc.

### 5. Error Handling

Use `anyhow` for rich errors:
```rust
use anyhow::{Context, Result};

fn generate_something() -> Result<String> {
    let data = load_template()
        .context("Failed to load template")?;
    Ok(data)
}
```

### 6. Consider Edge Cases

- Empty services
- No types defined
- Recursive types
- Nullable vs optional
- Array of arrays
- Union types (if supported)

## Example: Minimal Python Generator

```rust
use codegen::{Generator, GenIr, Config, VirtualFS, Result, TypeDef, ServiceDef};

pub struct PythonGenerator;

impl Generator for PythonGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        
        // Generate types
        let types_code = generate_types_file(&ir.types, config);
        vfs.write_file("types.py", types_code)?;
        
        // Generate services
        for service in &ir.services {
            let service_code = generate_service_file(service, config);
            let filename = format!("{}.py", service.name.to_lowercase());
            vfs.write_file(&filename, service_code)?;
        }
        
        // Generate __init__.py
        vfs.write_file("__init__.py", generate_init_file(&ir.services))?;
        
        Ok(vfs)
    }
    
    fn language(&self) -> &str {
        "python"
    }
}

fn generate_types_file(types: &[TypeDef], config: &Config) -> String {
    let mut output = String::from("from dataclasses import dataclass\nfrom typing import Optional, List\n\n");
    
    for type_def in types {
        if let TypeDef::Struct(s) = type_def {
            output.push_str("@dataclass\n");
            output.push_str(&format!("class {}:\n", s.name));
            for field in &s.fields {
                let field_type = map_type_ref(&field.type_ref);
                output.push_str(&format!("    {}: {}\n", field.name, field_type));
            }
            output.push('\n');
        }
    }
    
    output
}

fn generate_service_file(service: &ServiceDef, config: &Config) -> String {
    format!("class {}Service:\n    pass\n", service.name)
}

fn generate_init_file(services: &[ServiceDef]) -> String {
    let mut output = String::from("from .types import *\n");
    for service in services {
        output.push_str(&format!("from .{} import {}Service\n", 
            service.name.to_lowercase(), service.name));
    }
    output
}

fn map_type_ref(type_ref: &codegen::TypeRef) -> String {
    // Implementation
    "str".to_string()
}
```

## Resources

- See `templates/typescript/` for a complete example
- Check `examples/petstore.json` for test data
- Read `ARCHITECTURE.md` for design details
- Look at `codegen/src/lib.rs` for trait definitions

## Getting Help

- Open an issue with `[template]` prefix
- Ask in discussions
- Check existing templates for patterns

