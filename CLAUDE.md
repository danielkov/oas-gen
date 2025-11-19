# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OAS-Gen2 is a Rust-based OpenAPI 3.x code generator with a plugin architecture. It transforms OpenAPI specifications into language-specific SDKs through a clean pipeline: OpenAPI Spec → Parser → AST/GenIR → Generator → VirtualFS → Disk.

This is a Cargo workspace (Rust 2024 edition) with a CLI tool (`oas-gen`) that coordinates the generation process.

## Common Commands

```bash
# Build the project
cargo build
make build

# Run tests
cargo test
make test

# Test specific crate
cargo test -p parser
cargo test -p ast
cargo test -p codegen
cargo test -p typescript

# Use make targets
make run              # Generate from petstore example
make run-unkey        # Generate from unkey example
make run-stripe       # Generate from stripe example
make run-openrouter   # Generate from openrouter example

# Or run the CLI (development)
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v

# Build release binary
cargo build --release
make release

# Format and lint
cargo fmt
make fmt
cargo clippy -- -D warnings
make lint

# Full check pipeline (format + lint + test)
make check

# Generate documentation
cargo doc --open
make doc
```

## Architecture

### Pipeline Flow

```
OpenAPI Spec → Parser (parser2) → AST Conversion (ir) → GenIR (codegen) → Generator (templates/*) → VirtualFS → Disk
```

### Workspace Crates

- **`parser2/`**: Parses OpenAPI 3.x JSON/YAML specifications (uses `oas3` crate)
- **`ir/`**: Converts OpenAPI AST to language-agnostic GenIR (Intermediate Representation)
- **`codegen/`**: Core abstractions - `Generator` trait, `VirtualFS`, `Config`, and `GenIr` type
- **`generate/`**: Generator registry and plugin loading system
- **`templates/typescript/`**: TypeScript SDK generator (reference implementation)
- **`cli/`**: Command-line interface (`oas-gen` binary)

Note: There are legacy crates (`parser/`, `ast/`) that are being phased out. Use `parser2/` and `ir/` instead.

### Key Data Structures

**GenIr** (in `ir/gen_ir.rs`): The language-agnostic intermediate representation containing:

- `types: BTreeMap<StableId, TypeDecl>` - All type definitions
- `services: Vec<Service>` - Grouped operations
- `auth_schemes: Vec<AuthScheme>` - Authentication methods
- `server_sets: Vec<ServerSet>` - Server configurations
- `api: ApiMeta` - API metadata

**TypeDecl**: Represents types (Struct, Enum, Alias)

- Uses `StableId` for stable type references across renames
- Contains `CanonicalName` with multiple case styles (snake, pascal, camel, kebab, upper)

**Service**: Groups related API operations

- Operations grouped by OpenAPI tags
- Each operation has full HTTP metadata (method, path, params, body, responses)

**VirtualFS**: In-memory filesystem for generated code

- All generation happens in memory before atomic disk write
- Supports both text and binary files

### Generator Trait

All language generators implement this trait (in `codegen/src/lib.rs`):

```rust
pub trait Generator: Send + Sync {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS>;
    fn language(&self) -> &str;
    fn validate(&self, _ir: &GenIr) -> Result<()> { Ok(()) }

    // Optional hooks:
    fn before_generate(&self, _ir: &GenIr, _config: &Config) -> Result<()> { Ok(()) }
    fn after_generate(&self, _vfs: &VirtualFS, _ir: &GenIr, _config: &Config) -> Result<()> { Ok(()) }
    fn after_write_to_disk(&self, _output_dir: &Path, _vfs: &VirtualFS) -> Result<()> { Ok(()) }
}
```

## Configuration Options

**ServiceStyle** (affects how operations are organized):

- `PerService` (default): One file per service (grouped by OpenAPI tags)
- `SingleClient`: All operations in one client
- `ByTag`: Group by OpenAPI tags (similar to PerService)

**Config** struct:

- `service_style: ServiceStyle`
- `include_docs: bool` - Whether to generate documentation comments
- `lang_options: BTreeMap<String, Value>` - Language-specific options

## Adding a New Template

1. Create new crate: `cargo new --lib templates/python`

2. Add dependencies in `templates/python/Cargo.toml`:

```toml
[dependencies]
codegen = { path = "../../codegen" }
ir = { path = "../../ir" }
handlebars = "5.0"  # Optional, for templating
serde = { version = "1.0", features = ["derive"] }
```

3. Implement `Generator` trait in `templates/python/src/lib.rs`

4. Register in `generate/Cargo.toml`:

```toml
[dependencies]
python = { path = "../templates/python", optional = true }

[features]
default = ["typescript", "python"]
python = ["dep:python"]
```

5. Register in `generate/src/lib.rs`:

```rust
#[cfg(feature = "python")]
registry.register(Box::new(python::PythonGenerator::new()));
```

6. Update workspace in root `Cargo.toml`:

```toml
members = [..., "templates/python"]
```

## Important Patterns

### Converting OpenAPI to GenIR

The conversion happens in `ir/src/lib.rs`:

```rust
let spec: oas3::spec::Spec = parser2::parse(&json)?;
let gen_ir: GenIr = spec.into();  // Uses From trait
```

### Using the Generator

From CLI or integration code:

```rust
let registry = generate::GeneratorRegistry::with_defaults();
let vfs = registry.generate("typescript", &gen_ir, &config)?;
vfs.write_to_disk(&output_dir)?;
registry.after_write_to_disk("typescript", &output_dir, &vfs)?;
```

### CanonicalName for Type-Safe Naming

All names (types, fields, operations) use `CanonicalName` which provides multiple case styles:

```rust
let name = CanonicalName::from_string("my-api-name");
// name.snake → "my_api_name"
// name.pascal → "MyApiName"
// name.camel → "myApiName"
// name.kebab → "my-api-name"
// name.upper → "MY_API_NAME"
```

### StableId for Type References

Types use `StableId` instead of strings for stability:

```rust
let id = StableId::new("User");
gen_ir.types.get(&id)  // Access type by stable ID
```

## Error Handling

**In CLI** (`cli/`):

- Use `anyhow::Result` for rich error messages
- Add context with `.context("description")?`

**In Libraries** (all other crates):

- Use `thiserror` for structured error types
- Define custom error enums with `#[derive(Error)]`
- Return custom `Result<T>` type

## Development Notes

### Two Parser Crates

- `parser/`: Legacy parser (being phased out)
- `parser2/`: Current parser using `oas3` crate - **use this one**

### Two AST Crates

- `ast/`: Legacy AST conversion (being phased out)
- `ir/`: Current IR generation - **use this one**

The conversion from OpenAPI to GenIR happens in `ir/src/lib.rs` with `impl From<oas3::spec::Spec> for GenIr`.

### File Organization

```
oas-gen2/
├── cli/src/main.rs              # CLI entry point, argument parsing
├── codegen/src/lib.rs           # Core types: Generator trait, VirtualFS, Config, GenIr re-export
├── ir/src/lib.rs                # OpenAPI → GenIR conversion logic
├── ir/src/gen_ir.rs             # GenIr data structures
├── generate/src/lib.rs          # GeneratorRegistry, plugin system
├── parser2/src/lib.rs           # OpenAPI parser (uses oas3 crate)
├── templates/typescript/src/    # TypeScript generator implementation
└── examples/                    # Test OpenAPI specs
```

### Testing Strategy

- **Unit tests**: In `#[cfg(test)]` modules within each crate
- **Integration tests**: Full pipeline tests in `crate/tests/` directories
- Test with `examples/petstore.json` - comprehensive test spec with multiple resource types, CRUD operations, enums, pagination, etc.
- Verify generated code compiles in target language

### CLI Usage

```bash
# Basic usage
oas-gen <SPEC> --template <TEMPLATE>

# With options
oas-gen spec.json -t typescript -o ./sdk --service-style single-client -v

# Available options:
#   <SPEC>                         OpenAPI spec file (JSON or YAML)
#   -t, --template <TEMPLATE>      Template name (e.g., "typescript")
#   -o, --output <DIR>             Output directory (default: <spec>-<template>)
#   --service-style <STYLE>        per-service | single-client | by-tag
#   --no-docs                      Skip documentation comments
#   -v, --verbose                  Verbose output
```

### When Making Changes

**Core Type Changes** (in `codegen/` or `ir/`):

1. Update the type definition
2. Rebuild all: `cargo clean && cargo build`
3. Update all templates to handle the change
4. Update tests

**Template Changes** (in `templates/<lang>/`):

1. Modify generator code or templates
2. Test: `cargo run --bin oas-gen -- examples/petstore.json -t <lang> -v`
3. Verify generated code compiles in target language

**Parser Changes** (in `parser2/`):

1. Modify parsing logic
2. Test: `cargo test -p parser2`
3. Ensure GenIR conversion in `ir/` handles new structures

**CLI Changes** (in `cli/`):

1. Update argument parsing or main flow
2. Test: `cargo run --bin oas-gen -- examples/petstore.json -t typescript -v`

## Code Style Guidelines

- Use `edition = "2024"` in all Cargo.toml files
- Follow standard Rust naming conventions
- Use `///` doc comments for public items
- Run `cargo fmt` before committing
- Ensure `cargo clippy -- -D warnings` passes
- Use emoji in CLI output for better UX: 🔍 📄 🏗️ 📦 🔨 💾 ✅
- Generated code should have zero runtime dependencies on this project

## Important Considerations

1. **StableId vs String**: Always use `StableId` for type references, not raw strings
2. **CanonicalName**: Use this for all names to automatically get all case styles
3. **VirtualFS**: Always generate into VirtualFS first, then write atomically to disk
4. **Optional Fields**: Many OpenAPI fields are optional - handle `Option<T>` carefully
5. **Type Resolution**: References like `#/components/schemas/Pet` must be resolved to `StableId`
6. **Idempotency**: Operations automatically marked idempotent based on HTTP method (GET, PUT, DELETE)
7. **Binary Name**: The CLI binary is `oas-gen`, not `cli`

## Template Best Practices

1. Use Handlebars or similar template engine (don't build complex code via string concatenation)
2. Separate type generation from service generation
3. Handle all `TypeKind` variants (Struct, Enum, Alias)
4. Include documentation from OpenAPI in generated code if `config.include_docs` is true
5. Follow target language idioms and conventions
6. Format generated code with target language formatter if possible
7. Use the `after_write_to_disk` hook for post-generation commands (npm install, formatting, etc.)

## Debugging

```bash
# Run with verbose output
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v

# Run with backtrace
RUST_BACKTRACE=1 cargo run --bin oas-gen -- examples/petstore.json -t typescript

# Add debug prints in generator
eprintln!("Generating type: {:?}", type_decl.name);

# Run specific test with output
cargo test test_name -- --nocapture
```
