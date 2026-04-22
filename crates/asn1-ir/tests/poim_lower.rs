//! Lower every `.asn` file in `examples/poim/` into the IR — a smoke test that
//! the lowering step handles real-world modules without panicking.

use std::path::PathBuf;

use asn1_parser::{parse_source, SourceMap};

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").join("examples").join("poim")
}

#[test]
fn lowers_all_poim_modules() {
    let dir = fixture_dir();
    let mut sources = SourceMap::new();
    let mut parsed = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("poim fixture dir missing") {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|s| s.to_str()) == Some("asn") {
            let bytes = std::fs::read(entry.path()).unwrap();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let id = sources.add(entry.path(), text);
            parsed.push(parse_source(&sources, id).unwrap_or_else(|e| {
                panic!("{}", e.render(&sources));
            }));
        }
    }
    let program = asn1_ir::lower(&parsed);
    let total_types: usize = program.all_types().count();
    assert!(total_types > 300, "expected >300 types, got {total_types}");
    for (module, td) in program.all_types() {
        assert!(!td.name.is_empty(), "empty type name in {}", module.name);
    }
}
