//! Parse asn1-decoder doc comments into typed segments and render them.
//!
//! ASN.1 specs in this corpus follow a Javadoc-like convention. Recognised
//! line-starting tags (case-insensitive):
//!
//! | tag                       | rendered as
//! |---------------------------|---------------------------
//! | `@field NAME[: ...]`      | row in a 2-column field grid
//! | `@note[: ...]`            | extra prose paragraph
//! | `@category[: ...]`        | chip
//! | `@revision[: ...]`        | chip
//! | `@unit[: ...]` (`@units`) | chip
//! | any other `@tag[: ...]`   | falls through to an "extra" bucket
//!
//! Continuation rule: a line whose first non-whitespace character is not `@`
//! extends the previous segment's body (joined with a single space, since the
//! ASN.1 doc lines are usually re-flowed prose). Blank lines split the leading
//! prose into paragraphs.
//!
//! `@ref TARGET` is recognised *inline* within bodies — it is the only tag
//! that appears mid-sentence in the corpus. The HTML renderer wraps it in
//! `<span class="doc-ref">→ TARGET</span>`; the egui renderer rewrites it to
//! `→ TARGET` in plain text.

use std::fmt::Write as _;

use crate::html::html_escape;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DocBlock {
    /// Leading prose paragraphs plus any `@note` bodies, in source order.
    pub intro: Vec<String>,
    /// `(name, body)` pairs from `@field` rows.
    pub fields: Vec<(String, String)>,
    /// Single-value chips: `@category`, `@revision`, `@unit`.
    pub chips: Vec<(ChipKind, String)>,
    /// Catch-all for unrecognised `@<tag>` lines so nothing is dropped.
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipKind {
    Category,
    Revision,
    Unit,
}

impl ChipKind {
    fn label(self) -> &'static str {
        match self {
            ChipKind::Category => "category",
            ChipKind::Revision => "revision",
            ChipKind::Unit => "unit",
        }
    }
}

impl DocBlock {
    pub fn is_empty(&self) -> bool {
        self.intro.is_empty()
            && self.fields.is_empty()
            && self.chips.is_empty()
            && self.extra.is_empty()
    }
}

enum Bucket {
    Intro,
    Field(String),
    Chip(ChipKind),
    Extra(String),
}

pub(crate) fn parse(doc: &str) -> DocBlock {
    let mut block = DocBlock::default();
    let mut current: Option<(Bucket, String)> = None;
    // Buffer for prose appearing before any tagged segment, accumulated into
    // paragraphs separated by blank lines.
    let mut leading: Vec<String> = Vec::new();
    let mut leading_para = String::new();

    for line in doc.lines() {
        let trimmed = line.trim();

        if let Some(m) = match_tag(trimmed) {
            // Starting a new tagged segment — flush whatever we were filling.
            push_leading(&mut leading, &mut leading_para);
            flush(&mut current, &mut block);
            current = Some((m.bucket, m.body.to_string()));
            continue;
        }

        if trimmed.is_empty() {
            // Paragraph break.
            if current.is_some() {
                // Inside a tagged segment, blank lines just collapse to a single
                // space — most @note/@field bodies are short prose with no real
                // paragraph structure. We don't try to recover paragraphs here.
            } else {
                push_leading(&mut leading, &mut leading_para);
            }
            continue;
        }

        // Continuation line. List-item starts (`- foo`, `* foo`, `1. foo`)
        // are joined with a hard newline so bullet/numbered lists stay
        // visually separated; everything else joins with a space so prose
        // wrapped at column 80 in the source still re-flows naturally.
        let join: char = if is_list_item_start(trimmed) { '\n' } else { ' ' };
        match &mut current {
            Some((_, body)) => {
                if !body.is_empty() {
                    body.push(join);
                }
                body.push_str(trimmed);
            }
            None => {
                if !leading_para.is_empty() {
                    leading_para.push(join);
                }
                leading_para.push_str(trimmed);
            }
        }
    }

    push_leading(&mut leading, &mut leading_para);
    flush(&mut current, &mut block);

    // Splice leading paragraphs in front of any @note bodies that landed in intro.
    if !leading.is_empty() {
        let mut combined = leading;
        combined.append(&mut block.intro);
        block.intro = combined;
    }

    block
}

fn push_leading(out: &mut Vec<String>, para: &mut String) {
    let trimmed = para.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    para.clear();
}

fn flush(cur: &mut Option<(Bucket, String)>, block: &mut DocBlock) {
    let Some((bucket, body)) = cur.take() else { return };
    let body = body.trim().to_string();
    match bucket {
        Bucket::Intro => {
            if !body.is_empty() {
                block.intro.push(body);
            }
        }
        Bucket::Field(name) => block.fields.push((name, body)),
        Bucket::Chip(kind) => block.chips.push((kind, body)),
        Bucket::Extra(tag) => block.extra.push((tag, body)),
    }
}

struct TagMatch<'a> {
    bucket: Bucket,
    body: &'a str,
}

fn match_tag(line: &str) -> Option<TagMatch<'_>> {
    let after_at = line.strip_prefix('@')?;
    let tag_end =
        after_at.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after_at.len());
    if tag_end == 0 {
        return None;
    }
    let tag = &after_at[..tag_end];
    let mut rest = after_at[tag_end..].trim_start();
    let lower = tag.to_ascii_lowercase();

    let bucket = match lower.as_str() {
        "field" => {
            // `@field NAME[: body]` — the name token may begin with `&` for
            // information-object-class fields like `@field &Type:`.
            let name_end = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len());
            if name_end == 0 {
                // Malformed `@field` with no name — keep as extra so it surfaces.
                rest = strip_optional_colon(rest);
                return Some(TagMatch { bucket: Bucket::Extra(tag.to_string()), body: rest });
            }
            let name = rest[..name_end].to_string();
            rest = rest[name_end..].trim_start();
            rest = strip_optional_colon(rest);
            Bucket::Field(name)
        }
        "note" => {
            rest = strip_optional_colon(rest);
            Bucket::Intro
        }
        "category" => {
            rest = strip_optional_colon(rest);
            Bucket::Chip(ChipKind::Category)
        }
        "revision" => {
            rest = strip_optional_colon(rest);
            Bucket::Chip(ChipKind::Revision)
        }
        "unit" | "units" => {
            rest = strip_optional_colon(rest);
            Bucket::Chip(ChipKind::Unit)
        }
        _ => {
            rest = strip_optional_colon(rest);
            Bucket::Extra(tag.to_string())
        }
    };
    Some(TagMatch { bucket, body: rest })
}

fn strip_optional_colon(s: &str) -> &str {
    s.strip_prefix(':').map(str::trim_start).unwrap_or(s)
}

/// `true` when `line` opens a markdown-style list item — `- foo`, `* foo`, or
/// `1. foo`. Used by the continuation logic so bullet lists keep their
/// per-item line breaks instead of collapsing into a wall of text.
fn is_list_item_start(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'-' || bytes[0] == b'*') && bytes[1] == b' ' {
        return true;
    }
    // Numbered: one-or-more digits, then `.`, then a space.
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    i > 0 && i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1] == b' '
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

pub(crate) fn render_html(out: &mut String, doc: &str) {
    let block = parse(doc);
    if block.is_empty() {
        return;
    }
    out.push_str("<div class=\"doc\">\n");
    for para in &block.intro {
        out.push_str("<p class=\"doc-text\">");
        out.push_str(&inline_html(para));
        out.push_str("</p>\n");
    }
    if !block.fields.is_empty() {
        out.push_str("<dl class=\"doc-fields\">\n");
        for (name, body) in &block.fields {
            let _ = writeln!(out, "<dt>{}</dt><dd>{}</dd>", html_escape(name), inline_html(body),);
        }
        out.push_str("</dl>\n");
    }
    if !block.chips.is_empty() {
        out.push_str("<div class=\"doc-chips\">\n");
        for (kind, body) in &block.chips {
            let label = kind.label();
            let _ = writeln!(
                out,
                "<span class=\"doc-chip doc-chip-{label}\"><b>@{label}</b> {}</span>",
                inline_html(body),
            );
        }
        out.push_str("</div>\n");
    }
    for (tag, body) in &block.extra {
        let _ = writeln!(
            out,
            "<div class=\"doc-extra\"><b>@{}</b> {}</div>",
            html_escape(tag),
            inline_html(body),
        );
    }
    out.push_str("</div>\n");
}

/// HTML-escape `s` and stylise inline `@ref TARGET` occurrences as
/// `<span class="doc-ref">→ TARGET</span>`.
fn inline_html(s: &str) -> String {
    // Escape first, then transform — `@`, `→`, and identifier chars survive
    // html_escape unchanged, so the tag-and-target spans are easy to find.
    let escaped = html_escape(s);
    let mut out = String::with_capacity(escaped.len());
    let mut chars = escaped.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' && peek_word_eq_ignore_case(&chars, "ref") {
            // Consume "ref" and the trailing whitespace.
            for _ in 0..3 {
                chars.next();
            }
            // Skip exactly one whitespace char (the separator).
            if chars.peek().is_some_and(|p| p.is_whitespace()) {
                chars.next();
                let mut target = String::new();
                while let Some(&p) = chars.peek() {
                    if is_ref_terminator(p) {
                        break;
                    }
                    target.push(p);
                    chars.next();
                }
                if !target.is_empty() {
                    let _ = write!(out, "<span class=\"doc-ref\">→ {target}</span>");
                    continue;
                }
                // Empty target — emit the literal `@ref ` we consumed.
                out.push_str("@ref ");
                continue;
            }
            // No whitespace after `@ref` — emit raw.
            out.push_str("@ref");
            continue;
        }
        out.push(c);
    }
    out
}

fn peek_word_eq_ignore_case(
    chars: &std::iter::Peekable<std::str::Chars<'_>>,
    expect: &str,
) -> bool {
    let mut clone = chars.clone();
    for ec in expect.chars() {
        match clone.next() {
            Some(c) if c.eq_ignore_ascii_case(&ec) => {}
            _ => return false,
        }
    }
    // Word boundary check — next char (if any) must not be alphanumeric.
    !matches!(clone.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
}

fn is_ref_terminator(c: char) -> bool {
    c.is_whitespace() || matches!(c, ')' | ',' | ';' | '<' | '(' | '`')
}

// ---------------------------------------------------------------------------
// egui rendering
// ---------------------------------------------------------------------------

/// Render a parsed doc block into the supplied `egui::Ui`. `id_seed` keeps the
/// internal `Grid` id unique within the parent scope when several doc blocks
/// are rendered side-by-side (e.g. a field's own doc plus its referent's doc).
pub(crate) fn render_egui(ui: &mut egui::Ui, doc: &str, id_seed: &str) {
    let block = parse(doc);
    if block.is_empty() {
        return;
    }
    let accent = ui.visuals().hyperlink_color;

    for para in &block.intro {
        // Italics alone differentiate intro prose from labels; muting the
        // colour as well makes it hard to read against the panel background.
        ui.label(egui::RichText::new(transform_refs_plain(para)).italics());
    }

    if !block.fields.is_empty() {
        egui::Grid::new(format!("docfmt-fields-{id_seed}"))
            .num_columns(2)
            .spacing([10.0, 2.0])
            .striped(false)
            .show(ui, |ui| {
                for (name, body) in &block.fields {
                    ui.label(egui::RichText::new(name).monospace().color(accent));
                    ui.label(transform_refs_plain(body));
                    ui.end_row();
                }
            });
    }

    if !block.chips.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (kind, body) in &block.chips {
                chip(ui, kind.label(), body, accent);
            }
        });
    }

    for (tag, body) in &block.extra {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("@{tag}")).strong().color(accent));
            ui.label(transform_refs_plain(body));
        });
    }
}

fn chip(ui: &mut egui::Ui, label: &str, body: &str, accent: egui::Color32) {
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color))
        .rounding(egui::Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(6.0, 1.0))
        .fill(ui.visuals().faint_bg_color)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(egui::RichText::new(format!("@{label}")).strong().color(accent));
            ui.label(transform_refs_plain(body));
        });
}

/// Plain-text version of the `@ref TARGET` rewrite used by HTML's
/// `inline_html`: `→ TARGET`. Relies on the symbol-fallback font that
/// `install_symbol_fallback_font` registers at startup so the arrow glyph
/// renders.
fn transform_refs_plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' && peek_word_eq_ignore_case(&chars, "ref") {
            for _ in 0..3 {
                chars.next();
            }
            if chars.peek().is_some_and(|p| p.is_whitespace()) {
                chars.next();
                let mut target = String::new();
                while let Some(&p) = chars.peek() {
                    if is_ref_terminator(p) {
                        break;
                    }
                    target.push(p);
                    chars.next();
                }
                if !target.is_empty() {
                    out.push('→');
                    out.push(' ');
                    out.push_str(&target);
                    continue;
                }
                out.push_str("@ref ");
                continue;
            }
            out.push_str("@ref");
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_doc_yields_empty_block() {
        let b = parse("");
        assert!(b.is_empty());
    }

    #[test]
    fn plain_prose_lands_in_intro() {
        let b = parse("a parking facility.\nIt has spaces.");
        assert_eq!(b.intro, vec!["a parking facility. It has spaces.".to_string()]);
        assert!(b.fields.is_empty());
        assert!(b.chips.is_empty());
    }

    #[test]
    fn blank_line_splits_intro_paragraphs() {
        let b = parse("first paragraph.\n\nsecond paragraph.");
        assert_eq!(b.intro, vec!["first paragraph.".to_string(), "second paragraph.".to_string()]);
    }

    #[test]
    fn field_rows_are_extracted() {
        let b = parse("@field id: the identifier.\n@field name: the display name.");
        assert_eq!(
            b.fields,
            vec![
                ("id".into(), "the identifier.".into()),
                ("name".into(), "the display name.".into()),
            ]
        );
    }

    #[test]
    fn field_continuation_joins_with_space() {
        let b = parse("@field id: the identifier of the\n  parking area.");
        assert_eq!(b.fields, vec![("id".into(), "the identifier of the parking area.".into())]);
    }

    #[test]
    fn ampersand_field_names_are_kept() {
        let b = parse("@field &id: the class identifier.");
        assert_eq!(b.fields, vec![("&id".into(), "the class identifier.".into())]);
    }

    #[test]
    fn category_revision_unit_become_chips() {
        let b = parse("@category: Road topology\n@revision: V2.2.1\n@unit: 0,01 metre");
        assert_eq!(
            b.chips,
            vec![
                (ChipKind::Category, "Road topology".into()),
                (ChipKind::Revision, "V2.2.1".into()),
                (ChipKind::Unit, "0,01 metre".into()),
            ]
        );
    }

    #[test]
    fn unit_alias_units_collapses_to_unit() {
        let b = parse("@units: m/s");
        assert_eq!(b.chips, vec![(ChipKind::Unit, "m/s".into())]);
    }

    #[test]
    fn unit_without_colon_is_accepted() {
        let b = parse("@unit 0,1 m/s^2");
        assert_eq!(b.chips, vec![(ChipKind::Unit, "0,1 m/s^2".into())]);
    }

    #[test]
    fn note_with_or_without_colon_lands_in_intro() {
        let b = parse("@note: a clarifying remark.\n@note another remark.");
        assert_eq!(
            b.intro,
            vec!["a clarifying remark.".to_string(), "another remark.".to_string()]
        );
    }

    #[test]
    fn tag_match_is_case_insensitive() {
        let b = parse("@Note big-N note.\n@CATEGORY: Loud");
        assert_eq!(b.intro, vec!["big-N note.".to_string()]);
        assert_eq!(b.chips, vec![(ChipKind::Category, "Loud".into())]);
    }

    #[test]
    fn unknown_tag_falls_through_to_extra() {
        let b = parse("@options: experimental");
        assert_eq!(b.extra, vec![("options".into(), "experimental".into())]);
    }

    #[test]
    fn leading_prose_then_tags_then_more_prose_preserves_order_within_buckets() {
        let b = parse("intro line one.\n@field x: ex.\n@note: appendix prose.");
        // Untagged leading prose comes first, then any @note bodies.
        assert_eq!(b.intro, vec!["intro line one.".to_string(), "appendix prose.".to_string()]);
        assert_eq!(b.fields, vec![("x".into(), "ex.".into())]);
    }

    #[test]
    fn inline_ref_html_wraps_in_span() {
        let html = inline_html("see the @ref CauseCode for details.");
        assert!(html.contains(r#"<span class="doc-ref">→ CauseCode</span>"#));
        // Surrounding text is preserved.
        assert!(html.starts_with("see the "));
        assert!(html.ends_with(" for details."));
    }

    #[test]
    fn inline_ref_html_handles_dotted_target() {
        let html = inline_html("see @ref Mod.Type after.");
        assert!(html.contains("→ Mod.Type"));
    }

    #[test]
    fn inline_ref_html_stops_at_punctuation() {
        let html = inline_html("see @ref Foo, then bar.");
        assert!(html.contains("→ Foo"));
        assert!(html.contains(", then bar."));
    }

    #[test]
    fn inline_ref_html_escapes_surrounding_html() {
        let html = inline_html("a < b & @ref Foo > c.");
        assert!(html.contains("&lt;"));
        assert!(html.contains("&amp;"));
        assert!(html.contains("&gt;"));
        assert!(html.contains("→ Foo"));
    }

    #[test]
    fn render_html_emits_structural_classes() {
        let mut out = String::new();
        render_html(
            &mut out,
            "intro paragraph.\n@field id: the id.\n@category: Topology\n@revision: V2.2.1",
        );
        assert!(out.contains(r#"class="doc""#));
        assert!(out.contains(r#"class="doc-text""#));
        assert!(out.contains(r#"class="doc-fields""#));
        assert!(out.contains("<dt>id</dt>"));
        assert!(out.contains(r#"class="doc-chips""#));
        assert!(out.contains(r#"class="doc-chip doc-chip-category""#));
        assert!(out.contains(r#"class="doc-chip doc-chip-revision""#));
    }

    #[test]
    fn dash_bullet_continuations_keep_newlines() {
        let b = parse(
            "@note: header line.\nThe value shall be set to:\n- 1 - foo\n- 2 - bar\n- 3 - baz",
        );
        assert_eq!(
            b.intro,
            vec!["header line. The value shall be set to:\n- 1 - foo\n- 2 - bar\n- 3 - baz"
                .to_string()]
        );
    }

    #[test]
    fn numbered_list_continuations_keep_newlines() {
        let b = parse("@note: choices:\n1. first\n2. second\n3. third");
        assert_eq!(b.intro, vec!["choices:\n1. first\n2. second\n3. third".to_string()]);
    }

    #[test]
    fn list_item_continuation_lines_still_join_with_space() {
        // A line that is NOT a list item, following one that is, should be
        // joined to the previous item with a space — it's prose continuation
        // of that item, not a new bullet.
        let b = parse("@note: header.\n- item one\n  with continuation\n- item two");
        assert_eq!(b.intro, vec!["header.\n- item one with continuation\n- item two".to_string()]);
    }

    #[test]
    fn render_html_skips_wrapper_when_doc_is_empty() {
        let mut out = String::new();
        render_html(&mut out, "");
        assert!(out.is_empty());
    }
}
