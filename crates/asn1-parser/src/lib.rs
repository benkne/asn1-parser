//! ASN.1 concrete-syntax parser.
//!
//! ```no_run
//! use asn1_parser::{parse_source, SourceMap};
//! let mut map = SourceMap::new();
//! let src = std::fs::read_to_string("examples/poim/POIM-PDU-Description.asn").unwrap();
//! let file = map.add("POIM-PDU-Description.asn", src);
//! let module = parse_source(&map, file).unwrap();
//! println!("parsed module {}", module.name.value);
//! ```

#![deny(rust_2018_idioms)]

pub mod cst;
pub mod diagnostics;
mod grammar;
mod lexer;

pub use cst::*;
pub use diagnostics::{FileId, Location, ParseError, SourceFile, SourceMap, Span, Spanned};
pub use grammar::parse_module as parse_tokens;

/// Tokenize and parse the source text registered under `file` in `sources`.
pub fn parse_source(sources: &SourceMap, file: FileId) -> Result<Module, ParseError> {
    let source = sources
        .get(file)
        .ok_or_else(|| ParseError::new("file id not registered in source map", Span::DUMMY))?;
    let tokens = lexer::Lexer::new(file, &source.source).tokenize()?;
    grammar::parse_module(tokens)
}

/// Convenience: parse a source string that is not yet in a `SourceMap`.
///
/// Adds the file and returns both the source id and the parsed module so the
/// caller can render diagnostics with full context.
pub fn parse_text(
    sources: &mut SourceMap,
    path: impl Into<std::path::PathBuf>,
    source: String,
) -> Result<(FileId, Module), ParseError> {
    let file = sources.add(path, source);
    let module = parse_source(sources, file)?;
    Ok((file, module))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(src: &str) -> Module {
        let mut sm = SourceMap::new();
        let file = sm.add("test.asn", src.to_string());
        match parse_source(&sm, file) {
            Ok(m) => m,
            Err(e) => panic!("{}", e.render(&sm)),
        }
    }

    #[test]
    fn minimal_module() {
        let m = parse_str(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Bar ::= INTEGER
            END"#,
        );
        assert_eq!(m.name.value, "Foo");
        assert_eq!(m.tag_default, TagDefault::Automatic);
        assert_eq!(m.assignments.len(), 1);
    }

    #[test]
    fn sequence_of_with_size_constraint() {
        let m = parse_str(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Items ::= SEQUENCE (SIZE (1..8,...)) OF INTEGER
            END"#,
        );
        let Some(Assignment { kind: AssignmentKind::Type(t), .. }) =
            m.assignments.into_iter().next()
        else {
            panic!("expected type assignment");
        };
        match t.kind {
            TypeKind::SequenceOf(_) => {}
            _ => panic!("expected SEQUENCE OF"),
        }
        assert!(matches!(t.constraints.as_slice(), [Constraint::Size(_)]));
    }

    #[test]
    fn enumerated_with_extension() {
        let m = parse_str(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Color ::= ENUMERATED { red, green (1), blue, ..., yellow (99) }
            END"#,
        );
        if let AssignmentKind::Type(t) = &m.assignments[0].kind {
            if let TypeKind::Enumerated { items, extensible, extension_items } = &t.kind {
                assert_eq!(items.len(), 3);
                assert!(*extensible);
                assert_eq!(extension_items.len(), 1);
            } else {
                panic!("expected enumerated");
            }
        } else {
            panic!("expected type");
        }
    }

    #[test]
    fn doc_comment_attaches_to_assignment() {
        let m = parse_str(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                /** the answer */
                answer ::= INTEGER
            END"#,
        );
        assert_eq!(m.assignments[0].doc.as_deref(), Some("the answer"));
    }

    #[test]
    fn imports_parsed() {
        let m = parse_str(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                IMPORTS
                    A, B FROM OtherMod {iso(1) mod(2)} WITH SUCCESSORS
                    C FROM ThirdMod
                ;
                X ::= A
            END"#,
        );
        assert_eq!(m.imports.len(), 2);
        assert_eq!(m.imports[0].symbols.len(), 2);
        assert_eq!(m.imports[0].with, Some(WithClause::Successors));
        assert_eq!(m.imports[1].with, None);
    }
}
