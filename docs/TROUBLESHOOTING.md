# Troubleshooting Guide

Common issues and their solutions when working with OAS-Gen2.

## Build Issues

### Error: "edition 2024 is unstable"

**Problem**: Your Rust version doesn't support the 2024 edition.

**Solution**: Update Rust:
```bash
rustup update stable
rustc --version  # Should be 1.75+
```

### Error: "failed to load manifest"

**Problem**: Workspace configuration issue.

**Solution**: Ensure all crates are listed in root `Cargo.toml`:
```toml
[workspace]
members = ["ast", "cli", "codegen", "generate", "parser", "templates/typescript"]
```

### Error: "package collision"

**Problem**: Two crates with the same name.

**Solution**: Ensure unique crate names in each `Cargo.toml`.

## Runtime Issues

### Error: "Failed to parse OpenAPI specification"

**Problem**: Invalid JSON or malformed OpenAPI spec.

**Solutions**:
1. Validate your OpenAPI spec:
   ```bash
   # Using online validator
   # or install openapi-spec-validator
   pip install openapi-spec-validator
   openapi-spec-validator your-spec.json
   ```

2. Check JSON syntax:
   ```bash
   jq . your-spec.json
   ```

3. Ensure OpenAPI 3.x (not 2.x/Swagger):
   ```json
   {
     "openapi": "3.0.0",  // Must start with 3.x
     ...
   }
   ```

### Error: "Template 'xyz' not found"

**Problem**: Generator not registered or feature not enabled.

**Solutions**:
1. Check available templates:
   ```rust
   // In generate/src/lib.rs
   impl GeneratorRegistry {
       pub fn with_defaults() -> Self {
           // Ensure your template is registered here
       }
   }
   ```

2. Enable feature in `Cargo.toml`:
   ```toml
   [features]
   default = ["typescript", "your-template"]
   ```

3. Rebuild:
   ```bash
   cargo clean
   cargo build
   ```

### Error: "Failed to write files"

**Problem**: Permission issues or invalid output path.

**Solutions**:
1. Check directory permissions:
   ```bash
   ls -la
   ```

2. Try different output directory:
   ```bash
   cargo run --bin oas-gen -- spec.json -t typescript -o ~/tmp/output
   ```

3. Ensure parent directories exist.

## Generation Issues

### Generated TypeScript Doesn't Compile

**Problem**: Type mapping or syntax errors.

**Debugging Steps**:
1. Check generated files:
   ```bash
   cargo run --bin oas-gen -- examples/petstore.json -t typescript -v
   cat petstore-typescript/src/types/index.ts
   ```

2. Try to compile:
   ```bash
   cd petstore-typescript
   npm install
   npx tsc --noEmit
   ```

3. Check for:
   - Missing imports
   - Invalid type syntax
   - Incorrect naming

**Solutions**:
- Fix type mapping in template
- Add missing imports
- Escape reserved keywords

### Missing Types in Generated Code

**Problem**: OpenAPI types not converted to GenIR.

**Debugging**:
1. Add debug output in CLI:
   ```rust
   if args.verbose {
       eprintln!("GenIR types: {:#?}", gen_ir.types);
   }
   ```

2. Check AST conversion:
   ```bash
   cargo test -p ast -- --nocapture
   ```

**Solutions**:
- Ensure types are defined in OpenAPI `components.schemas`
- Check that `$ref` references are valid
- Verify AST conversion handles the schema type

### Operations Missing from Generated Services

**Problem**: OpenAPI operations not appearing in generated code.

**Debugging**:
```rust
if args.verbose {
    for service in &gen_ir.services {
        eprintln!("Service: {} ({} ops)", service.name, service.operations.len());
    }
}
```

**Common Causes**:
1. Missing `operationId` in OpenAPI
2. No tags on operations
3. ServiceStyle filtering

**Solutions**:
- Add `operationId` to all operations
- Add tags for organization
- Try different `--service-style`

## Test Failures

### Unit Test Failing

**Debugging**:
```bash
# Run specific test with output
cargo test test_name -- --nocapture --test-threads=1

# Run all tests in a crate
cargo test -p parser -- --nocapture
```

**Common Issues**:
- Hardcoded paths
- Missing test data
- State pollution between tests

### Integration Test Failing

**Debugging**:
```bash
# Run integration tests
cargo test --test integration_test -- --nocapture

# Check test artifacts
ls -la target/debug/deps/
```

**Solutions**:
- Ensure test data files exist
- Check file paths are relative to crate root
- Clean and rebuild

## CLI Issues

### "--help" Shows Wrong Information

**Problem**: Clap derive attributes out of sync.

**Solution**: Update `Args` struct in `cli/src/main.rs`:
```rust
#[derive(ClapParser, Debug)]
#[command(about = "Your updated description")]
struct Args {
    // Ensure help text matches functionality
    #[arg(short, long, help = "Your help text")]
    your_flag: bool,
}
```

### Verbose Flag Not Working

**Problem**: Verbose checks missing or incorrect.

**Solution**: Add verbose checks throughout:
```rust
if args.verbose {
    eprintln!("Debug info: {}", info);
}
```

## Development Issues

### Rust Analyzer Errors

**Problem**: IDE showing errors but code compiles.

**Solutions**:
1. Reload window (VS Code: Cmd+Shift+P → "Reload Window")
2. Restart Rust Analyzer
3. Clear cache:
   ```bash
   cargo clean
   rm -rf target/
   ```
4. Update Rust Analyzer extension

### Cargo Check Slow

**Problem**: Large dependency tree or incremental compilation issues.

**Solutions**:
1. Use cargo-watch:
   ```bash
   cargo install cargo-watch
   cargo watch -x check
   ```

2. Enable parallel compilation:
   ```toml
   # .cargo/config.toml
   [build]
   jobs = 8
   ```

3. Use `cargo check` instead of `cargo build`

### Clippy Too Strict

**Problem**: Too many clippy warnings.

**Solutions**:
1. Allow specific lints:
   ```rust
   #[allow(clippy::too_many_arguments)]
   fn my_function() { }
   ```

2. Configure in `Cargo.toml`:
   ```toml
   [lints.clippy]
   too_many_arguments = "allow"
   ```

## Template Development Issues

### Template Not Rendering

**Problem**: Handlebars template syntax errors.

**Debugging**:
```rust
use handlebars::Handlebars;

let mut hb = Handlebars::new();
match hb.register_template_string("test", template) {
    Ok(_) => println!("Template OK"),
    Err(e) => eprintln!("Template error: {}", e),
}
```

**Common Errors**:
- Unclosed `{{#each}}`
- Wrong helper names
- Invalid context data

### Generated Code Has Wrong Formatting

**Problem**: Generated code doesn't follow language conventions.

**Solutions**:
1. Use language formatter:
   ```rust
   // After generating
   std::process::Command::new("prettier")
       .arg("--write")
       .arg(&output_path)
       .spawn()?;
   ```

2. Fix template whitespace
3. Use language-specific formatting libraries

### VirtualFS Not Writing Files

**Problem**: Files not appearing on disk.

**Debugging**:
```rust
println!("VFS has {} files", vfs.len());
for (path, _) in vfs.files() {
    println!("  {}", path.display());
}
```

**Solutions**:
- Ensure `vfs.write_to_disk()` is called
- Check paths are relative
- Verify output directory permissions

## Performance Issues

### Generation Very Slow

**Problem**: Large OpenAPI spec taking too long.

**Profiling**:
```bash
cargo build --release
time ./target/release/oas-gen large-spec.json -t typescript
```

**Optimization Ideas**:
- Cache template compilation
- Parallelize file generation
- Optimize type resolution
- Use string builders efficiently

### High Memory Usage

**Problem**: Generator using too much RAM.

**Solutions**:
- Stream large files instead of loading fully
- Clear intermediate data structures
- Process types in batches
- Profile with `cargo flamegraph`

## Common Error Messages

### "no field `x` on type `GenIr`"

**Cause**: API mismatch between crates.

**Solution**: Rebuild all crates:
```bash
cargo clean
cargo build
```

### "trait bound not satisfied"

**Cause**: Missing trait implementation or wrong types.

**Solution**: Check trait requirements:
```rust
// Ensure all required traits are implemented
#[derive(Debug, Clone, Serialize)]
pub struct MyType { }
```

### "recursive type has infinite size"

**Cause**: Circular type references.

**Solution**: Use `Box` for recursive types:
```rust
pub struct Node {
    pub children: Vec<Box<Node>>,
}
```

## Getting More Help

### Enable Debug Logging

Add to code:
```rust
#[cfg(debug_assertions)]
eprintln!("Debug: {:?}", variable);
```

### Check Examples

```bash
# Run example generation
cargo run --bin oas-gen -- examples/petstore.json -t typescript -v

# Check example output
ls -la petstore-typescript/
```

### Read the Source

The codebase is well-structured:
- Start with `cli/src/main.rs` for CLI flow
- Check `codegen/src/lib.rs` for core types
- Look at `templates/typescript/` for template example

### Ask for Help

1. Check existing issues on GitHub
2. Open new issue with:
   - Rust version: `rustc --version`
   - Command run: `cargo run ...`
   - Error message
   - Minimal reproduction

### Useful Commands

```bash
# Full clean rebuild
cargo clean && cargo build

# Check everything
cargo fmt && cargo clippy && cargo test

# Generate with max verbosity
RUST_BACKTRACE=1 cargo run --bin oas-gen -- spec.json -t typescript -v

# Profile build times
cargo build --timings

# Check dependency tree
cargo tree
```

