//! Smoke test: parse every `.asn` file in `examples/poim/`.
//!
//! Any parse error is rendered against the `SourceMap` so the failure points at
//! the exact line/column in the offending file.

use std::path::PathBuf;

use asn1_parser::{parse_source, SourceMap};

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").join("examples").join("poim")
}

#[test]
fn parses_all_poim_modules() {
    let dir = fixture_dir();
    let mut sources = SourceMap::new();
    let mut files: Vec<(PathBuf, u32)> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("poim fixture dir missing") {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|s| s.to_str()) == Some("asn") {
            let bytes = std::fs::read(entry.path()).unwrap();
            // Some ETSI fixtures are ISO-8859; read lossily for resilience.
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let id = sources.add(entry.path(), text);
            files.push((entry.path(), id));
        }
    }
    assert!(!files.is_empty(), "no .asn files found in {:?}", dir);

    let mut failures = Vec::new();
    for (path, id) in &files {
        match parse_source(&sources, *id) {
            Ok(m) => {
                println!("parsed {}: {} assignments", path.display(), m.assignments.len());
            }
            Err(e) => {
                let rendered = e.render(&sources);
                eprintln!("{rendered}");
                failures.push(path.clone());
            }
        }
    }
    assert!(failures.is_empty(), "failed to parse: {:?}", failures);
}
