//! Top-level eframe app: window chrome, menus, picker, and drill-down driver.

use std::collections::HashSet;
use std::path::PathBuf;

use asn1_codegen_cpp::{generate as cpp_generate, CppOptions};
use asn1_codegen_java::{generate as java_generate, JavaOptions};
use asn1_ir::{render_type, IrDiagnostic, IrItem, IrProgram, IrStructMember, IrType};

use crate::loader::load_program;
use crate::theme::Theme;
use crate::tree::render_body;
use crate::WARN_COLOR;

/// Raw RGBA window-icon bytes. Kept as a plain struct so callers don't need
/// to depend on `egui` directly.
pub struct Icon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Knobs that the standalone `asn1-viz` binary needs to customize but that
/// the CLI's inline `visualize` subcommand leaves at defaults.
#[derive(Default)]
pub struct LaunchOptions {
    /// Window / taskbar icon. `None` keeps the platform default.
    pub icon: Option<Icon>,
    /// File path to load/store the user's theme choice. `None` disables
    /// persistence — the CLI's transient `visualize` launches use this so
    /// they don't write user state.
    pub theme_store_path: Option<PathBuf>,
}

/// Launch the visualizer UI with default options. `initial_paths` are loaded
/// at startup (same as if the user imported them via File → Open); pass an
/// empty slice to open an empty window. Blocks until the window is closed.
pub fn launch(initial_paths: Vec<PathBuf>) -> eframe::Result<()> {
    launch_with_options(initial_paths, LaunchOptions::default())
}

/// Like [`launch`] but lets the caller override window-level options such as
/// the taskbar icon. Used by the `asn1-tool` desktop binary.
pub fn launch_with_options(initial_paths: Vec<PathBuf>, opts: LaunchOptions) -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 780.0])
        .with_title("asn1-tool — visualizer")
        .with_app_id("asn1-tool");
    let icon = opts.icon.or_else(default_icon);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(egui::IconData {
            rgba: icon.rgba,
            width: icon.width,
            height: icon.height,
        }));
    }

    let native = eframe::NativeOptions { viewport, ..Default::default() };
    let theme_store = opts.theme_store_path;
    eframe::run_native(
        "asn1-tool",
        native,
        Box::new(move |cc| {
            install_symbol_fallback_font(&cc.egui_ctx);
            Box::new(VizApp::new(initial_paths, theme_store))
        }),
    )
}

/// Decode the embedded default window icon — the same star-with-`asn1 tool`
/// glyph the standalone desktop binary used to load explicitly. Returning
/// `None` on decode failure keeps the app launching with the platform default
/// icon rather than aborting.
fn default_icon() -> Option<Icon> {
    const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");
    match image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            Some(Icon { rgba: rgba.into_raw(), width, height })
        }
        Err(_) => None,
    }
}

/// Append a Unicode-symbol fallback font (DejaVu Sans Mono — covers the full
/// Arrows block, Geometric Shapes, Misc Symbols, etc.) to both the
/// proportional and monospace families. egui's bundled fonts ship only the
/// Latin block, so without this fallback `→`, `↺`, `↳`, `⚠` and friends
/// render as missing-glyph boxes.
///
/// The fallback is added *after* the default font, so Latin text still
/// renders with egui's primary face — DejaVu Sans Mono only kicks in for
/// codepoints the primary font doesn't cover.
fn install_symbol_fallback_font(ctx: &egui::Context) {
    const SYMBOL_FONT: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("symbol_fallback".to_owned(), egui::FontData::from_static(SYMBOL_FONT));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("symbol_fallback".to_owned());
    }
    ctx.set_fonts(fonts);
}

struct VizApp {
    /// `None` until something has been imported (either by the CLI caller or
    /// via File → Open).
    program: Option<IrProgram>,
    filter: String,
    /// When true, the picker filter also matches against doc comments,
    /// field names, enum item names, etc. — not just top-level type names.
    filter_in_body: bool,
    /// Currently-focused root type as `(module, type_name)`.
    root: Option<(String, String)>,
    theme: Theme,
    /// Where to persist `theme` between runs, if anywhere. `None` keeps the
    /// theme transient (CLI launches).
    theme_store_path: Option<PathBuf>,
    about_open: bool,
    diagnostics: Vec<IrDiagnostic>,
    diagnostics_open: bool,
    /// All paths (files or directories) currently contributing to `program`,
    /// in the order they were imported. Kept so that "Add file…" / "Add
    /// directory…" can reparse the full set — adding new sources may
    /// resolve references that were previously unresolved.
    loaded_paths: Vec<PathBuf>,
    /// Module names the user has dismissed via the × button on the picker.
    /// These modules are still parsed but dropped before lowering so their
    /// references surface as unresolved-reference warnings.
    excluded_modules: HashSet<String>,
    /// Rendered parse errors from the last load. Non-empty keeps the errors
    /// window on-screen so the user notices.
    load_errors: Vec<String>,
    load_errors_open: bool,
    /// Output lines from the most recent code-generation run (success or errors).
    codegen_log: Vec<String>,
    codegen_log_open: bool,
}

impl VizApp {
    fn new(initial_paths: Vec<PathBuf>, theme_store_path: Option<PathBuf>) -> Self {
        let theme = theme_store_path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| Theme::from_key(&s))
            .unwrap_or_else(Theme::system_default);
        let mut app = Self {
            program: None,
            filter: String::new(),
            filter_in_body: false,
            root: None,
            theme,
            theme_store_path,
            about_open: false,
            diagnostics: Vec::new(),
            diagnostics_open: false,
            loaded_paths: Vec::new(),
            excluded_modules: HashSet::new(),
            load_errors: Vec::new(),
            load_errors_open: false,
            codegen_log: Vec::new(),
            codegen_log_open: false,
        };
        if !initial_paths.is_empty() {
            app.replace_with(initial_paths);
        }
        app
    }

    /// Best-effort write of the current theme to `theme_store_path`. Errors
    /// (no path configured, parent dir missing, read-only filesystem) are
    /// silently swallowed — theme persistence is not critical and shouldn't
    /// disrupt the UI.
    fn save_theme(&self) {
        let Some(path) = &self.theme_store_path else { return };
        let _ = std::fs::write(path, self.theme.key());
    }

    /// Parse `paths` as the *complete* source set and replace current state.
    /// Resets filter/root/exclusions since the program has changed out from
    /// under them.
    fn replace_with(&mut self, paths: Vec<PathBuf>) {
        self.excluded_modules.clear();
        let loaded = load_program(&paths, &self.excluded_modules);
        self.diagnostics = loaded.program.diagnostics();
        self.program = Some(loaded.program);
        self.filter.clear();
        self.root = None;
        self.loaded_paths = paths;
        self.load_errors = loaded.parse_errors;
        self.load_errors_open = !self.load_errors.is_empty();
    }

    /// Extend the loaded path list with `extra` and re-parse everything.
    /// Filter and root are preserved — the user may be mid-drilldown — and
    /// the root only clears if it no longer resolves in the new program.
    /// Existing module exclusions are honored.
    fn append_paths(&mut self, extra: Vec<PathBuf>) {
        if extra.is_empty() {
            return;
        }
        let mut all = std::mem::take(&mut self.loaded_paths);
        all.extend(extra);
        self.loaded_paths = all;
        self.reload();
    }

    /// Add `name` to the excluded-modules set and reload. The module stays
    /// in `loaded_paths` (so it can be resurrected by clearing the exclusion
    /// or reopening) but drops out of the IR.
    fn remove_module(&mut self, name: String) {
        self.excluded_modules.insert(name);
        self.reload();
    }

    /// Re-parse `loaded_paths` honoring the current `excluded_modules` set.
    /// Preserves filter and root (clearing root only if it no longer
    /// resolves in the new program).
    fn reload(&mut self) {
        let loaded = load_program(&self.loaded_paths, &self.excluded_modules);
        self.diagnostics = loaded.program.diagnostics();
        if let Some((m, n)) = &self.root {
            if loaded.program.find_type(m, n).is_none() {
                self.root = None;
            }
        }
        self.program = Some(loaded.program);
        self.load_errors = loaded.parse_errors;
        self.load_errors_open = !self.load_errors.is_empty();
    }

    fn clear(&mut self) {
        self.program = None;
        self.filter.clear();
        self.root = None;
        self.diagnostics.clear();
        self.diagnostics_open = false;
        self.loaded_paths.clear();
        self.excluded_modules.clear();
        self.load_errors.clear();
        self.load_errors_open = false;
    }

    fn pick_files(title: &str) -> Option<Vec<PathBuf>> {
        rfd::FileDialog::new().add_filter("ASN.1 source", &["asn"]).set_title(title).pick_files()
    }

    fn pick_folder(title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    fn import_file(&mut self) {
        if let Some(paths) = Self::pick_files("Open ASN.1 file(s)") {
            self.replace_with(paths);
        }
    }

    fn import_directory(&mut self) {
        if let Some(path) = Self::pick_folder("Open directory of .asn files") {
            self.replace_with(vec![path]);
        }
    }

    fn add_file(&mut self) {
        if let Some(paths) = Self::pick_files("Add ASN.1 file(s)") {
            self.append_paths(paths);
        }
    }

    fn add_directory(&mut self) {
        if let Some(path) = Self::pick_folder("Add directory of .asn files") {
            self.append_paths(vec![path]);
        }
    }

    fn export_html(&mut self) {
        let Some(program) = &self.program else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HTML", &["html", "htm"])
            .set_file_name("asn1-tree.html")
            .set_title("Export HTML tree")
            .save_file()
        else {
            return;
        };
        let html = crate::html::export_html(program);
        if let Err(e) = std::fs::write(&path, &html) {
            self.load_errors.push(format!("writing {}: {e}", path.display()));
            self.load_errors_open = true;
        }
    }

    fn generate_java(&mut self) {
        let Some(program) = &self.program else { return };
        let Some(out_dir) = Self::pick_folder("Select output directory for Java sources") else {
            return;
        };
        let files = java_generate(program, &JavaOptions::default());
        self.codegen_log.clear();
        let mut errors = Vec::new();
        for f in &files {
            let dest = out_dir.join(&f.relative_path);
            if let Some(parent) = dest.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    errors.push(format!("error creating {}: {e}", parent.display()));
                    continue;
                }
            }
            if let Err(e) = std::fs::write(&dest, &f.contents) {
                errors.push(format!("error writing {}: {e}", dest.display()));
            }
        }
        if errors.is_empty() {
            self.codegen_log.push(format!(
                "Done — {} Java file(s) written to:\n{}",
                files.len(),
                out_dir.display()
            ));
        } else {
            self.codegen_log.extend(errors);
        }
        self.codegen_log_open = true;
    }

    fn generate_cpp(&mut self) {
        let Some(program) = &self.program else { return };
        let Some(out_dir) = Self::pick_folder("Select output directory for C++ headers") else {
            return;
        };
        let files = cpp_generate(program, &CppOptions::default());
        self.codegen_log.clear();
        let mut errors = Vec::new();
        for f in &files {
            let dest = out_dir.join(&f.relative_path);
            if let Some(parent) = dest.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    errors.push(format!("error creating {}: {e}", parent.display()));
                    continue;
                }
            }
            if let Err(e) = std::fs::write(&dest, &f.contents) {
                errors.push(format!("error writing {}: {e}", dest.display()));
            }
        }
        if errors.is_empty() {
            self.codegen_log.push(format!(
                "Done — {} C++ file(s) written to:\n{}",
                files.len(),
                out_dir.display()
            ));
        } else {
            self.codegen_log.extend(errors);
        }
        self.codegen_log_open = true;
    }
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
                    let has_program = self.program.is_some();
                    if ui
                        .add_enabled(has_program, egui::Button::new("Add file…"))
                        .on_hover_text(
                            "Import an additional .asn file alongside the current sources",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.add_file();
                    }
                    if ui
                        .add_enabled(has_program, egui::Button::new("Add directory…"))
                        .on_hover_text(
                            "Import an additional directory alongside the current sources",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.add_directory();
                    }
                    ui.separator();
                    if ui.add_enabled(has_program, egui::Button::new("Close")).clicked() {
                        ui.close_menu();
                        self.clear();
                    }
                });

                ui.menu_button("Tools", |ui| {
                    let has_program = self.program.is_some();
                    if ui
                        .add_enabled(has_program, egui::Button::new("Export HTML…"))
                        .on_hover_text("Save the current tree as a standalone HTML file")
                        .clicked()
                    {
                        ui.close_menu();
                        self.export_html();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(has_program, egui::Button::new("Generate Java…"))
                        .on_hover_text(
                            "Generate Java 17 source files from the loaded ASN.1 modules",
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.generate_java();
                    }
                    if ui
                        .add_enabled(has_program, egui::Button::new("Generate C++…"))
                        .on_hover_text("Generate C++ header files from the loaded ASN.1 modules")
                        .clicked()
                    {
                        ui.close_menu();
                        self.generate_cpp();
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.label("Theme");
                    ui.separator();
                    for t in [Theme::Light, Theme::Dark, Theme::Grey] {
                        if ui.radio(self.theme == t, t.label()).clicked() {
                            self.theme = t;
                            self.save_theme();
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
            .default_width(1000.0)
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

        let mut codegen_open = self.codegen_log_open;
        egui::Window::new("Code generation")
            .open(&mut codegen_open)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height(200.0)
            .default_pos(egui::pos2(320.0, 200.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for line in &self.codegen_log {
                        ui.label(egui::RichText::new(line).monospace());
                    }
                });
            });
        self.codegen_log_open = codegen_open;

        egui::SidePanel::left("picker").resizable(true).default_width(360.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let stroke = ui.visuals().widgets.inactive.bg_stroke;
                let rounding = ui.visuals().widgets.inactive.rounding;
                let fill = ui.visuals().extreme_bg_color;
                egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let btn = ui.add_enabled(
                                    !self.filter.is_empty(),
                                    egui::Button::new("✕").frame(false),
                                );
                                if btn.clicked() {
                                    self.filter.clear();
                                }
                                ui.add_sized(
                                    [ui.available_width(), ui.spacing().interact_size.y],
                                    egui::TextEdit::singleline(&mut self.filter).frame(false),
                                );
                            },
                        );
                    });
            });
            ui.checkbox(&mut self.filter_in_body, "search all");
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
        let in_body = self.filter_in_body;
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
                        IrItem::Type(t) => Some(t),
                        _ => None,
                    })
                    .filter(|t| {
                        if filter.is_empty() {
                            return true;
                        }
                        if t.name.to_lowercase().contains(&filter)
                            || m.name.to_lowercase().contains(&filter)
                        {
                            return true;
                        }
                        if in_body {
                            if t.doc
                                .as_deref()
                                .is_some_and(|d| d.to_lowercase().contains(&filter))
                            {
                                return true;
                            }
                            if type_body_contains(&t.ty, &filter) {
                                return true;
                            }
                        }
                        false
                    })
                    .map(|t| t.name.clone())
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
        // Deferred so we don't mutate `self` while we're iterating over
        // `&self.program`'s derived groups.
        let mut to_remove: Option<String> = None;
        for (module, types) in &groups {
            if types.is_empty() && filter_active {
                continue;
            }
            let id = ui.make_persistent_id(format!("mod::{module}"));
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                filter_active,
            );
            let header = state.show_header(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{module}  ({})", types.len())).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text(
                                "Remove this module from view (may produce new warnings)",
                            )
                            .clicked()
                        {
                            to_remove = Some(module.clone());
                        }
                    });
                });
            });
            header.body(|ui| {
                for n in types {
                    let selected = self.root.as_ref() == Some(&(module.clone(), n.clone()));
                    if ui.selectable_label(selected, n).clicked() {
                        self.root = Some((module.clone(), n.clone()));
                    }
                }
            });
        }
        if let Some(name) = to_remove {
            self.remove_module(name);
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
            crate::docfmt::render_egui(
                ui,
                doc,
                &format!("root-{root_mod}-{root_name}"),
                &crate::tree::required_field_names(&root_td.ty),
            );
        }
        ui.separator();
        ui.label(format!("{} ::= {}", root_td.name, render_type(&root_td.ty)));
        ui.separator();

        let visited = vec![(root_mod.clone(), root_name.clone())];
        let ty = root_td.ty.clone();
        render_body(ui, program, &root_mod, &[], &ty, &visited);
    }
}

/// Recursive case-insensitive substring match against a type's body — field
/// names, alternative names, enum item names, named-number labels, plus their
/// doc comments. `needle` must already be lower-cased. References are matched
/// by their rendered name only (no follow-through, to avoid cycles).
fn type_body_contains(ty: &IrType, needle: &str) -> bool {
    let str_hit = |s: &str| s.to_lowercase().contains(needle);
    let opt_hit = |s: &Option<String>| s.as_deref().is_some_and(str_hit);
    match ty {
        IrType::Sequence(s) | IrType::Set(s) => s.members.iter().any(|m| match m {
            IrStructMember::Field(f) => {
                str_hit(&f.name) || opt_hit(&f.doc) || type_body_contains(&f.ty, needle)
            }
            IrStructMember::ComponentsOf { type_ref } => str_hit(type_ref),
        }),
        IrType::Choice(c) => c.alternatives.iter().any(|f| {
            str_hit(&f.name) || opt_hit(&f.doc) || type_body_contains(&f.ty, needle)
        }),
        IrType::Enumerated { items, .. } => {
            items.iter().any(|i| str_hit(&i.name) || opt_hit(&i.doc))
        }
        IrType::Integer { named_numbers, .. } => named_numbers.iter().any(|(n, _)| str_hit(n)),
        IrType::BitString { named_bits, .. } => named_bits.iter().any(|(n, _)| str_hit(n)),
        IrType::SequenceOf { element, .. } | IrType::SetOf { element, .. } => {
            type_body_contains(element, needle)
        }
        IrType::Reference { module, name } => {
            module.as_deref().is_some_and(str_hit) || str_hit(name)
        }
        _ => false,
    }
}
