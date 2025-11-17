# OAS-Gen2

A flexible OpenAPI 3.x code generator with pluggable template architecture.

## Features

- 🔌 **Plugin Architecture**: Templates are separate crates for independent development
- 🎯 **Language Agnostic IR**: Clean intermediate representation optimized for code generation
- 🚀 **Zero Runtime Dependencies**: Generated code is pure and minimal
- 📦 **Multiple Output Styles**: Per-service, single-client, or by-tag organization
- 🎨 **Handlebars Templates**: Easy-to-customize code generation

## Architecture

```
OpenAPI Spec → Parser → AST/GenIR → Generator → Virtual FS → Disk
```

### Crates

- **`parser`**: Parses OpenAPI 3.x specifications
- **`ast`**: Converts OpenAPI to language-agnostic GenIR
- **`codegen`**: Core generation abstractions (VirtualFS, Generator trait, etc.)
- **`generate`**: Generator registry and plugin loading
- **`templates/*`**: Language-specific generators (TypeScript, Python, etc.)
- **`cli`**: Command-line interface

## Quick Start

### Installation

```bash
cargo build --release
```

### Generate an SDK

```bash
# Generate TypeScript SDK from the example
./target/release/oas-gen examples/petstore.json -t typescript

# Specify custom output directory
./target/release/oas-gen examples/petstore.json -t typescript -o ./my-sdk

# Single client style
./target/release/oas-gen examples/petstore.json -t typescript --service-style single-client

# Verbose output
./target/release/oas-gen examples/petstore.json -t typescript -v
```

### CLI Options

```
Usage: oas-gen [OPTIONS] <SPEC> --template <TEMPLATE>

Arguments:
  <SPEC>  Path to the OpenAPI specification file (JSON or YAML)

Options:
  -t, --template <TEMPLATE>              Template to use (e.g., "typescript")
  -o, --output <DIR>                     Output directory [default: <spec>-<template>]
      --service-style <SERVICE_STYLE>    Service organization [default: per-service]
                                         [possible values: per-service, single-client, by-tag]
      --no-docs                          Don't include documentation comments
  -v, --verbose                          Verbose output
  -h, --help                             Print help
  -V, --version                          Print version
```

## Example Output

Given `examples/petstore.json`, the TypeScript generator creates:

```
petstore-typescript/
├── package.json
├── tsconfig.json
└── src/
    ├── index.ts
    ├── types/
    │   └── index.ts          # Pet, Owner, Order, enums, etc.
    └── services/
        ├── pets.ts           # Pets service
        ├── owners.ts         # Owners service
        └── orders.ts         # Orders service
```

## Creating a New Template

1. Create a new crate in `templates/`:

```bash
cargo new --lib templates/python
```

2. Add dependencies in `templates/python/Cargo.toml`:

```toml
[dependencies]
codegen = { path = "../../codegen" }
ast = { path = "../../ast" }
serde = { version = "1.0", features = ["derive"] }
```

3. Implement the `Generator` trait:

```rust
use codegen::{Generator, GenIr, Config, VirtualFS, Result};

pub struct PythonGenerator;

impl Generator for PythonGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        // Add your generation logic
        Ok(vfs)
    }

    fn language(&self) -> &str {
        "python"
    }
}
```

4. Register it in `generate/Cargo.toml`:

```toml
[dependencies]
python = { path = "../templates/python", optional = true }

[features]
default = ["typescript", "python"]
```

5. Add to the registry in `generate/src/lib.rs`:

```rust
#[cfg(feature = "python")]
registry.register(Box::new(python::PythonGenerator::new()));
```

## Development

### Run Tests

```bash
cargo test
```

### Build Documentation

```bash
cargo doc --open
```

### Test the CLI

```bash
# Run without building release
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v
```

## Example: Pet Store API

The included `examples/petstore.json` demonstrates:

- ✅ Multiple resource types (Pets, Owners, Orders)
- ✅ CRUD operations
- ✅ Complex nested objects
- ✅ Enumerations (Species, PetStatus, OrderStatus)
- ✅ Arrays and collections
- ✅ Optional and nullable fields
- ✅ Date/time and UUID formats
- ✅ Pagination
- ✅ Path parameters
- ✅ Query parameters
- ✅ Request bodies
- ✅ Multiple response codes
- ✅ Tags for organization

## License

MIT
