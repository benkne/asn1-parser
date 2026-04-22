//! ASN.1 lexer.
//!
//! Produces a flat token stream the grammar then consumes. Comments are handled
//! here: `--` line comments and `/* */` block comments are discarded, while
//! `/** */` doc comments are kept as tokens so they can be attached to the next
//! assignment.
//!
//! Identifier rules follow X.680: a letter followed by letters, digits, and
//! internal single hyphens. Case of the leading letter determines whether the
//! identifier is a type-reference (uppercase) or a value-reference (lowercase)
//! — the distinction is made at the grammar level, not here.

use crate::diagnostics::{FileId, ParseError, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    /// Alphabetic token; the grammar disambiguates keyword / type-ref / value-ref.
    Ident(String),
    /// Unsigned integer literal (no sign; `-` is a separate token).
    Number(String),
    /// Real number literal (contains `.` or exponent).
    Real(String),
    /// `"..."` character string.
    CString(String),
    /// `'0101'B` binary string.
    BString(String),
    /// `'ABCD'H` hexadecimal string.
    HString(String),
    /// `/** ... */` doc comment, contents with delimiters stripped and leading `*` removed.
    Doc(String),

    Assign,   // ::=
    Range,    // ..
    Ellipsis, // ...
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    DblLBracket, // [[
    DblRBracket, // ]]
    Comma,
    Semi,
    Colon,
    Dot,
    At,
    Amp,
    Pipe,
    Excl,
    Caret,
    Lt,
    Gt,
    Hyphen,
    Plus,
    Star,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    file: FileId,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(file: FileId, src: &'a str) -> Self {
        Self { file, bytes: src.as_bytes(), pos: 0 }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, ParseError> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace_and_comments(&mut out)?;
            if self.pos >= self.bytes.len() {
                out.push(Token { kind: TokKind::Eof, span: self.span(self.pos, self.pos) });
                return Ok(out);
            }

            let start = self.pos;
            let b = self.bytes[self.pos];
            let kind = match b {
                b'{' => self.single(TokKind::LBrace),
                b'}' => self.single(TokKind::RBrace),
                b'(' => self.single(TokKind::LParen),
                b')' => self.single(TokKind::RParen),
                b',' => self.single(TokKind::Comma),
                b';' => self.single(TokKind::Semi),
                b'@' => self.single(TokKind::At),
                b'&' => self.single(TokKind::Amp),
                b'|' => self.single(TokKind::Pipe),
                b'!' => self.single(TokKind::Excl),
                b'^' => self.single(TokKind::Caret),
                b'<' => self.single(TokKind::Lt),
                b'>' => self.single(TokKind::Gt),
                b'+' => self.single(TokKind::Plus),
                b'*' => self.single(TokKind::Star),
                b'[' => {
                    self.pos += 1;
                    if self.peek(0) == Some(b'[') {
                        self.pos += 1;
                        TokKind::DblLBracket
                    } else {
                        TokKind::LBracket
                    }
                }
                b']' => {
                    self.pos += 1;
                    if self.peek(0) == Some(b']') {
                        self.pos += 1;
                        TokKind::DblRBracket
                    } else {
                        TokKind::RBracket
                    }
                }
                b':' => {
                    if self.peek(1) == Some(b':') && self.peek(2) == Some(b'=') {
                        self.pos += 3;
                        TokKind::Assign
                    } else {
                        self.pos += 1;
                        TokKind::Colon
                    }
                }
                b'.' => {
                    let dots = self.bytes[self.pos..].iter().take_while(|&&c| c == b'.').count();
                    if dots >= 3 {
                        self.pos += 3;
                        TokKind::Ellipsis
                    } else if dots == 2 {
                        self.pos += 2;
                        TokKind::Range
                    } else {
                        self.pos += 1;
                        TokKind::Dot
                    }
                }
                b'-' => {
                    // `--` comments are consumed in skip_whitespace_and_comments; if we
                    // still see `-` here it is a minus sign.
                    self.pos += 1;
                    TokKind::Hyphen
                }
                b'"' => self.read_cstring(start)?,
                b'\'' => self.read_bh_string(start)?,
                b if b.is_ascii_alphabetic() => self.read_identifier()?,
                b if b.is_ascii_digit() => self.read_number(start)?,
                _ => {
                    return Err(ParseError::new(
                        format!("unexpected character {:?}", b as char),
                        self.span(start, start + 1),
                    ));
                }
            };
            let span = self.span(start, self.pos);
            out.push(Token { kind, span });
        }
    }

    fn single(&mut self, k: TokKind) -> TokKind {
        self.pos += 1;
        k
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn skip_whitespace_and_comments(&mut self, out: &mut Vec<Token>) -> Result<(), ParseError> {
        loop {
            while let Some(&b) = self.bytes.get(self.pos) {
                if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0B | 0x0C) {
                    self.pos += 1;
                } else {
                    break;
                }
            }

            if self.pos + 1 < self.bytes.len()
                && self.bytes[self.pos] == b'-'
                && self.bytes[self.pos + 1] == b'-'
            {
                // Line comment: ends at next `--` on the same line or at newline.
                self.pos += 2;
                while self.pos < self.bytes.len() {
                    let b = self.bytes[self.pos];
                    if b == b'\n' {
                        self.pos += 1;
                        break;
                    }
                    if b == b'-' && self.peek(1) == Some(b'-') {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }

            if self.pos + 2 < self.bytes.len()
                && self.bytes[self.pos] == b'/'
                && self.bytes[self.pos + 1] == b'*'
                && self.bytes[self.pos + 2] == b'*'
                && self.peek(3) != Some(b'/')
            {
                let start = self.pos;
                self.pos += 3;
                let content_start = self.pos;
                let mut end_content = content_start;
                while self.pos + 1 < self.bytes.len() {
                    if self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/' {
                        end_content = self.pos;
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                if end_content == content_start && self.pos >= self.bytes.len() {
                    return Err(ParseError::new(
                        "unterminated doc comment",
                        self.span(start, self.pos),
                    ));
                }
                let raw =
                    std::str::from_utf8(&self.bytes[content_start..end_content]).map_err(|_| {
                        ParseError::new(
                            "doc comment is not valid UTF-8",
                            self.span(start, self.pos),
                        )
                    })?;
                let cleaned = clean_doc_comment(raw);
                let span = self.span(start, self.pos);
                out.push(Token { kind: TokKind::Doc(cleaned), span });
                continue;
            }

            if self.pos + 1 < self.bytes.len()
                && self.bytes[self.pos] == b'/'
                && self.bytes[self.pos + 1] == b'*'
            {
                let start = self.pos;
                self.pos += 2;
                let mut depth = 1;
                while self.pos + 1 < self.bytes.len() && depth > 0 {
                    let a = self.bytes[self.pos];
                    let b = self.bytes[self.pos + 1];
                    if a == b'*' && b == b'/' {
                        depth -= 1;
                        self.pos += 2;
                    } else if a == b'/' && b == b'*' {
                        depth += 1;
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                    }
                }
                if depth != 0 {
                    return Err(ParseError::new(
                        "unterminated block comment",
                        self.span(start, self.pos),
                    ));
                }
                continue;
            }

            break;
        }
        Ok(())
    }

    fn read_identifier(&mut self) -> Result<TokKind, ParseError> {
        let start = self.pos;
        self.pos += 1;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_alphanumeric() {
                self.pos += 1;
            } else if b == b'-' {
                // Include hyphen only if followed by alphanumeric and it is not `--`.
                if self.peek(1).map(|c| c.is_ascii_alphanumeric()).unwrap_or(false) {
                    self.pos += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| {
                ParseError::new("invalid UTF-8 in identifier", self.span(start, self.pos))
            })?
            .to_owned();
        Ok(TokKind::Ident(s))
    }

    fn read_number(&mut self, start: usize) -> Result<TokKind, ParseError> {
        while let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        let mut is_real = false;
        if self.peek(0) == Some(b'.') && self.peek(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            is_real = true;
            self.pos += 1;
            while let Some(&b) = self.bytes.get(self.pos) {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        if matches!(self.peek(0), Some(b'e') | Some(b'E')) {
            is_real = true;
            self.pos += 1;
            if matches!(self.peek(0), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while let Some(&b) = self.bytes.get(self.pos) {
                if b.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| ParseError::new("invalid UTF-8 in number", self.span(start, self.pos)))?
            .to_owned();
        Ok(if is_real { TokKind::Real(text) } else { TokKind::Number(text) })
    }

    fn read_cstring(&mut self, start: usize) -> Result<TokKind, ParseError> {
        self.pos += 1;
        let content_start = self.pos;
        let mut buf = String::new();
        loop {
            match self.bytes.get(self.pos) {
                None => {
                    return Err(ParseError::new(
                        "unterminated character string",
                        self.span(start, self.pos),
                    ));
                }
                Some(&b'"') => {
                    // `""` is an escaped double quote.
                    if self.peek(1) == Some(b'"') {
                        buf.push('"');
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        break;
                    }
                }
                Some(_) => {
                    let c = std::str::from_utf8(&self.bytes[self.pos..])
                        .map_err(|_| {
                            ParseError::new(
                                "invalid UTF-8 in character string",
                                self.span(start, self.pos),
                            )
                        })?
                        .chars()
                        .next()
                        .unwrap();
                    buf.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
        let _ = content_start;
        Ok(TokKind::CString(buf))
    }

    fn read_bh_string(&mut self, start: usize) -> Result<TokKind, ParseError> {
        self.pos += 1;
        let content_start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == b'\'' {
                break;
            }
            self.pos += 1;
        }
        if self.bytes.get(self.pos) != Some(&b'\'') {
            return Err(ParseError::new(
                "unterminated binary/hex string",
                self.span(start, self.pos),
            ));
        }
        let content_end = self.pos;
        self.pos += 1;
        let tag = self.bytes.get(self.pos).copied().ok_or_else(|| {
            ParseError::new("missing B/H tag on bstring/hstring", self.span(start, self.pos))
        })?;
        self.pos += 1;
        let raw = std::str::from_utf8(&self.bytes[content_start..content_end]).map_err(|_| {
            ParseError::new("invalid UTF-8 in bstring/hstring", self.span(start, self.pos))
        })?;
        let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        match tag {
            b'B' | b'b' => {
                if !cleaned.chars().all(|c| c == '0' || c == '1') {
                    return Err(ParseError::new(
                        "non-binary digit in bstring",
                        self.span(start, self.pos),
                    ));
                }
                Ok(TokKind::BString(cleaned))
            }
            b'H' | b'h' => {
                if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(ParseError::new(
                        "non-hex digit in hstring",
                        self.span(start, self.pos),
                    ));
                }
                Ok(TokKind::HString(cleaned))
            }
            _ => Err(ParseError::new(
                format!("expected 'B' or 'H' after quoted string, got {:?}", tag as char),
                self.span(start, self.pos),
            )),
        }
    }
}

fn clean_doc_comment(raw: &str) -> String {
    // Strip leading `*` and surrounding whitespace from each line, collapse the rest.
    let mut lines: Vec<&str> = raw.split('\n').collect();
    for line in lines.iter_mut() {
        let trimmed = line.trim();
        *line =
            if let Some(rest) = trimmed.strip_prefix('*') { rest.trim_start() } else { trimmed };
    }
    while lines.first().map(|l| l.is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        Lexer::new(0, src).tokenize().unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn line_comments_skipped() {
        let k = kinds("-- hello\nFoo ::= INTEGER -- trailing\n");
        assert_eq!(
            k,
            vec![
                TokKind::Ident("Foo".into()),
                TokKind::Assign,
                TokKind::Ident("INTEGER".into()),
                TokKind::Eof,
            ]
        );
    }

    #[test]
    fn hyphen_inside_identifier() {
        let k = kinds("POIM-PDU-Description");
        assert_eq!(k, vec![TokKind::Ident("POIM-PDU-Description".into()), TokKind::Eof]);
    }

    #[test]
    fn range_vs_ellipsis_vs_dot() {
        let k = kinds("a..b...c.d");
        assert_eq!(
            k,
            vec![
                TokKind::Ident("a".into()),
                TokKind::Range,
                TokKind::Ident("b".into()),
                TokKind::Ellipsis,
                TokKind::Ident("c".into()),
                TokKind::Dot,
                TokKind::Ident("d".into()),
                TokKind::Eof,
            ]
        );
    }

    #[test]
    fn doc_comment_captured() {
        let k = kinds("/** a\n * b\n*/\nFoo ::= INTEGER");
        assert!(matches!(k[0], TokKind::Doc(ref s) if s.contains("a") && s.contains("b")));
    }

    #[test]
    fn cstring_with_embedded_quote() {
        let k = kinds(r#""a""b""#);
        assert_eq!(k, vec![TokKind::CString("a\"b".into()), TokKind::Eof]);
    }

    #[test]
    fn bstring_and_hstring() {
        let k = kinds("'0101'B 'ABcd'H");
        assert_eq!(
            k,
            vec![TokKind::BString("0101".into()), TokKind::HString("ABcd".into()), TokKind::Eof]
        );
    }
}
