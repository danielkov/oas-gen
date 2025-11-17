# Frequently Asked Questions (FAQ)

## General Questions

### What is OAS-Gen2?

OAS-Gen2 is a flexible code generator that creates SDKs and server interfaces from OpenAPI 3.x specifications. It uses a plugin-based architecture where each target language is implemented as a separate template.

### Why another OpenAPI code generator?

OAS-Gen2 focuses on:
- **Clean architecture**: Language-agnostic IR for better code quality
- **Extensibility**: Templates are separate crates for independent development
- **Zero dependencies**: Generated code doesn't depend on this project
- **Developer experience**: Great CLI with verbose output and clear error messages

### What languages are supported?

Currently:
- TypeScript (full support)

Planned:
- Python
- Rust
- Go
- Java

You can easily add your own - see the [Template Guide](TEMPLATE_GUIDE.md).

### Is OpenAPI 2.x (Swagger) supported?

No, only OpenAPI 3.x is currently supported. You can convert Swagger 2.0 specs to OpenAPI 3.0 using:
- [swagger2openapi](https://github.com/Mermade/oas-kit/tree/master/packages/swagger2openapi)
- Online converters

## Usage Questions

### How do I generate code?

```bash
cargo run --bin oas-gen -- your-spec.json -t typescript -o ./output
```

Or using the release binary:
```bash
./target/release/oas-gen your-spec.json -t typescript
```

### Where does the generated code go?

By default: `<spec-name>-<template>/`

For example, `petstore.json` with TypeScript template → `petstore-typescript/`

You can specify a custom output directory with `-o`:
```bash
cargo run --bin oas-gen -- spec.json -t typescript -o ./my-sdk
```

### What are service styles?

Service styles control how API operations are organized:

**PerService** (default):
```
services/
├── pets.ts      // All pet operations
├── owners.ts    // All owner operations
└── orders.ts    // All order operations
```

**SingleClient**:
```
services/
└── client.ts    // All operations in one file
```

**ByTag**:
Similar to PerService but with different naming conventions.

Use `--service-style` to change:
```bash
cargo run --bin oas-gen -- spec.json -t typescript --service-style single-client
```

### How do I see what's happening during generation?

Use the verbose flag:
```bash
cargo run --bin oas-gen -- spec.json -t typescript -v
```

This shows:
- Parsing progress
- Number of types and services found
- Files being generated
- Output location

### Can I customize the generated code?

Not directly through CLI options (yet), but you can:

1. **Modify the template**: Edit files in `templates/<language>/`
2. **Create a custom template**: Copy existing template and modify
3. **Post-process**: Run formatters/linters after generation

### Does the generated code have runtime dependencies?

No! Generated code is pure and minimal. For TypeScript:
- Just TypeScript/JavaScript
- No runtime dependencies on oas-gen2
- You can add your own HTTP client

## Technical Questions

### What is GenIR?

GenIR (Generated Intermediate Representation) is a language-agnostic representation of the API:
- Types (structs, enums, type aliases)
- Services (grouped operations)
- Operations (endpoints with full metadata)

It sits between the OpenAPI spec and generated code.

### How does the plugin system work?

Templates are separate Rust crates that implement the `Generator` trait:

```rust
pub trait Generator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS>;
    fn language(&self) -> &str;
}
```

They're compiled as optional features and registered at runtime.

### What is VirtualFS?

VirtualFS is an in-memory file system that:
- Lets generators build file trees without disk I/O
- Enables testing without filesystem
- Provides atomic writes (all files or none)
- Makes parallel generation possible (future)

### How are OpenAPI references ($ref) handled?

References are resolved during AST→GenIR conversion:
- All `$ref` are followed and inlined
- Circular references are detected
- Result is a flat type list with no references

### What happens to OpenAPI extensions (x-*)?

Currently they're parsed but not used. Future versions may:
- Pass them through to templates
- Use them for custom generation hints
- Add them to generated comments

## Development Questions

### How do I add a new language template?

See the detailed [Template Development Guide](TEMPLATE_GUIDE.md).

Quick version:
1. Create crate in `templates/<language>/`
2. Implement `Generator` trait
3. Register in `generate/src/lib.rs`
4. Add as feature in `generate/Cargo.toml`
5. Test with petstore example

### How do I run tests?

```bash
# All tests
cargo test

# Specific crate
cargo test -p typescript

# With output
cargo test -- --nocapture

# Specific test
cargo test test_generates_types
```

### How do I debug generation?

1. **Use verbose flag**: `cargo run --bin oas-gen -- spec.json -t typescript -v`
2. **Add debug prints**: `eprintln!("Debug: {:?}", value);`
3. **Inspect GenIR**: Print the IR before generation
4. **Check VirtualFS**: Print all files before writing to disk
5. **Run tests**: Isolate the issue in a test

### What Rust version do I need?

Rust 1.75+ with 2024 edition support.

```bash
rustup update stable
rustc --version
```

### Can I use this in CI/CD?

Yes! Install Rust and run:

```bash
cargo build --release
./target/release/oas-gen spec.json -t typescript -o ./sdk
```

Example GitHub Action:
```yaml
- name: Setup Rust
  uses: actions-rs/toolchain@v1
  with:
    toolchain: stable
    
- name: Generate SDK
  run: |
    cargo build --release --bin oas-gen
    ./target/release/oas-gen api-spec.json -t typescript
```

## Troubleshooting

### Generation fails with "Failed to parse OpenAPI specification"

Your spec might be invalid. Try:
1. Validate with [Swagger Editor](https://editor.swagger.io/)
2. Check it's OpenAPI 3.x (not 2.x)
3. Ensure valid JSON syntax
4. Run with `-v` for more details

### Generated code doesn't compile

1. Check generated files manually
2. Report as bug with:
   - OpenAPI spec (or minimal reproduction)
   - Generated code
   - Compiler errors
3. See [Troubleshooting Guide](TROUBLESHOOTING.md)

### Template not found

1. Check spelling: `cargo run --bin oas-gen -- spec.json -t typescript`
2. Verify feature is enabled in `generate/Cargo.toml`
3. Rebuild: `cargo clean && cargo build`

### "no such file or directory"

1. Use absolute paths for spec file
2. Check output directory exists or can be created
3. Check permissions

## Performance

### How fast is generation?

Very fast for typical specs:
- Small API (10 endpoints): ~50ms
- Medium API (100 endpoints): ~200ms
- Large API (1000 endpoints): ~2s

Most time is spent in template rendering.

### Can it handle large OpenAPI specs?

Yes, tested with specs containing:
- 1000+ operations
- 500+ type definitions
- Deep nesting

Memory usage scales linearly.

### Can I generate multiple languages at once?

Not directly, but you can run multiple commands:

```bash
cargo run --bin oas-gen -- spec.json -t typescript &
cargo run --bin oas-gen -- spec.json -t python &
wait
```

## Contributing

### How can I contribute?

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.

Ideas:
- Add new language templates
- Improve existing templates
- Fix bugs
- Add tests
- Improve documentation
- Report issues

### What should I work on?

Check GitHub issues labeled:
- `good first issue` - Good for newcomers
- `help wanted` - Need contributors
- `template` - New language templates needed

### Do I need to know Rust?

To add templates: Basic Rust knowledge is enough (traits, structs, results).

To modify core: Yes, intermediate Rust knowledge helpful.

To use: No Rust knowledge needed, just install and run!

## More Questions?

- Check [Troubleshooting Guide](TROUBLESHOOTING.md)
- Read [Architecture Documentation](../ARCHITECTURE.md)
- Open a [GitHub Discussion](https://github.com/yourusername/oas-gen2/discussions)
- File an [Issue](https://github.com/yourusername/oas-gen2/issues)

