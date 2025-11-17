# Contributing to OAS-Gen2

Thank you for your interest in contributing! This guide will help you get started.

## Getting Started

### Prerequisites

- Rust 1.75+ (with 2024 edition support)
- Cargo
- Git

### Clone and Build

```bash
git clone <repository-url>
cd oas-gen2
cargo build
cargo test
```

### Try the CLI

```bash
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v
```

## Project Structure

```
oas-gen2/
├── parser/          # OpenAPI spec parsing
├── ast/            # AST and GenIR construction
├── codegen/        # Core generation abstractions
├── generate/       # Generator registry
├── templates/      # Language-specific generators
│   └── typescript/ # TypeScript generator
├── cli/           # Command-line interface
└── examples/      # Example OpenAPI specs
```

## Development Workflow

### 1. Create a Branch

```bash
git checkout -b feature/my-feature
# or
git checkout -b fix/bug-description
```

### 2. Make Changes

Follow the [Coding Guidelines](#coding-guidelines) below.

### 3. Test Your Changes

```bash
# Run all tests
cargo test

# Test specific crate
cargo test -p typescript

# Run the CLI
cargo run --bin oas-gen -- examples/petstore.json -t typescript -o test-output -v

# Check formatting
cargo fmt --check

# Run clippy
cargo clippy -- -D warnings
```

### 4. Commit Changes

Use clear, descriptive commit messages:

```bash
git add .
git commit -m "feat: add Python template generator"
# or
git commit -m "fix: handle nullable array types in TypeScript"
```

**Commit Message Format**:
- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation changes
- `test:` - Test additions or changes
- `refactor:` - Code refactoring
- `chore:` - Maintenance tasks

### 5. Submit Pull Request

- Push your branch to GitHub
- Open a Pull Request
- Describe your changes clearly
- Link any related issues

## Coding Guidelines

### Rust Style

- Use `cargo fmt` for consistent formatting
- Follow Rust naming conventions:
  - `snake_case` for functions, variables, modules
  - `CamelCase` for types, traits, enums
  - `SCREAMING_SNAKE_CASE` for constants
- Use `cargo clippy` and fix warnings
- Write documentation comments (`///`) for public items

### Error Handling

**In library crates** (parser, ast, codegen, generate):
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("something went wrong: {0}")]
    SomethingWentWrong(String),
}

pub type Result<T> = std::result::Result<T, MyError>;
```

**In CLI**:
```rust
use anyhow::{Context, Result};

fn do_something() -> Result<()> {
    let data = read_file("path")
        .context("Failed to read configuration file")?;
    Ok(())
}
```

### Testing

- Write tests for new functionality
- Use descriptive test names: `test_converts_array_schema_to_vec_type()`
- Test edge cases and error conditions
- Use the petstore example for integration tests

Example:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_creates_types_file() {
        let ir = create_test_ir();
        let config = Config::default();
        let generator = MyGenerator;
        
        let vfs = generator.generate(&ir, &config).unwrap();
        
        assert!(vfs.contains("src/types/index.ts"));
    }
}
```

### Documentation

- Document all public APIs
- Include examples in doc comments
- Update README.md if adding features
- Update ARCHITECTURE.md if changing design

```rust
/// Generates TypeScript types from GenIR.
///
/// # Arguments
///
/// * `ir` - The intermediate representation
/// * `config` - Generation configuration
///
/// # Returns
///
/// A VirtualFS containing the generated files
///
/// # Examples
///
/// ```
/// let ir = GenIr::default();
/// let config = Config::default();
/// let vfs = generate(&ir, &config)?;
/// ```
pub fn generate(ir: &GenIr, config: &Config) -> Result<VirtualFS> {
    // ...
}
```

## Adding a New Template

### Step 1: Create Template Crate

```bash
cargo new --lib templates/python
```

### Step 2: Set Up Cargo.toml

`templates/python/Cargo.toml`:
```toml
[package]
name = "python"
version = "0.1.0"
edition = "2024"

[dependencies]
codegen = { path = "../../codegen" }
ast = { path = "../../ast" }
handlebars = "5.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### Step 3: Implement Generator

`templates/python/src/lib.rs`:
```rust
use codegen::{Generator, GenIr, Config, VirtualFS, Result};

pub struct PythonGenerator;

impl Generator for PythonGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        
        // Implement generation logic
        
        Ok(vfs)
    }
    
    fn language(&self) -> &str {
        "python"
    }
}
```

### Step 4: Register Generator

Add to `generate/Cargo.toml`:
```toml
[dependencies]
python = { path = "../templates/python", optional = true }

[features]
default = ["typescript", "python"]
python = ["dep:python"]
```

Add to `generate/src/lib.rs`:
```rust
#[cfg(feature = "python")]
use python::PythonGenerator;

impl GeneratorRegistry {
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        
        #[cfg(feature = "typescript")]
        registry.register(Box::new(TypeScriptGenerator));
        
        #[cfg(feature = "python")]
        registry.register(Box::new(PythonGenerator));
        
        registry
    }
}
```

### Step 5: Add to Workspace

Update root `Cargo.toml`:
```toml
[workspace]
members = [
    "ast",
    "cli",
    "codegen",
    "generate",
    "parser",
    "templates/typescript",
    "templates/python",  # Add this
]
```

### Step 6: Write Tests

`templates/python/tests/integration_test.rs`:
```rust
use python::PythonGenerator;
use codegen::{Generator, GenIr, Config};

#[test]
fn test_generates_python_files() {
    // Test implementation
}
```

### Step 7: Test End-to-End

```bash
cargo build
cargo run --bin oas-gen -- examples/petstore.json -t python -o test-output
```

## Working with GenIR

### Understanding GenIR Structure

GenIR is the language-agnostic intermediate representation:

```rust
pub struct GenIr {
    pub types: Vec<TypeDef>,
    pub services: Vec<ServiceDef>,
}
```

### Iterating Over Types

```rust
for type_def in &ir.types {
    match type_def {
        TypeDef::Struct(struct_def) => {
            // Handle struct
            println!("Struct: {}", struct_def.name);
        }
        TypeDef::Enum(enum_def) => {
            // Handle enum
            println!("Enum: {}", enum_def.name);
        }
        // ... other variants
    }
}
```

### Iterating Over Services

```rust
for service in &ir.services {
    println!("Service: {}", service.name);
    
    for operation in &service.operations {
        println!("  {} {}", operation.method, operation.path);
    }
}
```

## Common Tasks

### Run Specific Tests

```bash
# All tests
cargo test

# Specific package
cargo test -p typescript

# Specific test
cargo test test_generator_creates_types_file

# With output
cargo test -- --nocapture
```

### Format Code

```bash
cargo fmt
```

### Lint Code

```bash
cargo clippy
```

### Build Documentation

```bash
cargo doc --open
```

### Build Release Binary

```bash
cargo build --release
./target/release/oas-gen --help
```

## Debugging Tips

### Enable Verbose Output

```bash
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v
```

### Inspect GenIR

Add debug prints in `cli/src/main.rs`:
```rust
if args.verbose {
    eprintln!("GenIR: {:#?}", gen_ir);
}
```

### Check Generated VirtualFS

```rust
if args.verbose {
    for (path, content) in vfs.files() {
        eprintln!("=== {} ===", path.display());
        eprintln!("{}", content);
    }
}
```

### Use Rust Analyzer

Install [Rust Analyzer](https://rust-analyzer.github.io/) for your editor/IDE for:
- Code completion
- Inline errors
- Go to definition
- Refactoring tools

## Questions?

- Open an issue for bugs or feature requests
- Start a discussion for questions about architecture
- Read ARCHITECTURE.md for design details

## Code of Conduct

- Be respectful and constructive
- Welcome newcomers
- Focus on the code, not the person
- Assume good intentions

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT).

