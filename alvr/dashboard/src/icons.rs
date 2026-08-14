use eframe::egui::{
    text::LayoutJob, Align, Color32, FontFamily, FontSelection, RichText, Style, WidgetText,
};

// Material Symbols Sharp codepoints from Google's Apache-2.0-licensed icon set.
// The embedded font is subset to only the glyphs referenced by this module.
pub const FAMILY_NAME: &str = "material_symbols_sharp";

pub const PLAY_ARROW: &str = "\u{e037}";
pub const DEVICES: &str = "\u{e326}";
pub const LIGHT_MODE: &str = "\u{e518}";
pub const DARK_MODE: &str = "\u{e51c}";
pub const REFRESH: &str = "\u{e5d5}";
pub const CLOSE: &str = "\u{e5cd}";
pub const RADIO_BUTTON_CHECKED: &str = "\u{e837}";
pub const BUG_REPORT: &str = "\u{e868}";
pub const DESCRIPTION: &str = "\u{e873}";
pub const INFO: &str = "\u{e88e}";
pub const SETTINGS: &str = "\u{e8b8}";
pub const HELP: &str = "\u{e8fd}";
pub const DELETE: &str = "\u{e92e}";
pub const INSTALL_DESKTOP: &str = "\u{eb71}";
pub const VIEW_IN_AR: &str = "\u{efc9}";
pub const RESTART_ALT: &str = "\u{f053}";
pub const WARNING: &str = "\u{f083}";
pub const MONITORING: &str = "\u{f190}";

pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY_NAME.into())
}

pub fn symbol(glyph: &str, size: f32, color: Color32) -> RichText {
    RichText::new(glyph)
        .family(family())
        .size(size)
        .color(color)
}

pub fn symbol_auto(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).family(family()).size(size)
}

pub fn label(style: &Style, glyph: &str, text: &str, size: f32) -> WidgetText {
    label_impl(style, glyph, text, size, None)
}

pub fn label_colored(
    style: &Style,
    glyph: &str,
    text: &str,
    size: f32,
    color: Color32,
) -> WidgetText {
    label_impl(style, glyph, text, size, Some(color))
}

fn label_impl(
    style: &Style,
    glyph: &str,
    text: &str,
    size: f32,
    color: Option<Color32>,
) -> WidgetText {
    let mut job = LayoutJob::default();

    let mut icon = RichText::new(glyph).family(family()).size(size + 3.0);
    let mut label = RichText::new(format!("  {text}")).size(size);
    if let Some(color) = color {
        icon = icon.color(color);
        label = label.color(color);
    }

    icon.append_to(&mut job, style, FontSelection::Default, Align::Center);
    label.append_to(&mut job, style, FontSelection::Default, Align::Center);

    job.into()
}
