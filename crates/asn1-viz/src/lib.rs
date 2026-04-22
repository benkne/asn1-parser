//! Interactive ASN.1 visualizer.
//!
//! Opens an egui window with a two-pane layout: a collapsible tree of modules
//! and types on the left, and a detail view for the currently selected node on
//! the right. The user can click any composite (SEQUENCE, SET, CHOICE,
//! ENUMERATED) to expand it and drill into its structure.

#![deny(rust_2018_idioms)]

use asn1_ir::{
    render_type, IrChoice, IrField, IrItem, IrModule, IrOptionality, IrProgram, IrStruct,
    IrStructMember, IrType, IrTypeDef,
};

/// Launch the visualizer UI. Blocks until the window is closed.
pub fn launch(program: IrProgram) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("asn1-decoder — visualizer"),
        ..Default::default()
    };
    eframe::run_native("asn1-decoder", options, Box::new(|_cc| Box::new(VizApp::new(program))))
}

struct VizApp {
    program: IrProgram,
    filter: String,
    selection: Option<Selection>,
}

#[derive(Clone)]
struct Selection {
    module: String,
    type_name: String,
    path: Vec<String>,
}

struct TreeModule {
    name: String,
    types: Vec<TreeType>,
}

struct TreeType {
    name: String,
    ty: IrType,
}

impl VizApp {
    fn new(program: IrProgram) -> Self {
        Self { program, filter: String::new(), selection: None }
    }

    fn selected_type(&self) -> Option<(&IrModule, &IrTypeDef)> {
        let sel = self.selection.as_ref()?;
        self.program.modules.iter().find(|m| m.name == sel.module).and_then(|m| {
            m.items.iter().find_map(|i| match i {
                IrItem::Type(t) if t.name == sel.type_name => Some((m, t)),
                _ => None,
            })
        })
    }
}

impl eframe::App for VizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("asn1-decoder");
                ui.separator();
                ui.label(format!("{} module(s)", self.program.modules.len()));
                ui.separator();
                ui.label("filter:");
                ui.text_edit_singleline(&mut self.filter);
            });
        });

        egui::SidePanel::left("tree_panel").resizable(true).default_width(420.0).show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                self.show_tree(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                self.show_details(ui);
            });
        });
    }
}

impl VizApp {
    fn show_tree(&mut self, ui: &mut egui::Ui) {
        let filter = self.filter.to_lowercase();
        let modules: Vec<TreeModule> = self
            .program
            .modules
            .iter()
            .map(|m| TreeModule {
                name: m.name.clone(),
                types: m
                    .items
                    .iter()
                    .filter_map(|i| match i {
                        IrItem::Type(t) => {
                            Some(TreeType { name: t.name.clone(), ty: t.ty.clone() })
                        }
                        _ => None,
                    })
                    .filter(|t| filter.is_empty() || t.name.to_lowercase().contains(&filter))
                    .collect(),
            })
            .collect();

        for module in modules {
            if module.types.is_empty() && !filter.is_empty() {
                continue;
            }
            let header =
                egui::CollapsingHeader::new(format!("{}  ({})", module.name, module.types.len()))
                    .id_source(format!("mod::{}", module.name))
                    .default_open(!filter.is_empty());
            header.show(ui, |ui| {
                for t in &module.types {
                    self.show_type_node(ui, &module.name, &t.name, &t.ty, &[]);
                }
            });
        }
    }

    fn show_type_node(
        &mut self,
        ui: &mut egui::Ui,
        module: &str,
        tname: &str,
        ty: &IrType,
        path: &[String],
    ) {
        let label = format!("{tname}: {}", render_type(ty));
        let is_composite = matches!(
            ty,
            IrType::Sequence(_)
                | IrType::Set(_)
                | IrType::Choice(_)
                | IrType::Enumerated { .. }
                | IrType::SequenceOf { .. }
                | IrType::SetOf { .. }
        );

        if !is_composite {
            let resp = ui.selectable_label(self.is_selected(module, tname, path), &label);
            if resp.clicked() {
                self.select(module, tname, path);
            }
            return;
        }

        let header = egui::CollapsingHeader::new(&label)
            .id_source(format!("{module}::{tname}::{}", path.join("/")))
            .default_open(false);
        let resp = header.show(ui, |ui| match ty {
            IrType::Sequence(s) | IrType::Set(s) => {
                self.show_struct_children(ui, module, tname, path, s)
            }
            IrType::Choice(c) => self.show_choice_children(ui, module, tname, path, c),
            IrType::Enumerated { items, .. } => {
                for item in items {
                    let tag = if item.is_extension { "[ext] " } else { "" };
                    let v = item.value.map(|v| format!(" = {v}")).unwrap_or_default();
                    ui.label(format!("• {tag}{}{v}", item.name));
                }
            }
            IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
                let mut next_path = path.to_vec();
                next_path.push("[]".into());
                self.show_type_node(ui, module, tname, element, &next_path);
            }
            _ => {}
        });
        if resp.header_response.clicked() {
            self.select(module, tname, path);
        }
    }

    fn show_struct_children(
        &mut self,
        ui: &mut egui::Ui,
        module: &str,
        tname: &str,
        path: &[String],
        s: &IrStruct,
    ) {
        for m in &s.members {
            match m {
                IrStructMember::Field(f) => self.show_field_node(ui, module, tname, path, f),
                IrStructMember::ComponentsOf { type_ref } => {
                    ui.label(format!("↳ COMPONENTS OF {type_ref}"));
                }
            }
        }
        if s.extensible {
            ui.label("…");
        }
    }

    fn show_choice_children(
        &mut self,
        ui: &mut egui::Ui,
        module: &str,
        tname: &str,
        path: &[String],
        c: &IrChoice,
    ) {
        for a in &c.alternatives {
            self.show_field_node(ui, module, tname, path, a);
        }
        if c.extensible {
            ui.label("…");
        }
    }

    fn show_field_node(
        &mut self,
        ui: &mut egui::Ui,
        module: &str,
        tname: &str,
        path: &[String],
        f: &IrField,
    ) {
        let suffix = match &f.optionality {
            IrOptionality::Required => "",
            IrOptionality::Optional => " OPTIONAL",
            IrOptionality::Default(v) => {
                return {
                    let mut next_path = path.to_vec();
                    next_path.push(f.name.clone());
                    self.show_type_node(ui, module, tname, &f.ty, &next_path);
                    ui.label(format!("    (DEFAULT {v})"));
                }
            }
        };
        let label = format!(
            "{}{} {}",
            if f.is_extension { "[ext] " } else { "" },
            f.name,
            render_type(&f.ty)
        );
        let is_composite = matches!(
            f.ty,
            IrType::Sequence(_)
                | IrType::Set(_)
                | IrType::Choice(_)
                | IrType::Enumerated { .. }
                | IrType::SequenceOf { .. }
                | IrType::SetOf { .. }
        );
        if !is_composite {
            ui.label(format!("{label}{suffix}"));
            return;
        }
        let mut next_path = path.to_vec();
        next_path.push(f.name.clone());
        let header = egui::CollapsingHeader::new(format!("{label}{suffix}"))
            .id_source(format!("{module}::{tname}::{}", next_path.join("/")))
            .default_open(false);
        header.show(ui, |ui| match &f.ty {
            IrType::Sequence(s) | IrType::Set(s) => {
                self.show_struct_children(ui, module, tname, &next_path, s)
            }
            IrType::Choice(c) => self.show_choice_children(ui, module, tname, &next_path, c),
            IrType::Enumerated { items, .. } => {
                for item in items {
                    let tag = if item.is_extension { "[ext] " } else { "" };
                    let v = item.value.map(|v| format!(" = {v}")).unwrap_or_default();
                    ui.label(format!("• {tag}{}{v}", item.name));
                }
            }
            IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
                let mut next = next_path.clone();
                next.push("[]".into());
                self.show_type_node(ui, module, tname, element, &next);
            }
            _ => {}
        });
    }

    fn select(&mut self, module: &str, tname: &str, path: &[String]) {
        self.selection =
            Some(Selection { module: module.into(), type_name: tname.into(), path: path.to_vec() });
    }

    fn is_selected(&self, module: &str, tname: &str, path: &[String]) -> bool {
        match &self.selection {
            Some(s) => s.module == module && s.type_name == tname && s.path == path,
            None => false,
        }
    }

    fn show_details(&mut self, ui: &mut egui::Ui) {
        let Some((m, td)) = self.selected_type() else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Select a type in the tree to see details.");
            });
            return;
        };
        ui.heading(&td.name);
        ui.label(format!("module: {}", m.name));
        if let Some(doc) = &td.doc {
            ui.separator();
            ui.label(doc);
        }
        ui.separator();
        ui.label(format!("kind: {}", render_type(&td.ty)));
        ui.separator();
        ui.label("full structure:");
        ui.monospace(describe(&td.ty, 0));
    }
}

fn describe(ty: &IrType, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => {
            let kind = if matches!(ty, IrType::Sequence(_)) { "SEQUENCE" } else { "SET" };
            let mut out = format!("{pad}{kind} {{\n");
            for m in &s.members {
                match m {
                    IrStructMember::Field(f) => {
                        out.push_str(&describe_field(f, depth + 1));
                    }
                    IrStructMember::ComponentsOf { type_ref } => {
                        out.push_str(&format!(
                            "{}COMPONENTS OF {type_ref}\n",
                            "  ".repeat(depth + 1)
                        ));
                    }
                }
            }
            if s.extensible {
                out.push_str(&format!("{}…\n", "  ".repeat(depth + 1)));
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
        IrType::Choice(c) => {
            let mut out = format!("{pad}CHOICE {{\n");
            for a in &c.alternatives {
                out.push_str(&describe_field(a, depth + 1));
            }
            if c.extensible {
                out.push_str(&format!("{}…\n", "  ".repeat(depth + 1)));
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
        IrType::Enumerated { items, extensible } => {
            let mut out = format!("{pad}ENUMERATED {{\n");
            for i in items {
                let v = i.value.map(|v| format!(" ({v})")).unwrap_or_default();
                let ext = if i.is_extension { " [ext]" } else { "" };
                out.push_str(&format!("{}{}{v}{ext}\n", "  ".repeat(depth + 1), i.name));
            }
            if *extensible {
                out.push_str(&format!("{}…\n", "  ".repeat(depth + 1)));
            }
            out.push_str(&format!("{pad}}}"));
            out
        }
        IrType::SequenceOf { element, .. } => {
            format!("{pad}SEQUENCE OF\n{}", describe(element, depth + 1))
        }
        IrType::SetOf { element, .. } => {
            format!("{pad}SET OF\n{}", describe(element, depth + 1))
        }
        other => format!("{pad}{}", render_type(other)),
    }
}

fn describe_field(f: &IrField, depth: usize) -> String {
    let pad = "  ".repeat(depth);
    let opt = match &f.optionality {
        IrOptionality::Required => "",
        IrOptionality::Optional => " OPTIONAL",
        IrOptionality::Default(_) => " DEFAULT …",
    };
    let ext = if f.is_extension { " [ext]" } else { "" };
    match &f.ty {
        IrType::Sequence(_)
        | IrType::Set(_)
        | IrType::Choice(_)
        | IrType::Enumerated { .. }
        | IrType::SequenceOf { .. }
        | IrType::SetOf { .. } => {
            format!("{pad}{} {{\n{}\n{pad}}}{opt}{ext}\n", f.name, describe(&f.ty, depth + 1))
        }
        _ => format!("{pad}{} {}{opt}{ext}\n", f.name, render_type(&f.ty)),
    }
}

// ---------------------------------------------------------------------------
// Standalone HTML export
// ---------------------------------------------------------------------------

/// Render the IR as a self-contained HTML document using `<details>` /
/// `<summary>` for native click-to-expand, requiring no JavaScript or external
/// assets. The output is suitable for opening directly in any browser or
/// bundling into documentation.
pub fn export_html(program: &IrProgram) -> String {
    let mut out = String::new();
    out.push_str(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>asn1-decoder — tree</title>
<style>
body { font: 14px/1.4 ui-sans-serif, system-ui, sans-serif; margin: 1rem 2rem; color: #1f2328; }
h1   { font-size: 1.25rem; margin: 0 0 .25rem; }
.meta { color: #656d76; margin-bottom: 1.25rem; }
details { margin: .1rem 0 .1rem .25rem; }
summary { cursor: pointer; list-style: none; padding: .1rem .25rem; border-radius: 3px; }
summary::-webkit-details-marker { display: none; }
summary::before { content: "▸"; display: inline-block; width: 1em; color: #656d76; transition: transform .1s; }
details[open] > summary::before { transform: rotate(90deg); }
summary:hover { background: #f6f8fa; }
.leaf { padding: .1rem .25rem .1rem 1.25rem; }
.kw   { color: #0550ae; }
.name { font-weight: 600; }
.ty   { color: #0a3069; }
.note { color: #656d76; font-style: italic; }
.ext  { color: #9a6700; }
.doc  { color: #656d76; margin: .1rem 0 .3rem 1.5rem; white-space: pre-wrap; }
.module > summary { font-weight: 700; font-size: 1.05rem; }
.module { margin-top: .6rem; border-top: 1px solid #eaecef; padding-top: .4rem; }
input[type=search] { width: 100%; padding: .4rem; box-sizing: border-box; margin-bottom: .75rem;
    font: inherit; border: 1px solid #d0d7de; border-radius: 4px; }
</style>
</head>
<body>
"#,
    );
    let type_total: usize = program.all_types().count();
    out.push_str(&format!(
        "<h1>asn1-decoder</h1>\n<div class=\"meta\">{} module(s), {} type(s)</div>\n",
        program.modules.len(),
        type_total
    ));
    out.push_str(
        r#"<input type="search" placeholder="Use browser find (Ctrl+F) to locate a type…" aria-label="Type names are plain text; use the browser's find">
"#,
    );
    for m in &program.modules {
        html_module(&mut out, m);
    }
    out.push_str("</body>\n</html>\n");
    out
}

fn html_module(out: &mut String, m: &IrModule) {
    let types: Vec<&IrTypeDef> = m
        .items
        .iter()
        .filter_map(|i| match i {
            IrItem::Type(t) => Some(t),
            _ => None,
        })
        .collect();
    out.push_str(&format!(
        "<details class=\"module\" open><summary>{} <span class=\"note\">({} types)</span></summary>\n",
        html_escape(&m.name),
        types.len()
    ));
    for t in types {
        html_type_def(out, t);
    }
    out.push_str("</details>\n");
}

fn html_type_def(out: &mut String, td: &IrTypeDef) {
    let summary = format!(
        "<span class=\"name\">{}</span> <span class=\"kw\">::=</span> <span class=\"ty\">{}</span>",
        html_escape(&td.name),
        html_escape(&render_type(&td.ty))
    );
    let has_children = matches!(
        &td.ty,
        IrType::Sequence(_)
            | IrType::Set(_)
            | IrType::Choice(_)
            | IrType::Enumerated { .. }
            | IrType::SequenceOf { .. }
            | IrType::SetOf { .. }
    );
    if !has_children && td.doc.is_none() {
        out.push_str(&format!("<div class=\"leaf\">{summary}</div>\n"));
        return;
    }
    out.push_str(&format!("<details><summary>{summary}</summary>\n"));
    if let Some(doc) = &td.doc {
        out.push_str(&format!("<div class=\"doc\">{}</div>\n", html_escape(doc)));
    }
    html_type_body(out, &td.ty);
    out.push_str("</details>\n");
}

fn html_type_body(out: &mut String, ty: &IrType) {
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => html_struct(out, s),
        IrType::Choice(c) => html_choice(out, c),
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
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            out.push_str("<details><summary><span class=\"kw\">[element]</span></summary>\n");
            html_type_body(out, element);
            out.push_str("</details>\n");
        }
        _ => {}
    }
}

fn html_struct(out: &mut String, s: &IrStruct) {
    for m in &s.members {
        match m {
            IrStructMember::Field(f) => html_field(out, f),
            IrStructMember::ComponentsOf { type_ref } => {
                out.push_str(&format!(
                    "<div class=\"leaf note\">↳ COMPONENTS OF {}</div>\n",
                    html_escape(type_ref)
                ));
            }
        }
    }
    if s.extensible {
        out.push_str("<div class=\"leaf note\">…</div>\n");
    }
}

fn html_choice(out: &mut String, c: &IrChoice) {
    for a in &c.alternatives {
        html_field(out, a);
    }
    if c.extensible {
        out.push_str("<div class=\"leaf note\">…</div>\n");
    }
}

fn html_field(out: &mut String, f: &IrField) {
    let opt = match &f.optionality {
        IrOptionality::Required => "",
        IrOptionality::Optional => " OPTIONAL",
        IrOptionality::Default(_) => " DEFAULT …",
    };
    let ext = if f.is_extension { " <span class=\"ext\">[ext]</span>" } else { "" };
    let summary = format!(
        "<span class=\"name\">{}</span> <span class=\"ty\">{}</span>{}{ext}",
        html_escape(&f.name),
        html_escape(&render_type(&f.ty)),
        html_escape(opt),
    );
    let has_children = matches!(
        f.ty,
        IrType::Sequence(_)
            | IrType::Set(_)
            | IrType::Choice(_)
            | IrType::Enumerated { .. }
            | IrType::SequenceOf { .. }
            | IrType::SetOf { .. }
    );
    if !has_children {
        out.push_str(&format!("<div class=\"leaf\">{summary}</div>\n"));
        return;
    }
    out.push_str(&format!("<details><summary>{summary}</summary>\n"));
    if let Some(doc) = &f.doc {
        out.push_str(&format!("<div class=\"doc\">{}</div>\n", html_escape(doc)));
    }
    html_type_body(out, &f.ty);
    out.push_str("</details>\n");
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
    use asn1_ir::{
        IrField, IrItem, IrModule, IrOptionality, IrProgram, IrStruct, IrStructMember, IrType,
        IrTypeDef,
    };

    fn tiny_program() -> IrProgram {
        let point = IrTypeDef {
            name: "Point".into(),
            doc: Some("a 2-d point".into()),
            ty: IrType::Sequence(IrStruct {
                extensible: false,
                members: vec![
                    IrStructMember::Field(IrField {
                        doc: None,
                        name: "x".into(),
                        ty: IrType::Integer {
                            named_numbers: vec![],
                            constraints: vec![],
                        },
                        optionality: IrOptionality::Required,
                        is_extension: false,
                    }),
                    IrStructMember::Field(IrField {
                        doc: None,
                        name: "y".into(),
                        ty: IrType::Integer {
                            named_numbers: vec![],
                            constraints: vec![],
                        },
                        optionality: IrOptionality::Optional,
                        is_extension: false,
                    }),
                ],
            }),
        };
        IrProgram {
            modules: vec![IrModule {
                name: "Geo".into(),
                oid: None,
                imports: vec![],
                items: vec![IrItem::Type(point)],
            }],
        }
    }

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
    fn html_escape_escapes_specials() {
        assert_eq!(html_escape("<a>&\"'"), "&lt;a&gt;&amp;&quot;&#39;");
    }
}
