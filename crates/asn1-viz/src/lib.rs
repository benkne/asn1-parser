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
//!
//! Also exposes a self-contained HTML export ([`export_html`]) that mirrors
//! the egui UI for offline sharing.

#![deny(rust_2018_idioms)]

mod app;
mod html;
mod loader;
mod theme;
mod tree;

#[cfg(test)]
mod test_fixtures;

pub use app::launch;
pub use html::export_html;

use asn1_ir::IrConstraint;

/// Subtle yellow-orange used for warnings and unresolved-reference labels,
/// shared between the egui UI (the header warning chip, the diagnostics
/// window, and inline `warn_leaf` labels).
pub(crate) const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(0xd2, 0x99, 0x22);

/// Pretty-print one constraint node. Shared between the egui tree renderer
/// and the HTML exporter so both views agree on constraint wording.
pub(crate) fn render_constraint(c: &IrConstraint) -> String {
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
