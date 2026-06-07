//! CLI for OpenAPI code generation.

use anyhow::{Context, Result};
use clap::{Parser as ClapParser, Subcommand};
use parser::{parse, read};
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Top-level solder.toml configuration.
#[derive(Debug, Clone, Default, Deserialize)]
struct SolderConfig {
    #[serde(rename = "rust-axum", default)]
    rust_axum: Option<RustAxumConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct RustAxumConfig {
    /// "single-crate" (default) or "multi-crate"
    #[serde(default = "default_layout")]
    layout: String,
    /// Template for tag-crate names; supports the "{tag}" placeholder.
    #[serde(default = "default_crate_name")]
    crate_name: String,
    /// Full name of the common crate (no templating).
    #[serde(default = "default_common_name")]
    common_name: String,
}

impl Default for RustAxumConfig {
    fn default() -> Self {
        Self {
            layout: default_layout(),
            crate_name: default_crate_name(),
            common_name: default_common_name(),
        }
    }
}

fn default_layout() -> String {
    "single-crate".to_string()
}
fn default_crate_name() -> String {
    "api-{tag}".to_string()
}
fn default_common_name() -> String {
    "api-common".to_string()
}

/// Load solder.toml from either an explicit path or by auto-discovering it in `cwd`.
fn load_solder_config(explicit: Option<&Path>, cwd: &Path) -> Result<SolderConfig> {
    let path = if let Some(p) = explicit {
        Some(p.to_path_buf())
    } else {
        let candidate = cwd.join("solder.toml");
        if candidate.is_file() {
            Some(candidate)
        } else {
            None
        }
    };

    let Some(path) = path else {
        return Ok(SolderConfig::default());
    };

    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let cfg: SolderConfig = toml::from_str(&text)
        .with_context(|| format!("Failed to parse solder config: {}", path.display()))?;
    Ok(cfg)
}

#[derive(ClapParser, Debug)]
#[command(name = "oas-gen")]
#[command(author, version, about = "Generate SDKs and server interfaces from OpenAPI specifications", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Generate SDK or server code from an OpenAPI specification
    Generate {
        /// Path to the OpenAPI specification file (JSON or YAML)
        #[arg(value_name = "SPEC")]
        spec: PathBuf,

        /// Template to use for code generation (e.g., "typescript", "rust-axum")
        #[arg(short, long, value_name = "TEMPLATE")]
        template: String,

        /// Output directory for generated code
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,

        /// Path to solder.toml (overrides auto-discovery in cwd)
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,

        /// Service organization style
        #[arg(long, value_enum, default_value = "per-service")]
        service_style: ServiceStyleArg,

        /// Don't include documentation comments
        #[arg(long)]
        no_docs: bool,

        /// Resolve external references before generating
        #[arg(short = 'r', long)]
        resolve: bool,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Resolve external $ref references and output a single combined spec
    Resolve {
        /// Path to the OpenAPI specification file (JSON or YAML)
        #[arg(value_name = "SPEC")]
        spec: PathBuf,

        /// Output file for resolved spec (defaults to stdout)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },

    /// Lint an OpenAPI specification
    Lint {
        /// Path to the OpenAPI specification file (JSON or YAML)
        #[arg(value_name = "SPEC")]
        spec: PathBuf,

        /// Comma-separated list of rulesets to enable (e.g., "recommended,strict")
        #[arg(long, value_name = "RULESETS")]
        rulesets: Option<String>,

        /// Comma-separated list of specific rules to enable (e.g., "operation-id-required,paths-not-empty")
        #[arg(long, value_name = "RULES")]
        rules: Option<String>,

        /// Filter by severity levels (e.g., "error,warning"). Available: error, warning, info, hint
        #[arg(long, value_name = "SEVERITY")]
        severity: Option<String>,
    },

    /// Deep merge multiple JSON or YAML files
    Merge {
        /// Files or directories to merge (first file determines output format)
        #[arg(value_name = "FILES", required = true, num_args = 1..)]
        files: Vec<PathBuf>,

        /// Output file (defaults to stdout)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum ServiceStyleArg {
    PerService,
    SingleClient,
    ByTag,
}

impl From<ServiceStyleArg> for codegen::ServiceStyle {
    fn from(style: ServiceStyleArg) -> Self {
        match style {
            ServiceStyleArg::PerService => codegen::ServiceStyle::PerService,
            ServiceStyleArg::SingleClient => codegen::ServiceStyle::SingleClient,
            ServiceStyleArg::ByTag => codegen::ServiceStyle::ByTag,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            spec,
            template,
            output,
            config,
            service_style,
            no_docs,
            resolve,
            verbose,
        } => handle_generate(
            spec,
            template,
            output,
            config,
            service_style,
            no_docs,
            resolve,
            verbose,
        ),
        Commands::Resolve {
            spec,
            output,
            verbose,
        } => handle_resolve(spec, output, verbose),
        Commands::Lint {
            spec,
            rulesets,
            rules,
            severity,
        } => handle_lint(spec, rulesets, rules, severity),
        Commands::Merge {
            files,
            output,
            verbose,
        } => handle_merge(files, output, verbose),
    }
}

fn handle_resolve(spec: PathBuf, output: Option<PathBuf>, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("🔍 Reading OpenAPI specification: {}", spec.display());
    }

    if verbose {
        eprintln!("🔗 Resolving external references...");
    }

    // Resolve the spec
    let resolved = parser::resolve::resolve(&spec)
        .with_context(|| format!("Failed to resolve spec at {}", spec.display()))?;

    if verbose {
        eprintln!("✅ Successfully resolved all external references");
    }

    // Write output
    if let Some(output_path) = output {
        if verbose {
            eprintln!("💾 Writing resolved spec to: {}", output_path.display());
        }
        std::fs::write(&output_path, &resolved)
            .with_context(|| format!("Failed to write to {}", output_path.display()))?;
        println!("✅ Resolved spec written to {}", output_path.display());
    } else {
        // Output to stdout
        println!("{}", resolved);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_generate(
    spec: PathBuf,
    template: String,
    output: Option<PathBuf>,
    config_path: Option<PathBuf>,
    service_style: ServiceStyleArg,
    no_docs: bool,
    resolve: bool,
    verbose: bool,
) -> Result<()> {
    if verbose {
        eprintln!("🔍 Reading OpenAPI specification: {}", spec.display());
    }

    // Read the OpenAPI specification
    let spec_content = if resolve {
        if verbose {
            eprintln!("🔗 Resolving external references...");
        }
        parser::resolve::resolve(&spec)
            .with_context(|| format!("Failed to resolve spec at {}", spec.display()))?
    } else {
        read(&spec)?
    };

    if verbose {
        eprintln!("📄 Parsing OpenAPI specification...");
    }

    // Parse the specification
    let oas = parse(&spec_content)?;

    if verbose {
        eprintln!("🏗️  Building intermediate representation...");
    }

    // Build the GenIR
    let gen_ir: codegen::GenIr = oas.into();

    if verbose {
        eprintln!(
            "📦 Found {} types, {} services",
            gen_ir.types.len(),
            gen_ir.services.len()
        );
    }

    // Determine output directory
    let output_dir = output.unwrap_or_else(|| {
        let spec_name = spec
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        PathBuf::from(format!("{}-{}", spec_name, template))
    });

    if verbose {
        eprintln!("📁 Output directory: {}", output_dir.display());
    }

    // Load solder.toml config (explicit path or auto-discovered in cwd).
    let cwd = std::env::current_dir().context("Failed to get current working directory")?;
    let solder_cfg = load_solder_config(config_path.as_deref(), &cwd)?;

    // Create configuration; inject the rust-axum block as a lang_option so the
    // generator can read it without adding a new trait dependency.
    let mut lang_options = std::collections::BTreeMap::new();
    if let Some(rust_axum) = &solder_cfg.rust_axum {
        let value = serde_json::json!({
            "layout": rust_axum.layout,
            "crate_name": rust_axum.crate_name,
            "common_name": rust_axum.common_name,
        });
        lang_options.insert("rust-axum".to_string(), value);
    }

    let config = codegen::Config {
        service_style: service_style.into(),
        include_docs: !no_docs,
        lang_options,
    };

    if verbose {
        eprintln!("🔨 Generating code using '{}' template...", template);
    }

    // Generate code
    let registry = generate::GeneratorRegistry::with_defaults();
    let vfs = registry
        .generate(&template, &gen_ir, &config)
        .with_context(|| format!("Failed to generate code for template '{}'", template))?;

    if verbose {
        eprintln!("💾 Writing {} files to disk...", vfs.len());
    }

    // Write to disk
    vfs.write_to_disk(&output_dir)
        .with_context(|| format!("Failed to write files to {}", output_dir.display()))?;

    if verbose {
        eprintln!("🪝 Running post-write hooks...");
    }

    // Call after_write_to_disk hook
    registry
        .after_write_to_disk(&template, &output_dir, &vfs)
        .with_context(|| "Failed to run after_write_to_disk hook")?;

    println!(
        "✅ Successfully generated {} files in {}",
        vfs.len(),
        output_dir.display()
    );

    // List generated files
    if verbose {
        println!("\nGenerated files:");
        for (path, _) in vfs.files() {
            println!("  📄 {}", path.display());
        }
    }

    Ok(())
}

fn handle_lint(
    spec: PathBuf,
    rulesets: Option<String>,
    rules: Option<String>,
    severity: Option<String>,
) -> Result<()> {
    // Read the spec file
    let spec_content = std::fs::read_to_string(&spec)
        .with_context(|| format!("Failed to read spec file: {}", spec.display()))?;

    // Build the ruleset based on provided options
    let rule_set = if let Some(ruleset_str) = rulesets {
        build_ruleset_from_names(&ruleset_str)?
    } else if let Some(rules_str) = rules {
        // Parse individual rules
        let rule_names: Vec<&str> = rules_str.split(',').map(|s| s.trim()).collect();
        lint::RuleSet::from_slice(&rule_names)
    } else {
        // Default to all rules
        lint::RuleSet::all()
    };

    // Parse severity filter
    let severity_filter = if let Some(severity_str) = severity {
        Some(parse_severity_filter(&severity_str)?)
    } else {
        None
    };

    // Run the linter
    let result =
        lint::lint_with_ruleset(&spec_content, rule_set).context("Failed to lint specification")?;

    // Print diagnostics in rustc-style format
    let spec_display = spec.display().to_string();
    print_diagnostics(
        &result,
        &spec_display,
        &spec_content,
        severity_filter.as_ref(),
    );

    // Exit with error code if there are errors (only if we're showing errors)
    if result.has_errors()
        && (severity_filter.is_none()
            || severity_filter
                .as_ref()
                .unwrap()
                .contains(&lint::Severity::Error))
    {
        std::process::exit(1);
    }

    Ok(())
}

fn parse_severity_filter(severity_str: &str) -> Result<Vec<lint::Severity>> {
    let mut severities = Vec::new();

    for s in severity_str.split(',').map(|s| s.trim()) {
        match s {
            "error" => severities.push(lint::Severity::Error),
            "warning" => severities.push(lint::Severity::Warning),
            "info" => severities.push(lint::Severity::Info),
            "hint" => severities.push(lint::Severity::Hint),
            other => {
                anyhow::bail!(
                    "Unknown severity level: '{}'. Available levels: error, warning, info, hint",
                    other
                );
            }
        }
    }

    Ok(severities)
}

fn build_ruleset_from_names(ruleset_str: &str) -> Result<lint::RuleSet> {
    let mut rule_set = lint::RuleSet::new();

    for name in ruleset_str.split(',').map(|s| s.trim()) {
        match name {
            "all" => {
                rule_set = lint::RuleSet::all();
            }
            "recommended" => {
                // Add recommended rules for good API design
                for rule in &[
                    "openapi-version-31",
                    "info-title-required",
                    "info-version-required",
                    "info-description-present",
                    "paths-not-empty",
                    "operation-id-required",
                    "operation-id-unique",
                    "responses-exist",
                    "response-description-required",
                    "path-params-declared",
                    "path-params-required",
                ] {
                    rule_set.enable(lint::RuleId::parse(rule).unwrap());
                }
            }
            "strict" => {
                // Add all error-level rules
                for rule in lint::available_rules() {
                    if matches!(rule.default_severity(), lint::Severity::Error) {
                        rule_set.enable(*rule);
                    }
                }
            }
            "security" => {
                // Security-focused rules
                for rule in &[
                    "servers-https-required",
                    "security-schemes-defined",
                    "security-declared",
                    "security-no-api-key-in-query",
                    "security-oauth2-complete",
                ] {
                    if let Some(rule_id) = lint::RuleId::parse(rule) {
                        rule_set.enable(rule_id);
                    }
                }
            }
            "style" => {
                // Style and consistency rules
                for rule in &[
                    "path-style-normalized",
                    "path-no-verbs",
                    "param-naming-consistent",
                    "schema-naming-conventions",
                    "operation-summary-required",
                ] {
                    if let Some(rule_id) = lint::RuleId::parse(rule) {
                        rule_set.enable(rule_id);
                    }
                }
            }
            other => {
                anyhow::bail!(
                    "Unknown ruleset: '{}'. Available rulesets: all, recommended, strict, security, style",
                    other
                );
            }
        }
    }

    Ok(rule_set)
}

fn print_diagnostics(
    result: &lint::Validation,
    spec_path: &str,
    spec_content: &str,
    severity_filter: Option<&Vec<lint::Severity>>,
) {
    // Filter diagnostics by severity if specified
    let filtered_diagnostics: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            if let Some(filter) = severity_filter {
                filter.contains(&d.severity)
            } else {
                true
            }
        })
        .collect();

    if filtered_diagnostics.is_empty() {
        println!("✅ No issues found");
        return;
    }

    let lines: Vec<&str> = spec_content.lines().collect();
    let error_count = filtered_diagnostics
        .iter()
        .filter(|d| matches!(d.severity, lint::Severity::Error))
        .count();
    let warning_count = filtered_diagnostics
        .iter()
        .filter(|d| matches!(d.severity, lint::Severity::Warning))
        .count();

    for diag in &filtered_diagnostics {
        let severity_str = match diag.severity {
            lint::Severity::Error => "\x1b[1;31merror\x1b[0m",
            lint::Severity::Warning => "\x1b[1;33mwarning\x1b[0m",
            lint::Severity::Info => "\x1b[1;36minfo\x1b[0m",
            lint::Severity::Hint => "\x1b[1;34mhint\x1b[0m",
        };

        // Print the main diagnostic line
        println!(
            "{}: {}\x1b[1m[{}]\x1b[0m",
            severity_str,
            diag.message,
            diag.rule.as_str()
        );

        // Print location
        let line_num = diag.range.start.line + 1;
        let col_num = diag.range.start.col + 1;
        println!(
            "  \x1b[1;36m-->\x1b[0m {}:{}:{}",
            spec_path, line_num, col_num
        );

        // Print the source line with context
        if let Some(line_text) = lines.get(diag.range.start.line as usize) {
            // Calculate gutter width based on line number
            let gutter_width = line_num.to_string().len();
            let gutter_padding = " ".repeat(gutter_width);

            println!("{} \x1b[1;36m|\x1b[0m", gutter_padding);
            println!(
                "\x1b[1;36m{:>width$} |\x1b[0m {}",
                line_num,
                line_text,
                width = gutter_width
            );

            // Print the caret indicator
            let spaces = " ".repeat(diag.range.start.col as usize);
            let carets = if diag.range.end.line == diag.range.start.line {
                let len = (diag.range.end.col - diag.range.start.col).max(1);
                "^".repeat(len as usize)
            } else {
                "^".to_string()
            };

            let caret_color = match diag.severity {
                lint::Severity::Error => "\x1b[1;31m",
                lint::Severity::Warning => "\x1b[1;33m",
                lint::Severity::Info => "\x1b[1;36m",
                lint::Severity::Hint => "\x1b[1;34m",
            };

            println!(
                "{} \x1b[1;36m|\x1b[0m {}{}{}\x1b[0m",
                gutter_padding, spaces, caret_color, carets
            );
            println!("{} \x1b[1;36m|\x1b[0m", gutter_padding);
        }

        println!();
    }

    // Print summary
    let mut summary_parts = Vec::new();
    if error_count > 0 {
        summary_parts.push(format!(
            "\x1b[1;31m{} error{}\x1b[0m",
            error_count,
            if error_count == 1 { "" } else { "s" }
        ));
    }
    if warning_count > 0 {
        summary_parts.push(format!(
            "\x1b[1;33m{} warning{}\x1b[0m",
            warning_count,
            if warning_count == 1 { "" } else { "s" }
        ));
    }

    if !summary_parts.is_empty() {
        println!("{}", summary_parts.join(", "));
    }
}

fn handle_merge(files: Vec<PathBuf>, output: Option<PathBuf>, verbose: bool) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("At least one file is required");
    }

    // Collect all files (expand directories)
    let all_files = collect_files(&files)?;

    if all_files.is_empty() {
        anyhow::bail!("No JSON or YAML files found");
    }

    if verbose {
        eprintln!("🔍 Found {} files to merge", all_files.len());
        for file in &all_files {
            eprintln!("  📄 {}", file.display());
        }
    }

    // Determine output format from first file
    let first_file = &all_files[0];
    let output_json = is_json_file(first_file);

    if verbose {
        eprintln!(
            "📝 Output format: {} (based on {})",
            if output_json { "JSON" } else { "YAML" },
            first_file.display()
        );
    }

    // Parse and merge all files
    let mut result: Option<Value> = None;

    for file in &all_files {
        if verbose {
            eprintln!("🔗 Merging: {}", file.display());
        }

        let content = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read file: {}", file.display()))?;

        let value: Value = if is_json_file(file) {
            serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse JSON: {}", file.display()))?
        } else {
            // Use strict_booleans to avoid YAML 1.1 quirks where y/n/yes/no/on/off
            // are interpreted as booleans instead of strings
            let options = serde_saphyr::Options {
                strict_booleans: true,
                ..Default::default()
            };
            serde_saphyr::from_str_with_options(&content, options)
                .with_context(|| format!("Failed to parse YAML: {}", file.display()))?
        };

        result = Some(match result {
            None => value,
            Some(left) => deep_merge(left, value),
        });
    }

    let merged = result.unwrap();

    // Serialize output
    let output_content = if output_json {
        serde_json::to_string_pretty(&merged).context("Failed to serialize JSON")?
    } else {
        serde_saphyr::to_string(&merged).context("Failed to serialize YAML")?
    };

    // Write output
    if let Some(output_path) = output {
        if verbose {
            eprintln!("💾 Writing merged output to: {}", output_path.display());
        }
        std::fs::write(&output_path, &output_content)
            .with_context(|| format!("Failed to write to {}", output_path.display()))?;
        println!(
            "✅ Merged {} files into {}",
            all_files.len(),
            output_path.display()
        );
    } else {
        println!("{}", output_content);
    }

    Ok(())
}

/// Collect all JSON/YAML files from paths (expanding directories)
fn collect_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        if path.is_dir() {
            // Collect and sort directory entries for deterministic ordering
            let mut dir_files: Vec<PathBuf> = std::fs::read_dir(path)
                .with_context(|| format!("Failed to read directory: {}", path.display()))?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| p.is_file() && is_json_or_yaml_file(p))
                .collect();
            dir_files.sort();
            files.extend(dir_files);
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            anyhow::bail!("Path does not exist: {}", path.display());
        }
    }

    Ok(files)
}

fn is_json_file(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

fn is_json_or_yaml_file(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|ext| {
        ext.eq_ignore_ascii_case("json")
            || ext.eq_ignore_ascii_case("yaml")
            || ext.eq_ignore_ascii_case("yml")
    })
}

/// Deep merge two JSON values.
/// - Primitives: left wins
/// - Arrays: concatenate [...left, ...right]
/// - Objects: recursively merge
fn deep_merge(left: Value, right: Value) -> Value {
    match (left, right) {
        // Both are objects: merge recursively
        (Value::Object(mut left_map), Value::Object(right_map)) => {
            for (key, right_val) in right_map {
                left_map
                    .entry(&key)
                    .and_modify(|left_val| {
                        *left_val = deep_merge(left_val.take(), right_val.clone());
                    })
                    .or_insert(right_val);
            }
            Value::Object(left_map)
        }
        // Both are arrays: concatenate
        (Value::Array(mut left_arr), Value::Array(right_arr)) => {
            left_arr.extend(right_arr);
            Value::Array(left_arr)
        }
        // Primitives or mismatched types: left wins
        (left, _) => left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "solder-cfg-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn load_returns_default_when_no_file_present() {
        let dir = tmp_dir();
        let cfg = load_solder_config(None, &dir).unwrap();
        assert!(cfg.rust_axum.is_none());
    }

    #[test]
    fn auto_discovers_solder_toml_in_cwd() {
        let dir = tmp_dir();
        std::fs::write(
            dir.join("solder.toml"),
            r#"
[rust-axum]
layout = "multi-crate"
crate_name = "tp-api-{tag}"
common_name = "tp-api-common"
"#,
        )
        .unwrap();
        let cfg = load_solder_config(None, &dir).unwrap();
        let ra = cfg.rust_axum.expect("rust-axum block present");
        assert_eq!(ra.layout, "multi-crate");
        assert_eq!(ra.crate_name, "tp-api-{tag}");
        assert_eq!(ra.common_name, "tp-api-common");
    }

    #[test]
    fn explicit_path_overrides_auto_discovery() {
        let dir = tmp_dir();
        // The file in cwd should be ignored when --config is provided.
        std::fs::write(
            dir.join("solder.toml"),
            r#"[rust-axum]
layout = "single-crate"
"#,
        )
        .unwrap();

        let other = dir.join("alternate.toml");
        std::fs::write(
            &other,
            r#"[rust-axum]
layout = "multi-crate"
crate_name = "alt-{tag}"
common_name = "alt-common"
"#,
        )
        .unwrap();

        let cfg = load_solder_config(Some(&other), &dir).unwrap();
        let ra = cfg.rust_axum.expect("rust-axum block present");
        assert_eq!(ra.layout, "multi-crate");
        assert_eq!(ra.crate_name, "alt-{tag}");
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let dir = tmp_dir();
        std::fs::write(
            dir.join("solder.toml"),
            r#"[rust-axum]
layout = "multi-crate"
"#,
        )
        .unwrap();
        let cfg = load_solder_config(None, &dir).unwrap();
        let ra = cfg.rust_axum.expect("rust-axum block present");
        assert_eq!(ra.layout, "multi-crate");
        assert_eq!(ra.crate_name, default_crate_name());
        assert_eq!(ra.common_name, default_common_name());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let dir = tmp_dir();
        std::fs::write(dir.join("solder.toml"), "this is [not valid").unwrap();
        assert!(load_solder_config(None, &dir).is_err());
    }
}
