//! Visualizer theme palettes — Light and Dark use egui's built-ins, Grey is a
//! hand-tuned mid-tone neutral for users who find dark too harsh and light too
//! bright.

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Theme {
    Light,
    Dark,
    Grey,
}

impl Theme {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Grey => "Grey",
        }
    }

    pub(crate) fn visuals(self) -> egui::Visuals {
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
