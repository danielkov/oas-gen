use crate::error::{ParserError, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Resolves all external $ref references in an OpenAPI specification.
///
/// This function takes a path to an OpenAPI spec file that may contain
/// external references (e.g., `$ref: './components/schemas/user.yaml#/User'`)
/// and returns a fully resolved spec with all external references converted
/// to internal references and their schemas added to components.
///
/// # Arguments
///
/// * `spec_path` - Path to the main OpenAPI specification file
///
/// # Returns
///
/// A JSON string containing the fully resolved OpenAPI specification
pub fn resolve(spec_path: impl AsRef<Path>) -> Result<String> {
    let spec_path = spec_path.as_ref();
    let base_dir = spec_path.parent().ok_or_else(|| {
        ParserError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot determine parent directory of spec file",
        ))
    })?;

    // Parse the main spec
    let content = std::fs::read_to_string(spec_path)?;
    let mut spec = parse_as_json(&content)?;

    // Cache for loaded external files
    let mut file_cache: HashMap<PathBuf, Value> = HashMap::new();

    // Track collected schemas to add to components
    let mut collected_schemas: HashMap<String, Value> = HashMap::new();

    // Resolve all references
    resolve_refs(&mut spec, base_dir, &mut file_cache, &mut collected_schemas)?;

    // Add collected schemas to components/schemas
    if !collected_schemas.is_empty() {
        add_schemas_to_components(&mut spec, collected_schemas);
    }

    // Serialize back to JSON
    let resolved = serde_json::to_string_pretty(&spec)?;
    Ok(resolved)
}

/// Parse YAML or JSON content as a JSON value
fn parse_as_json(content: &str) -> Result<Value> {
    // Try JSON first
    if let Ok(value) = serde_json::from_str(content) {
        return Ok(value);
    }

    // Fall back to YAML
    let value: Value = serde_saphyr::from_str(content)?;
    Ok(value)
}

/// Recursively resolve all $ref fields in the value
fn resolve_refs(
    value: &mut Value,
    base_dir: &Path,
    file_cache: &mut HashMap<PathBuf, Value>,
    collected_schemas: &mut HashMap<String, Value>,
) -> Result<()> {
    resolve_refs_with_context(value, base_dir, file_cache, collected_schemas, &[])
}

/// Recursively resolve all $ref fields in the value with context tracking
fn resolve_refs_with_context(
    value: &mut Value,
    base_dir: &Path,
    file_cache: &mut HashMap<PathBuf, Value>,
    collected_schemas: &mut HashMap<String, Value>,
    path: &[&str],
) -> Result<()> {
    match value {
        Value::Object(map) => {
            // Check if this object has a $ref field
            if let Some(Value::String(ref_str)) = map.get("$ref") {
                // Check if this is an external reference (contains a file path)
                if ref_str.contains("./") || ref_str.contains("../") {
                    // Parse the reference
                    let (file_path, json_pointer) = parse_ref(ref_str);

                    // Resolve the file path relative to base_dir
                    let full_path = base_dir.join(file_path);
                    let canonical_path = full_path.canonicalize().map_err(ParserError::Io)?;

                    // Load the external file (use cache if available)
                    let external_doc = if let Some(cached) = file_cache.get(&canonical_path) {
                        cached.clone()
                    } else {
                        let content = std::fs::read_to_string(&canonical_path)?;
                        let doc = parse_as_json(&content)?;
                        file_cache.insert(canonical_path.clone(), doc.clone());
                        doc
                    };

                    // Extract the referenced value using the JSON pointer
                    let mut referenced_value = extract_by_pointer(&external_doc, json_pointer)?;

                    // Check if this is a schema reference or should be inlined
                    let is_schema_ref = is_schema_context(path, json_pointer);

                    if is_schema_ref {
                        // This is a schema - collect it and replace with internal ref
                        let schema_name = extract_schema_name(json_pointer);

                        // Normalize internal refs in the schema
                        normalize_internal_refs(&mut referenced_value);

                        // Add this schema to collected schemas
                        collected_schemas.insert(schema_name.clone(), referenced_value.clone());

                        // Collect all dependencies (schemas referenced by this schema)
                        collect_dependencies(&referenced_value, &external_doc, collected_schemas)?;

                        // Replace external ref with internal ref
                        map.insert(
                            "$ref".to_string(),
                            Value::String(format!("#/components/schemas/{}", schema_name)),
                        );
                    } else {
                        // This is a path or other item - inline it
                        let new_base_dir = canonical_path.parent().ok_or_else(|| {
                            ParserError::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidInput,
                                "Cannot determine parent directory of external file",
                            ))
                        })?;

                        // Recursively resolve refs in the inlined content
                        resolve_refs_with_context(
                            &mut referenced_value,
                            new_base_dir,
                            file_cache,
                            collected_schemas,
                            path,
                        )?;

                        // Replace the entire object with the referenced value
                        *value = referenced_value;
                        return Ok(());
                    }
                } else {
                    // Internal reference - leave as is
                }
            } else {
                // No $ref, recursively process all values in the object
                for (key, v) in map.iter_mut() {
                    let mut new_path = path.to_vec();
                    new_path.push(key.as_str());
                    resolve_refs_with_context(
                        v,
                        base_dir,
                        file_cache,
                        collected_schemas,
                        &new_path,
                    )?;
                }
            }
        }
        Value::Array(arr) => {
            // Recursively process all items in the array
            for item in arr.iter_mut() {
                resolve_refs_with_context(item, base_dir, file_cache, collected_schemas, path)?;
            }
        }
        _ => {
            // Primitive values, nothing to resolve
        }
    }

    Ok(())
}

/// Determine if a reference should be collected as a schema or inlined
fn is_schema_context(path: &[&str], json_pointer: &str) -> bool {
    // Check if we're in a schema context (components/schemas, requestBody, responses)
    // or if the pointer points to a schema location

    // If the reference is in components/schemas, it's a schema
    if path.contains(&"schemas") {
        return true;
    }

    // If the JSON pointer starts with /~1 (escaped /), it's likely a path item
    if json_pointer.starts_with("/~1") {
        return false;
    }

    // If we're in paths section at the path item level, it's a path item
    if path.len() >= 2 && path[0] == "paths" && !path.contains(&"schema") {
        return false;
    }

    // If the reference is to a schema location in requestBody or responses
    if path.contains(&"schema") || path.contains(&"requestBody") || path.contains(&"responses") {
        return true;
    }

    // Default: treat as schema reference
    true
}

/// Extract schema name from JSON pointer (e.g., "/User" -> "User")
fn extract_schema_name(pointer: &str) -> String {
    pointer
        .trim_start_matches('/')
        .split('/')
        .next_back()
        .unwrap_or("Unknown")
        .to_string()
}

/// Collect all schemas that are referenced by the given schema
fn collect_dependencies(
    schema: &Value,
    source_doc: &Value,
    collected: &mut HashMap<String, Value>,
) -> Result<()> {
    let mut refs_to_collect = HashSet::new();
    find_internal_refs(schema, &mut refs_to_collect);

    // Collect each referenced schema
    for ref_name in refs_to_collect {
        if !collected.contains_key(&ref_name) {
            // Extract the schema from the source document
            let pointer = format!("/{}", ref_name);
            if let Ok(mut dep_schema) = extract_by_pointer(source_doc, &pointer) {
                // Normalize internal refs in this schema to use proper OpenAPI format
                normalize_internal_refs(&mut dep_schema);

                collected.insert(ref_name.clone(), dep_schema.clone());

                // Recursively collect dependencies of this schema
                collect_dependencies(&dep_schema, source_doc, collected)?;
            }
        }
    }

    Ok(())
}

/// Normalize internal $refs to use proper OpenAPI 3.0 format
/// Converts #/Something to #/components/schemas/Something
fn normalize_internal_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref").cloned() {
                // Only process internal refs that need normalization
                if ref_str.starts_with('#') && !ref_str.contains("./") && !ref_str.contains("../") {
                    // If it's already in the correct format, leave it
                    if !ref_str.starts_with("#/components/schemas/") {
                        // Extract schema name and reformat
                        let name = ref_str
                            .trim_start_matches('#')
                            .trim_start_matches('/')
                            .split('/')
                            .next_back()
                            .unwrap_or("");

                        if !name.is_empty() {
                            map.insert(
                                "$ref".to_string(),
                                Value::String(format!("#/components/schemas/{}", name)),
                            );
                        }
                    }
                }
            }

            // Recursively normalize all values
            for v in map.values_mut() {
                normalize_internal_refs(v);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                normalize_internal_refs(item);
            }
        }
        _ => {}
    }
}

/// Find all internal $ref names in a value (e.g., "#/User" -> "User")
fn find_internal_refs(value: &Value, refs: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref") {
                // Only process internal refs (start with # and no file path)
                if ref_str.starts_with('#') && !ref_str.contains("./") && !ref_str.contains("../") {
                    // Extract the schema name from "#/Something" or "#/components/schemas/Something"
                    let name = ref_str
                        .trim_start_matches('#')
                        .trim_start_matches('/')
                        .split('/')
                        .next_back()
                        .unwrap_or("")
                        .to_string();

                    if !name.is_empty() {
                        refs.insert(name);
                    }
                }
            }

            // Recursively search in all values
            for v in map.values() {
                find_internal_refs(v, refs);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                find_internal_refs(item, refs);
            }
        }
        _ => {}
    }
}

/// Add collected schemas to the spec's components/schemas section
fn add_schemas_to_components(spec: &mut Value, schemas: HashMap<String, Value>) {
    if let Value::Object(spec_map) = spec {
        // Ensure components exists
        let components = spec_map
            .entry("components".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));

        if let Value::Object(components_map) = components {
            // Ensure schemas exists
            let schemas_section = components_map
                .entry("schemas".to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()));

            if let Value::Object(schemas_map) = schemas_section {
                // Add all collected schemas
                for (name, schema) in schemas {
                    schemas_map.insert(name, schema);
                }
            }
        }
    }
}

/// Parse a $ref string into file path and JSON pointer
///
/// Examples:
/// - `./components/schemas/user.yaml#/User` -> ("./components/schemas/user.yaml", "/User")
/// - `./paths/users.yaml#/~1users` -> ("./paths/users.yaml", "/~1users")
fn parse_ref(ref_str: &str) -> (&str, &str) {
    if let Some(hash_pos) = ref_str.find('#') {
        let file_path = &ref_str[..hash_pos];
        let pointer = &ref_str[hash_pos + 1..];
        (file_path, pointer)
    } else {
        // No fragment, reference entire document
        (ref_str, "")
    }
}

/// Extract a value from a JSON document using a JSON pointer
///
/// Supports JSON Pointer syntax (RFC 6901):
/// - "/" separates path segments
/// - "~0" represents "~"
/// - "~1" represents "/"
/// - Empty string returns the root document
fn extract_by_pointer(doc: &Value, pointer: &str) -> Result<Value> {
    if pointer.is_empty() || pointer == "/" {
        return Ok(doc.clone());
    }

    let mut current = doc;
    let segments = pointer
        .strip_prefix('/')
        .unwrap_or(pointer)
        .split('/')
        .filter(|s| !s.is_empty());

    for segment in segments {
        // Decode JSON pointer escapes
        let decoded = segment.replace("~1", "/").replace("~0", "~");

        match current {
            Value::Object(map) => {
                current = map.get(&decoded).ok_or_else(|| {
                    ParserError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!(
                            "JSON pointer path not found: {} (looking for '{}')",
                            pointer, decoded
                        ),
                    ))
                })?;
            }
            Value::Array(arr) => {
                let index: usize = decoded.parse().map_err(|_| {
                    ParserError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Invalid array index in JSON pointer: {}", decoded),
                    ))
                })?;
                current = arr.get(index).ok_or_else(|| {
                    ParserError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Array index out of bounds: {}", index),
                    ))
                })?;
            }
            _ => {
                return Err(ParserError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Cannot traverse into non-object/array value at: {}",
                        pointer
                    ),
                )));
            }
        }
    }

    Ok(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ref() {
        let (file, pointer) = parse_ref("./components/schemas/user.yaml#/User");
        assert_eq!(file, "./components/schemas/user.yaml");
        assert_eq!(pointer, "/User");

        let (file, pointer) = parse_ref("./paths/users.yaml#/~1users~1{userId}");
        assert_eq!(file, "./paths/users.yaml");
        assert_eq!(pointer, "/~1users~1{userId}");
    }

    #[test]
    fn test_extract_by_pointer() {
        let doc = serde_json::json!({
            "User": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                }
            },
            "Product": {
                "type": "object"
            }
        });

        let result = extract_by_pointer(&doc, "/User").unwrap();
        assert_eq!(result["type"], "object");

        let result = extract_by_pointer(&doc, "/User/properties/name").unwrap();
        assert_eq!(result["type"], "string");
    }

    #[test]
    fn test_extract_by_pointer_with_escapes() {
        let doc = serde_json::json!({
            "/users": {
                "get": {
                    "summary": "List users"
                }
            }
        });

        let result = extract_by_pointer(&doc, "/~1users").unwrap();
        assert_eq!(result["get"]["summary"], "List users");
    }

    #[test]
    fn test_extract_schema_name() {
        assert_eq!(extract_schema_name("/User"), "User");
        assert_eq!(extract_schema_name("/components/schemas/User"), "User");
        assert_eq!(extract_schema_name("User"), "User");
    }

    #[test]
    fn test_find_internal_refs() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "address": { "$ref": "#/Address" },
                "billing": { "$ref": "#/BillingInfo" }
            }
        });

        let mut refs = HashSet::new();
        find_internal_refs(&schema, &mut refs);

        assert!(refs.contains("Address"));
        assert!(refs.contains("BillingInfo"));
        assert_eq!(refs.len(), 2);
    }
}
