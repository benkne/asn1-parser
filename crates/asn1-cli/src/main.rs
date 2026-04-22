//! `asn1-decoder` command-line driver.
//!
//! Subcommands:
//!   * `check`     — parse every input file, report diagnostics, exit non-zero on error.
//!   * `generate`  — parse + lower + emit Java sources into `--out`.
//!   * `visualize` — parse + lower + open the egui tree view.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use asn1_parser::{parse_source, Module, SourceMap};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "asn1-decoder", version, about = "ASN.1 → Java / visualizer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Parse ASN.1 sources and report diagnostics without emitting any output.
    Check {
        /// Files or directories containing `.asn` sources.
        inputs: Vec<PathBuf>,
    },
    /// Parse + lower + emit Java source files.
    Generate {
        /// Files or directories containing `.asn` sources.
        inputs: Vec<PathBuf>,
        /// Output root directory. Java files are placed under `<out>/<package-path>/Name.java`.
        #[arg(short, long)]
        out: PathBuf,
        /// Root Java package (default: `generated.asn1`).
        #[arg(long, default_value = "generated.asn1")]
        package: String,
    },
    /// Parse + lower, then open the interactive tree-view.
    Visualize {
        /// Files or directories containing `.asn` sources.
        inputs: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { inputs } => cmd_check(&inputs),
        Command::Generate { inputs, out, package } => cmd_generate(&inputs, &out, &package),
        Command::Visualize { inputs } => cmd_visualize(&inputs),
    }
}

fn cmd_check(inputs: &[PathBuf]) -> Result<()> {
    let (sources, modules) = load_inputs(inputs)?;
    let _ = (&sources, &modules);
    println!("parsed {} module(s) ok", modules.len());
    Ok(())
}

fn cmd_generate(inputs: &[PathBuf], out: &Path, package: &str) -> Result<()> {
    let (_, modules) = load_inputs(inputs)?;
    let ir = asn1_ir::lower(&modules);
    let opts =
        asn1_codegen_java::JavaOptions { base_package: package.to_string(), indent: "    ".into() };
    let files = asn1_codegen_java::generate(&ir, &opts);
    for f in &files {
        let path = out.join(&f.relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&path, &f.contents)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    println!("wrote {} Java file(s) under {}", files.len(), out.display());
    Ok(())
}

fn cmd_visualize(inputs: &[PathBuf]) -> Result<()> {
    let (_, modules) = load_inputs(inputs)?;
    let ir = asn1_ir::lower(&modules);
    asn1_viz::launch(ir).map_err(|e| anyhow!("visualizer failed: {e}"))
}

fn load_inputs(inputs: &[PathBuf]) -> Result<(SourceMap, Vec<Module>)> {
    if inputs.is_empty() {
        return Err(anyhow!("no input files or directories supplied"));
    }
    let mut paths = Vec::new();
    for input in inputs {
        if input.is_dir() {
            for entry in walkdir::WalkDir::new(input).into_iter().filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("asn") {
                    paths.push(p.to_path_buf());
                }
            }
        } else if input.is_file() {
            paths.push(input.clone());
        } else {
            return Err(anyhow!("not a file or directory: {}", input.display()));
        }
    }
    if paths.is_empty() {
        return Err(anyhow!("no `.asn` files found in inputs"));
    }

    let mut sources = SourceMap::new();
    let mut modules = Vec::new();
    let mut failures = 0usize;
    for p in &paths {
        let bytes = std::fs::read(p).with_context(|| format!("reading {}", p.display()))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let id = sources.add(p.clone(), text);
        match parse_source(&sources, id) {
            Ok(m) => modules.push(m),
            Err(e) => {
                eprintln!("{}", e.render(&sources));
                failures += 1;
            }
        }
    }
    if failures > 0 {
        return Err(anyhow!("{failures} file(s) failed to parse"));
    }
    Ok((sources, modules))
}
