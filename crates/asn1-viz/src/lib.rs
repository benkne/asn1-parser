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
mod docfmt;
mod html;
mod loader;
mod theme;
mod tree;

#[cfg(test)]
mod test_fixtures;

pub use app::{launch, launch_with_options, Icon, LaunchOptions};
pub use html::export_html;

use asn1_ir::IrConstraint;

/// Subtle yellow-orange used for warnings and unresolved-reference labels,
/// shared between the egui UI (the header warning chip, the diagnostics
/// window, and inline `warn_leaf` labels).
pub(crate) const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(0xd2, 0x99, 0x22);

/// Pretty-print one constraint node into renderer-agnostic parts. Returns
/// `(label, body)` where `label` is one of `"range"`, `"size"`, `"value"`, or
/// `"constraint"` (composite fallback) and `body` is the bound expression
/// already normalised — open ranges become `≥ N` / `≤ N`, extensible ranges
/// get a trailing `, …`. `Size(inner)` is flattened so a SIZE-of-range reads
/// `size: 1 … 16` rather than `size: range: 1 … 16`.
///
/// Shared between the egui tree renderer and the HTML exporter so both views
/// agree on constraint wording; each renderer styles `label` / `body`
/// independently.
pub(crate) fn describe_constraint(c: &IrConstraint) -> (&'static str, String) {
    match c {
        IrConstraint::Range { lower, upper, extensible } => {
            ("range", format_range(*lower, *upper, *extensible))
        }
        IrConstraint::Single(s) => ("value", s.clone()),
        IrConstraint::Size(inner) => {
            let (_, body) = describe_constraint(inner);
            ("size", body)
        }
        IrConstraint::Composite(s) => ("constraint", s.clone()),
    }
}

fn format_range(lower: Option<i64>, upper: Option<i64>, extensible: bool) -> String {
    let ext = if extensible { ", …" } else { "" };
    match (lower, upper) {
        (Some(l), Some(u)) => format!("{l} … {u}{ext}"),
        (None, Some(u)) => format!("≤ {u}{ext}"),
        (Some(l), None) => format!("≥ {l}{ext}"),
        (None, None) => format!("any{ext}"),
    }
}
