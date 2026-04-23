//! Walk files/directories, parse `.asn` sources, and lower them to an
//! [`IrProgram`]. Used by the visualizer's File menu so it can open content
//! from disk without going back through the CLI.

use std::path::{Path, PathBuf};

use asn1_ir::IrProgram;
use asn1_parser::{parse_source, SourceMap};

/// Outcome of one load attempt. `program` always reflects whatever parsed
/// cleanly; `parse_errors` holds per-file failures already rendered with
/// source context so the UI can display them verbatim.
pub(crate) struct LoadedProgram {
    pub program: IrProgram,
    pub parse_errors: Vec<String>,
}

/// Walk every input (files passed through, directories recursed) collecting
/// `.asn` files, parse them, and lower the successful ones. Directories
/// named `reference` are skipped — they typically hold upstream copies kept
/// for reference only.
pub(crate) fn load_program(inputs: &[PathBuf]) -> LoadedProgram {
    let paths = collect_asn_files(inputs);
    let mut sources = SourceMap::new();
    let mut modules = Vec::new();
    let mut parse_errors = Vec::new();
    for p in &paths {
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) => {
                parse_errors.push(format!("reading {}: {e}", p.display()));
                continue;
            }
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let id = sources.add(p.clone(), text);
        match parse_source(&sources, id) {
            Ok(m) => modules.push(m),
            Err(e) => parse_errors.push(e.render(&sources)),
        }
    }
    let program = asn1_ir::lower(&modules);
    LoadedProgram { program, parse_errors }
}

fn collect_asn_files(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            for entry in walkdir::WalkDir::new(input)
                .into_iter()
                .filter_entry(|e| e.file_name() != "reference")
                .filter_map(|e| e.ok())
            {
                let p = entry.path();
                if p.is_file() && is_asn(p) {
                    out.push(p.to_path_buf());
                }
            }
        } else if input.is_file() {
            out.push(input.clone());
        }
    }
    out
}

fn is_asn(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()).map(|e| e.eq_ignore_ascii_case("asn")).unwrap_or(false)
}
