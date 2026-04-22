//! Java 17 source generator for ASN.1 IR.
//!
//! Each [`asn1_ir::IrTypeDef`] produces one top-level Java file. SEQUENCE / SET
//! become `record`s, CHOICE becomes a `sealed interface` with nested record
//! alternatives, ENUMERATED becomes an `enum`, and primitive wrappers are emitted
//! as single-field records so every ASN.1 named type has a distinct Java type.
//!
//! Inline anonymous composites (a SEQUENCE inside a field, for instance) are
//! hoisted into nested static types under their enclosing class.

#![deny(rust_2018_idioms)]

use std::fmt::Write as _;
use std::path::PathBuf;

use asn1_ir::{
    IrCharKind, IrChoice, IrConstraint, IrField, IrItem, IrOptionality, IrProgram, IrStruct,
    IrStructMember, IrType, IrTypeDef,
};

/// Generator options — tweak how class files are laid out on disk.
#[derive(Debug, Clone)]
pub struct JavaOptions {
    /// Root Java package, e.g. `com.example.asn1`. The per-module package is
    /// appended as `<base>.<module_slug>`.
    pub base_package: String,
    pub indent: String,
}

impl Default for JavaOptions {
    fn default() -> Self {
        Self { base_package: "generated.asn1".into(), indent: "    ".into() }
    }
}

/// A single emitted Java source file.
#[derive(Debug, Clone)]
pub struct JavaFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

/// Top-level entry point: emit Java sources for every type in the program.
pub fn generate(program: &IrProgram, opts: &JavaOptions) -> Vec<JavaFile> {
    let resolver = ClassResolver::build(program, opts);
    let mut files = Vec::new();
    for module in &program.modules {
        let pkg = resolver.package_for(&module.name);
        for item in &module.items {
            if let IrItem::Type(t) = item {
                let contents = emit_type_file(&pkg, t, &resolver, opts);
                let mut path: PathBuf = pkg.split('.').collect();
                path.push(format!("{}.java", type_name(&t.name)));
                files.push(JavaFile { relative_path: path, contents });
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// File emission
// ---------------------------------------------------------------------------

fn emit_type_file(
    package: &str,
    td: &IrTypeDef,
    resolver: &ClassResolver<'_>,
    opts: &JavaOptions,
) -> String {
    let mut w = Writer::new(opts.indent.clone());
    w.line(&format!("package {package};"));
    w.blank();
    w.line("import java.util.List;");
    w.line("import java.util.Optional;");
    w.line("import java.util.BitSet;");
    w.blank();

    if let Some(doc) = &td.doc {
        emit_javadoc(&mut w, doc);
    }

    emit_type(&mut w, &type_name(&td.name), &td.ty, resolver, true);
    w.out
}

fn emit_type(
    w: &mut Writer,
    class_name: &str,
    ty: &IrType,
    resolver: &ClassResolver<'_>,
    top_level: bool,
) {
    let vis = if top_level { "public " } else { "public static " };
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => emit_record(w, vis, class_name, s, resolver),
        IrType::Choice(c) => emit_choice(w, vis, class_name, c, resolver),
        IrType::Enumerated { items, .. } => {
            w.line(&format!("{vis}enum {class_name} {{"));
            w.indent();
            let names: Vec<String> = items.iter().map(|i| enum_constant(&i.name)).collect();
            for (idx, (name, item)) in names.iter().zip(items).enumerate() {
                let sep = if idx + 1 == items.len() { ";" } else { "," };
                if let Some(doc) = &item.doc {
                    emit_javadoc(w, doc);
                }
                w.line(&format!("{name}{sep}"));
            }
            w.dedent();
            w.line("}");
        }
        _ => {
            // Wrap scalar / reference / collection types in a single-field record
            // so every named ASN.1 type gets a distinct Java identity.
            let (jty, _needs_optional) = java_type_for(ty, resolver);
            w.line(&format!("{vis}record {class_name}({jty} value) {{"));
            w.line("}");
        }
    }
}

fn emit_record(
    w: &mut Writer,
    vis: &str,
    class_name: &str,
    s: &IrStruct,
    resolver: &ClassResolver<'_>,
) {
    let mut fields: Vec<(String, String, Option<String>)> = Vec::new();
    let mut nested: Vec<(String, IrType)> = Vec::new();

    for member in &s.members {
        match member {
            IrStructMember::Field(f) => {
                let (jty, name, doc) = prepare_field(f, resolver, class_name, &mut nested);
                fields.push((jty, name, doc));
            }
            IrStructMember::ComponentsOf { type_ref } => {
                let comment = format!(
                    "// COMPONENTS OF {type_ref} — inlined members are not expanded in codegen."
                );
                fields.push((String::new(), String::new(), Some(comment)));
            }
        }
    }

    // Keep only real fields; move COMPONENTS-OF notes into javadoc on the class.
    let notes: Vec<String> =
        fields.iter().filter(|(t, _, _)| t.is_empty()).filter_map(|(_, _, d)| d.clone()).collect();
    let real_fields: Vec<(String, String, Option<String>)> =
        fields.into_iter().filter(|(t, _, _)| !t.is_empty()).collect();

    if !notes.is_empty() {
        w.line("/*");
        for n in &notes {
            w.line(&format!(" * {}", n.trim_start_matches("// ")));
        }
        w.line(" */");
    }

    if real_fields.is_empty() {
        w.line(&format!("{vis}record {class_name}() {{"));
    } else {
        w.line(&format!("{vis}record {class_name}("));
        w.indent();
        for (i, (jty, name, doc)) in real_fields.iter().enumerate() {
            if let Some(d) = doc {
                w.line(&format!("// {d}"));
            }
            let sep = if i + 1 == real_fields.len() { "" } else { "," };
            w.line(&format!("{jty} {name}{sep}"));
        }
        w.dedent();
        w.line(") {");
    }
    w.indent();
    for (nname, nty) in &nested {
        w.blank();
        emit_type(w, nname, nty, resolver, false);
    }
    w.dedent();
    w.line("}");
}

fn emit_choice(
    w: &mut Writer,
    vis: &str,
    class_name: &str,
    c: &IrChoice,
    resolver: &ClassResolver<'_>,
) {
    let alt_names: Vec<String> = c.alternatives.iter().map(|a| pascal_case(&a.name)).collect();
    let permits =
        alt_names.iter().map(|n| format!("{class_name}.{n}")).collect::<Vec<_>>().join(", ");
    if c.alternatives.is_empty() {
        w.line(&format!("{vis}sealed interface {class_name} {{"));
    } else {
        w.line(&format!("{vis}sealed interface {class_name} permits {permits} {{"));
    }
    w.indent();
    let mut nested: Vec<(String, IrType)> = Vec::new();
    for (alt, aname) in c.alternatives.iter().zip(alt_names.iter()) {
        if let Some(d) = &alt.doc {
            emit_javadoc(w, d);
        }
        let (jty, _) = prepare_alt_type(&alt.ty, resolver, class_name, aname, &mut nested);
        w.line(&format!("record {aname}({jty} value) implements {class_name} {{}}"));
    }
    for (nname, nty) in &nested {
        w.blank();
        emit_type(w, nname, nty, resolver, false);
    }
    w.dedent();
    w.line("}");
}

fn prepare_field(
    f: &IrField,
    resolver: &ClassResolver<'_>,
    parent_class: &str,
    nested: &mut Vec<(String, IrType)>,
) -> (String, String, Option<String>) {
    let fname = field_name(&f.name);
    let (jty, hoistable) = java_type_for_field(&f.ty, resolver, parent_class, &fname, nested);
    let _ = hoistable;
    let wrapped = match f.optionality {
        IrOptionality::Required => jty,
        IrOptionality::Optional | IrOptionality::Default(_) => wrap_optional(&jty),
    };
    let doc = f.doc.clone();
    (wrapped, fname, doc)
}

fn prepare_alt_type(
    ty: &IrType,
    resolver: &ClassResolver<'_>,
    parent_class: &str,
    alt_name: &str,
    nested: &mut Vec<(String, IrType)>,
) -> (String, bool) {
    java_type_for_field(ty, resolver, parent_class, alt_name, nested)
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

fn java_type_for_field(
    ty: &IrType,
    resolver: &ClassResolver<'_>,
    parent: &str,
    field_name_hint: &str,
    nested: &mut Vec<(String, IrType)>,
) -> (String, bool) {
    match ty {
        IrType::Sequence(_) | IrType::Set(_) | IrType::Choice(_) | IrType::Enumerated { .. } => {
            let nested_name = pascal_case(field_name_hint);
            let full = format!("{parent}.{nested_name}");
            nested.push((nested_name, ty.clone()));
            (full, true)
        }
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            let (elem, _) = java_type_for_field(
                element,
                resolver,
                parent,
                &format!("{}Item", pascal_case(field_name_hint)),
                nested,
            );
            (format!("List<{}>", box_primitive(&elem)), false)
        }
        _ => {
            let (jty, _) = java_type_for(ty, resolver);
            (jty, false)
        }
    }
}

fn java_type_for(ty: &IrType, resolver: &ClassResolver<'_>) -> (String, bool) {
    match ty {
        IrType::Boolean => ("boolean".into(), false),
        IrType::Integer { constraints, .. } => {
            if integer_fits_int(constraints) {
                ("int".into(), false)
            } else {
                ("long".into(), false)
            }
        }
        IrType::Real => ("double".into(), false),
        IrType::Null => ("Object".into(), false),
        IrType::BitString { .. } => ("BitSet".into(), false),
        IrType::OctetString { .. } => ("byte[]".into(), false),
        IrType::ObjectIdentifier | IrType::RelativeOid => ("long[]".into(), false),
        IrType::CharString { kind, .. } => (char_string_java_type(*kind), false),
        IrType::UtcTime | IrType::GeneralizedTime => ("java.time.Instant".into(), false),
        IrType::Enumerated { .. } | IrType::Sequence(_) | IrType::Set(_) | IrType::Choice(_) => {
            // These shouldn't reach here — inline composites are hoisted upstream.
            ("Object".into(), false)
        }
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            let (e, _) = java_type_for(element, resolver);
            (format!("List<{}>", box_primitive(&e)), false)
        }
        IrType::Reference { module, name } => {
            let tname = type_name(name);
            match module.as_deref().and_then(|m| resolver.package_of(m)) {
                Some(pkg) if pkg != resolver.current_package() => (format!("{pkg}.{tname}"), false),
                _ => (tname, false),
            }
        }
        IrType::Open { .. } => ("Object".into(), false),
        IrType::Any => ("Object".into(), false),
    }
}

fn box_primitive(t: &str) -> String {
    match t {
        "int" => "Integer".into(),
        "long" => "Long".into(),
        "double" => "Double".into(),
        "float" => "Float".into(),
        "boolean" => "Boolean".into(),
        "char" => "Character".into(),
        "byte" => "Byte".into(),
        "short" => "Short".into(),
        other => other.into(),
    }
}

fn wrap_optional(t: &str) -> String {
    format!("Optional<{}>", box_primitive(t))
}

fn integer_fits_int(cs: &[IrConstraint]) -> bool {
    for c in cs {
        if let IrConstraint::Range { lower, upper, .. } = c {
            let lo = lower.unwrap_or(i64::MIN);
            let hi = upper.unwrap_or(i64::MAX);
            if lo >= i32::MIN as i64 && hi <= i32::MAX as i64 {
                return true;
            }
        }
    }
    false
}

fn char_string_java_type(_k: IrCharKind) -> String {
    "String".into()
}

// ---------------------------------------------------------------------------
// Class / package resolver
// ---------------------------------------------------------------------------

struct ClassResolver<'a> {
    base_package: String,
    modules: Vec<(&'a str, String)>,
    current: std::cell::Cell<Option<usize>>,
}

impl<'a> ClassResolver<'a> {
    fn build(program: &'a IrProgram, opts: &JavaOptions) -> Self {
        let modules: Vec<(&str, String)> = program
            .modules
            .iter()
            .map(|m| {
                let pkg = format!("{}.{}", opts.base_package, package_slug(&m.name));
                (m.name.as_str(), pkg)
            })
            .collect();
        Self {
            base_package: opts.base_package.clone(),
            modules,
            current: std::cell::Cell::new(None),
        }
    }

    fn package_for(&self, module_name: &str) -> String {
        let idx = self.modules.iter().position(|(n, _)| *n == module_name).unwrap_or(usize::MAX);
        self.current.set(Some(idx));
        if idx == usize::MAX {
            format!("{}.unknown", self.base_package)
        } else {
            self.modules[idx].1.clone()
        }
    }

    fn current_package(&self) -> String {
        match self.current.get() {
            Some(i) if i != usize::MAX => self.modules[i].1.clone(),
            _ => self.base_package.clone(),
        }
    }

    fn package_of(&self, module_name: &str) -> Option<String> {
        self.modules.iter().find(|(n, _)| *n == module_name).map(|(_, p)| p.clone())
    }
}

// ---------------------------------------------------------------------------
// Name helpers
// ---------------------------------------------------------------------------

pub fn type_name(asn: &str) -> String {
    let mut s = pascal_case(asn);
    if is_java_reserved(&s) {
        s.push('_');
    }
    s
}

pub fn field_name(asn: &str) -> String {
    let mut s = camel_case(asn);
    if is_java_reserved(&s) {
        s.push('_');
    }
    s
}

pub fn enum_constant(asn: &str) -> String {
    let mut s = camel_case(asn);
    if is_java_reserved(&s) {
        s.push('_');
    }
    s
}

pub fn package_slug(asn: &str) -> String {
    let mut s = String::with_capacity(asn.len());
    let mut prev_sep = false;
    for ch in asn.chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                s.push(c);
            }
            prev_sep = false;
        } else if !prev_sep && !s.is_empty() {
            s.push('_');
            prev_sep = true;
        }
    }
    while s.ends_with('_') {
        s.pop();
    }
    if s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        s.insert(0, '_');
    }
    if s.is_empty() {
        "module".into()
    } else {
        s
    }
}

fn pascal_case(s: &str) -> String {
    let mut out = String::new();
    let mut upper_next = true;
    for c in s.chars() {
        if c == '-' || c == '_' || c == ' ' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn camel_case(s: &str) -> String {
    let p = pascal_case(s);
    let mut chars = p.chars();
    let first = match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>(),
        None => return String::new(),
    };
    format!("{first}{}", chars.as_str())
}

fn is_java_reserved(s: &str) -> bool {
    matches!(
        s,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "true"
            | "false"
            | "null"
            | "record"
            | "sealed"
            | "permits"
            | "yield"
            | "var"
    )
}

// ---------------------------------------------------------------------------
// Tiny writer
// ---------------------------------------------------------------------------

fn emit_javadoc(w: &mut Writer, doc: &str) {
    w.line("/**");
    for line in doc.lines() {
        let line = line.trim();
        if line.is_empty() {
            w.line(" *");
        } else {
            w.line(&format!(" * {line}"));
        }
    }
    w.line(" */");
}

struct Writer {
    out: String,
    indent_unit: String,
    level: usize,
}

impl Writer {
    fn new(indent_unit: String) -> Self {
        Self { out: String::new(), indent_unit, level: 0 }
    }

    fn indent(&mut self) {
        self.level += 1;
    }
    fn dedent(&mut self) {
        self.level = self.level.saturating_sub(1);
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.level {
            self.out.push_str(&self.indent_unit);
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn blank(&mut self) {
        self.out.push('\n');
    }
}

// Suppress the unused-writer helper when no write! calls exist.
#[allow(dead_code)]
fn _keep_write_import(w: &mut Writer, args: std::fmt::Arguments<'_>) {
    let _ = write!(w.out, "{}", args);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use asn1_ir::{lower, IrType as _IrType};
    use asn1_parser::{parse_source, SourceMap};

    fn gen(src: &str) -> Vec<JavaFile> {
        let mut sm = SourceMap::new();
        let f = sm.add("t.asn", src.to_string());
        let cst = parse_source(&sm, f).unwrap();
        let ir = lower(&[cst]);
        generate(&ir, &JavaOptions::default())
    }

    #[test]
    fn sequence_becomes_record() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Point ::= SEQUENCE { x INTEGER, y INTEGER OPTIONAL }
            END"#);
        let point = files.iter().find(|f| f.relative_path.ends_with("Point.java")).unwrap();
        assert!(point.contents.contains("public record Point("));
        assert!(point.contents.contains("Optional<Long> y"));
    }

    #[test]
    fn choice_becomes_sealed_interface() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Animal ::= CHOICE { dog INTEGER, cat INTEGER }
            END"#);
        let a = files.iter().find(|f| f.relative_path.ends_with("Animal.java")).unwrap();
        assert!(a.contents.contains("sealed interface Animal"));
        assert!(a.contents.contains("record Dog"));
        assert!(a.contents.contains("implements Animal"));
    }

    #[test]
    fn enumerated_becomes_enum() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Color ::= ENUMERATED { red, green, blue }
            END"#);
        let c = files.iter().find(|f| f.relative_path.ends_with("Color.java")).unwrap();
        assert!(c.contents.contains("public enum Color"));
        assert!(c.contents.contains("red"));
    }

    #[test]
    fn scalar_typedef_becomes_wrapper_record() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Id ::= INTEGER
            END"#);
        let id = files.iter().find(|f| f.relative_path.ends_with("Id.java")).unwrap();
        assert!(id.contents.contains("public record Id("));
        assert!(id.contents.contains("value"));
    }

    #[test]
    fn inline_sequence_hoisted_to_nested() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Outer ::= SEQUENCE { loc SEQUENCE { lat INTEGER, lon INTEGER } }
            END"#);
        let o = files.iter().find(|f| f.relative_path.ends_with("Outer.java")).unwrap();
        assert!(o.contents.contains("Outer.Loc loc"));
        assert!(o.contents.contains("public static record Loc"));
    }

    fn _unused_ir_type(_t: &_IrType) {}
}
