//! Interactive tree rendering for the egui drill-down pane.
//!
//! Invariants shared by every renderer:
//!
//! * `current_mod` — module whose lexical scope owns unqualified references.
//! * `path` — breadcrumb of field names from the current root, used to build
//!   stable egui ids.
//! * `visited` — `(module, type_name)` pairs entered on this branch,
//!   consulted before following a Reference so cycles are cut off instead of
//!   looping forever.

use asn1_ir::{
    render_type, IrChoice, IrConstraint, IrField, IrOptionality, IrProgram, IrStruct,
    IrStructMember, IrType,
};

use crate::{render_constraint, WARN_COLOR};

/// Approximate width of a `CollapsingHeader`'s disclosure triangle (▸ + gap),
/// used to line up leaf labels with their expandable siblings.
const TRIANGLE_INDENT: f32 = 18.0;

/// Leaf label that aligns with `CollapsingHeader` siblings by padding past the
/// triangle gutter, colored with [`WARN_COLOR`] so unresolved references stay
/// visually distinct from resolved ones.
fn warn_leaf(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.horizontal(|ui| {
        ui.add_space(TRIANGLE_INDENT);
        ui.label(egui::RichText::new(text.into()).color(WARN_COLOR));
    });
}

pub(crate) fn render_body(
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
                warn_leaf(ui, format!("(unresolved reference: {target_mod}.{name})"));
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
                        warn_leaf(ui, format!("↳ COMPONENTS OF {type_ref}  (unresolved)"));
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
            warn_leaf(ui, format!("{label}  (unresolved: {target_mod}.{target_name})"));
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

pub(crate) enum Expansion<'a> {
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

pub(crate) fn expand<'a>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::program_with_reference_chain;

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
}
