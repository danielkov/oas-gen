use std::sync::OnceLock;

use rustc_hash::FxHashSet;

use super::Indexes;
use crate::model::SpanDb;

/// Context passed to lint rules, providing read-only access to
/// the parsed spec and shared indexes.
pub struct LintCtx<'a> {
    /// The parsed OpenAPI specification
    pub spec: &'a oas3::Spec,
    /// Pre-computed indexes for efficient rule execution
    pub indexes: &'a Indexes,
    /// Span database for source locations
    pub span_db: &'a SpanDb,
    /// Lazily-computed set of all $ref values used in the spec
    used_refs: OnceLock<FxHashSet<String>>,
}

impl<'a> LintCtx<'a> {
    pub fn new(spec: &'a oas3::Spec, indexes: &'a Indexes, span_db: &'a SpanDb) -> Self {
        Self {
            spec,
            indexes,
            span_db,
            used_refs: OnceLock::new(),
        }
    }

    /// Returns the set of all $ref values used in the spec.
    /// Lazily computed on first access.
    pub fn used_refs(&self) -> &FxHashSet<String> {
        self.used_refs
            .get_or_init(|| collect_all_refs_from_spec(self.spec))
    }
}

/// Collect all $ref paths used in the specification by traversing the parsed spec
fn collect_all_refs_from_spec(spec: &oas3::Spec) -> FxHashSet<String> {
    let mut refs = FxHashSet::default();

    // Helper to collect a ref from ObjectOrReference
    fn collect_ref<T>(oor: &oas3::spec::ObjectOrReference<T>, out: &mut FxHashSet<String>) {
        if let oas3::spec::ObjectOrReference::Ref { ref_path, .. } = oor {
            out.insert(ref_path.clone());
        }
    }

    // Traverse paths and operations
    if let Some(paths) = &spec.paths {
        for path_item in paths.values() {
            // Check all operations
            for operation in [
                &path_item.get,
                &path_item.post,
                &path_item.put,
                &path_item.patch,
                &path_item.delete,
                &path_item.options,
                &path_item.head,
                &path_item.trace,
            ]
            .iter()
            .filter_map(|op| op.as_ref())
            {
                // Request body
                if let Some(rb) = &operation.request_body {
                    collect_ref(rb, &mut refs);
                    if let oas3::spec::ObjectOrReference::Object(rb_obj) = rb {
                        for media_type in rb_obj.content.values() {
                            if let Some(schema) = &media_type.schema {
                                collect_schema_refs(schema, &mut refs);
                            }
                        }
                    }
                }

                // Parameters
                for param in &operation.parameters {
                    collect_ref(param, &mut refs);
                    if let oas3::spec::ObjectOrReference::Object(param_obj) = param
                        && let Some(schema) = &param_obj.schema
                    {
                        collect_schema_refs(schema, &mut refs);
                    }
                }

                // Responses
                if let Some(responses) = &operation.responses {
                    for response in responses.values() {
                        collect_ref(response, &mut refs);
                        if let oas3::spec::ObjectOrReference::Object(response_obj) = response {
                            for media_type in response_obj.content.values() {
                                if let Some(schema) = &media_type.schema {
                                    collect_schema_refs(schema, &mut refs);
                                }
                            }
                        }
                    }
                }
            }

            // Path-level parameters
            for param in &path_item.parameters {
                collect_ref(param, &mut refs);
            }
        }
    }

    // Traverse components
    if let Some(components) = &spec.components {
        // Schemas can reference other schemas
        for schema in components.schemas.values() {
            collect_schema_refs(schema, &mut refs);
        }

        // Parameters can reference schemas
        for param in components.parameters.values() {
            if let oas3::spec::ObjectOrReference::Object(param_obj) = param
                && let Some(schema) = &param_obj.schema
            {
                collect_schema_refs(schema, &mut refs);
            }
        }

        // Responses can reference schemas
        for response in components.responses.values() {
            if let oas3::spec::ObjectOrReference::Object(response_obj) = response {
                for media_type in response_obj.content.values() {
                    if let Some(schema) = &media_type.schema {
                        collect_schema_refs(schema, &mut refs);
                    }
                }
            }
        }

        // Request bodies can reference schemas
        for request_body in components.request_bodies.values() {
            if let oas3::spec::ObjectOrReference::Object(rb_obj) = request_body {
                for media_type in rb_obj.content.values() {
                    if let Some(schema) = &media_type.schema {
                        collect_schema_refs(schema, &mut refs);
                    }
                }
            }
        }
    }

    refs
}

/// Recursively collect schema references
fn collect_schema_refs(
    schema: &oas3::spec::ObjectOrReference<oas3::spec::ObjectSchema>,
    refs: &mut FxHashSet<String>,
) {
    match schema {
        oas3::spec::ObjectOrReference::Ref { ref_path, .. } => {
            refs.insert(ref_path.clone());
        }
        oas3::spec::ObjectOrReference::Object(obj) => {
            // Properties
            for prop_schema in obj.properties.values() {
                collect_schema_refs(prop_schema, refs);
            }

            // allOf, oneOf, anyOf
            for sub_schema in &obj.all_of {
                collect_schema_refs(sub_schema, refs);
            }
            for sub_schema in &obj.one_of {
                collect_schema_refs(sub_schema, refs);
            }
            for sub_schema in &obj.any_of {
                collect_schema_refs(sub_schema, refs);
            }
        }
    }
}
