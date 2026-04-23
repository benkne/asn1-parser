//! Interactive ASN.1 visualizer.
//!
//! Two-pane layout: the left panel is a filterable picker of every type in
//! the program, grouped by module; clicking an entry makes that type the
//! drill-down *root*. The central panel shows the root as a click-to-expand
//! tree. Composite types (SEQUENCE / SET / CHOICE / ENUMERATED / SEQUENCE OF
//! / SET OF) expand in place, and named-type references are resolved against
//! the program so the user can keep drilling through aliases until primitive
//! leaves are reached. Cycles in the type graph are detected and shown as
//! `↺ recursive: Module.Name` rather than looped forever.

#![deny(rust_2018_idioms)]

use asn1_ir::{
    render_type, IrChoice, IrConstraint, IrField, IrItem, IrModule, IrOptionality, IrProgram,
    IrStruct, IrStructMember, IrType, IrTypeDef,
};

/// Launch the visualizer UI. Blocks until the window is closed.
pub fn launch(program: IrProgram) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 780.0])
            .with_title("asn1-decoder — visualizer"),
        ..Default::default()
    };
    eframe::run_native("asn1-decoder", options, Box::new(|_cc| Box::new(VizApp::new(program))))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Light,
    Dark,
    Grey,
}

impl Theme {
    fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Grey => "Grey",
        }
    }

    fn visuals(self) -> egui::Visuals {
        match self {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
            Theme::Grey => {
                // Mid-grey neutral palette: darker than light, lighter than dark,
                // with tinted panel/window backgrounds for visual hierarchy.
                let mut v = egui::Visuals::dark();
                v.panel_fill = egui::Color32::from_rgb(0x55, 0x58, 0x5c);
                v.window_fill = egui::Color32::from_rgb(0x5e, 0x61, 0x66);
                v.extreme_bg_color = egui::Color32::from_rgb(0x3f, 0x42, 0x46);
                v.faint_bg_color = egui::Color32::from_rgb(0x65, 0x68, 0x6c);
                v.override_text_color = Some(egui::Color32::from_rgb(0xe6, 0xe6, 0xe6));
                v
            }
        }
    }
}

struct VizApp {
    program: IrProgram,
    filter: String,
    /// Currently-focused root type as `(module, type_name)`.
    root: Option<(String, String)>,
    theme: Theme,
    about_open: bool,
}

impl VizApp {
    fn new(program: IrProgram) -> Self {
        Self { program, filter: String::new(), root: None, theme: Theme::Dark, about_open: false }
    }
}

impl eframe::App for VizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(self.theme.visuals());

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.heading("asn1-decoder");
                ui.separator();

                ui.menu_button("View", |ui| {
                    ui.label("Theme");
                    ui.separator();
                    for t in [Theme::Light, Theme::Dark, Theme::Grey] {
                        if ui.radio(self.theme == t, t.label()).clicked() {
                            self.theme = t;
                            ui.close_menu();
                        }
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.about_open = true;
                        ui.close_menu();
                    }
                });

                ui.separator();
                ui.label(format!("{} module(s)", self.program.modules.len()));
                if let Some((m, n)) = self.root.clone() {
                    ui.separator();
                    ui.label(format!("root: {m}:{n}"));
                    if ui.button("clear").clicked() {
                        self.root = None;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak());
                });
            });
        });

        let mut about_open = self.about_open;
        egui::Window::new("About asn1-decoder")
            .open(&mut about_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("asn1-decoder");
                ui.label(env!("CARGO_PKG_DESCRIPTION"));
                ui.separator();
                egui::Grid::new("about-grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    ui.label("Version:");
                    ui.label(env!("CARGO_PKG_VERSION"));
                    ui.end_row();
                    ui.label("Creator:");
                    ui.label(env!("CARGO_PKG_AUTHORS").replace(':', ", "));
                    ui.end_row();
                    ui.label("License:");
                    ui.label(env!("CARGO_PKG_LICENSE"));
                    ui.end_row();
                    let repo = env!("CARGO_PKG_REPOSITORY");
                    if !repo.is_empty() {
                        ui.label("Repository:");
                        ui.hyperlink(repo);
                        ui.end_row();
                    }
                });
            });
        self.about_open = about_open;

        egui::SidePanel::left("picker").resizable(true).default_width(360.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("filter:");
                ui.text_edit_singleline(&mut self.filter);
            });
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                self.show_picker(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                self.show_drilldown(ui);
            });
        });
    }
}

impl VizApp {
    fn show_picker(&mut self, ui: &mut egui::Ui) {
        let filter = self.filter.to_lowercase();
        // Build (module_name, matching types) groups in source order so each
        // module becomes its own collapsible.
        let groups: Vec<(String, Vec<String>)> = self
            .program
            .modules
            .iter()
            .map(|m| {
                let types: Vec<String> = m
                    .items
                    .iter()
                    .filter_map(|i| match i {
                        IrItem::Type(t) => Some(t.name.clone()),
                        _ => None,
                    })
                    .filter(|n| {
                        filter.is_empty()
                            || n.to_lowercase().contains(&filter)
                            || m.name.to_lowercase().contains(&filter)
                    })
                    .collect();
                (m.name.clone(), types)
            })
            .collect();

        let any_match = groups.iter().any(|(_, ts)| !ts.is_empty());
        if !any_match {
            ui.label("(no types match)");
            return;
        }

        let filter_active = !filter.is_empty();
        for (module, types) in &groups {
            if types.is_empty() && filter_active {
                continue;
            }
            egui::CollapsingHeader::new(format!("{module}  ({})", types.len()))
                .id_source(format!("mod::{module}"))
                .default_open(filter_active)
                .show(ui, |ui| {
                    for n in types {
                        let selected = self.root.as_ref() == Some(&(module.clone(), n.clone()));
                        if ui.selectable_label(selected, n).clicked() {
                            self.root = Some((module.clone(), n.clone()));
                        }
                    }
                });
        }
    }

    fn show_drilldown(&mut self, ui: &mut egui::Ui) {
        let Some((root_mod, root_name)) = self.root.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Pick a type on the left to start drilling.");
            });
            return;
        };
        let Some(root_td) = self.program.find_type(&root_mod, &root_name) else {
            ui.label(format!("unknown type: {root_mod}:{root_name}"));
            return;
        };

        ui.heading(&root_td.name);
        ui.label(egui::RichText::new(format!("module: {root_mod}")).weak());
        if let Some(doc) = &root_td.doc {
            ui.add_space(4.0);
            ui.label(doc);
        }
        ui.separator();
        ui.label(format!("{} ::= {}", root_td.name, render_type(&root_td.ty)));
        ui.separator();

        let visited = vec![(root_mod.clone(), root_name.clone())];
        render_body(ui, &self.program, &root_mod, &[], &root_td.ty, &visited);
    }
}

// ---------------------------------------------------------------------------
// Drill-down rendering
// ---------------------------------------------------------------------------
//
// Invariants:
//   * `current_mod`   : module whose lexical scope owns unqualified references.
//   * `path`          : breadcrumb of field names from the current root
//                       (used to build stable egui ids).
//   * `visited`       : (module, type_name) pairs entered on this branch —
//                       consulted before following a Reference so we cut off
//                       cycles instead of looping.

fn render_body(
    ui: &mut egui::Ui,
    program: &IrProgram,
    current_mod: &str,
    path: &[String],
    ty: &IrType,
    visited: &[(String, String)],
) {
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => {
            render_struct(ui, program, current_mod, path, s, visited)
        }
        IrType::Choice(c) => render_choice(ui, program, current_mod, path, c, visited),
        IrType::Enumerated { items, extensible } => {
            for i in items {
                let v = i.value.map(|v| format!(" = {v}")).unwrap_or_default();
                let ext = if i.is_extension { "[ext] " } else { "" };
                ui.label(format!("• {ext}{}{v}", i.name));
            }
            if *extensible {
                ui.label("…");
            }
        }
        // Only emit an [element] node if the element itself has structure
        // worth drilling into. For SEQUENCE OF INTEGER the parent label
        // already says it all.
        IrType::SequenceOf { element, constraints } | IrType::SetOf { element, constraints } => {
            render_constraints(ui, constraints);
            if expand(program, current_mod, element, visited).is_some() {
                let mut next_path = path.to_vec();
                next_path.push("[]".into());
                render_nested(
                    ui,
                    program,
                    current_mod,
                    &next_path,
                    "[element]",
                    element,
                    visited,
                    None,
                );
            }
        }
        IrType::Reference { module, name } => {
            let target_mod = module.clone().unwrap_or_else(|| current_mod.to_string());
            let key = (target_mod.clone(), name.clone());
            if visited.contains(&key) {
                ui.label(format!("↺ recursive: {target_mod}.{name}"));
                return;
            }
            let Some(td) = program.find_type(&target_mod, name) else {
                ui.label(format!("(unresolved reference: {target_mod}.{name})"));
                return;
            };
            let mut next = visited.to_vec();
            next.push(key);
            if let Some(doc) = &td.doc {
                ui.label(doc);
                ui.add_space(2.0);
            }
            render_body(ui, program, &target_mod, path, &td.ty, &next);
        }
        IrType::Integer { named_numbers, constraints } => {
            for (n, v) in named_numbers {
                ui.label(format!("• {n} = {v}"));
            }
            render_constraints(ui, constraints);
        }
        IrType::BitString { named_bits, constraints } => {
            for (n, v) in named_bits {
                ui.label(format!("• {n} = bit {v}"));
            }
            render_constraints(ui, constraints);
        }
        IrType::OctetString { constraints } => render_constraints(ui, constraints),
        IrType::CharString { kind, constraints } => {
            ui.label(format!("kind: {kind:?}"));
            render_constraints(ui, constraints);
        }
        _ => {}
    }
}

fn render_constraints(ui: &mut egui::Ui, cs: &[IrConstraint]) {
    for c in cs {
        ui.label(format!("constraint: {}", render_constraint(c)));
    }
}

fn render_constraint(c: &IrConstraint) -> String {
    match c {
        IrConstraint::Range { lower, upper, extensible } => {
            let l = lower.map(|v| v.to_string()).unwrap_or_else(|| "MIN".into());
            let u = upper.map(|v| v.to_string()).unwrap_or_else(|| "MAX".into());
            let ext = if *extensible { ", ..." } else { "" };
            format!("({l}..{u}{ext})")
        }
        IrConstraint::Single(s) => format!("({s})"),
        IrConstraint::Size(inner) => format!("SIZE {}", render_constraint(inner)),
        IrConstraint::Composite(s) => format!("({s})"),
    }
}

fn render_struct(
    ui: &mut egui::Ui,
    program: &IrProgram,
    current_mod: &str,
    path: &[String],
    s: &IrStruct,
    visited: &[(String, String)],
) {
    for m in &s.members {
        match m {
            IrStructMember::Field(f) => render_field(ui, program, current_mod, path, f, visited),
            IrStructMember::ComponentsOf { type_ref } => {
                let key = (current_mod.to_string(), type_ref.clone());
                if visited.contains(&key) {
                    ui.label(format!("↳ COMPONENTS OF {type_ref}  (↺ recursive)"));
                    continue;
                }
                match program.find_type(current_mod, type_ref) {
                    Some(td) => {
                        let mut next = visited.to_vec();
                        next.push(key);
                        let id = node_id(&next, path, &format!("components-of:{type_ref}"));
                        egui::CollapsingHeader::new(format!("↳ COMPONENTS OF {type_ref}"))
                            .id_source(id)
                            .default_open(true)
                            .show(ui, |ui| {
                                if let Some(doc) = &td.doc {
                                    ui.label(doc);
                                    ui.add_space(2.0);
                                }
                                render_body(ui, program, current_mod, path, &td.ty, &next);
                            });
                    }
                    None => {
                        ui.label(format!("↳ COMPONENTS OF {type_ref}  (unresolved)"));
                    }
                }
            }
        }
    }
    if s.extensible {
        ui.label("…");
    }
}

fn render_choice(
    ui: &mut egui::Ui,
    program: &IrProgram,
    current_mod: &str,
    path: &[String],
    c: &IrChoice,
    visited: &[(String, String)],
) {
    for a in &c.alternatives {
        render_field(ui, program, current_mod, path, a, visited);
    }
    if c.extensible {
        ui.label("…");
    }
}

fn render_field(
    ui: &mut egui::Ui,
    program: &IrProgram,
    current_mod: &str,
    path: &[String],
    f: &IrField,
    visited: &[(String, String)],
) {
    let suffix = match &f.optionality {
        IrOptionality::Required => String::new(),
        IrOptionality::Optional => " OPTIONAL".into(),
        IrOptionality::Default(v) => format!(" DEFAULT {v}"),
    };
    let ext = if f.is_extension { "[ext] " } else { "" };
    let label = format!("{ext}{}: {}{suffix}", f.name, render_type(&f.ty));

    let mut next_path = path.to_vec();
    next_path.push(f.name.clone());
    render_nested(ui, program, current_mod, &next_path, &label, &f.ty, visited, f.doc.as_deref());
}

/// Render one labelled node: either a leaf label, or a `CollapsingHeader`
/// whose body shows the children of `ty` (resolving through a reference if
/// needed). `field_doc` is an optional doc string attached to the field or
/// alternative this node represents; it's shown above the resolved body.
#[allow(clippy::too_many_arguments)]
fn render_nested(
    ui: &mut egui::Ui,
    program: &IrProgram,
    current_mod: &str,
    path: &[String],
    label: &str,
    ty: &IrType,
    visited: &[(String, String)],
    field_doc: Option<&str>,
) {
    match expand(program, current_mod, ty, visited) {
        None => {
            ui.label(label);
        }
        Some(Expansion::Inline) => {
            let id = node_id(visited, path, label);
            egui::CollapsingHeader::new(label).id_source(id).default_open(false).show(ui, |ui| {
                if let Some(doc) = field_doc {
                    ui.label(doc);
                    ui.add_space(2.0);
                }
                render_body(ui, program, current_mod, path, ty, visited);
            });
        }
        Some(Expansion::Cycle { target_mod, target_name }) => {
            ui.label(format!("{label}  ↺ recursive: {target_mod}.{target_name}"));
        }
        Some(Expansion::Dangling { target_mod, target_name }) => {
            ui.label(format!("{label}  (unresolved: {target_mod}.{target_name})"));
        }
        Some(Expansion::Via { target_mod, target_name, target_ty, visited: next }) => {
            let id = node_id(&next, path, label);
            // Re-find the resolved definition so we can show its own doc.
            let target_doc =
                program.find_type(&target_mod, &target_name).and_then(|td| td.doc.as_deref());
            egui::CollapsingHeader::new(label).id_source(id).default_open(false).show(ui, |ui| {
                if let Some(doc) = field_doc {
                    ui.label(doc);
                    ui.add_space(2.0);
                }
                ui.label(
                    egui::RichText::new(format!("→ {target_mod}.{target_name}")).weak().italics(),
                );
                if let Some(doc) = target_doc {
                    ui.add_space(2.0);
                    ui.label(doc);
                    ui.add_space(2.0);
                }
                render_body(ui, program, &target_mod, path, target_ty, &next);
            });
        }
    }
}

enum Expansion<'a> {
    /// `ty` is itself a composite — expand its children in place.
    Inline,
    /// `ty` is a reference that resolves; expand the referent's body under a
    /// new `(module, type)` visited frame.
    Via {
        target_mod: String,
        target_name: String,
        target_ty: &'a IrType,
        visited: Vec<(String, String)>,
    },
    /// Reference would re-enter a type already on the current branch.
    Cycle { target_mod: String, target_name: String },
    /// Reference could not be resolved against the program.
    Dangling { target_mod: String, target_name: String },
}

fn expand<'a>(
    program: &'a IrProgram,
    current_mod: &str,
    ty: &'a IrType,
    visited: &[(String, String)],
) -> Option<Expansion<'a>> {
    match ty {
        IrType::Sequence(_)
        | IrType::Set(_)
        | IrType::Choice(_)
        | IrType::SequenceOf { .. }
        | IrType::SetOf { .. } => Some(Expansion::Inline),
        IrType::Enumerated { items, .. } if !items.is_empty() => Some(Expansion::Inline),
        IrType::Reference { module, name } => {
            let target_mod = module.clone().unwrap_or_else(|| current_mod.to_string());
            let key = (target_mod.clone(), name.clone());
            if visited.contains(&key) {
                return Some(Expansion::Cycle { target_mod, target_name: name.clone() });
            }
            match program.find_type(&target_mod, name) {
                Some(td) => {
                    let mut next = visited.to_vec();
                    next.push(key);
                    Some(Expansion::Via {
                        target_mod,
                        target_name: name.clone(),
                        target_ty: &td.ty,
                        visited: next,
                    })
                }
                None => Some(Expansion::Dangling { target_mod, target_name: name.clone() }),
            }
        }
        _ => None,
    }
}

fn node_id(visited: &[(String, String)], path: &[String], label: &str) -> String {
    let v: Vec<String> = visited.iter().map(|(m, n)| format!("{m}.{n}")).collect();
    format!("{}#{}#{}", v.join("/"), path.join("/"), label)
}

// ---------------------------------------------------------------------------
// Standalone HTML export
// ---------------------------------------------------------------------------
//
// Mirrors the egui UI: each field can drill through reference aliases inline
// (showing the referent's doc + body), primitive aliases reveal their named
// numbers / bits / constraints, and cycles are cut off with a recursive
// marker. A sticky header exposes creator / version info, expand-all /
// collapse-all controls, and a Light / Dark / Grey theme picker whose choice
// is persisted in `localStorage`.

/// Render the IR as a self-contained HTML document using `<details>` /
/// `<summary>` for native click-to-expand, requiring no external assets.
pub fn export_html(program: &IrProgram) -> String {
    let mut out = String::new();
    out.push_str(HTML_HEAD);
    out.push_str("<header>\n  <h1>asn1-decoder</h1>\n");
    out.push_str(&format!(
        "  <span class=\"info\">v{} — by {}</span>\n",
        env!("CARGO_PKG_VERSION"),
        html_escape(&env!("CARGO_PKG_AUTHORS").replace(':', ", ")),
    ));
    out.push_str(HTML_HEADER_CONTROLS);
    let type_total: usize = program.all_types().count();
    out.push_str(&format!(
        "<div class=\"meta\">{} module(s), {} type(s)</div>\n",
        program.modules.len(),
        type_total,
    ));
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
<html lang="en" data-theme="dark">
<head>
<meta charset="utf-8">
<title>asn1-decoder — tree</title>
<style>
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
.unresolved { color: var(--unresolved); font-style: italic; }
.constraint { color: var(--muted); padding: .1rem 0 .1rem 1.25rem; }
.named { padding: .1rem 0 .1rem 1.25rem; }
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
    <option value="dark" selected>Dark</option>
    <option value="grey">Grey</option>
  </select>
</header>
<main>
"#;

const HTML_TAIL: &str = r#"</main>
<script>
(function(){
  try {
    var t = localStorage.getItem('asn1-theme');
    if (t === 'light' || t === 'dark' || t === 'grey') {
      document.documentElement.setAttribute('data-theme', t);
      var sel = document.getElementById('theme-sel');
      if (sel) sel.value = t;
    }
  } catch (e) {}
})();
</script>
</body>
</html>
"#;

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
        "<details class=\"module\" open><summary>{} <span class=\"note\">({} types)</span></summary>\n",
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
    let summary = format!(
        "<span id=\"{anchor}\" class=\"name\">{}</span> <span class=\"kw\">::=</span> {}",
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
            if html_expandable(program, module, element, visited) {
                out.push_str("<details><summary><span class=\"kw\">[element]</span> ");
                out.push_str(&html_type_ref_or_plain(module, element));
                out.push_str("</summary>\n");
                html_type_body(out, program, module, element, visited);
                out.push_str("</details>\n");
            } else {
                out.push_str(&format!(
                    "<div class=\"leaf\"><span class=\"kw\">[element]</span> {}</div>\n",
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
    let summary = format!(
        "<span class=\"name\">{}</span>: {}{}{ext}",
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
                None => true, // worth emitting the "(unresolved)" marker
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
    use asn1_ir::{
        IrField, IrItem, IrModule, IrOptionality, IrProgram, IrStruct, IrStructMember, IrType,
        IrTypeDef,
    };

    fn program_with_reference_chain() -> IrProgram {
        let id = IrTypeDef {
            name: "Id".into(),
            doc: None,
            ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
        };
        let inner = IrTypeDef {
            name: "Inner".into(),
            doc: None,
            ty: IrType::Sequence(IrStruct {
                extensible: false,
                members: vec![IrStructMember::Field(IrField {
                    doc: None,
                    name: "id".into(),
                    ty: IrType::Reference { module: Some("M".into()), name: "Id".into() },
                    optionality: IrOptionality::Required,
                    is_extension: false,
                })],
            }),
        };
        let outer = IrTypeDef {
            name: "Outer".into(),
            doc: None,
            ty: IrType::Sequence(IrStruct {
                extensible: false,
                members: vec![IrStructMember::Field(IrField {
                    doc: None,
                    name: "inner".into(),
                    ty: IrType::Reference { module: Some("M".into()), name: "Inner".into() },
                    optionality: IrOptionality::Required,
                    is_extension: false,
                })],
            }),
        };
        IrProgram {
            modules: vec![IrModule {
                name: "M".into(),
                oid: None,
                imports: vec![],
                items: vec![IrItem::Type(id), IrItem::Type(inner), IrItem::Type(outer)],
            }],
        }
    }

    #[test]
    fn expand_follows_reference_to_composite() {
        let p = program_with_reference_chain();
        let outer_ref = IrType::Reference { module: Some("M".into()), name: "Outer".into() };
        let visited = vec![];
        match expand(&p, "M", &outer_ref, &visited) {
            Some(Expansion::Via { target_mod, target_name, .. }) => {
                assert_eq!(target_mod, "M");
                assert_eq!(target_name, "Outer");
            }
            other => panic!("expected Via, got {:?}", label(&other)),
        }
    }

    #[test]
    fn expand_follows_reference_to_primitive() {
        // References to primitive aliases are expandable so the user can see
        // the referent's doc, named numbers, and constraints.
        let p = program_with_reference_chain();
        let id_ref = IrType::Reference { module: Some("M".into()), name: "Id".into() };
        match expand(&p, "M", &id_ref, &[]) {
            Some(Expansion::Via { target_name, .. }) => assert_eq!(target_name, "Id"),
            other => panic!("expected Via, got {:?}", label(&other)),
        }
    }

    #[test]
    fn expand_detects_cycle() {
        let p = program_with_reference_chain();
        let outer_ref = IrType::Reference { module: Some("M".into()), name: "Outer".into() };
        let visited = vec![("M".into(), "Outer".into())];
        match expand(&p, "M", &outer_ref, &visited) {
            Some(Expansion::Cycle { target_mod, target_name }) => {
                assert_eq!(target_mod, "M");
                assert_eq!(target_name, "Outer");
            }
            other => panic!("expected Cycle, got {:?}", label(&other)),
        }
    }

    #[test]
    fn expand_flags_dangling_reference() {
        let p = program_with_reference_chain();
        let missing = IrType::Reference { module: Some("M".into()), name: "Gone".into() };
        match expand(&p, "M", &missing, &[]) {
            Some(Expansion::Dangling { target_name, .. }) => assert_eq!(target_name, "Gone"),
            other => panic!("expected Dangling, got {:?}", label(&other)),
        }
    }

    fn label(e: &Option<Expansion<'_>>) -> &'static str {
        match e {
            None => "None",
            Some(Expansion::Inline) => "Inline",
            Some(Expansion::Via { .. }) => "Via",
            Some(Expansion::Cycle { .. }) => "Cycle",
            Some(Expansion::Dangling { .. }) => "Dangling",
        }
    }

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
                        ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
                        optionality: IrOptionality::Required,
                        is_extension: false,
                    }),
                    IrStructMember::Field(IrField {
                        doc: None,
                        name: "y".into(),
                        ty: IrType::Integer { named_numbers: vec![], constraints: vec![] },
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
        assert!(html.contains("— by "), "header should show creator(s)");
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

    fn program_with_self_reference() -> IrProgram {
        // Node ::= SEQUENCE { child Node OPTIONAL }
        let node = IrTypeDef {
            name: "Node".into(),
            doc: None,
            ty: IrType::Sequence(IrStruct {
                extensible: false,
                members: vec![IrStructMember::Field(IrField {
                    doc: None,
                    name: "child".into(),
                    ty: IrType::Reference { module: Some("M".into()), name: "Node".into() },
                    optionality: IrOptionality::Optional,
                    is_extension: false,
                })],
            }),
        };
        IrProgram {
            modules: vec![IrModule {
                name: "M".into(),
                oid: None,
                imports: vec![],
                items: vec![IrItem::Type(node)],
            }],
        }
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
