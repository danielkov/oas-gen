use crate::LintError;
use crate::lint::{Diagnostic, Indexes, LintCtx, RuleId, RuleSet, resolve_diagnostics, run_rules};
use crate::model::LineIndex;
use crate::parse::SpanDbBuilder;

/// Result of linting an OpenAPI specification
#[derive(Debug)]
pub struct Validation {
    /// All diagnostics found during linting
    pub diagnostics: Vec<Diagnostic>,
    /// Whether the spec parsed successfully
    pub parsed_ok: bool,
}

impl Validation {
    /// Check if there are any errors
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| matches!(d.severity, crate::lint::Severity::Error))
    }

    /// Check if there are any warnings
    pub fn has_warnings(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| matches!(d.severity, crate::lint::Severity::Warning))
    }

    /// Get only error-level diagnostics
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, crate::lint::Severity::Error))
    }

    /// Get only warning-level diagnostics
    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| matches!(d.severity, crate::lint::Severity::Warning))
    }

    /// Check if validation passed (no errors)
    pub fn is_valid(&self) -> bool {
        self.parsed_ok && !self.has_errors()
    }
}

/// Lint an OpenAPI specification with the specified rules.
///
/// # Arguments
/// * `spec` - The OpenAPI specification as a YAML or JSON string
/// * `rules` - Rule IDs to run (e.g., `["operation-id-unique", "path-params-required"]`)
///
/// # Returns
/// * `Ok(Validation)` - Validation result with diagnostics
/// * `Err(LintError)` - If the spec could not be parsed at all
///
/// # Example
/// ```
/// use lint::lint;
///
/// let spec = r#"
/// openapi: "3.1.0"
/// info:
///   title: My API
///   version: "1.0"
/// paths: {}
/// "#;
///
/// let result = lint(spec, &["openapi-version-31", "paths-not-empty"]).unwrap();
/// for diag in result.diagnostics {
///     println!("{}: {}", diag.rule.as_str(), diag.message);
/// }
/// ```
pub fn lint(spec: &str, rules: &[&str]) -> Result<Validation, LintError> {
    let rule_set = RuleSet::from_slice(rules);
    lint_with_ruleset(spec, rule_set)
}

/// Lint an OpenAPI specification with all rules enabled.
pub fn lint_all(spec: &str) -> Result<Validation, LintError> {
    lint_with_ruleset(spec, RuleSet::all())
}

/// Lint with a pre-built RuleSet
pub fn lint_with_ruleset(spec: &str, rule_set: RuleSet) -> Result<Validation, LintError> {
    // Build span database from YAML
    let span_db = SpanDbBuilder::build(spec).map_err(|e| LintError::YamlParse(e.to_string()))?;

    // Build line index for line/column conversion
    let line_index = LineIndex::new(spec);

    // Parse as OpenAPI spec
    let parsed_spec: oas3::Spec = serde_saphyr::from_str(spec).map_err(|e| {
        // Try to get useful location info from the error
        LintError::OpenApiParse(e.to_string())
    })?;

    // Build indexes
    let indexes = Indexes::build(&parsed_spec);

    // Create lint context
    let ctx = LintCtx::new(&parsed_spec, &indexes, &span_db);

    // Run rules and collect findings
    let findings = run_rules(&ctx, rule_set);

    // Resolve to diagnostics with source locations
    let diagnostics = resolve_diagnostics(findings, &span_db, &line_index);

    Ok(Validation {
        diagnostics,
        parsed_ok: true,
    })
}

/// Get all available rule IDs
pub fn available_rules() -> &'static [RuleId] {
    RuleId::all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_valid_spec() {
        let spec = r#"
openapi: "3.1.0"
info:
  title: Test API
  version: "1.0"
paths:
  /pets:
    get:
      operationId: listPets
      responses:
        "200":
          description: OK
"#;

        let result = lint_all(spec).unwrap();
        assert!(result.parsed_ok);
        // Should have no errors
        assert!(!result.has_errors());
    }

    #[test]
    fn test_lint_missing_operation_id() {
        let spec = r#"
openapi: "3.1.0"
info:
  title: Test API
  version: "1.0"
paths:
  /pets:
    get:
      responses:
        "200":
          description: OK
"#;

        let result = lint(spec, &["operation-id-required"]).unwrap();
        assert!(result.parsed_ok);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].rule, RuleId::OperationIdRequired);
    }

    #[test]
    fn test_lint_with_spans() {
        let spec = r#"openapi: "3.1.0"
info:
  title: Test API
  version: "1.0"
paths: {}
"#;

        let result = lint(spec, &["paths-not-empty"]).unwrap();
        assert!(result.parsed_ok);
        assert_eq!(result.diagnostics.len(), 1);

        // Should have valid line/col
        let diag = &result.diagnostics[0];
        assert!(diag.range.start.line > 0 || diag.range.start.col > 0);
    }

    #[test]
    fn test_lint_invalid_yaml() {
        let spec = "openapi: [invalid yaml";
        let result = lint_all(spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_available_rules() {
        let rules = available_rules();
        assert!(!rules.is_empty());
        assert!(rules.contains(&RuleId::OpenApiVersion31));
        assert!(rules.contains(&RuleId::OperationIdUnique));
    }

    #[test]
    fn test_lint_specific_rules() {
        let spec = r#"
openapi: "3.0.3"
info:
  title: ""
  version: ""
paths: {}
"#;

        // Only check version rule
        let result = lint(spec, &["openapi-version-31"]).unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].rule, RuleId::OpenApiVersion31);

        // Check all rules
        let result = lint_all(spec).unwrap();
        assert!(result.diagnostics.len() > 1);
    }
}
