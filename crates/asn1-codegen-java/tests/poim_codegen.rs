//! Ensure codegen produces some Java for every POIM fixture without panicking.

use std::path::PathBuf;

use asn1_parser::{parse_source, SourceMap};

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").join("examples").join("poim")
}

#[test]
fn generates_java_for_poim() {
    let dir = fixture_dir();
    let mut sources = SourceMap::new();
    let mut parsed = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("poim fixture dir missing") {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|s| s.to_str()) == Some("asn") {
            let bytes = std::fs::read(entry.path()).unwrap();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let id = sources.add(entry.path(), text);
            parsed.push(parse_source(&sources, id).unwrap());
        }
    }
    let ir = asn1_ir::lower(&parsed);
    let files = asn1_codegen_java::generate(&ir, &asn1_codegen_java::JavaOptions::default());
    assert!(files.len() > 100, "expected many Java files, got {}", files.len());
    for f in &files {
        assert!(f.contents.starts_with("package "));
        assert!(!f.contents.is_empty());
    }
}
