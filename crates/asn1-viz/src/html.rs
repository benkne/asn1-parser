//! Standalone HTML export.
//!
//! Mirrors the egui UI: each field can drill through reference aliases inline
//! (showing the referent's doc + body), primitive aliases reveal their named
//! numbers / bits / constraints, and cycles are cut off with a recursive
//! marker. A sticky header exposes creator / version info, expand-all /
//! collapse-all controls, and a Light / Dark / Grey theme picker whose choice
//! is persisted in `localStorage`.

use asn1_ir::{
    render_type, IrChoice, IrConstraint, IrField, IrItem, IrModule, IrOptionality, IrProgram,
    IrStruct, IrStructMember, IrType, IrTypeDef,
};

use crate::render_constraint;

/// Render the IR as a self-contained HTML document using `<details>` /
/// `<summary>` for native click-to-expand, requiring no external assets.
pub fn export_html(program: &IrProgram) -> String {
    let mut out = String::new();
    out.push_str(HTML_HEAD);
    out.push_str("<header>\n  <h1>asn1-tool</h1>\n");
    out.push_str(&format!("  <span class=\"info\">v{}</span>\n", env!("CARGO_PKG_VERSION"),));
    out.push_str(HTML_HEADER_CONTROLS);
    let type_total: usize = program.all_types().count();
    out.push_str(&format!(
        "<div class=\"meta\">{} module(s), {} type(s)</div>\n",
        program.modules.len(),
        type_total,
    ));
    html_diagnostics(&mut out, program);
    out.push_str(
        "<input type=\"search\" placeholder=\"Use browser find (Ctrl+F) to locate a type…\" aria-label=\"Type names are plain text; use the browser's find\">\n",
    );
    for m in &program.modules {
        html_module(&mut out, program, m);
    }
    out.push_str(HTML_TAIL);
    out
}

const HTML_HEAD: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>asn1-decoder — tree</title>
<script>
(function(){
  var t = null;
  try { t = localStorage.getItem('asn1-theme'); } catch (e) {}
  if (t !== 'light' && t !== 'dark' && t !== 'grey') {
    // Default to dark; only flip to light when the OS explicitly prefers it.
    // Using `(prefers-color-scheme: light)` instead of negating dark means
    // that "no preference" (which is also what browsers return on file://
    // when detection is blocked) stays on dark.
    t = 'dark';
    try {
      if (window.matchMedia('(prefers-color-scheme: light)').matches) t = 'light';
    } catch (e) {}
  }
  document.documentElement.setAttribute('data-theme', t);
})();
</script>
<style>
:root, [data-theme="dark"] {
    --bg: #0d1117; --fg: #e6edf3; --muted: #8d96a0;
    --kw: #79c0ff; --ty: #a5d6ff; --ext: #d29922;
    --hover: #21262d; --border: #30363d; --panel: #161b22;
    --input-bg: #0d1117; --input-border: #30363d;
    --recursive: #d29922; --unresolved: #ff7b72;
}
@media (prefers-color-scheme: light) {
    :root:not([data-theme]) {
        --bg: #ffffff; --fg: #1f2328; --muted: #656d76;
        --kw: #0550ae; --ty: #0a3069; --ext: #9a6700;
        --hover: #f6f8fa; --border: #eaecef; --panel: #f6f8fa;
        --input-bg: #ffffff; --input-border: #d0d7de;
        --recursive: #bf8700; --unresolved: #cf222e;
    }
}
[data-theme="light"] {
    --bg: #ffffff; --fg: #1f2328; --muted: #656d76;
    --kw: #0550ae; --ty: #0a3069; --ext: #9a6700;
    --hover: #f6f8fa; --border: #eaecef; --panel: #f6f8fa;
    --input-bg: #ffffff; --input-border: #d0d7de;
    --recursive: #bf8700; --unresolved: #cf222e;
}
[data-theme="dark"] {
    --bg: #0d1117; --fg: #e6edf3; --muted: #8d96a0;
    --kw: #79c0ff; --ty: #a5d6ff; --ext: #d29922;
    --hover: #21262d; --border: #30363d; --panel: #161b22;
    --input-bg: #0d1117; --input-border: #30363d;
    --recursive: #d29922; --unresolved: #ff7b72;
}
[data-theme="grey"] {
    --bg: #5e6166; --fg: #e6e6e6; --muted: #c8c8c8;
    --kw: #b0d8ff; --ty: #d4e6ff; --ext: #ffcc66;
    --hover: #6b6e73; --border: #4a4d52; --panel: #555558;
    --input-bg: #505356; --input-border: #6a6d72;
    --recursive: #ffcc66; --unresolved: #ffa0a0;
}
body { background: var(--bg); color: var(--fg); font: 14px/1.4 ui-sans-serif, system-ui, sans-serif; margin: 0; }
header { background: var(--panel); padding: .6rem 1.5rem; border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: .75rem; flex-wrap: wrap; position: sticky; top: 0; z-index: 10; }
header h1 { font-size: 1.1rem; margin: 0; }
header .info { color: var(--muted); font-size: .85rem; }
header .spacer { flex: 1; }
header select, header button { background: var(--input-bg); color: var(--fg); border: 1px solid var(--input-border); border-radius: 4px; padding: .2rem .5rem; font: inherit; cursor: pointer; }
header button:hover, header select:hover { background: var(--hover); }
main { padding: 1rem 2rem; }
.meta { color: var(--muted); margin-bottom: 1.25rem; }
details { margin: .1rem 0 .1rem .25rem; }
summary { cursor: pointer; list-style: none; padding: .1rem .25rem; border-radius: 3px; color: var(--fg); }
summary::-webkit-details-marker { display: none; }
summary::before { content: "▸"; display: inline-block; width: 1em; color: var(--muted); transition: transform .1s; }
details[open] > summary::before { transform: rotate(90deg); }
summary:hover { background: var(--hover); }
.leaf { padding: .1rem .25rem .1rem 1.25rem; }
.kw   { color: var(--kw); }
.name { font-weight: 600; }
.ty   { color: var(--ty); }
.note { color: var(--muted); font-style: italic; }
.ext  { color: var(--ext); }
.doc  { color: var(--muted); margin: .1rem 0 .3rem 1.5rem; white-space: pre-wrap; }
.target { color: var(--muted); font-style: italic; margin: .1rem 0 .2rem 1.5rem; }
.module > summary { font-weight: 700; font-size: 1.05rem; }
.module { margin-top: .6rem; border-top: 1px solid var(--border); padding-top: .4rem; }
a.tyref { color: var(--ty); text-decoration: none; border-bottom: 1px dashed var(--input-border); }
a.tyref:hover { background: var(--hover); }
input[type=search] { width: 100%; padding: .4rem; box-sizing: border-box; margin-bottom: .75rem; font: inherit; background: var(--input-bg); color: var(--fg); border: 1px solid var(--input-border); border-radius: 4px; }
.recursive { color: var(--recursive); font-style: italic; }
.unresolved { color: var(--ext); font-style: italic; }
.constraint { color: var(--muted); padding: .1rem 0 .1rem 1.25rem; }
.named { padding: .1rem 0 .1rem 1.25rem; }
.warnings { margin: 0 0 1rem 0; border: 1px solid var(--ext); border-radius: 4px; padding: .25rem .5rem; background: var(--panel); }
.warnings > summary { color: var(--ext); font-weight: 600; }
.warnings .intro { color: var(--muted); font-style: italic; margin: .25rem 0 .4rem 1.5rem; }
.warnings .item { color: var(--ext); margin: .1rem 0 .1rem 1.5rem; }
</style>
</head>
<body>
"#;

const HTML_HEADER_CONTROLS: &str = r#"  <span class="spacer"></span>
  <button type="button" onclick="document.querySelectorAll('details').forEach(function(d){d.open=true;});">Expand all</button>
  <button type="button" onclick="document.querySelectorAll('details').forEach(function(d){d.open=false;});">Collapse all</button>
  <label for="theme-sel" class="info">Theme:</label>
  <select id="theme-sel" onchange="document.documentElement.setAttribute('data-theme',this.value);try{localStorage.setItem('asn1-theme',this.value);}catch(e){}">
    <option value="light">Light</option>
    <option value="dark">Dark</option>
    <option value="grey">Grey</option>
  </select>
</header>
<main>
"#;

const HTML_TAIL: &str = r#"</main>
<script>
(function(){
  var t = document.documentElement.getAttribute('data-theme');
  var sel = document.getElementById('theme-sel');
  if (sel && t) sel.value = t;
})();
</script>
</body>
</html>
"#;

/// If `ty` is a `Reference` whose target cannot be resolved against the
/// loaded program, return the `(module, name)` of that dangling target.
/// Used to short-circuit expansion so unresolved references render as flat
/// yellow leaves instead of click-to-expand headers.
fn unresolved_ref_target(
    program: &IrProgram,
    current_mod: &str,
    ty: &IrType,
) -> Option<(String, String)> {
    if let IrType::Reference { module: tm, name } = ty {
        let target_mod = tm.clone().unwrap_or_else(|| current_mod.to_string());
        if program.find_type(&target_mod, name).is_none() {
            return Some((target_mod, name.clone()));
        }
    }
    None
}

/// Emit the trailing `(unresolved: Mod.Name)` marker in the warning color.
fn unresolved_marker(target_mod: &str, target_name: &str) -> String {
    format!(
        " <span class=\"unresolved\">(unresolved: {}.{})</span>",
        html_escape(target_mod),
        html_escape(target_name)
    )
}

/// Render a subtle, collapsible banner summarizing unresolved references —
/// the HTML counterpart of the egui header's ⚠ chip + diagnostics window.
/// Nothing is emitted when the program has no diagnostics.
fn html_diagnostics(out: &mut String, program: &IrProgram) {
    let diags = program.diagnostics();
    if diags.is_empty() {
        return;
    }
    let n = diags.len();
    let plural = if n == 1 { "" } else { "s" };
    out.push_str(&format!(
        "<details class=\"warnings\"><summary>⚠ {n} warning{plural} — unresolved types &amp; modules</summary>\n"
    ));
    out.push_str(
        "<div class=\"intro\">These references could not be resolved against the loaded modules. \
         The tree still renders; missing types are shown as <code>(unresolved…)</code>.</div>\n",
    );
    for d in &diags {
        out.push_str(&format!("<div class=\"item\">⚠ {}</div>\n", html_escape(&d.to_string())));
    }
    out.push_str("</details>\n");
}

fn html_module(out: &mut String, program: &IrProgram, m: &IrModule) {
    let types: Vec<&IrTypeDef> = m
        .items
        .iter()
        .filter_map(|i| match i {
            IrItem::Type(t) => Some(t),
            _ => None,
        })
        .collect();
    out.push_str(&format!(
        "<details class=\"module\"><summary>{} <span class=\"note\">({} types)</span></summary>\n",
        html_escape(&m.name),
        types.len()
    ));
    for t in types {
        html_type_def(out, program, &m.name, t);
    }
    out.push_str("</details>\n");
}

fn html_type_def(out: &mut String, program: &IrProgram, module: &str, td: &IrTypeDef) {
    let anchor = type_anchor(module, &td.name);
    let tail = match unresolved_ref_target(program, module, &td.ty) {
        Some((tm, tn)) => unresolved_marker(&tm, &tn),
        None => String::new(),
    };
    let summary = format!(
        "<span id=\"{anchor}\" class=\"name\">{}</span> <span class=\"kw\">::=</span> {}{tail}",
        html_escape(&td.name),
        html_type_ref_or_plain(module, &td.ty)
    );
    let visited = vec![(module.to_string(), td.name.clone())];
    let expandable = html_expandable(program, module, &td.ty, &visited);
    if !expandable && td.doc.is_none() {
        out.push_str(&format!("<div class=\"leaf\">{summary}</div>\n"));
        return;
    }
    out.push_str(&format!("<details><summary>{summary}</summary>\n"));
    if let Some(doc) = &td.doc {
        out.push_str(&format!("<div class=\"doc\">{}</div>\n", html_escape(doc)));
    }
    html_type_body(out, program, module, &td.ty, &visited);
    out.push_str("</details>\n");
}

fn html_type_body(
    out: &mut String,
    program: &IrProgram,
    module: &str,
    ty: &IrType,
    visited: &[(String, String)],
) {
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => html_struct(out, program, module, s, visited),
        IrType::Choice(c) => html_choice(out, program, module, c, visited),
        IrType::Enumerated { items, extensible } => {
            for i in items {
                let v = i.value.map(|v| format!(" = {v}")).unwrap_or_default();
                let ext = if i.is_extension { " <span class=\"ext\">[ext]</span>" } else { "" };
                out.push_str(&format!(
                    "<div class=\"leaf\">• <span class=\"name\">{}</span>{}{}</div>\n",
                    html_escape(&i.name),
                    html_escape(&v),
                    ext
                ));
            }
            if *extensible {
                out.push_str("<div class=\"leaf note\">…</div>\n");
            }
        }
        IrType::SequenceOf { element, constraints } | IrType::SetOf { element, constraints } => {
            html_constraints(out, constraints);
            let elem_tail = match unresolved_ref_target(program, module, element) {
                Some((tm, tn)) => unresolved_marker(&tm, &tn),
                None => String::new(),
            };
            if html_expandable(program, module, element, visited) {
                out.push_str("<details><summary><span class=\"kw\">[element]</span> ");
                out.push_str(&html_type_ref_or_plain(module, element));
                out.push_str(&elem_tail);
                out.push_str("</summary>\n");
                html_type_body(out, program, module, element, visited);
                out.push_str("</details>\n");
            } else {
                out.push_str(&format!(
                    "<div class=\"leaf\"><span class=\"kw\">[element]</span> {}{elem_tail}</div>\n",
                    html_type_ref_or_plain(module, element)
                ));
            }
        }
        IrType::Integer { named_numbers, constraints } => {
            for (n, v) in named_numbers {
                out.push_str(&format!(
                    "<div class=\"named\">• <span class=\"name\">{}</span> = {}</div>\n",
                    html_escape(n),
                    v
                ));
            }
            html_constraints(out, constraints);
        }
        IrType::BitString { named_bits, constraints } => {
            for (n, v) in named_bits {
                out.push_str(&format!(
                    "<div class=\"named\">• <span class=\"name\">{}</span> = bit {}</div>\n",
                    html_escape(n),
                    v
                ));
            }
            html_constraints(out, constraints);
        }
        IrType::OctetString { constraints } => html_constraints(out, constraints),
        IrType::CharString { kind, constraints } => {
            out.push_str(&format!(
                "<div class=\"leaf note\">kind: {}</div>\n",
                html_escape(&format!("{kind:?}"))
            ));
            html_constraints(out, constraints);
        }
        IrType::Reference { module: tm, name } => {
            let target_mod = tm.clone().unwrap_or_else(|| module.to_string());
            html_resolve_reference(out, program, module, &target_mod, name, visited);
        }
        _ => {}
    }
}

fn html_struct(
    out: &mut String,
    program: &IrProgram,
    module: &str,
    s: &IrStruct,
    visited: &[(String, String)],
) {
    for m in &s.members {
        match m {
            IrStructMember::Field(f) => html_field(out, program, module, f, visited),
            IrStructMember::ComponentsOf { type_ref } => {
                let key = (module.to_string(), type_ref.clone());
                if visited.contains(&key) {
                    out.push_str(&format!(
                        "<div class=\"leaf recursive\">↳ COMPONENTS OF {} (↺ recursive)</div>\n",
                        html_type_ref_link(module, module, type_ref)
                    ));
                    continue;
                }
                match program.find_type(module, type_ref) {
                    Some(td) => {
                        let mut next = visited.to_vec();
                        next.push(key);
                        out.push_str(&format!(
                            "<details><summary><span class=\"note\">↳ COMPONENTS OF</span> {}</summary>\n",
                            html_type_ref_link(module, module, type_ref)
                        ));
                        if let Some(doc) = &td.doc {
                            out.push_str(&format!(
                                "<div class=\"doc\">{}</div>\n",
                                html_escape(doc)
                            ));
                        }
                        html_type_body(out, program, module, &td.ty, &next);
                        out.push_str("</details>\n");
                    }
                    None => {
                        out.push_str(&format!(
                            "<div class=\"leaf unresolved\">↳ COMPONENTS OF {} (unresolved)</div>\n",
                            html_escape(type_ref)
                        ));
                    }
                }
            }
        }
    }
    if s.extensible {
        out.push_str("<div class=\"leaf note\">…</div>\n");
    }
}

fn html_choice(
    out: &mut String,
    program: &IrProgram,
    module: &str,
    c: &IrChoice,
    visited: &[(String, String)],
) {
    for a in &c.alternatives {
        html_field(out, program, module, a, visited);
    }
    if c.extensible {
        out.push_str("<div class=\"leaf note\">…</div>\n");
    }
}

fn html_field(
    out: &mut String,
    program: &IrProgram,
    module: &str,
    f: &IrField,
    visited: &[(String, String)],
) {
    let opt = match &f.optionality {
        IrOptionality::Required => "",
        IrOptionality::Optional => " OPTIONAL",
        IrOptionality::Default(_) => " DEFAULT …",
    };
    let ext = if f.is_extension { " <span class=\"ext\">[ext]</span>" } else { "" };
    let unresolved = unresolved_ref_target(program, module, &f.ty);
    let tail = match &unresolved {
        Some((tm, tn)) => unresolved_marker(tm, tn),
        None => String::new(),
    };
    let summary = format!(
        "<span class=\"name\">{}</span>: {}{}{ext}{tail}",
        html_escape(&f.name),
        html_type_ref_or_plain(module, &f.ty),
        html_escape(opt),
    );
    let expandable = html_expandable(program, module, &f.ty, visited);
    if !expandable && f.doc.is_none() {
        out.push_str(&format!("<div class=\"leaf\">{summary}</div>\n"));
        return;
    }
    out.push_str(&format!("<details><summary>{summary}</summary>\n"));
    if let Some(doc) = &f.doc {
        out.push_str(&format!("<div class=\"doc\">{}</div>\n", html_escape(doc)));
    }
    html_type_body(out, program, module, &f.ty, visited);
    out.push_str("</details>\n");
}

/// Follow a reference inline: emit a `→ Module.Name` pointer, the target's
/// doc (if any), then the target's body recursively — or a `↺ recursive` /
/// `(unresolved)` marker when following would loop or dangle.
fn html_resolve_reference(
    out: &mut String,
    program: &IrProgram,
    current_mod: &str,
    target_mod: &str,
    target_name: &str,
    visited: &[(String, String)],
) {
    let key = (target_mod.to_string(), target_name.to_string());
    if visited.contains(&key) {
        out.push_str(&format!(
            "<div class=\"leaf recursive\">↺ recursive: {}</div>\n",
            html_type_ref_link(current_mod, target_mod, target_name)
        ));
        return;
    }
    match program.find_type(target_mod, target_name) {
        None => {
            out.push_str(&format!(
                "<div class=\"leaf unresolved\">(unresolved: {}.{})</div>\n",
                html_escape(target_mod),
                html_escape(target_name)
            ));
        }
        Some(td) => {
            let mut next = visited.to_vec();
            next.push(key);
            out.push_str(&format!(
                "<div class=\"target\">→ {}</div>\n",
                html_type_ref_link(current_mod, target_mod, target_name)
            ));
            if let Some(doc) = &td.doc {
                out.push_str(&format!("<div class=\"doc\">{}</div>\n", html_escape(doc)));
            }
            html_type_body(out, program, target_mod, &td.ty, &next);
        }
    }
}

fn html_constraints(out: &mut String, cs: &[IrConstraint]) {
    for c in cs {
        out.push_str(&format!(
            "<div class=\"constraint\">constraint: {}</div>\n",
            html_escape(&render_constraint(c))
        ));
    }
}

/// Is there any content `html_type_body` would emit for this type? Mirrors
/// the UI's `expand` logic, but we also expand primitive aliases (INTEGER
/// with named numbers / constraints, BIT STRING with named bits, etc.)
/// because the reader should be able to peek at their details.
fn html_expandable(
    program: &IrProgram,
    module: &str,
    ty: &IrType,
    visited: &[(String, String)],
) -> bool {
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => !s.members.is_empty() || s.extensible,
        IrType::Choice(c) => !c.alternatives.is_empty() || c.extensible,
        IrType::Enumerated { items, extensible } => !items.is_empty() || *extensible,
        IrType::SequenceOf { element, constraints } | IrType::SetOf { element, constraints } => {
            !constraints.is_empty() || html_expandable(program, module, element, visited)
        }
        IrType::Integer { named_numbers, constraints } => {
            !named_numbers.is_empty() || !constraints.is_empty()
        }
        IrType::BitString { named_bits, constraints } => {
            !named_bits.is_empty() || !constraints.is_empty()
        }
        IrType::OctetString { constraints } => !constraints.is_empty(),
        IrType::CharString { constraints, .. } => !constraints.is_empty(),
        IrType::Reference { module: tm, name } => {
            let target_mod = tm.clone().unwrap_or_else(|| module.to_string());
            let key = (target_mod.clone(), name.clone());
            if visited.contains(&key) {
                // Worth emitting the recursive marker.
                return true;
            }
            match program.find_type(&target_mod, name) {
                // Unresolved refs render as a flat yellow leaf at the field
                // level (see `html_field`) — no drilldown needed.
                None => false,
                Some(td) => {
                    let mut next = visited.to_vec();
                    next.push(key);
                    td.doc.is_some() || html_expandable(program, &target_mod, &td.ty, &next)
                }
            }
        }
        _ => false,
    }
}

/// Render a type as plain text except that `Reference` variants become `<a>`
/// links to the target type's anchor, so the reader can jump to the referent.
fn html_type_ref_or_plain(current_mod: &str, ty: &IrType) -> String {
    match ty {
        IrType::Reference { module, name } => {
            let target_mod = module.as_deref().unwrap_or(current_mod);
            html_type_ref_link(current_mod, target_mod, name)
        }
        IrType::SequenceOf { element, .. } => {
            format!(
                "<span class=\"ty\">SEQUENCE OF</span> {}",
                html_type_ref_or_plain(current_mod, element)
            )
        }
        IrType::SetOf { element, .. } => {
            format!(
                "<span class=\"ty\">SET OF</span> {}",
                html_type_ref_or_plain(current_mod, element)
            )
        }
        _ => format!("<span class=\"ty\">{}</span>", html_escape(&render_type(ty))),
    }
}

fn html_type_ref_link(_current_mod: &str, target_mod: &str, target_name: &str) -> String {
    let anchor = type_anchor(target_mod, target_name);
    let display = if _current_mod == target_mod {
        target_name.to_string()
    } else {
        format!("{target_mod}.{target_name}")
    };
    format!("<a class=\"tyref\" href=\"#{}\">{}</a>", html_escape(&anchor), html_escape(&display))
}

fn type_anchor(module: &str, name: &str) -> String {
    let mut out = String::with_capacity(module.len() + name.len() + 4);
    out.push_str("ty-");
    for c in module.chars().chain(std::iter::once('-')).chain(name.chars()) {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{
        program_with_reference_chain, program_with_self_reference, tiny_program,
    };
    use asn1_ir::{IrConstraint, IrItem, IrModule, IrProgram, IrTypeDef};

    #[test]
    fn export_html_contains_module_and_fields() {
        let html = export_html(&tiny_program());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("Geo"));
        assert!(html.contains("Point"));
        assert!(html.contains("OPTIONAL"));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn export_html_links_references_to_anchors() {
        let html = export_html(&program_with_reference_chain());
        // Outer references Inner and Inner references Id; both should appear
        // as links to anchors in the same document.
        assert!(html.contains("id=\"ty-M-Inner\""));
        assert!(html.contains("id=\"ty-M-Id\""));
        assert!(html.contains("href=\"#ty-M-Inner\""));
        assert!(html.contains("href=\"#ty-M-Id\""));
    }

    #[test]
    fn html_escape_escapes_specials() {
        assert_eq!(html_escape("<a>&\"'"), "&lt;a&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn export_html_header_has_version_creator_and_themes() {
        let html = export_html(&tiny_program());
        assert!(
            html.contains(&format!("v{}", env!("CARGO_PKG_VERSION"))),
            "header should show version from Cargo metadata"
        );
        assert!(html.contains(r#"id="theme-sel""#), "theme selector should be present");
        for theme in ["light", "dark", "grey"] {
            assert!(
                html.contains(&format!(r#"value="{theme}""#)),
                "theme option `{theme}` missing"
            );
            assert!(
                html.contains(&format!(r#"[data-theme="{theme}"]"#)),
                "theme stylesheet for `{theme}` missing"
            );
        }
    }

    #[test]
    fn export_html_inlines_referenced_type_body() {
        // When a field's type is a reference, the field's <details> body
        // should inline the target's body, not just link to it — matching
        // the egui drill-down behavior.
        let html = export_html(&program_with_reference_chain());
        // Find Outer's own <details> body and confirm it contains Inner's
        // `id` field inlined (a reference would only show "Inner" text).
        let outer_marker = r#"id="ty-M-Outer""#;
        let outer_idx = html.find(outer_marker).expect("Outer type def missing");
        let after_outer = &html[outer_idx..];
        let outer_end = after_outer.find("</details>").expect("Outer block unclosed");
        let outer_block = &after_outer[..outer_end];
        assert!(outer_block.contains("inner"), "Outer block should list the inner field");
        assert!(
            outer_block.contains("→ "),
            "Outer block should show `→ Module.Name` pointer for the inlined reference"
        );
    }

    #[test]
    fn export_html_marks_recursive_reference() {
        let html = export_html(&program_with_self_reference());
        assert!(
            html.contains("↺ recursive"),
            "self-referential type should be cut off with a recursive marker"
        );
        assert!(html.contains(r#"class="leaf recursive""#));
    }

    #[test]
    fn export_html_omits_diagnostics_banner_when_clean() {
        let html = export_html(&tiny_program());
        assert!(
            !html.contains("class=\"warnings\""),
            "clean program should not render the warnings banner"
        );
    }

    #[test]
    fn export_html_lists_unresolved_references() {
        // Module M references an Unknown type that doesn't exist — diagnostics
        // should surface it in the warnings banner.
        let outer = IrTypeDef {
            name: "Outer".into(),
            doc: None,
            ty: asn1_ir::IrType::Sequence(asn1_ir::IrStruct {
                extensible: false,
                members: vec![asn1_ir::IrStructMember::Field(asn1_ir::IrField {
                    doc: None,
                    name: "x".into(),
                    ty: asn1_ir::IrType::Reference { module: None, name: "Unknown".into() },
                    optionality: asn1_ir::IrOptionality::Required,
                    is_extension: false,
                })],
            }),
        };
        let p = IrProgram {
            modules: vec![IrModule {
                name: "M".into(),
                oid: None,
                imports: vec![],
                items: vec![IrItem::Type(outer)],
            }],
        };
        let html = export_html(&p);
        assert!(html.contains("class=\"warnings\""), "warnings banner should be present");
        assert!(html.contains("Unknown"), "unresolved type name should appear in the banner");
    }

    #[test]
    fn export_html_inlines_primitive_details() {
        // DeltaLat ::= INTEGER { unavailable(131072) } — when expanded, the
        // named number and constraint should appear inline.
        let delta = IrTypeDef {
            name: "DeltaLat".into(),
            doc: Some("offset from reference position".into()),
            ty: IrType::Integer {
                named_numbers: vec![("unavailable".into(), 131072)],
                constraints: vec![IrConstraint::Range {
                    lower: Some(-131071),
                    upper: Some(131072),
                    extensible: false,
                }],
            },
        };
        let p = IrProgram {
            modules: vec![IrModule {
                name: "M".into(),
                oid: None,
                imports: vec![],
                items: vec![IrItem::Type(delta)],
            }],
        };
        let html = export_html(&p);
        assert!(html.contains("unavailable"), "named number should appear");
        assert!(html.contains("131072"), "named number value should appear");
        assert!(html.contains("constraint:"), "constraint row should appear");
        assert!(html.contains("offset from reference position"), "doc should appear");
    }
}
