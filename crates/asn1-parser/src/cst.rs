//! Concrete syntax tree for ASN.1 modules.
//!
//! The parser produces a `Module` per input file; the IR crate lowers these into a
//! resolved, cross-referenced semantic representation. Spans are attached to every
//! syntactic construct so diagnostics can point into the original source.

use crate::diagnostics::{Span, Spanned};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Module {
    pub name: Spanned<String>,
    pub oid: Option<Vec<OidComponent>>,
    pub tag_default: TagDefault,
    pub extensibility_implied: bool,
    pub exports: ExportClause,
    pub imports: Vec<ImportClause>,
    pub assignments: Vec<Assignment>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TagDefault {
    Explicit,
    Implicit,
    Automatic,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OidComponent {
    pub name: Option<Spanned<String>>,
    pub value: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExportClause {
    All,
    None,
    List(Vec<Spanned<String>>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ImportClause {
    pub symbols: Vec<Spanned<String>>,
    pub from_module: Spanned<String>,
    pub from_oid: Option<Vec<OidComponent>>,
    pub with: Option<WithClause>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum WithClause {
    Successors,
    Descendants,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Assignment {
    pub doc: Option<String>,
    pub name: Spanned<String>,
    pub kind: AssignmentKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AssignmentKind {
    /// `Foo ::= Type`
    Type(Type),
    /// `foo Type ::= Value`
    Value { ty: Type, value: Value },
    /// `Foo ::= CLASS { ... }` — an information object class
    ObjectClass(ObjectClass),
    /// `fooSet CLASS ::= { ... }` — an information object set
    ObjectSet { class_name: Spanned<String>, set: ObjectSet },
    /// `foo CLASS ::= { ... }` — an information object
    Object { class_name: Spanned<String>, object: ObjectDef },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Type {
    pub kind: TypeKind,
    pub constraints: Vec<Constraint>,
    pub tag: Option<Tag>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TypeKind {
    Boolean,
    Integer {
        named_numbers: Vec<NamedNumber>,
    },
    Real,
    Null,
    BitString {
        named_bits: Vec<NamedNumber>,
    },
    OctetString,
    ObjectIdentifier,
    RelativeOid,
    CharString(CharStringKind),
    UtcTime,
    GeneralizedTime,
    Enumerated {
        items: Vec<EnumItem>,
        extensible: bool,
        extension_items: Vec<EnumItem>,
    },
    Sequence(StructType),
    SequenceOf(Box<Type>),
    Set(StructType),
    SetOf(Box<Type>),
    Choice(ChoiceType),
    Reference(Spanned<String>),
    /// e.g. `BLOCK-TYPE.&id` or `BLOCK-TYPE.&Content`
    ClassField {
        class: Spanned<String>,
        path: Vec<FieldRef>,
    },
    /// Fallback for constructs we parse syntactically but do not model semantically.
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CharStringKind {
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
pub struct StructType {
    pub components: Vec<StructMember>,
    pub extensible: bool,
    pub extension_additions: Vec<StructMember>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StructMember {
    /// A named component field.
    Named(Component),
    /// `COMPONENTS OF <TypeRef>` — inline all components of another SEQUENCE/SET.
    ComponentsOf { ty: Type, span: Span },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ChoiceType {
    pub alternatives: Vec<Component>,
    pub extensible: bool,
    pub extension_alternatives: Vec<Component>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Component {
    pub doc: Option<String>,
    pub name: Spanned<String>,
    pub ty: Type,
    pub optionality: Optionality,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Optionality {
    Required,
    Optional,
    Default(Value),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NamedNumber {
    pub name: Spanned<String>,
    pub value: NamedNumberValue,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NamedNumberValue {
    Literal(i64),
    Reference(Spanned<String>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EnumItem {
    pub doc: Option<String>,
    pub name: Spanned<String>,
    pub value: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Tag {
    pub class: TagClass,
    pub number: TagNumber,
    pub kind: TagKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TagClass {
    Universal,
    Application,
    Private,
    ContextSpecific,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TagNumber {
    Literal(i64),
    Reference(Spanned<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TagKind {
    Explicit,
    Implicit,
    Automatic,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FieldRef {
    /// `&Type` — the field refers to a type field (uppercase after `&`).
    Type(Spanned<String>),
    /// `&value` — the field refers to a value field (lowercase after `&`).
    Value(Spanned<String>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Constraint {
    Size(Box<Constraint>),
    ValueRange {
        lower: ValueBound,
        upper: ValueBound,
        extensible: bool,
    },
    SingleValue(Value),
    Union(Vec<Constraint>),
    Intersection(Vec<Constraint>),
    WithComponents(WithComponentsConstraint),
    Pattern(String),
    ContainedSubtype(Box<Type>),
    ObjectSet(Spanned<String>),
    Extensible(Box<Constraint>),
    /// Unparsed / intentionally dropped inner constraint content.
    Opaque,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ValueBound {
    Min,
    Max,
    Value(Value),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct WithComponentsConstraint {
    pub partial: bool,
    pub components: Vec<ComponentConstraint>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ComponentConstraint {
    pub name: Spanned<String>,
    pub value_constraint: Option<Box<Constraint>>,
    pub presence: Option<Presence>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Presence {
    Present,
    Absent,
    Optional,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Value {
    Boolean(bool),
    Null,
    Integer(i64),
    Real(f64),
    String(String),
    BString(String),
    HString(String),
    NamedNumber(Spanned<String>),
    Reference(Spanned<String>),
    Oid(Vec<OidComponent>),
    Sequence(Vec<(Spanned<String>, Value)>),
    SequenceOf(Vec<Value>),
    Choice(Spanned<String>, Box<Value>),
}

// -- Information object classes / sets ---------------------------------------

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjectClass {
    pub fields: Vec<FieldSpec>,
    pub syntax: Option<Vec<SyntaxToken>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FieldSpec {
    /// `&Type` — an open type field.
    TypeField { name: Spanned<String>, optional: bool, default: Option<Type>, span: Span },
    /// `&value Type` (value field, optional UNIQUE, OPTIONAL, DEFAULT).
    ValueField {
        name: Spanned<String>,
        ty: Type,
        unique: bool,
        optional: bool,
        default: Option<Value>,
        span: Span,
    },
    /// `&value CLASS.&TypeField` — open type driven by a sibling field.
    VariableTypeValueField {
        name: Spanned<String>,
        field_path: Vec<FieldRef>,
        optional: bool,
        span: Span,
    },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SyntaxToken {
    Literal(String),
    FieldName(Spanned<String>),
    Optional(Vec<SyntaxToken>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjectSet {
    pub elements: Vec<ObjectSetElement>,
    pub extensible: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ObjectSetElement {
    Object(ObjectDef),
    Reference(Spanned<String>),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ObjectDef {
    pub fields: Vec<ObjectFieldSetting>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ObjectFieldSetting {
    Type { name: Spanned<String>, ty: Type },
    Value { name: Spanned<String>, value: Value },
}
