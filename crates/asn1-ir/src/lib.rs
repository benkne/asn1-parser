//! Lowered, resolved intermediate representation of ASN.1 modules.
//!
//! The parser crate produces a [`asn1_parser::Module`] per input file; this crate
//! flattens a slice of those modules into an [`IrProgram`] that the codegen and
//! visualization crates consume. Lowering keeps source spans and doc comments but
//! normalizes the type tree so downstream consumers don't have to re-derive
//! structural information from the CST.

#![deny(rust_2018_idioms)]

use std::collections::HashMap;

use asn1_parser as cst;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Root of the lowered representation — one entry per input module.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrProgram {
    pub modules: Vec<IrModule>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrModule {
    pub name: String,
    pub oid: Option<Vec<IrOidPart>>,
    pub imports: Vec<IrImport>,
    pub items: Vec<IrItem>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrOidPart {
    pub name: Option<String>,
    pub value: Option<i64>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrImport {
    pub symbols: Vec<String>,
    pub from_module: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrItem {
    Type(IrTypeDef),
    Value(IrValueDef),
    ObjectClass(IrObjectClassDef),
    ObjectSet(IrObjectSetDef),
    Object(IrObjectDef),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrTypeDef {
    pub name: String,
    pub doc: Option<String>,
    pub ty: IrType,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrValueDef {
    pub name: String,
    pub doc: Option<String>,
    pub ty: IrType,
    pub value: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrObjectClassDef {
    pub name: String,
    pub doc: Option<String>,
    pub fields: Vec<IrClassField>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrClassField {
    Type { name: String, optional: bool },
    Value { name: String, ty: IrType, optional: bool, unique: bool },
    VariableType { name: String, field_path: Vec<String>, optional: bool },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrObjectSetDef {
    pub name: String,
    pub class_name: String,
    pub extensible: bool,
    pub members: usize,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrObjectDef {
    pub name: String,
    pub class_name: String,
    pub fields: Vec<IrObjectFieldBinding>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrObjectFieldBinding {
    pub name: String,
    pub binding: IrObjectFieldValue,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrObjectFieldValue {
    Type(IrType),
    Value(String),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrType {
    Boolean,
    Integer {
        named_numbers: Vec<(String, i64)>,
        constraints: Vec<IrConstraint>,
    },
    Real,
    Null,
    BitString {
        named_bits: Vec<(String, i64)>,
        constraints: Vec<IrConstraint>,
    },
    OctetString {
        constraints: Vec<IrConstraint>,
    },
    ObjectIdentifier,
    RelativeOid,
    CharString {
        kind: IrCharKind,
        constraints: Vec<IrConstraint>,
    },
    UtcTime,
    GeneralizedTime,
    Enumerated {
        items: Vec<IrEnumItem>,
        extensible: bool,
    },
    Sequence(IrStruct),
    Set(IrStruct),
    SequenceOf {
        element: Box<IrType>,
        constraints: Vec<IrConstraint>,
    },
    SetOf {
        element: Box<IrType>,
        constraints: Vec<IrConstraint>,
    },
    Choice(IrChoice),
    /// Resolved or unresolved named reference.
    Reference {
        module: Option<String>,
        name: String,
    },
    /// `CLASS.&field` — open type, left unresolved.
    Open {
        description: String,
    },
    /// Any fallback that we parsed structurally but don't model semantically.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrCharKind {
    Utf8,
    Ia5,
    Printable,
    Numeric,
    Visible,
    Bmp,
    Universal,
    General,
    Graphic,
    Teletex,
    T61,
    Videotex,
    Iso646,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrEnumItem {
    pub doc: Option<String>,
    pub name: String,
    pub value: Option<i64>,
    pub is_extension: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrStruct {
    pub members: Vec<IrStructMember>,
    pub extensible: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrStructMember {
    Field(IrField),
    ComponentsOf {
        /// Name of the referenced SEQUENCE/SET type whose components are inlined.
        type_ref: String,
    },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrField {
    pub doc: Option<String>,
    pub name: String,
    pub ty: IrType,
    pub optionality: IrOptionality,
    pub is_extension: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrOptionality {
    Required,
    Optional,
    Default(String),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IrChoice {
    pub alternatives: Vec<IrField>,
    pub extensible: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IrConstraint {
    /// Inclusive `[lower, upper]`, either bound may be open (`None` means MIN/MAX).
    Range { lower: Option<i64>, upper: Option<i64>, extensible: bool },
    /// A single permitted value, rendered as a string.
    Single(String),
    /// A SIZE constraint — the inner constraint describes the size range.
    Size(Box<IrConstraint>),
    /// Nested union/intersection or anything not reducible to the above.
    Composite(String),
}

// ---------------------------------------------------------------------------
// Lowering
// ---------------------------------------------------------------------------

/// Lower a slice of parsed CST modules into an [`IrProgram`].
///
/// Cross-module references are recorded; unresolvable references are preserved as
/// [`IrType::Reference`] with `module` unset so consumers can decide how to render
/// them.
pub fn lower(modules: &[cst::Module]) -> IrProgram {
    let mut resolver = Resolver::new(modules);
    let ir_modules = modules.iter().map(|m| lower_module(m, &mut resolver)).collect();
    IrProgram { modules: ir_modules }
}

fn lower_module<'a>(m: &'a cst::Module, resolver: &mut Resolver<'a>) -> IrModule {
    resolver.set_current(&m.name.value, &m.imports);
    let items = m.assignments.iter().map(|a| lower_item(a, resolver)).collect();
    IrModule {
        name: m.name.value.clone(),
        oid: m.oid.as_ref().map(|oid| {
            oid.iter()
                .map(|c| IrOidPart {
                    name: c.name.as_ref().map(|n| n.value.clone()),
                    value: c.value,
                })
                .collect()
        }),
        imports: m
            .imports
            .iter()
            .map(|i| IrImport {
                symbols: i.symbols.iter().map(|s| s.value.clone()).collect(),
                from_module: i.from_module.value.clone(),
            })
            .collect(),
        items,
    }
}

fn lower_item(a: &cst::Assignment, resolver: &Resolver<'_>) -> IrItem {
    match &a.kind {
        cst::AssignmentKind::Type(t) => IrItem::Type(IrTypeDef {
            name: a.name.value.clone(),
            doc: a.doc.clone(),
            ty: lower_type(t, resolver),
        }),
        cst::AssignmentKind::Value { ty, value } => IrItem::Value(IrValueDef {
            name: a.name.value.clone(),
            doc: a.doc.clone(),
            ty: lower_type(ty, resolver),
            value: render_value(value),
        }),
        cst::AssignmentKind::ObjectClass(class) => IrItem::ObjectClass(IrObjectClassDef {
            name: a.name.value.clone(),
            doc: a.doc.clone(),
            fields: class.fields.iter().map(|f| lower_class_field(f, resolver)).collect(),
        }),
        cst::AssignmentKind::ObjectSet { class_name, set } => IrItem::ObjectSet(IrObjectSetDef {
            name: a.name.value.clone(),
            class_name: class_name.value.clone(),
            extensible: set.extensible,
            members: set.elements.len(),
        }),
        cst::AssignmentKind::Object { class_name, object } => IrItem::Object(IrObjectDef {
            name: a.name.value.clone(),
            class_name: class_name.value.clone(),
            fields: object.fields.iter().map(|f| lower_object_field(f, resolver)).collect(),
        }),
    }
}

fn lower_class_field(f: &cst::FieldSpec, resolver: &Resolver<'_>) -> IrClassField {
    match f {
        cst::FieldSpec::TypeField { name, optional, .. } => {
            IrClassField::Type { name: name.value.clone(), optional: *optional }
        }
        cst::FieldSpec::ValueField { name, ty, unique, optional, .. } => IrClassField::Value {
            name: name.value.clone(),
            ty: lower_type(ty, resolver),
            optional: *optional,
            unique: *unique,
        },
        cst::FieldSpec::VariableTypeValueField { name, field_path, optional, .. } => {
            IrClassField::VariableType {
                name: name.value.clone(),
                field_path: field_path
                    .iter()
                    .map(|r| match r {
                        cst::FieldRef::Type(n) | cst::FieldRef::Value(n) => n.value.clone(),
                    })
                    .collect(),
                optional: *optional,
            }
        }
    }
}

fn lower_object_field(
    f: &cst::ObjectFieldSetting,
    resolver: &Resolver<'_>,
) -> IrObjectFieldBinding {
    match f {
        cst::ObjectFieldSetting::Type { name, ty } => IrObjectFieldBinding {
            name: name.value.clone(),
            binding: IrObjectFieldValue::Type(lower_type(ty, resolver)),
        },
        cst::ObjectFieldSetting::Value { name, value } => IrObjectFieldBinding {
            name: name.value.clone(),
            binding: IrObjectFieldValue::Value(render_value(value)),
        },
    }
}

fn lower_type(t: &cst::Type, resolver: &Resolver<'_>) -> IrType {
    let leading = lower_constraints(&t.constraints);
    match &t.kind {
        cst::TypeKind::Boolean => IrType::Boolean,
        cst::TypeKind::Integer { named_numbers } => IrType::Integer {
            named_numbers: named_numbers
                .iter()
                .filter_map(|nn| match &nn.value {
                    cst::NamedNumberValue::Literal(v) => Some((nn.name.value.clone(), *v)),
                    cst::NamedNumberValue::Reference(_) => None,
                })
                .collect(),
            constraints: leading,
        },
        cst::TypeKind::Real => IrType::Real,
        cst::TypeKind::Null => IrType::Null,
        cst::TypeKind::BitString { named_bits } => IrType::BitString {
            named_bits: named_bits
                .iter()
                .filter_map(|nn| match &nn.value {
                    cst::NamedNumberValue::Literal(v) => Some((nn.name.value.clone(), *v)),
                    cst::NamedNumberValue::Reference(_) => None,
                })
                .collect(),
            constraints: leading,
        },
        cst::TypeKind::OctetString => IrType::OctetString { constraints: leading },
        cst::TypeKind::ObjectIdentifier => IrType::ObjectIdentifier,
        cst::TypeKind::RelativeOid => IrType::RelativeOid,
        cst::TypeKind::CharString(k) => {
            IrType::CharString { kind: lower_char_kind(*k), constraints: leading }
        }
        cst::TypeKind::UtcTime => IrType::UtcTime,
        cst::TypeKind::GeneralizedTime => IrType::GeneralizedTime,
        cst::TypeKind::Enumerated { items, extensible, extension_items } => IrType::Enumerated {
            items: items
                .iter()
                .map(|i| IrEnumItem {
                    doc: i.doc.clone(),
                    name: i.name.value.clone(),
                    value: i.value,
                    is_extension: false,
                })
                .chain(extension_items.iter().map(|i| IrEnumItem {
                    doc: i.doc.clone(),
                    name: i.name.value.clone(),
                    value: i.value,
                    is_extension: true,
                }))
                .collect(),
            extensible: *extensible,
        },
        cst::TypeKind::Sequence(s) => IrType::Sequence(lower_struct(s, resolver)),
        cst::TypeKind::Set(s) => IrType::Set(lower_struct(s, resolver)),
        cst::TypeKind::SequenceOf(inner) => IrType::SequenceOf {
            element: Box::new(lower_type(inner, resolver)),
            constraints: leading,
        },
        cst::TypeKind::SetOf(inner) => {
            IrType::SetOf { element: Box::new(lower_type(inner, resolver)), constraints: leading }
        }
        cst::TypeKind::Choice(c) => IrType::Choice(lower_choice(c, resolver)),
        cst::TypeKind::Reference(name) => {
            let (module, local) = resolver.resolve(&name.value);
            IrType::Reference { module, name: local }
        }
        cst::TypeKind::ClassField { class, path } => IrType::Open {
            description: format!(
                "{}.{}",
                class.value,
                path.iter()
                    .map(|r| match r {
                        cst::FieldRef::Type(n) => format!("&{}", n.value),
                        cst::FieldRef::Value(n) => format!("&{}", n.value),
                    })
                    .collect::<Vec<_>>()
                    .join(".")
            ),
        },
        cst::TypeKind::Any => IrType::Any,
    }
}

fn lower_char_kind(k: cst::CharStringKind) -> IrCharKind {
    match k {
        cst::CharStringKind::Utf8 => IrCharKind::Utf8,
        cst::CharStringKind::Ia5 => IrCharKind::Ia5,
        cst::CharStringKind::Printable => IrCharKind::Printable,
        cst::CharStringKind::Numeric => IrCharKind::Numeric,
        cst::CharStringKind::Visible => IrCharKind::Visible,
        cst::CharStringKind::Bmp => IrCharKind::Bmp,
        cst::CharStringKind::Universal => IrCharKind::Universal,
        cst::CharStringKind::General => IrCharKind::General,
        cst::CharStringKind::Graphic => IrCharKind::Graphic,
        cst::CharStringKind::Teletex => IrCharKind::Teletex,
        cst::CharStringKind::T61 => IrCharKind::T61,
        cst::CharStringKind::Videotex => IrCharKind::Videotex,
        cst::CharStringKind::Iso646 => IrCharKind::Iso646,
    }
}

fn lower_struct(s: &cst::StructType, resolver: &Resolver<'_>) -> IrStruct {
    let mut members = Vec::new();
    for m in &s.components {
        members.push(lower_struct_member(m, resolver, false));
    }
    for m in &s.extension_additions {
        members.push(lower_struct_member(m, resolver, true));
    }
    IrStruct { members, extensible: s.extensible }
}

fn lower_struct_member(
    m: &cst::StructMember,
    resolver: &Resolver<'_>,
    is_extension: bool,
) -> IrStructMember {
    match m {
        cst::StructMember::Named(c) => {
            IrStructMember::Field(lower_field(c, resolver, is_extension))
        }
        cst::StructMember::ComponentsOf { ty, .. } => {
            let name = match &ty.kind {
                cst::TypeKind::Reference(n) => n.value.clone(),
                _ => "<inline>".to_string(),
            };
            IrStructMember::ComponentsOf { type_ref: name }
        }
    }
}

fn lower_field(c: &cst::Component, resolver: &Resolver<'_>, is_extension: bool) -> IrField {
    IrField {
        doc: c.doc.clone(),
        name: c.name.value.clone(),
        ty: lower_type(&c.ty, resolver),
        optionality: match &c.optionality {
            cst::Optionality::Required => IrOptionality::Required,
            cst::Optionality::Optional => IrOptionality::Optional,
            cst::Optionality::Default(v) => IrOptionality::Default(render_value(v)),
        },
        is_extension,
    }
}

fn lower_choice(c: &cst::ChoiceType, resolver: &Resolver<'_>) -> IrChoice {
    let mut alternatives = Vec::new();
    for a in &c.alternatives {
        alternatives.push(lower_field(a, resolver, false));
    }
    for a in &c.extension_alternatives {
        alternatives.push(lower_field(a, resolver, true));
    }
    IrChoice { alternatives, extensible: c.extensible }
}

fn lower_constraints(cs: &[cst::Constraint]) -> Vec<IrConstraint> {
    cs.iter().map(lower_constraint).collect()
}

fn lower_constraint(c: &cst::Constraint) -> IrConstraint {
    match c {
        cst::Constraint::Size(inner) => IrConstraint::Size(Box::new(lower_constraint(inner))),
        cst::Constraint::ValueRange { lower, upper, extensible } => IrConstraint::Range {
            lower: bound_to_int(lower),
            upper: bound_to_int(upper),
            extensible: *extensible,
        },
        cst::Constraint::SingleValue(v) => IrConstraint::Single(render_value(v)),
        cst::Constraint::Union(list) | cst::Constraint::Intersection(list) => {
            IrConstraint::Composite(
                list.iter().map(render_constraint_brief).collect::<Vec<_>>().join(" / "),
            )
        }
        cst::Constraint::WithComponents(_) => IrConstraint::Composite("WITH COMPONENTS".into()),
        cst::Constraint::Pattern(p) => IrConstraint::Composite(format!("PATTERN {p}")),
        cst::Constraint::ContainedSubtype(_) => IrConstraint::Composite("CONTAINING".into()),
        cst::Constraint::ObjectSet(n) => IrConstraint::Composite(format!("{{{{{}}}}}", n.value)),
        cst::Constraint::Extensible(inner) => {
            let lowered = lower_constraint(inner);
            match lowered {
                IrConstraint::Range { lower, upper, .. } => {
                    IrConstraint::Range { lower, upper, extensible: true }
                }
                other => IrConstraint::Composite(format!("{}, ...", render_ir_constraint(&other))),
            }
        }
        cst::Constraint::Opaque => IrConstraint::Composite("…".into()),
    }
}

fn bound_to_int(b: &cst::ValueBound) -> Option<i64> {
    match b {
        cst::ValueBound::Min | cst::ValueBound::Max => None,
        cst::ValueBound::Value(v) => match v {
            cst::Value::Integer(n) => Some(*n),
            _ => None,
        },
    }
}

fn render_constraint_brief(c: &cst::Constraint) -> String {
    match c {
        cst::Constraint::SingleValue(v) => render_value(v),
        cst::Constraint::ValueRange { lower, upper, .. } => {
            format!("{}..{}", render_bound(lower), render_bound(upper))
        }
        cst::Constraint::Size(_) => "SIZE(..)".into(),
        _ => "..".into(),
    }
}

fn render_bound(b: &cst::ValueBound) -> String {
    match b {
        cst::ValueBound::Min => "MIN".into(),
        cst::ValueBound::Max => "MAX".into(),
        cst::ValueBound::Value(v) => render_value(v),
    }
}

fn render_ir_constraint(c: &IrConstraint) -> String {
    match c {
        IrConstraint::Range { lower, upper, .. } => format!(
            "{}..{}",
            lower.map(|n| n.to_string()).unwrap_or_else(|| "MIN".into()),
            upper.map(|n| n.to_string()).unwrap_or_else(|| "MAX".into()),
        ),
        IrConstraint::Single(s) => s.clone(),
        IrConstraint::Size(inner) => format!("SIZE({})", render_ir_constraint(inner)),
        IrConstraint::Composite(s) => s.clone(),
    }
}

fn render_value(v: &cst::Value) -> String {
    match v {
        cst::Value::Boolean(b) => b.to_string(),
        cst::Value::Null => "NULL".into(),
        cst::Value::Integer(n) => n.to_string(),
        cst::Value::Real(r) => r.to_string(),
        cst::Value::String(s) => format!("\"{s}\""),
        cst::Value::BString(s) => format!("'{s}'B"),
        cst::Value::HString(s) => format!("'{s}'H"),
        cst::Value::NamedNumber(n) | cst::Value::Reference(n) => n.value.clone(),
        cst::Value::Oid(parts) => {
            let s = parts
                .iter()
                .map(|p| match (&p.name, p.value) {
                    (Some(n), Some(v)) => format!("{}({v})", n.value),
                    (Some(n), None) => n.value.clone(),
                    (None, Some(v)) => v.to_string(),
                    (None, None) => "?".into(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("{{ {s} }}")
        }
        cst::Value::Sequence(fields) => {
            let body = fields
                .iter()
                .map(|(n, v)| format!("{} {}", n.value, render_value(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {body} }}")
        }
        cst::Value::SequenceOf(items) => {
            let body = items.iter().map(render_value).collect::<Vec<_>>().join(", ");
            format!("{{ {body} }}")
        }
        cst::Value::Choice(name, inner) => format!("{} : {}", name.value, render_value(inner)),
    }
}

// ---------------------------------------------------------------------------
// Name resolver
// ---------------------------------------------------------------------------

struct Resolver<'a> {
    /// Maps module name -> set of type names defined there.
    module_types: HashMap<&'a str, Vec<&'a str>>,
    current_module: &'a str,
    /// Maps imported symbol name to its origin module.
    imports: HashMap<&'a str, &'a str>,
}

impl<'a> Resolver<'a> {
    fn new(modules: &'a [cst::Module]) -> Self {
        let mut module_types: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        for m in modules {
            let names: Vec<&str> = m
                .assignments
                .iter()
                .filter(|a| matches!(a.kind, cst::AssignmentKind::Type(_)))
                .map(|a| a.name.value.as_str())
                .collect();
            module_types.insert(m.name.value.as_str(), names);
        }
        Self { module_types, current_module: "", imports: HashMap::new() }
    }

    fn set_current(&mut self, name: &'a str, imports: &'a [cst::ImportClause]) {
        self.current_module = name;
        self.imports.clear();
        for imp in imports {
            for sym in &imp.symbols {
                self.imports.insert(sym.value.as_str(), imp.from_module.value.as_str());
            }
        }
    }

    /// Returns `(module, name)`; `module = None` when the reference could not
    /// be resolved to any known module.
    fn resolve(&self, name: &str) -> (Option<String>, String) {
        if let Some(types) = self.module_types.get(self.current_module) {
            if types.contains(&name) {
                return (Some(self.current_module.to_string()), name.to_string());
            }
        }
        if let Some(origin) = self.imports.get(name) {
            return (Some(origin.to_string()), name.to_string());
        }
        (None, name.to_string())
    }
}

// ---------------------------------------------------------------------------
// Convenience queries
// ---------------------------------------------------------------------------

impl IrProgram {
    /// Find a type definition by module + name.
    pub fn find_type(&self, module: &str, name: &str) -> Option<&IrTypeDef> {
        self.modules.iter().find(|m| m.name == module)?.items.iter().find_map(|i| match i {
            IrItem::Type(t) if t.name == name => Some(t),
            _ => None,
        })
    }

    /// Iterate every type definition across modules.
    pub fn all_types(&self) -> impl Iterator<Item = (&IrModule, &IrTypeDef)> {
        self.modules.iter().flat_map(|m| {
            m.items.iter().filter_map(move |i| match i {
                IrItem::Type(t) => Some((m, t)),
                _ => None,
            })
        })
    }
}

/// Pretty-render an [`IrType`] into a short human string (used by viz and tests).
pub fn render_type(ty: &IrType) -> String {
    match ty {
        IrType::Boolean => "BOOLEAN".into(),
        IrType::Integer { .. } => "INTEGER".into(),
        IrType::Real => "REAL".into(),
        IrType::Null => "NULL".into(),
        IrType::BitString { .. } => "BIT STRING".into(),
        IrType::OctetString { .. } => "OCTET STRING".into(),
        IrType::ObjectIdentifier => "OBJECT IDENTIFIER".into(),
        IrType::RelativeOid => "RELATIVE-OID".into(),
        IrType::CharString { kind, .. } => format!("{kind:?}String").replace("Ia5", "IA5"),
        IrType::UtcTime => "UTCTime".into(),
        IrType::GeneralizedTime => "GeneralizedTime".into(),
        IrType::Enumerated { .. } => "ENUMERATED".into(),
        IrType::Sequence(_) => "SEQUENCE".into(),
        IrType::Set(_) => "SET".into(),
        IrType::SequenceOf { element, .. } => format!("SEQUENCE OF {}", render_type(element)),
        IrType::SetOf { element, .. } => format!("SET OF {}", render_type(element)),
        IrType::Choice(_) => "CHOICE".into(),
        IrType::Reference { module, name } => match module {
            Some(m) => format!("{m}.{name}"),
            None => name.clone(),
        },
        IrType::Open { description } => format!("OPEN({description})"),
        IrType::Any => "ANY".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asn1_parser::{parse_source, SourceMap};

    fn parse(src: &str) -> cst::Module {
        let mut sm = SourceMap::new();
        let f = sm.add("t.asn", src.to_string());
        parse_source(&sm, f).unwrap()
    }

    #[test]
    fn lowers_simple_sequence() {
        let m = parse(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Point ::= SEQUENCE { x INTEGER, y INTEGER OPTIONAL }
            END"#,
        );
        let ir = lower(&[m]);
        let point = ir.find_type("Foo", "Point").unwrap();
        let IrType::Sequence(s) = &point.ty else {
            panic!("expected sequence");
        };
        assert_eq!(s.members.len(), 2);
        match &s.members[1] {
            IrStructMember::Field(f) => assert!(matches!(f.optionality, IrOptionality::Optional)),
            _ => panic!("field"),
        }
    }

    #[test]
    fn resolves_intra_module_references() {
        let m = parse(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Id ::= INTEGER
                Wrapper ::= SEQUENCE { id Id }
            END"#,
        );
        let ir = lower(&[m]);
        let w = ir.find_type("Foo", "Wrapper").unwrap();
        let IrType::Sequence(s) = &w.ty else { panic!() };
        let IrStructMember::Field(f) = &s.members[0] else { panic!() };
        match &f.ty {
            IrType::Reference { module, name } => {
                assert_eq!(module.as_deref(), Some("Foo"));
                assert_eq!(name, "Id");
            }
            _ => panic!("expected reference"),
        }
    }

    #[test]
    fn enumerated_lowered_with_extensions() {
        let m = parse(
            r#"Foo DEFINITIONS AUTOMATIC TAGS ::= BEGIN
                Color ::= ENUMERATED { red, green (1), blue, ..., yellow (99) }
            END"#,
        );
        let ir = lower(&[m]);
        let IrType::Enumerated { items, extensible } = &ir.find_type("Foo", "Color").unwrap().ty
        else {
            panic!();
        };
        assert!(*extensible);
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|i| i.name == "yellow" && i.is_extension));
    }
}
