//! C++ source generator for ASN.1 IR.
//!
//! Emits one `.hpp` header per [`asn1_ir::IrTypeDef`]. SEQUENCE / SET become
//! `struct`s with plain members, CHOICE becomes a wrapper around
//! `std::variant`, ENUMERATED becomes an `enum class`, and scalar / collection
//! typedefs are wrapped in single-field structs so every named ASN.1 type has
//! a distinct C++ identity — matching the Java backend.
//!
//! Inline anonymous composites (a SEQUENCE inside a field, for instance) are
//! hoisted into nested types declared inside their enclosing struct.
//!
//! Layout: `<module_slug>/TypeName.hpp`. Types live under
//! `<base_namespace>::<module_slug>`. Cross-module references emit the
//! appropriate `#include` and qualified name.

#![deny(rust_2018_idioms)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use asn1_ir::{
    IrChoice, IrConstraint, IrItem, IrOptionality, IrProgram, IrStruct, IrStructMember, IrType,
    IrTypeDef,
};

/// Generator options.
#[derive(Debug, Clone)]
pub struct CppOptions {
    /// Root C++ namespace, e.g. `generated::asn1`. The per-module namespace is
    /// appended as `<base>::<module_slug>`.
    pub base_namespace: String,
    pub indent: String,
}

impl Default for CppOptions {
    fn default() -> Self {
        Self { base_namespace: "generated::asn1".into(), indent: "    ".into() }
    }
}

/// A single emitted C++ header file.
#[derive(Debug, Clone)]
pub struct CppFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

/// Top-level entry point: emit a C++ header for every named type in the program.
pub fn generate(program: &IrProgram, opts: &CppOptions) -> Vec<CppFile> {
    let resolver = NamespaceResolver::build(program, opts);
    let mut files = Vec::new();
    for module in &program.modules {
        for item in &module.items {
            if let IrItem::Type(t) = item {
                let file = emit_type_file(module.name.as_str(), t, &resolver, opts);
                files.push(file);
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// File emission
// ---------------------------------------------------------------------------

fn emit_type_file(
    module_name: &str,
    td: &IrTypeDef,
    resolver: &NamespaceResolver<'_>,
    opts: &CppOptions,
) -> CppFile {
    let slug = resolver.slug_of(module_name).unwrap_or_else(|| "module".into());
    let type_id = type_name(&td.name);
    let ns = format!("{}::{}", opts.base_namespace, slug);

    let mut deps = Deps::new(module_name.to_string());
    // Walk the type once to collect includes before we emit anything.
    collect_deps(&td.ty, &mut deps, resolver);

    let mut w = Writer::new(opts.indent.clone());
    w.line("#pragma once");
    w.blank();

    // Standard headers needed by the emitted code.
    let std_headers = deps.std_headers();
    for h in &std_headers {
        w.line(&format!("#include <{h}>"));
    }
    if !std_headers.is_empty() {
        w.blank();
    }

    // Cross-module / intra-module headers (sorted, deduplicated).
    let rel_includes = deps.relative_includes(resolver, &type_id);
    for inc in &rel_includes {
        w.line(&format!("#include \"{inc}\""));
    }
    if !rel_includes.is_empty() {
        w.blank();
    }

    // Open namespace and emit.
    emit_namespace_open(&mut w, &ns);
    if let Some(doc) = &td.doc {
        emit_doxygen(&mut w, doc);
    }
    emit_type(&mut w, &type_id, &td.ty, resolver, module_name);
    emit_namespace_close(&mut w, &ns);

    let mut path: PathBuf = PathBuf::from(&slug);
    path.push(format!("{type_id}.hpp"));
    CppFile { relative_path: path, contents: w.out }
}

fn emit_namespace_open(w: &mut Writer, ns: &str) {
    for part in ns.split("::") {
        w.line(&format!("namespace {part} {{"));
    }
}

fn emit_namespace_close(w: &mut Writer, ns: &str) {
    let parts: Vec<&str> = ns.split("::").collect();
    for part in parts.iter().rev() {
        w.line(&format!("}} // namespace {part}"));
    }
}

fn emit_type(
    w: &mut Writer,
    name: &str,
    ty: &IrType,
    resolver: &NamespaceResolver<'_>,
    current_module: &str,
) {
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => {
            emit_struct(w, name, s, resolver, current_module);
        }
        IrType::Choice(c) => emit_choice(w, name, c, resolver, current_module),
        IrType::Enumerated { items, .. } => {
            w.line(&format!("enum class {name} {{"));
            w.indent();
            for (idx, item) in items.iter().enumerate() {
                if let Some(doc) = &item.doc {
                    emit_doxygen(w, doc);
                }
                let sep = if idx + 1 == items.len() { "" } else { "," };
                let ident = enum_constant(&item.name);
                match item.value {
                    Some(v) => w.line(&format!("{ident} = {v}{sep}")),
                    None => w.line(&format!("{ident}{sep}")),
                }
            }
            w.dedent();
            w.line("};");
        }
        _ => {
            // Wrap scalar / reference / collection types in a single-field
            // struct so every named ASN.1 type gets a distinct C++ identity.
            let ct = cpp_type_for(ty, resolver, current_module);
            w.line(&format!("struct {name} {{"));
            w.indent();
            w.line(&format!("{ct} value;"));
            w.dedent();
            w.line("};");
        }
    }
}

fn emit_struct(
    w: &mut Writer,
    name: &str,
    s: &IrStruct,
    resolver: &NamespaceResolver<'_>,
    current_module: &str,
) {
    // Split members into real fields and COMPONENTS-OF notes.
    let mut fields: Vec<(String, String, Option<String>)> = Vec::new();
    let mut nested: Vec<(String, IrType, Option<String>)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    for member in &s.members {
        match member {
            IrStructMember::Field(f) => {
                let fname = field_name(&f.name);
                let (cty, _) = cpp_type_for_field(
                    &f.ty,
                    resolver,
                    current_module,
                    &fname,
                    &mut nested,
                    f.doc.clone(),
                );
                let wrapped = match f.optionality {
                    IrOptionality::Required => cty,
                    IrOptionality::Optional | IrOptionality::Default(_) => {
                        format!("std::optional<{cty}>")
                    }
                };
                fields.push((wrapped, fname, f.doc.clone()));
            }
            IrStructMember::ComponentsOf { type_ref } => {
                notes.push(format!(
                    "COMPONENTS OF {type_ref} \u{2014} inlined members are not expanded in codegen."
                ));
            }
        }
    }

    w.line(&format!("struct {name} {{"));
    w.indent();
    // Nested type declarations first, so the enclosing struct can reference them.
    for (nname, nty, ndoc) in &nested {
        if let Some(doc) = ndoc {
            emit_doxygen(w, doc);
        }
        emit_type(w, nname, nty, resolver, current_module);
    }
    if !nested.is_empty() && !fields.is_empty() {
        w.blank();
    }

    for note in &notes {
        w.line(&format!("// {note}"));
    }

    for (cty, name, doc) in &fields {
        if let Some(d) = doc {
            emit_doxygen(w, d);
        }
        w.line(&format!("{cty} {name};"));
    }
    w.dedent();
    w.line("};");
}

fn emit_choice(
    w: &mut Writer,
    name: &str,
    c: &IrChoice,
    resolver: &NamespaceResolver<'_>,
    current_module: &str,
) {
    let mut nested: Vec<(String, IrType, Option<String>)> = Vec::new();
    let mut alt_types: Vec<(String, String, Option<String>)> = Vec::new();
    for alt in &c.alternatives {
        let aname = pascal_case(&alt.name);
        let (cty, _) = cpp_type_for_field(
            &alt.ty,
            resolver,
            current_module,
            &aname,
            &mut nested,
            alt.doc.clone(),
        );
        alt_types.push((aname, cty, alt.doc.clone()));
    }

    w.line(&format!("struct {name} {{"));
    w.indent();
    for (nname, nty, ndoc) in &nested {
        if let Some(doc) = ndoc {
            emit_doxygen(w, doc);
        }
        emit_type(w, nname, nty, resolver, current_module);
    }
    if !nested.is_empty() {
        w.blank();
    }

    if alt_types.is_empty() {
        w.line("std::monostate value;");
    } else {
        for (aname, _, doc) in &alt_types {
            if let Some(d) = doc {
                emit_doxygen(w, d);
            }
            w.line(&format!("// alternative: {aname}"));
        }
        let joined = alt_types.iter().map(|(_, t, _)| t.as_str()).collect::<Vec<_>>().join(", ");
        w.line(&format!("std::variant<{joined}> value;"));
    }
    w.dedent();
    w.line("};");
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

fn cpp_type_for_field(
    ty: &IrType,
    resolver: &NamespaceResolver<'_>,
    current_module: &str,
    hint: &str,
    nested: &mut Vec<(String, IrType, Option<String>)>,
    hoist_doc: Option<String>,
) -> (String, bool) {
    match ty {
        IrType::Sequence(_) | IrType::Set(_) | IrType::Choice(_) | IrType::Enumerated { .. } => {
            let nested_name = pascal_case(hint);
            nested.push((nested_name.clone(), ty.clone(), hoist_doc));
            (nested_name, true)
        }
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            let (elem, _) = cpp_type_for_field(
                element,
                resolver,
                current_module,
                &format!("{}Item", pascal_case(hint)),
                nested,
                None,
            );
            (format!("std::vector<{elem}>"), false)
        }
        _ => (cpp_type_for(ty, resolver, current_module), false),
    }
}

fn cpp_type_for(ty: &IrType, resolver: &NamespaceResolver<'_>, current_module: &str) -> String {
    match ty {
        IrType::Boolean => "bool".into(),
        IrType::Integer { constraints, .. } => integer_cpp_type(constraints).into(),
        IrType::Real => "double".into(),
        IrType::Null => "std::monostate".into(),
        IrType::BitString { .. } => "std::vector<bool>".into(),
        IrType::OctetString { .. } => "std::vector<std::uint8_t>".into(),
        IrType::ObjectIdentifier | IrType::RelativeOid => "std::vector<std::uint64_t>".into(),
        IrType::CharString { .. } => "std::string".into(),
        IrType::UtcTime | IrType::GeneralizedTime => "std::string".into(),
        IrType::Enumerated { .. } | IrType::Sequence(_) | IrType::Set(_) | IrType::Choice(_) => {
            // Shouldn't reach here — inline composites are hoisted upstream.
            "std::monostate".into()
        }
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            format!("std::vector<{}>", cpp_type_for(element, resolver, current_module))
        }
        IrType::Reference { module, name } => {
            let tname = type_name(name);
            match module.as_deref() {
                Some(m) if m != current_module => match resolver.slug_of(m) {
                    Some(slug) => format!("{}::{}::{}", resolver.base, slug, tname),
                    None => tname,
                },
                _ => tname,
            }
        }
        IrType::Open { .. } => "std::any".into(),
        IrType::Any => "std::any".into(),
    }
}

fn integer_cpp_type(cs: &[IrConstraint]) -> &'static str {
    for c in cs {
        if let IrConstraint::Range { lower, upper, .. } = c {
            let lo = lower.unwrap_or(i64::MIN);
            let hi = upper.unwrap_or(i64::MAX);
            if lo >= 0 {
                if hi <= u8::MAX as i64 {
                    return "std::uint8_t";
                }
                if hi <= u16::MAX as i64 {
                    return "std::uint16_t";
                }
                if hi <= u32::MAX as i64 {
                    return "std::uint32_t";
                }
                return "std::uint64_t";
            }
            if lo >= i32::MIN as i64 && hi <= i32::MAX as i64 {
                return "std::int32_t";
            }
        }
    }
    "std::int64_t"
}

// ---------------------------------------------------------------------------
// Dependency collection
// ---------------------------------------------------------------------------

/// Tracks everything a type file needs to `#include`.
struct Deps {
    current_module: String,
    needs_vector: bool,
    needs_optional: bool,
    needs_variant: bool,
    needs_string: bool,
    needs_cstdint: bool,
    needs_any: bool,
    /// `(module, type)` pairs referenced by the emitted type.
    refs: BTreeSet<(String, String)>,
}

impl Deps {
    fn new(current_module: String) -> Self {
        Self {
            current_module,
            needs_vector: false,
            needs_optional: false,
            needs_variant: false,
            needs_string: false,
            needs_cstdint: false,
            needs_any: false,
            refs: BTreeSet::new(),
        }
    }

    fn std_headers(&self) -> Vec<&'static str> {
        let mut out: Vec<&str> = Vec::new();
        if self.needs_optional {
            out.push("optional");
        }
        if self.needs_variant {
            out.push("variant");
        }
        if self.needs_vector {
            out.push("vector");
        }
        if self.needs_string {
            out.push("string");
        }
        if self.needs_cstdint {
            out.push("cstdint");
        }
        if self.needs_any {
            out.push("any");
        }
        out
    }

    fn relative_includes(&self, resolver: &NamespaceResolver<'_>, self_type: &str) -> Vec<String> {
        let mut out = BTreeSet::new();
        for (module, tname) in &self.refs {
            let Some(slug) = resolver.slug_of(module) else { continue };
            if module == &self.current_module && tname == self_type {
                continue; // no self-include
            }
            out.insert(format!("{slug}/{tname}.hpp"));
        }
        out.into_iter().collect()
    }
}

fn collect_deps(ty: &IrType, d: &mut Deps, resolver: &NamespaceResolver<'_>) {
    match ty {
        IrType::Boolean | IrType::Real => {}
        IrType::Integer { .. } => d.needs_cstdint = true,
        IrType::Null => d.needs_variant = true, // std::monostate lives in <variant>
        IrType::BitString { .. } => d.needs_vector = true,
        IrType::OctetString { .. } | IrType::ObjectIdentifier | IrType::RelativeOid => {
            d.needs_vector = true;
            d.needs_cstdint = true;
        }
        IrType::CharString { .. } | IrType::UtcTime | IrType::GeneralizedTime => {
            d.needs_string = true;
        }
        IrType::Enumerated { .. } => {}
        IrType::Sequence(s) | IrType::Set(s) => {
            for m in &s.members {
                if let IrStructMember::Field(f) = m {
                    collect_deps(&f.ty, d, resolver);
                    if matches!(f.optionality, IrOptionality::Optional | IrOptionality::Default(_))
                    {
                        d.needs_optional = true;
                    }
                }
            }
        }
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            d.needs_vector = true;
            collect_deps(element, d, resolver);
        }
        IrType::Choice(c) => {
            d.needs_variant = true;
            if c.alternatives.is_empty() {
                // std::monostate placeholder
            }
            for alt in &c.alternatives {
                collect_deps(&alt.ty, d, resolver);
            }
        }
        IrType::Reference { module, name } => {
            let m = match module.as_deref() {
                Some(m) => m.to_string(),
                None => d.current_module.clone(),
            };
            if resolver.slug_of(&m).is_some() {
                d.refs.insert((m, type_name(name)));
            }
        }
        IrType::Open { .. } | IrType::Any => d.needs_any = true,
    }
}

// ---------------------------------------------------------------------------
// Namespace resolver
// ---------------------------------------------------------------------------

struct NamespaceResolver<'a> {
    base: String,
    modules: Vec<(&'a str, String)>,
}

impl<'a> NamespaceResolver<'a> {
    fn build(program: &'a IrProgram, opts: &CppOptions) -> Self {
        let modules =
            program.modules.iter().map(|m| (m.name.as_str(), namespace_slug(&m.name))).collect();
        Self { base: opts.base_namespace.clone(), modules }
    }

    fn slug_of(&self, module_name: &str) -> Option<String> {
        self.modules.iter().find(|(n, _)| *n == module_name).map(|(_, s)| s.clone())
    }
}

// ---------------------------------------------------------------------------
// Name helpers
// ---------------------------------------------------------------------------

/// Convert an ASN.1 type name to a C++ identifier (PascalCase, reserved-safe).
pub fn type_name(asn: &str) -> String {
    let mut s = pascal_case(asn);
    if is_cpp_reserved(&s) {
        s.push('_');
    }
    s
}

/// Convert an ASN.1 field name to a C++ identifier (snake_case, reserved-safe).
pub fn field_name(asn: &str) -> String {
    let mut s = snake_case(asn);
    if is_cpp_reserved(&s) {
        s.push('_');
    }
    s
}

/// Convert an ASN.1 enumerator to a C++ `enum class` member (PascalCase).
pub fn enum_constant(asn: &str) -> String {
    let mut s = pascal_case(asn);
    if is_cpp_reserved(&s) {
        s.push('_');
    }
    s
}

/// Module-name → namespace-component slug: lowercase, hyphens/spaces → `_`.
pub fn namespace_slug(asn: &str) -> String {
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

fn snake_case(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower_or_digit = false;
    for c in s.chars() {
        if c == '-' || c == ' ' || c == '_' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower_or_digit = false;
        } else if c.is_ascii_uppercase() {
            if prev_lower_or_digit && !out.ends_with('_') {
                out.push('_');
            }
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_lower_or_digit = false;
        } else {
            out.push(c);
            prev_lower_or_digit = c.is_ascii_alphanumeric();
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        out.insert(0, '_');
    }
    out
}

fn is_cpp_reserved(s: &str) -> bool {
    matches!(
        s,
        "alignas"
            | "alignof"
            | "and"
            | "and_eq"
            | "asm"
            | "auto"
            | "bitand"
            | "bitor"
            | "bool"
            | "break"
            | "case"
            | "catch"
            | "char"
            | "char8_t"
            | "char16_t"
            | "char32_t"
            | "class"
            | "compl"
            | "concept"
            | "const"
            | "consteval"
            | "constexpr"
            | "constinit"
            | "const_cast"
            | "continue"
            | "co_await"
            | "co_return"
            | "co_yield"
            | "decltype"
            | "default"
            | "delete"
            | "do"
            | "double"
            | "dynamic_cast"
            | "else"
            | "enum"
            | "explicit"
            | "export"
            | "extern"
            | "false"
            | "float"
            | "for"
            | "friend"
            | "goto"
            | "if"
            | "inline"
            | "int"
            | "long"
            | "mutable"
            | "namespace"
            | "new"
            | "noexcept"
            | "not"
            | "not_eq"
            | "nullptr"
            | "operator"
            | "or"
            | "or_eq"
            | "private"
            | "protected"
            | "public"
            | "reflexpr"
            | "register"
            | "reinterpret_cast"
            | "requires"
            | "return"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "static_assert"
            | "static_cast"
            | "struct"
            | "switch"
            | "template"
            | "this"
            | "thread_local"
            | "throw"
            | "true"
            | "try"
            | "typedef"
            | "typeid"
            | "typename"
            | "union"
            | "unsigned"
            | "using"
            | "virtual"
            | "void"
            | "volatile"
            | "wchar_t"
            | "while"
            | "xor"
            | "xor_eq"
    )
}

// ---------------------------------------------------------------------------
// Tiny writer
// ---------------------------------------------------------------------------

fn emit_doxygen(w: &mut Writer, doc: &str) {
    w.line("/**");
    for line in doc.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            w.line(" *");
        } else {
            w.line(&format!(" * {trimmed}"));
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

// Suppress dead-code warning on the write! helper path.
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
    use asn1_ir::lower;
    use asn1_parser::{parse_source, SourceMap};

    fn gen(src: &str) -> Vec<CppFile> {
        let mut sm = SourceMap::new();
        let f = sm.add("t.asn", src.to_string());
        let cst = parse_source(&sm, f).unwrap();
        let ir = lower(&[cst]);
        generate(&ir, &CppOptions::default())
    }

    fn find<'a>(files: &'a [CppFile], name: &str) -> &'a CppFile {
        files
            .iter()
            .find(|f| f.relative_path.file_name().and_then(|s| s.to_str()) == Some(name))
            .unwrap_or_else(|| panic!("no file named {name}"))
    }

    #[test]
    fn sequence_becomes_struct_with_optional() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Point ::= SEQUENCE { x INTEGER, y INTEGER OPTIONAL }
            END"#);
        let p = find(&files, "Point.hpp");
        assert!(p.contents.contains("#pragma once"));
        assert!(p.contents.contains("struct Point {"));
        assert!(p.contents.contains("std::optional<std::int64_t> y;"));
        assert!(p.contents.contains("namespace foo {"));
    }

    #[test]
    fn choice_uses_variant() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Animal ::= CHOICE { dog INTEGER, cat INTEGER }
            END"#);
        let a = find(&files, "Animal.hpp");
        assert!(a.contents.contains("struct Animal {"));
        assert!(a.contents.contains("std::variant<"));
        assert!(a.contents.contains("#include <variant>"));
    }

    #[test]
    fn enumerated_becomes_enum_class() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Color ::= ENUMERATED { red, green, blue }
            END"#);
        let c = find(&files, "Color.hpp");
        assert!(c.contents.contains("enum class Color {"));
        assert!(c.contents.contains("Red"));
    }

    #[test]
    fn scalar_typedef_becomes_wrapper_struct() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Id ::= INTEGER
            END"#);
        let id = find(&files, "Id.hpp");
        assert!(id.contents.contains("struct Id {"));
        assert!(id.contents.contains("value;"));
    }

    #[test]
    fn inline_sequence_hoisted_to_nested() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Outer ::= SEQUENCE { loc SEQUENCE { lat INTEGER, lon INTEGER } }
            END"#);
        let o = find(&files, "Outer.hpp");
        assert!(o.contents.contains("struct Loc {"));
        assert!(o.contents.contains("Loc loc;"));
    }

    #[test]
    fn sequence_of_uses_vector() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Ids ::= SEQUENCE OF INTEGER
            END"#);
        let ids = find(&files, "Ids.hpp");
        assert!(ids.contents.contains("std::vector<std::int64_t>"));
        assert!(ids.contents.contains("#include <vector>"));
    }

    #[test]
    fn integer_range_picks_narrow_type() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Byte ::= INTEGER (0..255)
                Small ::= INTEGER (-100..100)
            END"#);
        let b = find(&files, "Byte.hpp");
        assert!(b.contents.contains("std::uint8_t"));
        let s = find(&files, "Small.hpp");
        assert!(s.contents.contains("std::int32_t"));
    }

    #[test]
    fn reference_to_other_module_includes_header() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                A ::= SEQUENCE { b B }
                B ::= INTEGER
            END"#);
        let a = find(&files, "A.hpp");
        // Same module — no namespace qualifier, include of the sibling header.
        assert!(a.contents.contains("B b;"));
        assert!(a.contents.contains("#include \"foo/B.hpp\""));
    }

    #[test]
    fn reserved_field_name_gets_suffix() {
        let files = gen(r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                S ::= SEQUENCE { class INTEGER }
            END"#);
        let s = find(&files, "S.hpp");
        assert!(s.contents.contains("class_;"));
    }
}
