# Project Overview

Quick reference guide for AI assistants working with OAS-Gen2.

## What is This Project?

A Rust-based OpenAPI 3.x code generator with a plugin architecture. Transforms OpenAPI specifications into language-specific SDKs.

## Project Type

- **Language**: Rust (edition 2024)
- **Structure**: Cargo workspace (monorepo)
- **Binary**: CLI tool (`oas-gen`)
- **Architecture**: Pipeline-based (Parser → AST → GenIR → Generator → Output)

## Quick Commands

```bash
# Build everything
cargo build

# Run CLI
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v

# Run tests
cargo test

# Build release
cargo build --release

# Format code
cargo fmt

# Lint
cargo clippy

# Generate docs
cargo doc --open
```

## Workspace Crates

| Crate                  | Purpose                                           | Type    | Key Files                         |
| ---------------------- | ------------------------------------------------- | ------- | --------------------------------- |
| `parser2`              | Parse OpenAPI 3.x JSON/YAML (uses oas3)           | Library | `parser2/src/lib.rs`              |
| `ir`                   | Convert OpenAPI → GenIR                           | Library | `ir/src/lib.rs`                   |
| `codegen`              | Core traits & types (Generator, VirtualFS, GenIR) | Library | `codegen/src/lib.rs`              |
| `generate`             | Generator registry & plugin system                | Library | `generate/src/lib.rs`             |
| `templates/typescript` | TypeScript code generator                         | Library | `templates/typescript/src/lib.rs` |
| `cli`                  | Command-line interface                            | Binary  | `cli/src/main.rs`                 |

**Note:** Legacy crates `parser/` and `ast/` are being phased out. Use `parser2/` and `ir/` instead.

## Data Flow

```
┌──────────────┐
│ OpenAPI JSON │
└──────┬───────┘
       │ parse (parser2 crate)
       ▼
┌──────────────┐
│ oas3::Spec   │ (strongly-typed OpenAPI)
└──────┬───────┘
       │ convert (ir crate)
       ▼
┌──────────────┐
│    GenIR     │ (language-agnostic IR)
└──────┬───────┘
       │ generate (template crate)
       ▼
┌──────────────┐
│  VirtualFS   │ (in-memory files)
└──────┬───────┘
       │ write
       ▼
┌──────────────┐
│  Disk Files  │
└──────────────┘
```

## Core Types (codegen crate)

### GenIr

The intermediate representation containing:

- `types: Vec<TypeDef>` - All type definitions
- `services: Vec<ServiceDef>` - Grouped operations

### TypeDef

```rust
enum TypeDef {
    Struct(StructDef),      // Objects
    Enum(EnumDef),          // Enumerations
    TypeAlias(TypeAliasDef),// Type aliases
    Primitive(PrimitiveDef),// Built-ins
}
```

### ServiceDef

Groups related API operations:

- `name: String`
- `operations: Vec<OperationDef>`
- `description: Option<String>`

### OperationDef

Complete API endpoint metadata:

- `id, method, path`
- `parameters, request_body, responses`
- `summary, description`

## CLI Usage

```bash
# Basic generation
oas-gen examples/petstore.json -t typescript

# With options
oas-gen spec.json \
  --template typescript \
  --output ./my-sdk \
  --service-style per-service \
  --verbose

# Service styles: per-service | single-client | by-tag
```

## Adding a New Template

1. **Create crate**: `cargo new --lib templates/python`
2. **Implement trait**: `impl Generator for PythonGenerator`
3. **Register**: Add to `generate/src/lib.rs` and `generate/Cargo.toml`
4. **Test**: `cargo test -p python`

See `docs/TEMPLATE_GUIDE.md` for detailed guide.

## Testing Strategy

- **Unit tests**: In each crate (`#[cfg(test)]` modules)
- **Integration tests**: In `crate/tests/` directories
- **CLI tests**: Manual testing with `examples/petstore.json`
- **Template tests**: Generate code and verify it compiles

## Key Files to Know

### For Understanding Architecture

- `codegen/src/lib.rs` - Core types (Generator trait, VirtualFS, Config)
- `ir/src/lib.rs` - OpenAPI → GenIR conversion
- `ir/src/gen_ir.rs` - GenIR data structures
- `cli/src/main.rs` - Full pipeline in action

### For Template Development

- `templates/typescript/src/lib.rs` - Reference implementation
- `templates/typescript/src/templates.rs` - Handlebars templates
- `codegen/src/lib.rs` - Generator trait definition

### For Testing

- `examples/petstore.json` - Comprehensive test spec
- `generate/tests/integration_test.rs` - Integration test example

## Common Tasks

### Run the Generator

```bash
cd /Users/danielkov/projects/oas-gen2
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v
```

### Add a New Type Variant

1. Modify `TypeKind` enum in `ir/src/gen_ir.rs`
2. Update OpenAPI conversion in `ir/src/lib.rs`
3. Update all templates to handle new variant
4. Add tests

### Change Generated Output

1. Modify template in `templates/<lang>/src/`
2. Rebuild: `cargo build`
3. Test: `cargo run --bin oas-gen -- examples/petstore.json -t <lang>`
4. Verify output compiles in target language

### Debug Generation

```rust
# Add debug prints in cli/src/main.rs
if args.verbose {
    eprintln!("GenIR: {:#?}", gen_ir);
}

# Or in template
eprintln!("Generating type: {}", type_def.name);
```

## Dependencies

### Core

- `oas = "0.2.1"` - OpenAPI types
- `serde = "1.0"` - Serialization
- `serde_json = "1.0"` - JSON parsing

### CLI

- `clap = "4.5"` - Argument parsing
- `anyhow = "1.0"` - Error handling

### Templates

- `handlebars = "5.0"` - Template rendering

## Code Style

- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Follow Rust naming conventions
- Add `///` doc comments for public items
- Use `anyhow::Result` in CLI, custom `Result` in libraries

## Error Handling

**In CLI** (`cli/`):

```rust
use anyhow::{Context, Result};

fn do_thing() -> Result<()> {
    let data = read_file("path")
        .context("Failed to read file")?;
    Ok(())
}
```

**In Libraries** (everything else):

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("something failed: {0}")]
    Failed(String),
}

pub type Result<T> = std::result::Result<T, MyError>;
```

## Configuration

Generated code behavior controlled by `Config`:

- `output_dir` - Base output directory
- `service_style` - How to group operations
- `include_docs` - Generate doc comments
- `lang_options` - Language-specific options

## VirtualFS

In-memory filesystem for generation:

```rust
let mut vfs = VirtualFS::new();
vfs.write_file("src/types.ts", content)?;
vfs.write_file("src/services/pets.ts", content)?;
vfs.write_to_disk("./output")?;
```

## ServiceStyle Options

### PerService (default)

One file per service (grouped by OpenAPI tags).

**Output**:

```
services/
├── pets.ts
├── owners.ts
└── orders.ts
```

### SingleClient

All operations in one client.

**Output**:

```
services/
└── client.ts
```

### ByTag

Group by OpenAPI tags (similar to PerService).

## Important Patterns

### Converting OpenAPI to GenIR

```rust
let spec: oas3::spec::Spec = parser2::parse(&json)?;
let gen_ir: GenIr = spec.into();  // Uses From trait in ir crate
```

### Using Generator

```rust
let registry = GeneratorRegistry::with_defaults();
let vfs = registry.generate("typescript", &gen_ir, &config)?;
vfs.write_to_disk(&output_dir)?;
```

### Type Mapping

```rust
fn map_type_ref(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Array(inner) => format!("{}[]", map_type_ref(inner)),
        TypeRef::Optional(inner) => format!("{} | null", map_type_ref(inner)),
        TypeRef::Primitive(prim) => map_primitive(prim),
    }
}
```

## Documentation Files

| File                      | Purpose                     |
| ------------------------- | --------------------------- |
| `README.md`               | User-facing documentation   |
| `ARCHITECTURE.md`         | System design and structure |
| `CONTRIBUTING.md`         | Contribution guidelines     |
| `docs/TEMPLATE_GUIDE.md`  | How to create templates     |
| `docs/TROUBLESHOOTING.md` | Common issues and solutions |
| `docs/FAQ.md`             | Frequently asked questions  |
| `docs/API_REFERENCE.md`   | API documentation           |
| `.cursorrules`            | AI assistant guidelines     |

## Example Files

- `examples/petstore.json` - Comprehensive test OpenAPI spec
- `examples/README.md` - Example documentation
- `test-generated/` - Generated code examples (gitignored)

## Git Workflow

```bash
# Create feature branch
git checkout -b feature/my-feature

# Make changes
cargo fmt
cargo clippy
cargo test

# Commit
git add .
git commit -m "feat: add something"

# Push and create PR
git push origin feature/my-feature
```

## Release Process

1. Update `CHANGELOG.md`
2. Update version in all `Cargo.toml` files
3. Build release: `cargo build --release`
4. Test: `./target/release/oas-gen examples/petstore.json -t typescript`
5. Tag: `git tag v0.x.0`
6. Push: `git push --tags`

## Performance

- Small APIs (~10 endpoints): ~50ms
- Medium APIs (~100 endpoints): ~200ms
- Large APIs (~1000 endpoints): ~2s

Bottleneck: Template rendering

## Future Enhancements

- [ ] YAML support
- [ ] More templates (Python, Rust, Go)
- [ ] Server stub generation
- [ ] Custom template support
- [ ] Watch mode
- [ ] Parallel generation
- [ ] Better error messages

## Getting Help

1. Check `docs/TROUBLESHOOTING.md`
2. Read `ARCHITECTURE.md`
3. Look at existing templates
4. Open GitHub issue

## Quick Reference: File Locations

```
oas-gen2/
├── cli/src/main.rs              → CLI entry point
├── codegen/src/lib.rs           → Core types & traits
├── ir/src/lib.rs                → OpenAPI → GenIR conversion
├── ir/src/gen_ir.rs             → GenIR data structures
├── generate/src/lib.rs          → Generator registry
├── templates/typescript/src/    → TypeScript generator
├── parser2/src/lib.rs           → OpenAPI parser (uses oas3)
├── examples/petstore.json       → Test data
└── docs/                        → All documentation
```

## When Working On...

### CLI Changes

Edit: `cli/src/main.rs`
Test: `cargo run --bin oas-gen -- examples/petstore.json -t typescript -v`

### Core Type Changes

Edit: `codegen/src/lib.rs`
Rebuild all: `cargo clean && cargo build`
Update templates to handle changes

### Template Changes

Edit: `templates/<lang>/src/`
Test: `cargo run --bin oas-gen -- examples/petstore.json -t <lang>`
Verify: Compile generated code

### Parser Changes

Edit: `parser2/src/lib.rs`
Test: `cargo test -p parser2`

### IR Changes

Edit: `ir/src/lib.rs` or `ir/src/gen_ir.rs`
Test: `cargo test -p ir`
Check: All templates still work

## Environment

- **Rust**: 1.75+ (2024 edition)
- **OS**: macOS, Linux, Windows
- **Editor**: Any (recommend Rust Analyzer)

## License

MIT - See LICENSE file
