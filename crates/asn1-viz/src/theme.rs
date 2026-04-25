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
    /// Pick Light or Dark based on the OS's reported preference; Grey is an
    /// opt-in choice only. Falls back to Dark when detection fails.
    pub(crate) fn system_default() -> Self {
        match dark_light::detect() {
            dark_light::Mode::Light => Theme::Light,
            dark_light::Mode::Dark | dark_light::Mode::Default => Theme::Dark,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Grey => "Grey",
        }
    }

    /// Stable lowercase key used when persisting the theme to disk and
    /// matching the `data-theme` attribute used by the HTML export.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Grey => "grey",
        }
    }

    /// Parse a stored theme key. Accepts `gray` as a synonym for `grey`. Whitespace
    /// is trimmed and comparison is case-insensitive.
    pub(crate) fn from_key(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            "grey" | "gray" => Some(Theme::Grey),
            _ => None,
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
