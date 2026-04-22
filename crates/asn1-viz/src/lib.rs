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
