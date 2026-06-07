use oas3::spec::{ObjectOrReference, ParameterIn};

use crate::{
    lint::{Finding, LintCtx, RuleId},
    model::escape_pointer_segment,
};

/// Parameter names should be consistent case (camelCase for query, kebab-case for headers)
pub fn param_naming_consistent(ctx: &LintCtx, out: &mut Vec<Finding>) {
    let Some(paths) = &ctx.spec.paths else {
        return;
    };

    for (path, path_item) in paths {
        let path_ptr = format!("/paths/{}", escape_pointer_segment(path));

        // Check path-level parameters
        for (i, param) in path_item.parameters.iter().enumerate() {
            if let ObjectOrReference::Object(p) = param {
                check_param_naming(p, &format!("{}/parameters/{}", path_ptr, i), out);
            }
        }

        // Check operation-level parameters
        for (method, operation) in iter_operations(path_item) {
            let op_ptr = format!("{}/{}", path_ptr, method);
            for (i, param) in operation.parameters.iter().enumerate() {
                if let ObjectOrReference::Object(p) = param {
                    check_param_naming(p, &format!("{}/parameters/{}", op_ptr, i), out);
                }
            }
        }
    }
}

fn check_param_naming(param: &oas3::spec::Parameter, ptr: &str, out: &mut Vec<Finding>) {
    let name = &param.name;

    match param.location {
        ParameterIn::Query
            // Query params should be camelCase or snake_case
            if name.contains('-') => {
                out.push(Finding::new(
                    RuleId::ParamNamingConsistent,
                    format!("{}/name", ptr),
                    format!(
                        "Query parameter '{}' uses kebab-case; consider camelCase or snake_case",
                        name
                    ),
                ));
            }
        ParameterIn::Header => {
            // Headers typically use X-Header-Case or lowercase
            // Skip standard headers
            let lower = name.to_lowercase();
            if !is_standard_header(&lower) && !name.starts_with("X-") && name.contains('_') {
                out.push(Finding::new(
                    RuleId::ParamNamingConsistent,
                    format!("{}/name", ptr),
                    format!(
                        "Header parameter '{}' uses underscores; headers typically use hyphens",
                        name
                    ),
                ));
            }
        }
        _ => {}
    }
}

/// Query parameters must declare schema and type
pub fn param_schema_required(ctx: &LintCtx, out: &mut Vec<Finding>) {
    let Some(paths) = &ctx.spec.paths else {
        return;
    };

    for (path, path_item) in paths {
        let path_ptr = format!("/paths/{}", escape_pointer_segment(path));

        // Check path-level parameters
        for (i, param) in path_item.parameters.iter().enumerate() {
            if let ObjectOrReference::Object(p) = param
                && p.location == ParameterIn::Query
                && p.schema.is_none()
                && p.content.is_none()
            {
                out.push(Finding::new(
                    RuleId::ParamSchemaRequired,
                    format!("{}/parameters/{}", path_ptr, i),
                    format!("Query parameter '{}' must declare a schema", p.name),
                ));
            }
        }

        // Check operation-level parameters
        for (method, operation) in iter_operations(path_item) {
            let op_ptr = format!("{}/{}", path_ptr, method);
            for (i, param) in operation.parameters.iter().enumerate() {
                if let ObjectOrReference::Object(p) = param
                    && p.location == ParameterIn::Query
                    && p.schema.is_none()
                    && p.content.is_none()
                {
                    out.push(Finding::new(
                        RuleId::ParamSchemaRequired,
                        format!("{}/parameters/{}", op_ptr, i),
                        format!("Query parameter '{}' must declare a schema", p.name),
                    ));
                }
            }
        }
    }
}

/// Enum parameters should have documented values
pub fn param_enum_documented(ctx: &LintCtx, out: &mut Vec<Finding>) {
    let Some(paths) = &ctx.spec.paths else {
        return;
    };

    for (path, path_item) in paths {
        let path_ptr = format!("/paths/{}", escape_pointer_segment(path));

        // Check path-level parameters
        for (i, param) in path_item.parameters.iter().enumerate() {
            if let ObjectOrReference::Object(p) = param {
                check_param_enum(p, &format!("{}/parameters/{}", path_ptr, i), out);
            }
        }

        // Check operation-level parameters
        for (method, operation) in iter_operations(path_item) {
            let op_ptr = format!("{}/{}", path_ptr, method);
            for (i, param) in operation.parameters.iter().enumerate() {
                if let ObjectOrReference::Object(p) = param {
                    check_param_enum(p, &format!("{}/parameters/{}", op_ptr, i), out);
                }
            }
        }
    }
}

fn check_param_enum(param: &oas3::spec::Parameter, ptr: &str, out: &mut Vec<Finding>) {
    if let Some(ObjectOrReference::Object(schema)) = &param.schema
        && !schema.enum_values.is_empty()
        && param.description.is_none()
    {
        out.push(Finding::new(
            RuleId::ParamEnumDocumented,
            ptr,
            format!(
                "Enum parameter '{}' should have a description documenting allowed values",
                param.name
            ),
        ));
    }
}

/// Date/time formats must be explicit (format: date or date-time)
pub fn param_date_time_format(ctx: &LintCtx, out: &mut Vec<Finding>) {
    let Some(paths) = &ctx.spec.paths else {
        return;
    };

    for (path, path_item) in paths {
        let path_ptr = format!("/paths/{}", escape_pointer_segment(path));

        // Check path-level parameters
        for (i, param) in path_item.parameters.iter().enumerate() {
            if let ObjectOrReference::Object(p) = param {
                check_param_datetime(p, &format!("{}/parameters/{}", path_ptr, i), out);
            }
        }

        // Check operation-level parameters
        for (method, operation) in iter_operations(path_item) {
            let op_ptr = format!("{}/{}", path_ptr, method);
            for (i, param) in operation.parameters.iter().enumerate() {
                if let ObjectOrReference::Object(p) = param {
                    check_param_datetime(p, &format!("{}/parameters/{}", op_ptr, i), out);
                }
            }
        }
    }
}

fn check_param_datetime(param: &oas3::spec::Parameter, ptr: &str, out: &mut Vec<Finding>) {
    let name_lower = param.name.to_lowercase();

    // Check if name suggests a date/time field
    let is_date_like = name_lower.contains("date")
        || name_lower.contains("time")
        || name_lower.contains("timestamp")
        || name_lower.ends_with("_at")
        || name_lower.ends_with("at");

    if is_date_like
        && let Some(ObjectOrReference::Object(schema)) = &param.schema
        && !schema
            .format
            .as_ref()
            .is_some_and(|f| f == "date" || f == "date-time")
    {
        out.push(Finding::new(
            RuleId::ParamDateTimeFormat,
            format!("{}/schema", ptr),
            format!(
                "Parameter '{}' appears to be a date/time; use format: date or date-time",
                param.name
            ),
        ));
    }
}

fn is_standard_header(name: &str) -> bool {
    matches!(
        name,
        "accept"
            | "accept-charset"
            | "accept-encoding"
            | "accept-language"
            | "authorization"
            | "cache-control"
            | "content-type"
            | "content-length"
            | "cookie"
            | "host"
            | "if-match"
            | "if-modified-since"
            | "if-none-match"
            | "origin"
            | "referer"
            | "user-agent"
    )
}

fn iter_operations(
    path_item: &oas3::spec::PathItem,
) -> impl Iterator<Item = (&'static str, &oas3::spec::Operation)> {
    let methods: [(&'static str, Option<&oas3::spec::Operation>); 8] = [
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
    use crate::lint::RuleId;
    use crate::testutil::yaml_to_json;
    use crate::{RuleSet, lint_with_ruleset};

    #[test]
    fn test_param_naming_consistent_good() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: pageSize
          in: query
          schema:
            type: integer
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamNamingConsistent.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());

        // Should also pass with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamNamingConsistent.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());
    }

    #[test]
    fn test_param_naming_consistent_query_kebab() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: page-size
          in: query
          schema:
            type: integer
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamNamingConsistent.as_str()]),
        )
        .unwrap();

        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(
            validation.diagnostics[0].rule,
            RuleId::ParamNamingConsistent
        );
        assert_eq!(
            validation.diagnostics[0].message,
            "Query parameter 'page-size' uses kebab-case; consider camelCase or snake_case"
        );

        // Should also fail with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamNamingConsistent.as_str()]),
        )
        .unwrap();
        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(
            validation.diagnostics[0].message,
            "Query parameter 'page-size' uses kebab-case; consider camelCase or snake_case"
        );
    }

    #[test]
    fn test_param_schema_required_present() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: limit
          in: query
          schema:
            type: integer
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamSchemaRequired.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());

        // Should also pass with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamSchemaRequired.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());
    }

    #[test]
    fn test_param_schema_required_missing() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: limit
          in: query
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamSchemaRequired.as_str()]),
        )
        .unwrap();

        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(validation.diagnostics[0].rule, RuleId::ParamSchemaRequired);
        assert_eq!(
            validation.diagnostics[0].message,
            "Query parameter 'limit' must declare a schema"
        );

        // Should also fail with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamSchemaRequired.as_str()]),
        )
        .unwrap();
        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(
            validation.diagnostics[0].message,
            "Query parameter 'limit' must declare a schema"
        );
    }

    #[test]
    fn test_param_enum_documented_good() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: status
          in: query
          description: Filter by status
          schema:
            type: string
            enum:
              - available
              - pending
              - sold
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamEnumDocumented.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());

        // Should also pass with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamEnumDocumented.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());
    }

    #[test]
    fn test_param_enum_documented_missing() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: status
          in: query
          schema:
            type: string
            enum:
              - available
              - pending
              - sold
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamEnumDocumented.as_str()]),
        )
        .unwrap();

        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(validation.diagnostics[0].rule, RuleId::ParamEnumDocumented);
        assert_eq!(
            validation.diagnostics[0].message,
            "Enum parameter 'status' should have a description documenting allowed values"
        );

        // Should also fail with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamEnumDocumented.as_str()]),
        )
        .unwrap();
        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(
            validation.diagnostics[0].message,
            "Enum parameter 'status' should have a description documenting allowed values"
        );
    }

    #[test]
    fn test_param_date_time_format_good() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: createdAt
          in: query
          schema:
            type: string
            format: date-time
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamDateTimeFormat.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());

        // Should also pass with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamDateTimeFormat.as_str()]),
        )
        .unwrap();
        assert!(validation.diagnostics.is_empty());
    }

    #[test]
    fn test_param_date_time_format_missing() {
        let yaml = r#"
openapi: 3.1.0
info:
  title: Test API
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      parameters:
        - name: createdAt
          in: query
          schema:
            type: string
      responses:
        '200':
          description: Success
"#;
        let validation = lint_with_ruleset(
            yaml,
            RuleSet::from_slice(&[RuleId::ParamDateTimeFormat.as_str()]),
        )
        .unwrap();

        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(validation.diagnostics[0].rule, RuleId::ParamDateTimeFormat);
        assert_eq!(
            validation.diagnostics[0].message,
            "Parameter 'createdAt' appears to be a date/time; use format: date or date-time"
        );

        // Should also fail with JSON input
        let json = yaml_to_json(yaml);
        let validation = lint_with_ruleset(
            &json,
            RuleSet::from_slice(&[RuleId::ParamDateTimeFormat.as_str()]),
        )
        .unwrap();
        assert_eq!(validation.diagnostics.len(), 1);
        assert_eq!(
            validation.diagnostics[0].message,
            "Parameter 'createdAt' appears to be a date/time; use format: date or date-time"
        );
    }
}
