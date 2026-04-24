//! Top-level eframe app: window chrome, menus, picker, and drill-down driver.

use std::path::Path;

use asn1_ir::{render_type, IrDiagnostic, IrItem, IrProgram};

use crate::loader::{load_program, LoadedProgram};
use crate::theme::Theme;
use crate::tree::render_body;
use crate::WARN_COLOR;

/// Launch the visualizer UI. If `program` is `None`, the window opens empty
/// and the user can load sources via the File menu. Blocks until the window
/// is closed.
pub fn launch(program: Option<IrProgram>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 780.0])
            .with_title("asn1-tool — visualizer"),
        ..Default::default()
    };
    eframe::run_native("asn1-tool", options, Box::new(|_cc| Box::new(VizApp::new(program))))
}

struct VizApp {
    /// `None` until something has been imported (either by the CLI caller or
    /// via File → Open).
    program: Option<IrProgram>,
    filter: String,
    /// Currently-focused root type as `(module, type_name)`.
    root: Option<(String, String)>,
    theme: Theme,
    about_open: bool,
    diagnostics: Vec<IrDiagnostic>,
    diagnostics_open: bool,
    /// Human-readable label for what is currently loaded (file name or
    /// folder name). Shown in the header.
    loaded_label: Option<String>,
    /// Rendered parse errors from the last load. Non-empty keeps the errors
    /// window on-screen so the user notices.
    load_errors: Vec<String>,
    load_errors_open: bool,
}

impl VizApp {
    fn new(program: Option<IrProgram>) -> Self {
        let diagnostics = program.as_ref().map(IrProgram::diagnostics).unwrap_or_default();
        Self {
            program,
            filter: String::new(),
            root: None,
            theme: Theme::system_default(),
            about_open: false,
            diagnostics,
            diagnostics_open: false,
            loaded_label: None,
            load_errors: Vec::new(),
            load_errors_open: false,
        }
    }

    fn apply_loaded(&mut self, loaded: LoadedProgram, label: String) {
        self.diagnostics = loaded.program.diagnostics();
        self.program = Some(loaded.program);
        self.filter.clear();
        self.root = None;
        self.loaded_label = Some(label);
        self.load_errors = loaded.parse_errors;
        self.load_errors_open = !self.load_errors.is_empty();
    }

    fn clear(&mut self) {
        self.program = None;
        self.filter.clear();
        self.root = None;
        self.diagnostics.clear();
        self.diagnostics_open = false;
        self.loaded_label = None;
        self.load_errors.clear();
        self.load_errors_open = false;
    }

    fn import_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("ASN.1 source", &["asn"])
            .set_title("Open ASN.1 file")
            .pick_file()
        {
            let label = label_for(&path);
            self.apply_loaded(load_program(&[path]), label);
        }
    }

    fn import_directory(&mut self) {
        if let Some(path) =
            rfd::FileDialog::new().set_title("Open directory of .asn files").pick_folder()
        {
            let label = label_for(&path);
            self.apply_loaded(load_program(&[path]), label);
        }
    }
}

fn label_for(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl eframe::App for VizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(self.theme.visuals());

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.heading("asn1-tool");
                ui.separator();

                ui.menu_button("File", |ui| {
                    if ui.button("Open file…").clicked() {
                        ui.close_menu();
                        self.import_file();
                    }
                    if ui.button("Open directory…").clicked() {
                        ui.close_menu();
                        self.import_directory();
                    }
                    ui.separator();
                    let can_close = self.program.is_some();
                    if ui.add_enabled(can_close, egui::Button::new("Close")).clicked() {
                        ui.close_menu();
                        self.clear();
                    }
                });

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
                match &self.program {
                    Some(p) => ui.label(format!("{} module(s)", p.modules.len())),
                    None => ui.label(egui::RichText::new("no sources loaded").weak().italics()),
                };
                if let Some(label) = &self.loaded_label {
                    ui.separator();
                    ui.label(egui::RichText::new(format!("source: {label}")).weak());
                }
                if !self.load_errors.is_empty() {
                    ui.separator();
                    let n = self.load_errors.len();
                    let chip = egui::RichText::new(format!(
                        "✖ {n} parse error{}",
                        if n == 1 { "" } else { "s" }
                    ))
                    .color(WARN_COLOR);
                    if ui
                        .link(chip)
                        .on_hover_text("click to view files that failed to parse")
                        .clicked()
                    {
                        self.load_errors_open = !self.load_errors_open;
                    }
                }
                if !self.diagnostics.is_empty() {
                    ui.separator();
                    let n = self.diagnostics.len();
                    let chip = egui::RichText::new(format!(
                        "⚠ {n} warning{}",
                        if n == 1 { "" } else { "s" }
                    ))
                    .color(WARN_COLOR);
                    if ui
                        .link(chip)
                        .on_hover_text("click to view unresolved types/modules")
                        .clicked()
                    {
                        self.diagnostics_open = !self.diagnostics_open;
                    }
                }
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
        egui::Window::new("About asn1-tool")
            .open(&mut about_open)
            .collapsible(false)
            .resizable(false)
            .default_pos(egui::pos2(440.0, 200.0))
            .show(ctx, |ui| {
                ui.heading("asn1-tool");
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

        let mut diag_open = self.diagnostics_open;
        egui::Window::new("Unresolved types & modules")
            .open(&mut diag_open)
            .collapsible(false)
            .resizable(true)
            .default_width(820.0)
            .default_height(560.0)
            .default_pos(egui::pos2(240.0, 120.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "These references could not be resolved against the loaded modules. \
                         The tree view still renders; missing types are shown as `(unresolved…)`.",
                    )
                    .weak()
                    .italics(),
                );
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for d in &self.diagnostics {
                        ui.label(egui::RichText::new(format!("⚠ {d}")).color(WARN_COLOR));
                    }
                });
            });
        self.diagnostics_open = diag_open;

        let mut errors_open = self.load_errors_open;
        egui::Window::new("Parse errors")
            .open(&mut errors_open)
            .collapsible(false)
            .resizable(true)
            .default_width(820.0)
            .default_height(560.0)
            .default_pos(egui::pos2(280.0, 160.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "These files failed to parse and are not part of the loaded program.",
                    )
                    .weak()
                    .italics(),
                );
                ui.separator();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for e in &self.load_errors {
                        ui.label(egui::RichText::new(e).color(WARN_COLOR).monospace());
                        ui.separator();
                    }
                });
            });
        self.load_errors_open = errors_open;

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
        let Some(program) = &self.program else {
            ui.label(
                egui::RichText::new("Use File → Open file… or Open directory… to load sources.")
                    .weak(),
            );
            return;
        };

        let filter = self.filter.to_lowercase();
        // Build (module_name, matching types) groups in source order so each
        // module becomes its own collapsible.
        let groups: Vec<(String, Vec<String>)> = program
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
        if self.program.is_none() {
            let mut want_file = false;
            let mut want_dir = false;
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading("No sources loaded");
                ui.add_space(8.0);
                ui.label("Use File → Open file… to load a single .asn file,");
                ui.label("or File → Open directory… to scan a folder.");
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    if ui.button("Open file…").clicked() {
                        want_file = true;
                    }
                    if ui.button("Open directory…").clicked() {
                        want_dir = true;
                    }
                });
            });
            if want_file {
                self.import_file();
            } else if want_dir {
                self.import_directory();
            }
            return;
        }
        let program = self.program.as_ref().unwrap();
        let Some((root_mod, root_name)) = self.root.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Pick a type on the left to start drilling.");
            });
            return;
        };
        let Some(root_td) = program.find_type(&root_mod, &root_name) else {
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
        let ty = root_td.ty.clone();
        render_body(ui, program, &root_mod, &[], &ty, &visited);
    }
}
