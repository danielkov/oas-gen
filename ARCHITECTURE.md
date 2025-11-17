# Architecture

## Overview

OAS-Gen2 is designed as a clean, modular pipeline for generating code from OpenAPI specifications. The architecture emphasizes separation of concerns, testability, and extensibility.

## Design Principles

1. **Language-Agnostic IR**: The GenIR (Generated Intermediate Representation) is designed to be completely independent of any target language
2. **Plugin Architecture**: Templates are separate crates that can be developed and tested independently
3. **Zero Runtime Dependencies**: Generated code should not depend on this codebase at runtime
4. **Virtual Filesystem**: All generation happens in memory before writing to disk
5. **Type Safety**: Leverage Rust's type system to catch errors at compile time

## Pipeline

```
┌─────────────┐      ┌────────┐      ┌─────────┐      ┌───────────┐      ┌──────┐
│ OpenAPI     │─────▶│ Parser │─────▶│   AST   │─────▶│ Generator │─────▶│ Disk │
│ Spec (.json)│      │        │      │ (GenIR) │      │  (VFS)    │      │      │
└─────────────┘      └────────┘      └─────────┘      └───────────┘      └──────┘
```

### Stage 1: Parsing (parser crate)

**Responsibility**: Read and parse OpenAPI 3.x specifications

**Input**: JSON or YAML file containing OpenAPI spec

**Output**: Strongly-typed `oas::OpenAPIV3` structure

**Key Components**:

- Uses the `oas` crate for type-safe OpenAPI parsing
- Validates basic structure
- Preserves all OpenAPI information

**Error Handling**: Returns parse errors for invalid JSON/YAML or malformed OpenAPI

### Stage 2: AST Generation (ast crate)

**Responsibility**: Convert OpenAPI structures into language-agnostic GenIR

**Input**: `oas::OpenAPIV3`

**Output**: `codegen::GenIr`

**Key Components**:

- `GenIr`: Top-level IR containing types and services
- `TypeDef`: Struct, Enum, TypeAlias, Primitive representations
- `ServiceDef`: Groups of related operations
- `OperationDef`: Individual API endpoints with full metadata

**Transformations**:

- OpenAPI schemas → TypeDef structures
- OpenAPI paths → OperationDef endpoints
- Components/references → Resolved type definitions
- Tags → Service groupings

**Design Decisions**:

- Flattens OpenAPI's nested structure
- Resolves all `$ref` references
- Normalizes types (e.g., all integers → i64)
- Makes implicit information explicit

### Stage 3: Code Generation (codegen + templates crates)

**Responsibility**: Transform GenIR into language-specific code

**Input**: `GenIr` + `Config`

**Output**: `VirtualFS` (in-memory file tree)

**Key Abstractions**:

#### Generator Trait

```rust
pub trait Generator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS>;
    fn language(&self) -> &str;
}
```

Every template implements this trait.

#### VirtualFS

An in-memory representation of the output file structure:

- Allows generators to build file trees without I/O
- Supports nested directories
- Provides atomic write-to-disk operation
- Makes testing easier (no filesystem dependencies)

#### Config

Configuration for generation:

- `output_dir`: Base directory for generated files
- `service_style`: How to organize operations (per-service, single-client, by-tag)
- `include_docs`: Whether to generate documentation comments
- `lang_options`: Language-specific options

**Template Architecture**:
Each template crate:

1. Implements `Generator` trait
2. Uses template engine (e.g., Handlebars) or string building
3. Generates language-specific files (types, services, config, etc.)
4. Returns complete VirtualFS

### Stage 4: Registry & Plugin Loading (generate crate)

**Responsibility**: Manage available generators and route to correct one

**Key Components**:

- `GeneratorRegistry`: Holds all registered generators
- Feature flags: Each template is an optional Cargo feature
- Dynamic dispatch: Uses trait objects for polymorphism

**Usage**:

```rust
let registry = GeneratorRegistry::with_defaults();
let vfs = registry.generate("typescript", &gen_ir, &config)?;
```

### Stage 5: Disk Writing (codegen crate)

**Responsibility**: Write VirtualFS to actual filesystem

**Process**:

1. Create output directory if it doesn't exist
2. Write each file preserving directory structure
3. Set appropriate file permissions
4. Atomic operation (all or nothing)

## Data Structures

### GenIr (Intermediate Representation)

```rust
pub struct GenIr {
    pub types: Vec<TypeDef>,      // All type definitions
    pub services: Vec<ServiceDef>, // Service groupings
}
```

### TypeDef

Represents all possible type definitions:

- **Struct**: Object with named fields
- **Enum**: Enumeration with variants (string or integer)
- **TypeAlias**: Alias for another type
- **Primitive**: Built-in types (string, number, boolean, etc.)

### ServiceDef

```rust
pub struct ServiceDef {
    pub name: String,
    pub operations: Vec<OperationDef>,
    pub description: Option<String>,
}
```

Groups related operations together. How operations are grouped depends on `ServiceStyle`.

### OperationDef

```rust
pub struct OperationDef {
    pub id: String,
    pub method: HttpMethod,
    pub path: String,
    pub parameters: Vec<ParameterDef>,
    pub request_body: Option<TypeRef>,
    pub responses: BTreeMap<StatusCode, ResponseDef>,
    pub summary: Option<String>,
    pub description: Option<String>,
}
```

Complete metadata for one API endpoint.

## Service Organization Styles

### PerService (default)

Operations grouped by OpenAPI tags. Each tag becomes a service.

**Example Output**:

```
services/
├── pets.ts      // Operations tagged "pets"
├── owners.ts    // Operations tagged "owners"
└── orders.ts    // Operations tagged "orders"
```

### SingleClient

All operations in one service/client.

**Example Output**:

```
services/
└── client.ts    // All operations
```

### ByTag

Similar to PerService but with different naming conventions.

## Template Development

### Creating a New Template

1. **Create the crate**:

```bash
cargo new --lib templates/python
```

2. **Set up dependencies** (`templates/python/Cargo.toml`):

```toml
[dependencies]
codegen = { path = "../../codegen" }
ast = { path = "../../ast" }
handlebars = "5.0"
serde = { version = "1.0", features = ["derive"] }
```

3. **Implement Generator**:

```rust
pub struct PythonGenerator;

impl Generator for PythonGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();

        // Generate types
        generate_types(&ir.types, &mut vfs, config)?;

        // Generate services
        generate_services(&ir.services, &mut vfs, config)?;

        // Generate package files
        generate_package_files(&mut vfs)?;

        Ok(vfs)
    }

    fn language(&self) -> &str {
        "python"
    }
}
```

4. **Register in generate crate**:

`generate/Cargo.toml`:

```toml
[dependencies]
python = { path = "../templates/python", optional = true }

[features]
default = ["typescript", "python"]
python = ["dep:python"]
```

`generate/src/lib.rs`:

```rust
#[cfg(feature = "python")]
registry.register(Box::new(python::PythonGenerator));
```

### Template Best Practices

1. **Use Handlebars or similar**: Don't build complex code via string concatenation
2. **Separate concerns**: Types generation vs services generation
3. **Handle optionals**: Many OpenAPI fields are optional
4. **Add documentation**: Include comments from OpenAPI in generated code
5. **Format output**: Run language formatter if possible
6. **Test thoroughly**: Generate from petstore.json and verify compilation
7. **Consider idioms**: Follow target language conventions

## CLI Integration (cli crate)

The CLI ties everything together:

1. Parse command-line arguments (clap)
2. Read OpenAPI spec file
3. Parse to `oas::OpenAPIV3`
4. Convert to GenIR
5. Get generator from registry
6. Generate VirtualFS
7. Write to disk
8. Report success

**Error Handling Strategy**:

- Use `anyhow::Result` for CLI (rich error messages)
- Use `.context()` to add helpful error context
- Print user-friendly error messages
- Use verbose flag for debugging output

## Testing Strategy

### Unit Tests

- Test individual functions in each crate
- Test type conversions (OpenAPI → GenIR)
- Test helper functions

### Integration Tests

- Full pipeline tests in `generate/tests/`
- Generate from `examples/petstore.json`
- Verify VirtualFS contents
- Check generated code compiles (if target toolchain available)

### Example Test

```rust
#[test]
fn test_typescript_generation() {
    let spec = load_petstore_spec();
    let ir: GenIr = spec.into();
    let config = Config::default();
    let generator = TypeScriptGenerator;
    let vfs = generator.generate(&ir, &config).unwrap();

    assert!(vfs.contains("src/types/index.ts"));
    assert!(vfs.contains("src/services/pets.ts"));
}
```

## Extension Points

### Adding New Type Kinds

1. Add variant to `TypeDef` enum
2. Update all match statements
3. Implement conversion from OpenAPI schema
4. Update all templates to handle new type

### Adding Configuration Options

1. Add field to `Config` struct
2. Add CLI flag in `cli/src/main.rs`
3. Use in templates as needed
4. Document in help text and README

### Adding Response Formats

1. Extend `ResponseDef` if needed
2. Update GenIR conversion
3. Update templates to generate appropriate code

## Performance Considerations

- **Parsing**: Uses `serde_json`, very fast
- **IR Generation**: In-memory, negligible overhead
- **Code Generation**: Template rendering is fast for typical specs
- **Disk I/O**: VirtualFS writes atomically

For large specs (1000+ operations):

- Memory usage scales linearly with spec size
- Generation time dominated by template rendering
- Disk I/O is buffered

## Future Enhancements

Potential areas for expansion:

- YAML support (in addition to JSON)
- Server stub generation
- More authentication schemes
- Validation code generation
- Mock server generation
- Multiple file formats per language
- Custom template support (user-provided)
- Incremental generation
- Watch mode for development
