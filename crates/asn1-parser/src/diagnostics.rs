//! Span-aware diagnostics for ASN.1 parsing.
//!
//! A `Span` is a byte range within a known source file. The parser attaches spans
//! to every syntactic node so the IR and downstream tools can render precise
//! error messages (`file:line:col`) without re-lexing.

use std::fmt;
use std::path::{Path, PathBuf};

pub type FileId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const DUMMY: Span = Span { file: 0, start: 0, end: 0 };

    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Self { file, start: start as u32, end: end as u32 }
    }

    pub fn join(self, other: Span) -> Span {
        debug_assert_eq!(self.file, other.file, "cannot join spans across files");
        Span { file: self.file, start: self.start.min(other.start), end: self.end.max(other.end) }
    }

    pub fn is_dummy(self) -> bool {
        self == Self::DUMMY
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned { value: f(self.value), span: self.span }
    }
}

impl<T: fmt::Display> fmt::Display for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value.fmt(f)
    }
}

/// A collection of source files indexed by `FileId`.
///
/// A parsed module carries spans that reference files here; downstream tooling
/// uses the map to translate byte offsets into `file:line:column` locations.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: FileId,
    pub path: PathBuf,
    pub source: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    fn new(id: FileId, path: PathBuf, source: String) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in source.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { id, path, source, line_starts }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.line_starts[line_idx];
        let col = self.source[line_start..offset.min(self.source.len())].chars().count();
        (line_idx + 1, col + 1)
    }

    pub fn line_text(&self, line: usize) -> &str {
        if line == 0 || line > self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[line - 1];
        let end = if line < self.line_starts.len() {
            self.line_starts[line] - 1
        } else {
            self.source.len()
        };
        self.source[start..end].trim_end_matches('\r')
    }
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, path: impl Into<PathBuf>, source: String) -> FileId {
        let id = self.files.len() as FileId;
        self.files.push(SourceFile::new(id, path.into(), source));
        id
    }

    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id as usize)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn location(&self, span: Span) -> Option<Location<'_>> {
        let file = self.get(span.file)?;
        let (line, col) = file.line_col(span.start as usize);
        Some(Location { path: &file.path, line, col })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Location<'a> {
    pub path: &'a Path,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Location<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.path.display(), self.line, self.col)
    }
}

/// Parse / resolve error with an optional source span.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ParseError {
    pub message: String,
    pub span: Span,
    pub notes: Vec<(String, Span)>,
}

impl ParseError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self { message: message.into(), span, notes: Vec::new() }
    }

    pub fn with_note(mut self, note: impl Into<String>, span: Span) -> Self {
        self.notes.push((note.into(), span));
        self
    }

    /// Renders the error with source context if the span's file exists in `sources`.
    pub fn render(&self, sources: &SourceMap) -> String {
        let mut out = String::new();
        render_one(&mut out, "error", &self.message, self.span, sources);
        for (msg, span) in &self.notes {
            render_one(&mut out, "note", msg, *span, sources);
        }
        out
    }
}

fn render_one(out: &mut String, level: &str, msg: &str, span: Span, sources: &SourceMap) {
    use std::fmt::Write;
    if span.is_dummy() || sources.get(span.file).is_none() {
        writeln!(out, "{level}: {msg}").ok();
        return;
    }
    let file = sources.get(span.file).unwrap();
    let (line, col) = file.line_col(span.start as usize);
    let line_text = file.line_text(line);
    writeln!(out, "{level}: {msg}").ok();
    writeln!(out, "  --> {}:{}:{}", file.path.display(), line, col).ok();
    writeln!(out, "   |").ok();
    writeln!(out, "{line:>3}| {line_text}").ok();
    let caret_len = (span.end.saturating_sub(span.start)).max(1) as usize;
    let padding = col.saturating_sub(1);
    writeln!(out, "   | {}{}", " ".repeat(padding), "^".repeat(caret_len)).ok();
}
