use egui::{self, Color32, Context, CornerRadius, Stroke, TextStyle, ThemePreference, Visuals};

// Material Design 2 brand colors. The primary highlight is fixed to the user-selected #038387.
pub const ACCENT: Color32 = Color32::from_rgb(3, 131, 135);
pub const ACCENT_DARK: Color32 = Color32::from_rgb(0, 86, 90);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(79, 179, 183);

// Dark defaults are kept for the launcher and static graph declarations.
pub const BG: Color32 = Color32::from_rgb(18, 18, 18);
pub const LIGHTER_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const SECTION_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const RAISED_BG: Color32 = Color32::from_rgb(36, 36, 36);
pub const DARKER_BG: Color32 = Color32::from_rgb(18, 18, 18);
pub const SEPARATOR_BG: Color32 = Color32::from_rgb(55, 55, 55);
pub const FG: Color32 = Color32::from_rgb(238, 238, 238);
pub const MUTED_FG: Color32 = Color32::from_rgb(158, 158, 158);
pub const OK_GREEN: Color32 = Color32::from_rgb(102, 187, 106);
pub const KO_RED: Color32 = Color32::from_rgb(207, 102, 121);

#[derive(Clone, Copy)]
pub struct Palette {
    pub accent: Color32,
    pub accent_variant: Color32,
    pub on_accent: Color32,
    pub background: Color32,
    pub navigation: Color32,
    pub surface: Color32,
    pub raised_surface: Color32,
    pub inset_surface: Color32,
    pub separator: Color32,
    pub foreground: Color32,
    pub muted_foreground: Color32,
    pub ok: Color32,
    pub error: Color32,
}

pub fn palette_for(dark_mode: bool) -> Palette {
    if dark_mode {
        Palette {
            accent: ACCENT,
            accent_variant: ACCENT_LIGHT,
            on_accent: Color32::WHITE,
            background: BG,
            navigation: LIGHTER_BG,
            surface: SECTION_BG,
            raised_surface: RAISED_BG,
            inset_surface: DARKER_BG,
            separator: SEPARATOR_BG,
            foreground: FG,
            muted_foreground: MUTED_FG,
            ok: OK_GREEN,
            error: KO_RED,
        }
    } else {
        Palette {
            accent: ACCENT,
            accent_variant: ACCENT_DARK,
            on_accent: Color32::WHITE,
            background: Color32::from_rgb(250, 250, 250),
            navigation: Color32::WHITE,
            surface: Color32::WHITE,
            raised_surface: Color32::from_rgb(245, 245, 245),
            inset_surface: Color32::from_rgb(245, 245, 245),
            separator: Color32::from_rgb(224, 224, 224),
            foreground: Color32::from_rgb(33, 33, 33),
            muted_foreground: Color32::from_rgb(97, 97, 97),
            ok: Color32::from_rgb(46, 125, 50),
            error: Color32::from_rgb(176, 0, 32),
        }
    }
}

pub fn palette(ctx: &Context) -> Palette {
    palette_for(ctx.style().visuals.dark_mode)
}

pub mod log_colors {
    use egui::epaint::Color32;

    pub const ERROR_LIGHT: Color32 = Color32::from_rgb(255, 50, 50);
    pub const WARNING_LIGHT: Color32 = Color32::from_rgb(205, 147, 9);
    pub const INFO_LIGHT: Color32 = Color32::from_rgb(134, 171, 241);
    pub const DEBUG_LIGHT: Color32 = Color32::LIGHT_GRAY;
    pub const EVENT_LIGHT: Color32 = Color32::GRAY;
}

// Graph colors
pub mod graph_colors {
    use egui::Color32;

    // Colors taken from https://colorhunt.co/palette/ff6b6bffd93d6bcb774d96ff
    pub const RENDER_EXTERNAL: Color32 = Color32::from_rgb(64, 64, 64);
    pub const RENDER_EXTERNAL_LABEL: Color32 = Color32::GRAY;
    pub const RENDER: Color32 = Color32::RED;
    pub const IDLE: Color32 = Color32::from_rgb(255, 217, 61);
    pub const TRANSCODE: Color32 = Color32::from_rgb(107, 203, 119);
    pub const NETWORK: Color32 = Color32::from_rgb(77, 150, 255);

    pub const SERVER_FPS: Color32 = Color32::LIGHT_BLUE;
    pub const CLIENT_FPS: Color32 = Color32::KHAKI;

    pub const INITIAL_CALCULATED_THROUGHPUT: Color32 = Color32::GRAY;
    pub const ENCODER_DECODER_LATENCY_LIMITER: Color32 = TRANSCODE;
    pub const NETWORK_LATENCY_LIMITER: Color32 = NETWORK;
    pub const MIN_MAX_LATENCY_THROUGHPUT: Color32 = Color32::RED;
    pub const REQUESTED_BITRATE: Color32 = Color32::GREEN;
    pub const RECORDED_THROUGHPUT: Color32 = Color32::KHAKI;
    pub const RECORDED_BITRATE: Color32 = super::FG;
}

pub fn set_theme(ctx: &Context) {
    set_theme_mode(ctx, true);
}

pub fn set_theme_mode(ctx: &Context, dark_mode: bool) {
    ctx.set_theme(if dark_mode {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });

    let mut style = (*ctx.style()).clone();
    style.spacing.slider_width = 200.0;
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
    style.spacing.indent = 24.0;
    style.text_styles.get_mut(&TextStyle::Body).unwrap().size = 14.0;
    style.text_styles.get_mut(&TextStyle::Button).unwrap().size = 14.0;
    ctx.set_style(style);

    let palette = palette_for(dark_mode);
    let mut visuals = if dark_mode {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let corner_radius = CornerRadius::same(4);

    visuals.override_text_color = Some(palette.foreground);
    visuals.panel_fill = palette.background;
    visuals.window_fill = palette.surface;
    visuals.window_stroke = Stroke::new(1.0, palette.separator);
    visuals.window_corner_radius = corner_radius;
    visuals.menu_corner_radius = corner_radius;
    visuals.extreme_bg_color = palette.inset_surface;
    visuals.code_bg_color = palette.inset_surface;
    visuals.faint_bg_color = palette.raised_surface;
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = log_colors::WARNING_LIGHT;
    visuals.error_fg_color = palette.error;
    visuals.striped = true;
    visuals.slider_trailing_fill = true;

    visuals.widgets.active.bg_fill = palette.accent;
    visuals.widgets.active.weak_bg_fill = palette.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent_variant);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.on_accent);
    visuals.widgets.active.corner_radius = corner_radius;

    visuals.widgets.inactive.bg_fill = palette.surface;
    visuals.widgets.inactive.weak_bg_fill = palette.surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.separator);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.foreground);
    visuals.widgets.inactive.corner_radius = corner_radius;

    visuals.widgets.hovered.bg_fill = palette.raised_surface;
    visuals.widgets.hovered.weak_bg_fill = palette.raised_surface;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.foreground);
    visuals.widgets.hovered.corner_radius = corner_radius;

    visuals.widgets.open.bg_fill = palette.raised_surface;
    visuals.widgets.open.weak_bg_fill = palette.raised_surface;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, palette.foreground);
    visuals.widgets.open.corner_radius = corner_radius;

    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = Stroke::new(1.0, palette.on_accent);

    visuals.widgets.noninteractive.bg_fill = palette.surface;
    visuals.widgets.noninteractive.weak_bg_fill = palette.surface;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.foreground);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.separator);
    visuals.widgets.noninteractive.corner_radius = corner_radius;

    ctx.set_visuals(visuals);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_primary_color_is_exact() {
        assert_eq!(ACCENT, Color32::from_rgb(0x03, 0x83, 0x87));
    }

    #[test]
    fn light_and_dark_palettes_have_distinct_surfaces() {
        let light = palette_for(false);
        let dark = palette_for(true);

        assert_ne!(light.background, dark.background);
        assert_eq!(light.accent, ACCENT);
        assert_eq!(dark.accent, ACCENT);
    }
}
