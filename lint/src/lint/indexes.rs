use oas3::spec::{Operation, PathItem};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::escape_pointer_segment;

/// Pre-computed indexes for efficient lint rule execution.
/// Built in a single traversal over the spec.
#[derive(Debug, Default)]
pub struct Indexes {
    /// All operations in the spec
    pub ops: Vec<OpInfo>,
    /// operationId -> list of (path, method) for duplicate detection
    pub operation_ids: FxHashMap<String, Vec<(String, String)>>,
    /// Path template parameters extracted from paths (e.g., "{petId}" from "/pets/{petId}")
    pub path_template_params: FxHashMap<String, FxHashSet<String>>,
    /// Tags used by operations
    pub used_tags: FxHashSet<String>,
    /// Tags declared at top level
    pub declared_tags: FxHashSet<String>,
}

/// Stored operation info (owned data for the indexes)
#[derive(Debug, Clone)]
pub struct OpInfo {
    pub path: String,
    pub method: &'static str,
    pub ptr_base: String,
    pub operation_id: Option<String>,
    pub tags: Vec<String>,
    pub has_responses: bool,
    pub has_2xx_response: bool,
    pub path_params: Vec<PathParamInfo>,
}

/// Information about a path parameter
#[derive(Debug, Clone)]
pub struct PathParamInfo {
    pub name: String,
    pub required: Option<bool>,
    pub ptr: String,
}

impl Indexes {
    /// Build indexes from a parsed OpenAPI spec
    pub fn build(spec: &oas3::Spec) -> Self {
        let mut indexes = Indexes::default();

        // Collect declared tags
        for tag in &spec.tags {
            indexes.declared_tags.insert(tag.name.clone());
        }

        // Traverse paths and operations
        let Some(paths) = &spec.paths else {
            return indexes;
        };
        for (path_template, path_item) in paths {
            let path_ptr = format!("/paths/{}", escape_pointer_segment(path_template));

            // Extract template parameters from path
            let template_params = extract_template_params(path_template);
            if !template_params.is_empty() {
                indexes
                    .path_template_params
                    .insert(path_template.clone(), template_params);
            }

            // Process each HTTP method
            for (method, operation) in iter_operations(path_item) {
                let op_ptr = format!("{}/{}", path_ptr, method);

                // Collect path parameters from both path-item and operation level
                let mut path_params = Vec::new();

                // Path-item level parameters
                for (i, param) in path_item.parameters.iter().enumerate() {
                    if let oas3::spec::ObjectOrReference::Object(p) = param
                        && p.location == oas3::spec::ParameterIn::Path
                    {
                        path_params.push(PathParamInfo {
                            name: p.name.clone(),
                            required: p.required,
                            ptr: format!("{}/parameters/{}", path_ptr, i),
                        });
                    }
                }

                // Operation level parameters
                for (i, param) in operation.parameters.iter().enumerate() {
                    if let oas3::spec::ObjectOrReference::Object(p) = param
                        && p.location == oas3::spec::ParameterIn::Path
                    {
                        path_params.push(PathParamInfo {
                            name: p.name.clone(),
                            required: p.required,
                            ptr: format!("{}/parameters/{}", op_ptr, i),
                        });
                    }
                }

                // Check responses
                let has_responses = operation
                    .responses
                    .as_ref()
                    .is_some_and(|responses| !responses.is_empty());
                let has_2xx_response = operation.responses.as_ref().is_some_and(|responses| {
                    responses
                        .keys()
                        .any(|k| k.starts_with('2') || k == "default")
                });

                // Track operation ID
                if let Some(ref op_id) = operation.operation_id {
                    indexes
                        .operation_ids
                        .entry(op_id.clone())
                        .or_default()
                        .push((path_template.clone(), method.to_string()));
                }

                // Track used tags
                for tag in &operation.tags {
                    indexes.used_tags.insert(tag.clone());
                }

                indexes.ops.push(OpInfo {
                    path: path_template.clone(),
                    method,
                    ptr_base: op_ptr,
                    operation_id: operation.operation_id.clone(),
                    tags: operation.tags.clone(),
                    has_responses,
                    has_2xx_response,
                    path_params,
                });
            }
        }

        indexes
    }
}

/// Extract template parameters from a path (e.g., "{petId}" from "/pets/{petId}")
fn extract_template_params(path: &str) -> FxHashSet<String> {
    let mut params = FxHashSet::default();
    let mut start = None;

    for (i, c) in path.char_indices() {
        match c {
            '{' => start = Some(i + 1),
            '}' if start.is_some() => {
                let s = start.unwrap();
                if s < i {
                    params.insert(path[s..i].to_string());
                }
                start = None;
            }
            _ => {}
        }
    }

    params
}

/// Iterate over all operations in a path item
fn iter_operations(path_item: &PathItem) -> impl Iterator<Item = (&'static str, &Operation)> {
    let methods: [(&'static str, Option<&Operation>); 8] = [
        ("get", path_item.get.as_ref()),
        ("put", path_item.put.as_ref()),
        ("post", path_item.post.as_ref()),
        ("delete", path_item.delete.as_ref()),
        ("options", path_item.options.as_ref()),
        ("head", path_item.head.as_ref()),
        ("patch", path_item.patch.as_ref()),
        ("trace", path_item.trace.as_ref()),
    ];

    methods
        .into_iter()
        .filter_map(|(method, op)| op.map(|o| (method, o)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_template_params() {
        let params = extract_template_params("/pets/{petId}");
        assert!(params.contains("petId"));
        assert_eq!(params.len(), 1);

        let params = extract_template_params("/users/{userId}/posts/{postId}");
        assert!(params.contains("userId"));
        assert!(params.contains("postId"));
        assert_eq!(params.len(), 2);

        let params = extract_template_params("/pets");
        assert!(params.is_empty());
    }
}
