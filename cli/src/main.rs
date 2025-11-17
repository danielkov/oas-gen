//! CLI for OpenAPI code generation.

use anyhow::{Context, Result};
use clap::Parser as ClapParser;
use parser2::{parse, read};
use std::path::PathBuf;

#[derive(ClapParser, Debug)]
#[command(name = "oas-gen")]
#[command(author, version, about = "Generate SDKs and server interfaces from OpenAPI specifications", long_about = None)]
struct Args {
    /// Path to the OpenAPI specification file (JSON or YAML)
    #[arg(value_name = "SPEC")]
    spec: PathBuf,

    /// Template to use for code generation (e.g., "typescript", "python", "rust")
    #[arg(short, long, value_name = "TEMPLATE")]
    template: String,

    /// Output directory for generated code
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Service organization style
    #[arg(long, value_enum, default_value = "per-service")]
    service_style: ServiceStyleArg,

    /// Don't include documentation comments
    #[arg(long)]
    no_docs: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
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
    let args = Args::parse();

    if args.verbose {
        eprintln!("🔍 Reading OpenAPI specification: {}", args.spec.display());
    }

    // Read the OpenAPI specification
    let spec_content = read(&args.spec)?;

    if args.verbose {
        eprintln!("📄 Parsing OpenAPI specification...");
    }

    // Parse the specification
    let oas = parse(&spec_content)?;

    if args.verbose {
        eprintln!("🏗️  Building intermediate representation...");
    }

    // Build the GenIR
    let gen_ir: codegen::GenIr = oas.into();

    if args.verbose {
        eprintln!(
            "📦 Found {} types, {} services",
            gen_ir.types.len(),
            gen_ir.services.len()
        );
    }

    // Determine output directory
    let output_dir = args.output.unwrap_or_else(|| {
        let spec_name = args
            .spec
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        PathBuf::from(format!("{}-{}", spec_name, args.template))
    });

    if args.verbose {
        eprintln!("📁 Output directory: {}", output_dir.display());
    }

    // Create configuration
    let config = codegen::Config {
        service_style: args.service_style.into(),
        include_docs: !args.no_docs,
        lang_options: std::collections::BTreeMap::new(),
    };

    if args.verbose {
        eprintln!("🔨 Generating code using '{}' template...", args.template);
    }

    // Generate code
    let registry = generate::GeneratorRegistry::with_defaults();
    let vfs = registry
        .generate(&args.template, &gen_ir, &config)
        .with_context(|| format!("Failed to generate code for template '{}'", args.template))?;

    if args.verbose {
        eprintln!("💾 Writing {} files to disk...", vfs.len());
    }

    // Write to disk
    vfs.write_to_disk(&output_dir)
        .with_context(|| format!("Failed to write files to {}", output_dir.display()))?;

    if args.verbose {
        eprintln!("🪝 Running post-write hooks...");
    }

    // Call after_write_to_disk hook
    registry
        .after_write_to_disk(&args.template, &output_dir, &vfs)
        .with_context(|| "Failed to run after_write_to_disk hook")?;

    println!(
        "✅ Successfully generated {} files in {}",
        vfs.len(),
        output_dir.display()
    );

    // List generated files
    if args.verbose {
        println!("\nGenerated files:");
        for (path, _) in vfs.files() {
            println!("  📄 {}", path.display());
        }
    }

    Ok(())
}
