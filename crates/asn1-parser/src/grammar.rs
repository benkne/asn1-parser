//! Recursive-descent grammar for ASN.1.
//!
//! The grammar consumes the token stream produced by [`crate::lexer`] and builds a
//! [`crate::cst::Module`]. It is pragmatic: constructs that appear in the POIM
//! reference corpus are modeled precisely, while exotic constructs (parameterized
//! assignments, selection types, user-defined constraints) are parsed at the brace-
//! balanced level and stored as `Opaque` / `Any` so the parser never rejects a
//! syntactically valid module.

use crate::cst::*;
use crate::diagnostics::{ParseError, Span, Spanned};
use crate::lexer::{TokKind, Token};

pub fn parse_module(tokens: Vec<Token>) -> Result<Module, ParseError> {
    let mut p = Parser::new(tokens);
    let module = p.parse_module()?;
    p.expect_eof()?;
    Ok(module)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pending_doc: Option<String>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, pending_doc: None }
    }

    // ---------------------------------------------------------------------
    // Token helpers
    // ---------------------------------------------------------------------

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokKind::Eof)
    }

    fn expect_eof(&self) -> Result<(), ParseError> {
        if self.at_eof() {
            Ok(())
        } else {
            let tok = self.peek();
            Err(ParseError::new(format!("expected end of input, got {:?}", tok.kind), tok.span))
        }
    }

    #[allow(dead_code)]
    fn eat(&mut self, kind: &TokKind) -> bool {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_kind(&mut self, kind: &TokKind, what: &str) -> Result<Token, ParseError> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind) {
            Ok(self.bump())
        } else {
            Err(ParseError::new(
                format!("expected {}, got {}", what, describe(&self.peek().kind)),
                self.peek().span,
            ))
        }
    }

    fn peek_ident_is(&self, word: &str) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(s) if s == word)
    }

    fn eat_ident(&mut self, word: &str) -> bool {
        if self.peek_ident_is(word) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_ident_word(&mut self, word: &str) -> Result<Token, ParseError> {
        if self.peek_ident_is(word) {
            Ok(self.bump())
        } else {
            Err(ParseError::new(
                format!("expected keyword `{}`, got {}", word, describe(&self.peek().kind)),
                self.peek().span,
            ))
        }
    }

    fn consume_doc_comments(&mut self) {
        while let TokKind::Doc(text) = &self.peek().kind {
            self.pending_doc = Some(text.clone());
            self.bump();
        }
    }

    fn take_doc(&mut self) -> Option<String> {
        self.pending_doc.take()
    }

    fn expect_any_ident(&mut self) -> Result<Spanned<String>, ParseError> {
        match &self.peek().kind {
            TokKind::Ident(s) => {
                let span = self.peek().span;
                let name = s.clone();
                self.bump();
                Ok(Spanned::new(name, span))
            }
            other => Err(ParseError::new(
                format!("expected identifier, got {}", describe(other)),
                self.peek().span,
            )),
        }
    }

    // ---------------------------------------------------------------------
    // Module
    // ---------------------------------------------------------------------

    fn parse_module(&mut self) -> Result<Module, ParseError> {
        self.consume_doc_comments();
        let start = self.peek().span;
        let name = self.expect_any_ident()?;
        let oid = if matches!(self.peek().kind, TokKind::LBrace) {
            Some(self.parse_oid_body()?)
        } else {
            None
        };
        self.expect_ident_word("DEFINITIONS")?;
        let tag_default = if self.eat_ident("EXPLICIT") {
            self.expect_ident_word("TAGS")?;
            TagDefault::Explicit
        } else if self.eat_ident("IMPLICIT") {
            self.expect_ident_word("TAGS")?;
            TagDefault::Implicit
        } else if self.eat_ident("AUTOMATIC") {
            self.expect_ident_word("TAGS")?;
            TagDefault::Automatic
        } else {
            TagDefault::Explicit
        };
        let extensibility_implied = if self.eat_ident("EXTENSIBILITY") {
            self.expect_ident_word("IMPLIED")?;
            true
        } else {
            false
        };
        self.expect_kind(&TokKind::Assign, "`::=`")?;
        self.expect_ident_word("BEGIN")?;

        let exports = self.parse_exports()?;
        let imports = self.parse_imports()?;
        let mut assignments = Vec::new();
        loop {
            self.consume_doc_comments();
            if self.peek_ident_is("END") {
                break;
            }
            if self.at_eof() {
                return Err(ParseError::new(
                    "unexpected end of input inside module body",
                    self.peek().span,
                ));
            }
            assignments.push(self.parse_assignment()?);
        }
        let end = self.peek().span;
        self.expect_ident_word("END")?;

        Ok(Module {
            name,
            oid,
            tag_default,
            extensibility_implied,
            exports,
            imports,
            assignments,
            span: start.join(end),
        })
    }

    fn parse_oid_body(&mut self) -> Result<Vec<OidComponent>, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut comps = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace) {
            let start = self.peek().span;
            let comp = match &self.peek().kind {
                TokKind::Ident(_) => {
                    let name = self.expect_any_ident()?;
                    if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump();
                        let value = match &self.peek().kind {
                            TokKind::Number(n) => {
                                let v = n.parse::<i64>().map_err(|_| {
                                    ParseError::new("bad OID value", self.peek().span)
                                })?;
                                self.bump();
                                Some(v)
                            }
                            _ => None,
                        };
                        self.expect_kind(&TokKind::RParen, "`)`")?;
                        OidComponent {
                            name: Some(name.clone()),
                            value,
                            span: start.join(self.prev_span()),
                        }
                    } else {
                        OidComponent { name: Some(name.clone()), value: None, span: name.span }
                    }
                }
                TokKind::Number(n) => {
                    let v = n
                        .parse::<i64>()
                        .map_err(|_| ParseError::new("bad OID number", self.peek().span))?;
                    let span = self.peek().span;
                    self.bump();
                    OidComponent { name: None, value: Some(v), span }
                }
                _ => {
                    return Err(ParseError::new(
                        format!("unexpected {} in OID", describe(&self.peek().kind)),
                        self.peek().span,
                    ));
                }
            };
            comps.push(comp);
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(comps)
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            Span::DUMMY
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn parse_exports(&mut self) -> Result<ExportClause, ParseError> {
        if !self.eat_ident("EXPORTS") {
            return Ok(ExportClause::None);
        }
        if self.eat_ident("ALL") {
            self.expect_kind(&TokKind::Semi, "`;`")?;
            return Ok(ExportClause::All);
        }
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
            return Ok(ExportClause::List(Vec::new()));
        }
        let mut syms = Vec::new();
        loop {
            syms.push(self.expect_any_ident()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::Semi, "`;`")?;
        Ok(ExportClause::List(syms))
    }

    fn parse_imports(&mut self) -> Result<Vec<ImportClause>, ParseError> {
        if !self.eat_ident("IMPORTS") {
            return Ok(Vec::new());
        }
        let mut groups = Vec::new();
        while !matches!(self.peek().kind, TokKind::Semi) {
            let start = self.peek().span;
            let mut symbols = Vec::new();
            loop {
                let sym = self.expect_any_ident()?;
                symbols.push(sym);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            self.expect_ident_word("FROM")?;
            let module = self.expect_any_ident()?;
            let oid = if matches!(self.peek().kind, TokKind::LBrace) {
                Some(self.parse_oid_body()?)
            } else {
                None
            };
            let with = if self.eat_ident("WITH") {
                if self.eat_ident("SUCCESSORS") {
                    Some(WithClause::Successors)
                } else if self.eat_ident("DESCENDANTS") {
                    Some(WithClause::Descendants)
                } else {
                    return Err(ParseError::new(
                        "expected `SUCCESSORS` or `DESCENDANTS` after `WITH`",
                        self.peek().span,
                    ));
                }
            } else {
                None
            };
            let end = self.prev_span();
            groups.push(ImportClause {
                symbols,
                from_module: module,
                from_oid: oid,
                with,
                span: start.join(end),
            });
        }
        self.expect_kind(&TokKind::Semi, "`;`")?;
        Ok(groups)
    }

    // ---------------------------------------------------------------------
    // Assignments
    // ---------------------------------------------------------------------

    fn parse_assignment(&mut self) -> Result<Assignment, ParseError> {
        self.consume_doc_comments();
        let doc = self.take_doc();
        let start = self.peek().span;
        let name = self.expect_any_ident()?;
        if matches!(self.peek().kind, TokKind::Assign) {
            self.bump();
            if self.peek_ident_is("CLASS") {
                let class = self.parse_object_class()?;
                let end = self.prev_span();
                return Ok(Assignment {
                    doc,
                    name,
                    kind: AssignmentKind::ObjectClass(class),
                    span: start.join(end),
                });
            }
            let ty = self.parse_type()?;
            let end = self.prev_span();
            return Ok(Assignment {
                doc,
                name,
                kind: AssignmentKind::Type(ty),
                span: start.join(end),
            });
        }

        let class_or_type = self.expect_any_ident()?;
        self.expect_kind(&TokKind::Assign, "`::=`")?;

        if matches!(self.peek().kind, TokKind::LBrace)
            && matches!(self.peek_at(1).kind, TokKind::LBrace)
        {
            let set = self.parse_object_set_body()?;
            let end = self.prev_span();
            return Ok(Assignment {
                doc,
                name,
                kind: AssignmentKind::ObjectSet { class_name: class_or_type, set },
                span: start.join(end),
            });
        }

        let ty = Type {
            kind: TypeKind::Reference(class_or_type),
            constraints: Vec::new(),
            tag: None,
            span: name.span,
        };
        let value = self.parse_value()?;
        let end = self.prev_span();
        Ok(Assignment {
            doc,
            name,
            kind: AssignmentKind::Value { ty, value },
            span: start.join(end),
        })
    }

    // ---------------------------------------------------------------------
    // Types
    // ---------------------------------------------------------------------

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let start = self.peek().span;
        let tag = if matches!(self.peek().kind, TokKind::LBracket) {
            Some(self.parse_tag_prefix()?)
        } else {
            None
        };
        let (kind, leading) = self.parse_primary_type()?;
        let mut constraints = leading;
        while matches!(self.peek().kind, TokKind::LParen) {
            constraints.push(self.parse_constraint()?);
        }
        let end = self.prev_span();
        Ok(Type { kind, constraints, tag, span: start.join(end) })
    }

    fn parse_tag_prefix(&mut self) -> Result<Tag, ParseError> {
        let start = self.expect_kind(&TokKind::LBracket, "`[`")?.span;
        let class = if self.eat_ident("APPLICATION") {
            TagClass::Application
        } else if self.eat_ident("PRIVATE") {
            TagClass::Private
        } else if self.eat_ident("UNIVERSAL") {
            TagClass::Universal
        } else {
            TagClass::ContextSpecific
        };
        let number = match &self.peek().kind {
            TokKind::Number(n) => {
                let v = n
                    .parse::<i64>()
                    .map_err(|_| ParseError::new("bad tag number", self.peek().span))?;
                self.bump();
                TagNumber::Literal(v)
            }
            TokKind::Ident(_) => {
                let id = self.expect_any_ident()?;
                TagNumber::Reference(id)
            }
            _ => {
                return Err(ParseError::new(
                    format!("expected tag number, got {}", describe(&self.peek().kind)),
                    self.peek().span,
                ));
            }
        };
        self.expect_kind(&TokKind::RBracket, "`]`")?;
        let kind = if self.eat_ident("IMPLICIT") {
            TagKind::Implicit
        } else if self.eat_ident("EXPLICIT") {
            TagKind::Explicit
        } else {
            TagKind::Automatic
        };
        let end = self.prev_span();
        Ok(Tag { class, number, kind, span: start.join(end) })
    }

    fn parse_primary_type(&mut self) -> Result<(TypeKind, Vec<Constraint>), ParseError> {
        let tok = self.peek().clone();
        let ident = match &tok.kind {
            TokKind::Ident(s) => s.clone(),
            _ => {
                return Err(ParseError::new(
                    format!("expected type, got {}", describe(&tok.kind)),
                    tok.span,
                ));
            }
        };
        match ident.as_str() {
            "BOOLEAN" => {
                self.bump();
                Ok((TypeKind::Boolean, Vec::new()))
            }
            "INTEGER" => {
                self.bump();
                let named_numbers = if matches!(self.peek().kind, TokKind::LBrace) {
                    self.parse_named_number_list()?
                } else {
                    Vec::new()
                };
                Ok((TypeKind::Integer { named_numbers }, Vec::new()))
            }
            "REAL" => {
                self.bump();
                Ok((TypeKind::Real, Vec::new()))
            }
            "NULL" => {
                self.bump();
                Ok((TypeKind::Null, Vec::new()))
            }
            "BIT" => {
                self.bump();
                self.expect_ident_word("STRING")?;
                let named_bits = if matches!(self.peek().kind, TokKind::LBrace) {
                    self.parse_named_number_list()?
                } else {
                    Vec::new()
                };
                Ok((TypeKind::BitString { named_bits }, Vec::new()))
            }
            "OCTET" => {
                self.bump();
                self.expect_ident_word("STRING")?;
                Ok((TypeKind::OctetString, Vec::new()))
            }
            "OBJECT" => {
                self.bump();
                self.expect_ident_word("IDENTIFIER")?;
                Ok((TypeKind::ObjectIdentifier, Vec::new()))
            }
            "RELATIVE-OID" => {
                self.bump();
                Ok((TypeKind::RelativeOid, Vec::new()))
            }
            "ENUMERATED" => {
                self.bump();
                Ok((self.parse_enumerated_body()?, Vec::new()))
            }
            "SEQUENCE" => {
                self.bump();
                self.parse_sequence_like(true)
            }
            "SET" => {
                self.bump();
                self.parse_sequence_like(false)
            }
            "CHOICE" => {
                self.bump();
                Ok((self.parse_choice_body()?, Vec::new()))
            }
            "UTCTime" => {
                self.bump();
                Ok((TypeKind::UtcTime, Vec::new()))
            }
            "GeneralizedTime" => {
                self.bump();
                Ok((TypeKind::GeneralizedTime, Vec::new()))
            }
            "UTF8String" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Utf8), Vec::new()))
            }
            "BMPString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Bmp), Vec::new()))
            }
            "IA5String" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Ia5), Vec::new()))
            }
            "PrintableString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Printable), Vec::new()))
            }
            "NumericString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Numeric), Vec::new()))
            }
            "VisibleString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Visible), Vec::new()))
            }
            "UniversalString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Universal), Vec::new()))
            }
            "GeneralString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::General), Vec::new()))
            }
            "GraphicString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Graphic), Vec::new()))
            }
            "TeletexString" | "T61String" => {
                self.bump();
                let k = if ident == "T61String" {
                    CharStringKind::T61
                } else {
                    CharStringKind::Teletex
                };
                Ok((TypeKind::CharString(k), Vec::new()))
            }
            "VideotexString" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Videotex), Vec::new()))
            }
            "ISO646String" => {
                self.bump();
                Ok((TypeKind::CharString(CharStringKind::Iso646), Vec::new()))
            }
            "CHARACTER" => {
                self.bump();
                self.expect_ident_word("STRING")?;
                Ok((TypeKind::CharString(CharStringKind::Utf8), Vec::new()))
            }
            _ => {
                let kind = self.parse_reference_or_class_field()?;
                Ok((kind, Vec::new()))
            }
        }
    }

    fn parse_named_number_list(&mut self) -> Result<Vec<NamedNumber>, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut out = Vec::new();
        loop {
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            let start = self.peek().span;
            let name = self.expect_any_ident()?;
            self.expect_kind(&TokKind::LParen, "`(`")?;
            let mut negative = false;
            if matches!(self.peek().kind, TokKind::Hyphen) {
                self.bump();
                negative = true;
            }
            let value = match &self.peek().kind {
                TokKind::Number(n) => {
                    let v = n
                        .parse::<i64>()
                        .map_err(|_| ParseError::new("bad integer literal", self.peek().span))?;
                    let v = if negative { -v } else { v };
                    self.bump();
                    NamedNumberValue::Literal(v)
                }
                TokKind::Ident(_) => {
                    let id = self.expect_any_ident()?;
                    NamedNumberValue::Reference(id)
                }
                _ => {
                    return Err(ParseError::new(
                        format!(
                            "expected integer or identifier in named-number value, got {}",
                            describe(&self.peek().kind)
                        ),
                        self.peek().span,
                    ));
                }
            };
            self.expect_kind(&TokKind::RParen, "`)`")?;
            let end = self.prev_span();
            out.push(NamedNumber { name, value, span: start.join(end) });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(out)
    }

    fn parse_enumerated_body(&mut self) -> Result<TypeKind, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut items = Vec::new();
        let mut ext_items = Vec::new();
        let mut extensible = false;
        let mut seen_ellipsis = false;
        loop {
            self.consume_doc_comments();
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::Ellipsis) {
                self.bump();
                extensible = true;
                seen_ellipsis = true;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            let item = self.parse_enum_item()?;
            if seen_ellipsis {
                ext_items.push(item);
            } else {
                items.push(item);
            }
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(TypeKind::Enumerated { items, extensible, extension_items: ext_items })
    }

    fn parse_enum_item(&mut self) -> Result<EnumItem, ParseError> {
        let doc = self.take_doc();
        let start = self.peek().span;
        let name = self.expect_any_ident()?;
        let value = if matches!(self.peek().kind, TokKind::LParen) {
            self.bump();
            let mut negative = false;
            if matches!(self.peek().kind, TokKind::Hyphen) {
                self.bump();
                negative = true;
            }
            let v = match &self.peek().kind {
                TokKind::Number(n) => {
                    let v = n
                        .parse::<i64>()
                        .map_err(|_| ParseError::new("bad enum value", self.peek().span))?;
                    self.bump();
                    Some(if negative { -v } else { v })
                }
                TokKind::Ident(_) => {
                    // enumerated value can be a reference; we flatten to None and let the IR resolve.
                    self.bump();
                    None
                }
                _ => None,
            };
            self.expect_kind(&TokKind::RParen, "`)`")?;
            v
        } else {
            None
        };
        let end = self.prev_span();
        Ok(EnumItem { doc, name, value, span: start.join(end) })
    }

    fn parse_sequence_like(
        &mut self,
        is_sequence: bool,
    ) -> Result<(TypeKind, Vec<Constraint>), ParseError> {
        // A leading `(...)` before OF attaches as a constraint to the outer type.
        // Also accept the bare `SIZE (...)` form: `SEQUENCE SIZE (1..13) OF T`.
        let leading = if matches!(self.peek().kind, TokKind::LParen) {
            vec![self.parse_constraint()?]
        } else if self.peek_ident_is("SIZE") {
            self.bump();
            self.expect_kind(&TokKind::LParen, "`(`")?;
            let inner = self.parse_union_constraint()?;
            let inner = if self.eat_extensible_marker() {
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let _ = self.parse_union_constraint()?;
                }
                Constraint::Extensible(Box::new(inner))
            } else {
                inner
            };
            self.expect_kind(&TokKind::RParen, "`)`")?;
            vec![Constraint::Size(Box::new(inner))]
        } else {
            Vec::new()
        };

        if self.eat_ident("OF") {
            let inner = self.parse_type()?;
            let kind = if is_sequence {
                TypeKind::SequenceOf(Box::new(inner))
            } else {
                TypeKind::SetOf(Box::new(inner))
            };
            Ok((kind, leading))
        } else {
            self.expect_kind(&TokKind::LBrace, "`{` or `OF`")?;
            let struct_ty = self.parse_struct_body()?;
            let kind =
                if is_sequence { TypeKind::Sequence(struct_ty) } else { TypeKind::Set(struct_ty) };
            Ok((kind, leading))
        }
    }

    fn parse_struct_body(&mut self) -> Result<StructType, ParseError> {
        let mut components = Vec::new();
        let mut ext_additions = Vec::new();
        let mut extensible = false;
        let mut seen_ellipsis = false;
        loop {
            self.consume_doc_comments();
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::Ellipsis) {
                self.bump();
                extensible = true;
                seen_ellipsis = true;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            if matches!(self.peek().kind, TokKind::DblLBracket) {
                self.bump();
                loop {
                    self.consume_doc_comments();
                    if matches!(self.peek().kind, TokKind::DblRBracket) {
                        break;
                    }
                    let m = self.parse_struct_member()?;
                    ext_additions.push(m);
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        continue;
                    }
                    break;
                }
                self.expect_kind(&TokKind::DblRBracket, "`]]`")?;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
                continue;
            }
            let m = self.parse_struct_member()?;
            if seen_ellipsis {
                ext_additions.push(m);
            } else {
                components.push(m);
            }
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(StructType { components, extensible, extension_additions: ext_additions })
    }

    fn parse_struct_member(&mut self) -> Result<StructMember, ParseError> {
        if self.peek_ident_is("COMPONENTS")
            && matches!(self.peek_at(1).kind, TokKind::Ident(ref s) if s == "OF")
        {
            let start = self.bump().span;
            self.bump();
            let ty = self.parse_type()?;
            let end = self.prev_span();
            return Ok(StructMember::ComponentsOf { ty, span: start.join(end) });
        }
        Ok(StructMember::Named(self.parse_component()?))
    }

    fn parse_component(&mut self) -> Result<Component, ParseError> {
        self.consume_doc_comments();
        let doc = self.take_doc();
        let start = self.peek().span;
        let name = self.expect_any_ident()?;
        let ty = self.parse_type()?;
        let optionality = if self.eat_ident("OPTIONAL") {
            Optionality::Optional
        } else if self.eat_ident("DEFAULT") {
            let v = self.parse_value()?;
            Optionality::Default(v)
        } else {
            Optionality::Required
        };
        let end = self.prev_span();
        Ok(Component { doc, name, ty, optionality, span: start.join(end) })
    }

    fn parse_choice_body(&mut self) -> Result<TypeKind, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut alternatives = Vec::new();
        let mut ext_alternatives = Vec::new();
        let mut extensible = false;
        let mut seen_ellipsis = false;
        loop {
            self.consume_doc_comments();
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::Ellipsis) {
                self.bump();
                extensible = true;
                seen_ellipsis = true;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            if matches!(self.peek().kind, TokKind::DblLBracket) {
                self.bump();
                loop {
                    if matches!(self.peek().kind, TokKind::DblRBracket) {
                        break;
                    }
                    let c = self.parse_component()?;
                    ext_alternatives.push(c);
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        continue;
                    }
                    break;
                }
                self.expect_kind(&TokKind::DblRBracket, "`]]`")?;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
                continue;
            }
            let c = self.parse_component()?;
            if seen_ellipsis {
                ext_alternatives.push(c);
            } else {
                alternatives.push(c);
            }
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(TypeKind::Choice(ChoiceType {
            alternatives,
            extensible,
            extension_alternatives: ext_alternatives,
        }))
    }

    fn parse_reference_or_class_field(&mut self) -> Result<TypeKind, ParseError> {
        let name = self.expect_any_ident()?;
        if !matches!(self.peek().kind, TokKind::Dot) {
            return Ok(TypeKind::Reference(name));
        }
        let mut path = Vec::new();
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            self.expect_kind(&TokKind::Amp, "`&`")?;
            let field = self.expect_any_ident()?;
            let is_type =
                field.value.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
            path.push(if is_type { FieldRef::Type(field) } else { FieldRef::Value(field) });
        }
        Ok(TypeKind::ClassField { class: name, path })
    }

    // ---------------------------------------------------------------------
    // Constraints
    // ---------------------------------------------------------------------

    fn parse_constraint(&mut self) -> Result<Constraint, ParseError> {
        self.expect_kind(&TokKind::LParen, "`(`")?;
        let c = self.parse_union_constraint()?;
        let c = if self.eat_extensible_marker() {
            // Optional additional element set after the extension marker:
            //   `(root, ..., additional)`
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                let _ = self.parse_union_constraint()?;
            }
            Constraint::Extensible(Box::new(c))
        } else {
            c
        };
        self.expect_kind(&TokKind::RParen, "`)`")?;
        Ok(c)
    }

    /// Consumes `,...` atomically if present; returns whether it was consumed.
    fn eat_extensible_marker(&mut self) -> bool {
        if matches!(self.peek().kind, TokKind::Comma)
            && matches!(self.peek_at(1).kind, TokKind::Ellipsis)
        {
            self.bump();
            self.bump();
            true
        } else {
            false
        }
    }

    fn parse_union_constraint(&mut self) -> Result<Constraint, ParseError> {
        let mut parts = vec![self.parse_intersection_constraint()?];
        while matches!(self.peek().kind, TokKind::Pipe) || self.peek_ident_is("UNION") {
            self.bump();
            parts.push(self.parse_intersection_constraint()?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Constraint::Union(parts) })
    }

    fn parse_intersection_constraint(&mut self) -> Result<Constraint, ParseError> {
        let mut parts = vec![self.parse_constraint_element()?];
        while matches!(self.peek().kind, TokKind::Caret) || self.peek_ident_is("INTERSECTION") {
            self.bump();
            parts.push(self.parse_constraint_element()?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Constraint::Intersection(parts) })
    }

    fn parse_constraint_element(&mut self) -> Result<Constraint, ParseError> {
        if self.eat_ident("SIZE") {
            self.expect_kind(&TokKind::LParen, "`(`")?;
            let inner = self.parse_union_constraint()?;
            let inner = if self.eat_extensible_marker() {
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let _ = self.parse_union_constraint()?;
                }
                Constraint::Extensible(Box::new(inner))
            } else {
                inner
            };
            self.expect_kind(&TokKind::RParen, "`)`")?;
            return Ok(Constraint::Size(Box::new(inner)));
        }
        if self.peek_ident_is("WITH") {
            let save = self.pos;
            self.bump();
            if self.eat_ident("COMPONENTS") {
                let wc = self.parse_with_components()?;
                return Ok(Constraint::WithComponents(wc));
            }
            if self.eat_ident("COMPONENT") {
                // WITH COMPONENT (...) — inner-type constraint on SET OF / SEQUENCE OF
                self.expect_kind(&TokKind::LParen, "`(`")?;
                let _inner = self.parse_union_constraint()?;
                let _ = self.eat_extensible_marker();
                self.expect_kind(&TokKind::RParen, "`)`")?;
                return Ok(Constraint::Opaque);
            }
            self.pos = save;
        }
        if self.eat_ident("CONTAINING") {
            let ty = self.parse_type()?;
            return Ok(Constraint::ContainedSubtype(Box::new(ty)));
        }
        if self.eat_ident("PATTERN") {
            if let TokKind::CString(s) = &self.peek().kind {
                let s = s.clone();
                self.bump();
                return Ok(Constraint::Pattern(s));
            }
            return Err(ParseError::new("expected string literal after PATTERN", self.peek().span));
        }
        if matches!(self.peek().kind, TokKind::LParen) {
            // Parenthesized sub-constraint: `( inner )`.
            self.bump();
            let inner = self.parse_union_constraint()?;
            let inner = if self.eat_extensible_marker() {
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let _ = self.parse_union_constraint()?;
                }
                Constraint::Extensible(Box::new(inner))
            } else {
                inner
            };
            self.expect_kind(&TokKind::RParen, "`)`")?;
            return Ok(inner);
        }
        if matches!(self.peek().kind, TokKind::LBrace) {
            // Object-set / value-set reference in braces, possibly composed with an
            // at-notation companion group: `{ObjectSet}{@field}`.
            self.skip_balanced_braces();
            while matches!(self.peek().kind, TokKind::LBrace) {
                self.skip_balanced_braces();
            }
            return Ok(Constraint::Opaque);
        }

        // Otherwise, try a value range / single value.
        let lower = self.parse_value_bound()?;
        if matches!(self.peek().kind, TokKind::Range) {
            self.bump();
            let upper = self.parse_value_bound()?;
            // Extensibility is owned by the enclosing constraint, not the range itself.
            return Ok(Constraint::ValueRange { lower, upper, extensible: false });
        }
        let ValueBound::Value(v) = lower else {
            return Err(ParseError::new(
                "MIN/MAX cannot be used as a single value constraint",
                self.peek().span,
            ));
        };
        Ok(Constraint::SingleValue(v))
    }

    fn parse_value_bound(&mut self) -> Result<ValueBound, ParseError> {
        if self.eat_ident("MIN") {
            return Ok(ValueBound::Min);
        }
        if self.eat_ident("MAX") {
            return Ok(ValueBound::Max);
        }
        let v = self.parse_value()?;
        Ok(ValueBound::Value(v))
    }

    fn parse_with_components(&mut self) -> Result<WithComponentsConstraint, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut partial = false;
        if matches!(self.peek().kind, TokKind::Ellipsis) {
            self.bump();
            partial = true;
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let mut components = Vec::new();
        loop {
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            let start = self.peek().span;
            let name = self.expect_any_ident()?;
            let value_constraint = if matches!(self.peek().kind, TokKind::LParen) {
                Some(Box::new(self.parse_constraint()?))
            } else {
                None
            };
            let presence = if self.eat_ident("PRESENT") {
                Some(Presence::Present)
            } else if self.eat_ident("ABSENT") {
                Some(Presence::Absent)
            } else if self.eat_ident("OPTIONAL") {
                Some(Presence::Optional)
            } else {
                None
            };
            let end = self.prev_span();
            components.push(ComponentConstraint {
                name,
                value_constraint,
                presence,
                span: start.join(end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(WithComponentsConstraint { partial, components })
    }

    fn skip_balanced_braces(&mut self) {
        let mut depth = 0i32;
        if matches!(self.peek().kind, TokKind::LBrace) {
            depth += 1;
            self.bump();
        }
        while depth > 0 {
            match self.peek().kind {
                TokKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokKind::RBrace => {
                    depth -= 1;
                    self.bump();
                }
                TokKind::Eof => break,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // Values
    // ---------------------------------------------------------------------

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match &self.peek().kind {
            TokKind::Number(_) | TokKind::Hyphen => {
                let mut negative = false;
                if matches!(self.peek().kind, TokKind::Hyphen) {
                    self.bump();
                    negative = true;
                }
                match &self.peek().kind {
                    TokKind::Number(n) => {
                        let v = n.parse::<i64>().map_err(|_| {
                            ParseError::new("bad integer literal", self.peek().span)
                        })?;
                        self.bump();
                        Ok(Value::Integer(if negative { -v } else { v }))
                    }
                    TokKind::Real(r) => {
                        let v: f64 = r
                            .parse()
                            .map_err(|_| ParseError::new("bad real literal", self.peek().span))?;
                        self.bump();
                        Ok(Value::Real(if negative { -v } else { v }))
                    }
                    _ => {
                        Err(ParseError::new("expected integer or real after `-`", self.peek().span))
                    }
                }
            }
            TokKind::Real(r) => {
                let v: f64 =
                    r.parse().map_err(|_| ParseError::new("bad real literal", self.peek().span))?;
                self.bump();
                Ok(Value::Real(v))
            }
            TokKind::CString(s) => {
                let s = s.clone();
                self.bump();
                Ok(Value::String(s))
            }
            TokKind::BString(s) => {
                let s = s.clone();
                self.bump();
                Ok(Value::BString(s))
            }
            TokKind::HString(s) => {
                let s = s.clone();
                self.bump();
                Ok(Value::HString(s))
            }
            TokKind::Ident(s) => {
                let name = s.clone();
                let span = self.peek().span;
                match name.as_str() {
                    "TRUE" => {
                        self.bump();
                        Ok(Value::Boolean(true))
                    }
                    "FALSE" => {
                        self.bump();
                        Ok(Value::Boolean(false))
                    }
                    "NULL" => {
                        self.bump();
                        Ok(Value::Null)
                    }
                    _ => {
                        self.bump();
                        Ok(Value::Reference(Spanned::new(name, span)))
                    }
                }
            }
            TokKind::LBrace => self.parse_brace_value(),
            other => Err(ParseError::new(
                format!("expected value, got {}", describe(other)),
                self.peek().span,
            )),
        }
    }

    fn parse_brace_value(&mut self) -> Result<Value, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        // Try: sequence/set value `{ name value, ... }`, OR sequence-of value `{ v, v, ... }`,
        // OR OID value `{ item item item }`.
        if matches!(self.peek().kind, TokKind::RBrace) {
            self.bump();
            return Ok(Value::SequenceOf(Vec::new()));
        }
        // Heuristic: if two identifiers in a row (possibly with `(value)`) appear without
        // a comma, treat as an OID value. Otherwise try named-value vs. positional.
        // POIM OID values always appear as module OID bodies, which go through parse_oid_body
        // directly; value assignments we care about are integer or named references.
        // We therefore try: parse first element; if followed by `,` or `}` it's a list of values;
        // if the first is an ident followed by a value it's a record; else OID-like.
        let save = self.pos;
        if let TokKind::Ident(_) = self.peek().kind {
            let name = self.expect_any_ident()?;
            if matches!(self.peek().kind, TokKind::LParen)
                || matches!(self.peek().kind, TokKind::RBrace)
            {
                // OID-style component: "name (n)" or just "name"
                self.pos = save;
                return self.parse_oid_value_body();
            }
            if matches!(self.peek().kind, TokKind::Comma)
                || matches!(self.peek().kind, TokKind::RBrace)
            {
                // Single-item list containing a reference.
                let first = Value::Reference(name);
                let mut items = vec![first];
                while matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    items.push(self.parse_value()?);
                }
                self.expect_kind(&TokKind::RBrace, "`}`")?;
                return Ok(Value::SequenceOf(items));
            }
            // Named record field: `name value, ...`
            let value = self.parse_value()?;
            let mut fields = vec![(name, value)];
            while matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                let n = self.expect_any_ident()?;
                let v = self.parse_value()?;
                fields.push((n, v));
            }
            self.expect_kind(&TokKind::RBrace, "`}`")?;
            return Ok(Value::Sequence(fields));
        }
        // Positional list of values.
        let mut items = vec![self.parse_value()?];
        while matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
            items.push(self.parse_value()?);
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(Value::SequenceOf(items))
    }

    fn parse_oid_value_body(&mut self) -> Result<Value, ParseError> {
        let mut comps = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace) {
            let start = self.peek().span;
            let comp = match &self.peek().kind {
                TokKind::Ident(_) => {
                    let name = self.expect_any_ident()?;
                    if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump();
                        let value = match &self.peek().kind {
                            TokKind::Number(n) => {
                                let v = n.parse::<i64>().map_err(|_| {
                                    ParseError::new("bad OID value", self.peek().span)
                                })?;
                                self.bump();
                                Some(v)
                            }
                            _ => None,
                        };
                        self.expect_kind(&TokKind::RParen, "`)`")?;
                        OidComponent { name: Some(name), value, span: start.join(self.prev_span()) }
                    } else {
                        OidComponent { name: Some(name.clone()), value: None, span: name.span }
                    }
                }
                TokKind::Number(n) => {
                    let v = n
                        .parse::<i64>()
                        .map_err(|_| ParseError::new("bad OID number", self.peek().span))?;
                    let span = self.peek().span;
                    self.bump();
                    OidComponent { name: None, value: Some(v), span }
                }
                _ => {
                    return Err(ParseError::new(
                        format!("unexpected {} in OID value", describe(&self.peek().kind)),
                        self.peek().span,
                    ));
                }
            };
            comps.push(comp);
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(Value::Oid(comps))
    }

    // ---------------------------------------------------------------------
    // Information object classes / sets
    // ---------------------------------------------------------------------

    fn parse_object_class(&mut self) -> Result<ObjectClass, ParseError> {
        let start = self.expect_ident_word("CLASS")?.span;
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut fields = Vec::new();
        loop {
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            fields.push(self.parse_field_spec()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;

        let syntax = if self.eat_ident("WITH") {
            self.expect_ident_word("SYNTAX")?;
            Some(self.parse_class_syntax()?)
        } else {
            None
        };

        let end = self.prev_span();
        Ok(ObjectClass { fields, syntax, span: start.join(end) })
    }

    fn parse_field_spec(&mut self) -> Result<FieldSpec, ParseError> {
        let start = self.expect_kind(&TokKind::Amp, "`&`")?.span;
        let name = self.expect_any_ident()?;
        let is_type_field =
            name.value.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);

        if is_type_field {
            // TypeField: `&Type` optionally followed by `OPTIONAL` or `DEFAULT Type`.
            let mut optional = false;
            let mut default = None;
            if self.eat_ident("OPTIONAL") {
                optional = true;
            } else if self.eat_ident("DEFAULT") {
                default = Some(self.parse_type()?);
            }
            let end = self.prev_span();
            return Ok(FieldSpec::TypeField { name, optional, default, span: start.join(end) });
        }

        // Value field: `&value Type [UNIQUE] [OPTIONAL | DEFAULT value]`
        // or variable-type-value field: `&value CLASS.&TypeField`
        if matches!(self.peek().kind, TokKind::Ident(_)) {
            // Could be a plain type name or a class reference used for variable-type-value field.
            let save = self.pos;
            let ident = self.expect_any_ident()?;
            if matches!(self.peek().kind, TokKind::Dot)
                && matches!(self.peek_at(1).kind, TokKind::Amp)
            {
                // Variable type value field
                let mut path = Vec::new();
                while matches!(self.peek().kind, TokKind::Dot) {
                    self.bump();
                    self.expect_kind(&TokKind::Amp, "`&`")?;
                    let f = self.expect_any_ident()?;
                    let is_type =
                        f.value.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
                    path.push(if is_type { FieldRef::Type(f) } else { FieldRef::Value(f) });
                }
                let optional = self.eat_ident("OPTIONAL");
                let end = self.prev_span();
                // Use ident as the class; we currently only care about the field reference shape.
                let _ = ident;
                return Ok(FieldSpec::VariableTypeValueField {
                    name,
                    field_path: path,
                    optional,
                    span: start.join(end),
                });
            }
            // Regular value field — reset and parse type properly so constraints attach.
            self.pos = save;
        }
        let ty = self.parse_type()?;
        let unique = self.eat_ident("UNIQUE");
        let mut optional = false;
        let mut default = None;
        if self.eat_ident("OPTIONAL") {
            optional = true;
        } else if self.eat_ident("DEFAULT") {
            default = Some(self.parse_value()?);
        }
        let end = self.prev_span();
        Ok(FieldSpec::ValueField { name, ty, unique, optional, default, span: start.join(end) })
    }

    fn parse_class_syntax(&mut self) -> Result<Vec<SyntaxToken>, ParseError> {
        self.expect_kind(&TokKind::LBrace, "`{`")?;
        let mut toks = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => break,
                TokKind::LBracket => {
                    self.bump();
                    let mut inner = Vec::new();
                    while !matches!(self.peek().kind, TokKind::RBracket) {
                        if matches!(self.peek().kind, TokKind::Eof) {
                            return Err(ParseError::new(
                                "unterminated optional-group in WITH SYNTAX",
                                self.peek().span,
                            ));
                        }
                        inner.push(self.parse_syntax_token()?);
                    }
                    self.expect_kind(&TokKind::RBracket, "`]`")?;
                    toks.push(SyntaxToken::Optional(inner));
                }
                _ => toks.push(self.parse_syntax_token()?),
            }
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        Ok(toks)
    }

    fn parse_syntax_token(&mut self) -> Result<SyntaxToken, ParseError> {
        if matches!(self.peek().kind, TokKind::Amp) {
            self.bump();
            let name = self.expect_any_ident()?;
            return Ok(SyntaxToken::FieldName(name));
        }
        let tok = self.bump();
        let word = match tok.kind {
            TokKind::Ident(s) => s,
            other => {
                return Err(ParseError::new(
                    format!("expected syntax literal or `&`, got {}", describe(&other)),
                    tok.span,
                ));
            }
        };
        Ok(SyntaxToken::Literal(word))
    }

    fn parse_object_set_body(&mut self) -> Result<ObjectSet, ParseError> {
        let start = self.expect_kind(&TokKind::LBrace, "`{`")?.span;
        let mut elements = Vec::new();
        let mut extensible = false;
        loop {
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::Ellipsis) {
                self.bump();
                extensible = true;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
            let el = if matches!(self.peek().kind, TokKind::LBrace) {
                let obj = self.parse_object_def()?;
                ObjectSetElement::Object(obj)
            } else {
                let r = self.expect_any_ident()?;
                ObjectSetElement::Reference(r)
            };
            elements.push(el);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        let end = self.prev_span();
        Ok(ObjectSet { elements, extensible, span: start.join(end) })
    }

    fn parse_object_def(&mut self) -> Result<ObjectDef, ParseError> {
        // We accept either `{ &field value, ... }` style (field-setting form) or
        // the defined-syntax form `{ Type IDENTIFIED BY name }` used by POIM.
        // To keep codegen moving, we capture the tokens as raw field settings or
        // skip when we can't match.
        let start = self.expect_kind(&TokKind::LBrace, "`{`")?.span;
        let mut fields = Vec::new();
        // Defined-syntax detection: starts with a type reference, then keywords.
        if let TokKind::Ident(s) = &self.peek().kind.clone() {
            if s.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                // Try to parse as `<Type> IDENTIFIED BY <value>` — POIM pattern.
                let ty_tok = self.expect_any_ident()?;
                if self.eat_ident("IDENTIFIED") && self.eat_ident("BY") {
                    let value_name = self.expect_any_ident()?;
                    fields.push(ObjectFieldSetting::Type {
                        name: Spanned::new("Type".to_string(), ty_tok.span),
                        ty: Type {
                            kind: TypeKind::Reference(ty_tok.clone()),
                            constraints: Vec::new(),
                            tag: None,
                            span: ty_tok.span,
                        },
                    });
                    fields.push(ObjectFieldSetting::Value {
                        name: Spanned::new("id".to_string(), value_name.span),
                        value: Value::Reference(value_name),
                    });
                    self.expect_kind(&TokKind::RBrace, "`}`")?;
                    let end = self.prev_span();
                    return Ok(ObjectDef { fields, span: start.join(end) });
                }
                // Otherwise fall through to generic skip.
            }
        }
        // Generic skip — consume tokens to the matching `}`.
        let mut depth = 1i32;
        while depth > 0 {
            match self.peek().kind {
                TokKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    self.bump();
                }
                TokKind::Eof => break,
                _ => {
                    self.bump();
                }
            }
        }
        self.expect_kind(&TokKind::RBrace, "`}`")?;
        let end = self.prev_span();
        Ok(ObjectDef { fields, span: start.join(end) })
    }
}

fn describe(kind: &TokKind) -> String {
    match kind {
        TokKind::Ident(s) => format!("`{}`", s),
        TokKind::Number(_) => "integer literal".to_string(),
        TokKind::Real(_) => "real literal".to_string(),
        TokKind::CString(_) => "character string".to_string(),
        TokKind::BString(_) => "bstring".to_string(),
        TokKind::HString(_) => "hstring".to_string(),
        TokKind::Doc(_) => "doc comment".to_string(),
        TokKind::Assign => "`::=`".to_string(),
        TokKind::Range => "`..`".to_string(),
        TokKind::Ellipsis => "`...`".to_string(),
        TokKind::LBrace => "`{`".to_string(),
        TokKind::RBrace => "`}`".to_string(),
        TokKind::LParen => "`(`".to_string(),
        TokKind::RParen => "`)`".to_string(),
        TokKind::LBracket => "`[`".to_string(),
        TokKind::RBracket => "`]`".to_string(),
        TokKind::DblLBracket => "`[[`".to_string(),
        TokKind::DblRBracket => "`]]`".to_string(),
        TokKind::Comma => "`,`".to_string(),
        TokKind::Semi => "`;`".to_string(),
        TokKind::Colon => "`:`".to_string(),
        TokKind::Dot => "`.`".to_string(),
        TokKind::At => "`@`".to_string(),
        TokKind::Amp => "`&`".to_string(),
        TokKind::Pipe => "`|`".to_string(),
        TokKind::Excl => "`!`".to_string(),
        TokKind::Caret => "`^`".to_string(),
        TokKind::Lt => "`<`".to_string(),
        TokKind::Gt => "`>`".to_string(),
        TokKind::Hyphen => "`-`".to_string(),
        TokKind::Plus => "`+`".to_string(),
        TokKind::Star => "`*`".to_string(),
        TokKind::Eof => "end of input".to_string(),
    }
}
