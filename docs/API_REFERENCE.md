# API Reference

Internal API documentation for developers working on OAS-Gen2.

## Core Types (codegen crate)

### Generator Trait

The main trait that all language templates must implement.

```rust
pub trait Generator: Send + Sync {
    /// Generate code from GenIR
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS>;
    
    /// Return the language identifier
    fn language(&self) -> &str;
}
```

**Example Implementation**:
```rust
pub struct TypeScriptGenerator;

impl Generator for TypeScriptGenerator {
    fn generate(&self, ir: &GenIr, config: &Config) -> Result<VirtualFS> {
        let mut vfs = VirtualFS::new();
        // Generate code
        Ok(vfs)
    }
    
    fn language(&self) -> &str {
        "typescript"
    }
}
```

---

### GenIr

The language-agnostic intermediate representation.

```rust
pub struct GenIr {
    /// All type definitions extracted from OpenAPI
    pub types: Vec<TypeDef>,
    
    /// Services (grouped operations)
    pub services: Vec<ServiceDef>,
}
```

**Methods**:
- None (plain data struct)

**Usage**:
```rust
// Convert from OpenAPI
let gen_ir: GenIr = openapi_spec.into();

// Iterate types
for type_def in &gen_ir.types {
    // Handle type
}

// Iterate services
for service in &gen_ir.services {
    // Handle service
}
```

---

### TypeDef

Represents all possible type definitions.

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

**Example**:
```rust
TypeDef::Struct(StructDef {
    name: "User".to_string(),
    fields: vec![
        FieldDef {
            name: "id".to_string(),
            type_ref: TypeRef::Primitive(PrimitiveType::Integer),
            required: true,
            description: Some("User ID".to_string()),
        },
        FieldDef {
            name: "name".to_string(),
            type_ref: TypeRef::Primitive(PrimitiveType::String),
            required: true,
            description: None,
        },
    ],
    description: Some("User model".to_string()),
})
```

#### EnumDef

```rust
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<String>,
    pub underlying_type: EnumType,
    pub description: Option<String>,
}

pub enum EnumType {
    String,
    Integer,
}
```

**Example**:
```rust
TypeDef::Enum(EnumDef {
    name: "Status".to_string(),
    variants: vec!["active".to_string(), "inactive".to_string()],
    underlying_type: EnumType::String,
    description: Some("User status".to_string()),
})
```

#### TypeAliasDef

```rust
pub struct TypeAliasDef {
    pub name: String,
    pub target: TypeRef,
    pub description: Option<String>,
}
```

---

### TypeRef

References to types, handles complex types like arrays and optionals.

```rust
pub enum TypeRef {
    /// Reference to a named type
    Named(String),
    
    /// Array of another type
    Array(Box<TypeRef>),
    
    /// Optional/nullable type
    Optional(Box<TypeRef>),
    
    /// Built-in primitive type
    Primitive(PrimitiveType),
}
```

#### PrimitiveType

```rust
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

**Usage**:
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

---

### ServiceDef

Represents a group of related operations.

```rust
pub struct ServiceDef {
    pub name: String,
    pub operations: Vec<OperationDef>,
    pub description: Option<String>,
}
```

**Example**:
```rust
ServiceDef {
    name: "Pets".to_string(),
    operations: vec![
        // GET /pets
        OperationDef { /* ... */ },
        // POST /pets
        OperationDef { /* ... */ },
    ],
    description: Some("Pet management operations".to_string()),
}
```

---

### OperationDef

Complete metadata for one API endpoint.

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

pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}
```

**Example**:
```rust
OperationDef {
    id: "getPets".to_string(),
    method: HttpMethod::Get,
    path: "/pets".to_string(),
    parameters: vec![
        ParameterDef {
            name: "limit".to_string(),
            location: ParameterLocation::Query,
            type_ref: TypeRef::Primitive(PrimitiveType::Integer),
            required: false,
            description: Some("Max records to return".to_string()),
        }
    ],
    request_body: None,
    responses: {
        let mut map = BTreeMap::new();
        map.insert(200, ResponseDef {
            status_code: 200,
            type_ref: Some(TypeRef::Array(Box::new(TypeRef::Named("Pet".to_string())))),
            description: Some("Success".to_string()),
        });
        map
    },
    summary: Some("List all pets".to_string()),
    description: None,
}
```

---

### ParameterDef

Represents a parameter (path, query, header, cookie).

```rust
pub struct ParameterDef {
    pub name: String,
    pub location: ParameterLocation,
    pub type_ref: TypeRef,
    pub required: bool,
    pub description: Option<String>,
}

pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}
```

---

### ResponseDef

Represents an operation response.

```rust
pub struct ResponseDef {
    pub status_code: u16,
    pub type_ref: Option<TypeRef>,
    pub description: Option<String>,
}
```

---

### Config

Configuration for code generation.

```rust
pub struct Config {
    /// Base output directory (e.g., "src")
    pub output_dir: String,
    
    /// How to organize services
    pub service_style: ServiceStyle,
    
    /// Include documentation comments
    pub include_docs: bool,
    
    /// Language-specific options
    pub lang_options: BTreeMap<String, String>,
}

pub enum ServiceStyle {
    /// One file per service (grouped by tag)
    PerService,
    
    /// All operations in one client
    SingleClient,
    
    /// Group by OpenAPI tags
    ByTag,
}
```

**Example**:
```rust
let config = Config {
    output_dir: "src".to_string(),
    service_style: ServiceStyle::PerService,
    include_docs: true,
    lang_options: BTreeMap::new(),
};
```

---

### VirtualFS

In-memory file system for generated code.

```rust
pub struct VirtualFS {
    files: HashMap<PathBuf, String>,
}
```

**Methods**:

#### `new()`
```rust
pub fn new() -> Self
```
Create a new empty virtual filesystem.

#### `write_file()`
```rust
pub fn write_file(&mut self, path: impl AsRef<Path>, content: String) -> Result<()>
```
Add a file to the virtual filesystem.

**Example**:
```rust
let mut vfs = VirtualFS::new();
vfs.write_file("src/types.ts", "export interface User { ... }".to_string())?;
vfs.write_file("src/services/users.ts", "export class UsersService { ... }".to_string())?;
```

#### `write_to_disk()`
```rust
pub fn write_to_disk(&self, base_path: impl AsRef<Path>) -> Result<()>
```
Write all files to disk, creating directories as needed.

**Example**:
```rust
vfs.write_to_disk("./output")?;
// Creates:
// ./output/src/types.ts
// ./output/src/services/users.ts
```

#### `contains()`
```rust
pub fn contains(&self, path: impl AsRef<Path>) -> bool
```
Check if a file exists in the VFS.

#### `len()`
```rust
pub fn len(&self) -> usize
```
Get the number of files.

#### `files()`
```rust
pub fn files(&self) -> impl Iterator<Item = (&Path, &String)>
```
Iterate over all files.

**Example**:
```rust
for (path, content) in vfs.files() {
    println!("{}: {} bytes", path.display(), content.len());
}
```

---

## Generator Registry (generate crate)

### GeneratorRegistry

Manages available code generators.

```rust
pub struct GeneratorRegistry {
    generators: HashMap<String, Box<dyn Generator>>,
}
```

**Methods**:

#### `new()`
```rust
pub fn new() -> Self
```
Create empty registry.

#### `with_defaults()`
```rust
pub fn with_defaults() -> Self
```
Create registry with all enabled templates.

**Example**:
```rust
let registry = GeneratorRegistry::with_defaults();
```

#### `register()`
```rust
pub fn register(&mut self, generator: Box<dyn Generator>)
```
Register a new generator.

**Example**:
```rust
let mut registry = GeneratorRegistry::new();
registry.register(Box::new(TypeScriptGenerator));
```

#### `generate()`
```rust
pub fn generate(
    &self,
    template: &str,
    ir: &GenIr,
    config: &Config,
) -> Result<VirtualFS>
```
Generate code using specified template.

**Example**:
```rust
let vfs = registry.generate("typescript", &gen_ir, &config)?;
```

#### `list_generators()`
```rust
pub fn list_generators(&self) -> Vec<&str>
```
Get names of all registered generators.

---

## Parser (parser crate)

### parse_openapi()

```rust
pub fn parse_openapi(content: &str) -> Result<oas::OpenAPIV3>
```

Parse OpenAPI JSON string.

**Example**:
```rust
use parser::parse_openapi;

let json = std::fs::read_to_string("spec.json")?;
let openapi = parse_openapi(&json)?;
```

---

## AST Conversion (ast crate)

### From<OpenAPIV3> for GenIr

Convert OpenAPI to GenIR.

```rust
impl From<oas::OpenAPIV3> for codegen::GenIr {
    fn from(spec: oas::OpenAPIV3) -> Self {
        // Conversion logic
    }
}
```

**Example**:
```rust
let openapi: oas::OpenAPIV3 = /* ... */;
let gen_ir: GenIr = openapi.into();
```

---

## Error Types

### codegen::Error

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Template error: {0}")]
    Template(String),
    
    #[error("Generation error: {0}")]
    Generation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

### parser::Error

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Invalid spec: {0}")]
    InvalidSpec(String),
}
```

---

## Testing Utilities

### Test Helpers

```rust
#[cfg(test)]
pub fn create_test_ir() -> GenIr {
    GenIr {
        types: vec![
            TypeDef::Struct(StructDef {
                name: "TestType".to_string(),
                fields: vec![],
                description: None,
            })
        ],
        services: vec![],
    }
}
```

---

## CLI (cli crate)

Not intended for library use, but the main flow is:

1. Parse arguments with `clap`
2. Read OpenAPI file
3. Parse with `serde_json`
4. Convert to GenIR with `Into` trait
5. Get generator from registry
6. Generate VirtualFS
7. Write to disk

See `cli/src/main.rs` for implementation.

---

## Template-Specific APIs

### TypeScript Template

```rust
pub struct TypeScriptGenerator;

impl Generator for TypeScriptGenerator {
    // Implementation
}
```

Located in `templates/typescript/src/`.

---

## Type Conversion Examples

### OpenAPI Schema → TypeRef

```rust
fn schema_to_type_ref(schema: &oas::Schema) -> TypeRef {
    match schema.schema_type {
        Some("string") => TypeRef::Primitive(PrimitiveType::String),
        Some("integer") => TypeRef::Primitive(PrimitiveType::Integer),
        Some("array") => {
            let items = schema_to_type_ref(&schema.items);
            TypeRef::Array(Box::new(items))
        }
        _ => TypeRef::Primitive(PrimitiveType::Any),
    }
}
```

### TypeRef → Language Type String

```rust
fn type_ref_to_typescript(type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Named(name) => name.clone(),
        TypeRef::Array(inner) => format!("{}[]", type_ref_to_typescript(inner)),
        TypeRef::Optional(inner) => format!("{} | null", type_ref_to_typescript(inner)),
        TypeRef::Primitive(PrimitiveType::String) => "string".to_string(),
        TypeRef::Primitive(PrimitiveType::Integer) => "number".to_string(),
        // ... etc
    }
}
```

---

## Extension Points

To extend the system:

1. **Add new Generator**: Implement `Generator` trait
2. **Add new TypeDef variant**: Modify `TypeDef` enum
3. **Add new Config option**: Add field to `Config`
4. **Add new ServiceStyle**: Add variant to `ServiceStyle` enum
5. **Custom VirtualFS behavior**: Extend `VirtualFS` methods

---

## Trait Bounds

Common trait bounds used:

- `Send + Sync`: For `Generator` (thread-safe)
- `Serialize + Deserialize`: For template context types
- `Debug + Clone`: For most data structures
- `Display`: For error types

---

## Feature Flags

In `generate/Cargo.toml`:

```toml
[features]
default = ["typescript"]
typescript = ["dep:typescript"]
python = ["dep:python"]
# Add more as needed
```

Use with:
```bash
cargo build --features python
cargo build --no-default-features --features typescript,python
```

---

## More Information

- See source code for full implementation details
- Run `cargo doc --open` for generated documentation
- Check tests for usage examples

