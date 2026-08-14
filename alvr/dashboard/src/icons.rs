use eframe::egui::{
    Color32, ColorImage, Context, FontFamily, RichText, TextureHandle, TextureOptions,
};

// Material Symbols Sharp codepoints from Google's Apache-2.0-licensed icon set.
// The embedded font is subset to only the glyphs referenced by this module.
pub const FAMILY_NAME: &str = "material_symbols_sharp";

pub const PLAY_ARROW: &str = "\u{e037}";
pub const REMOVE: &str = "\u{e15b}";
pub const DEVICES: &str = "\u{e326}";
pub const CROP_SQUARE: &str = "\u{e3c6}";
pub const FILTER_NONE: &str = "\u{e3e0}";
pub const MENU: &str = "\u{e5d2}";
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

pub fn load_alvr_logo(context: &Context) -> TextureHandle {
    let rgba = image::load_from_memory(include_bytes!("../resources/ALVR-Icon.png"))
        .expect("embedded ALVR title-bar icon must be a valid PNG")
        .into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];

    context.load_texture(
        "alvr_title_bar_logo",
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_alvr_logo_is_a_transparent_square() {
        let rgba = image::load_from_memory(include_bytes!("../resources/ALVR-Icon.png"))
            .unwrap()
            .into_rgba8();

        assert_eq!(rgba.dimensions(), (128, 128));
        assert!(rgba.pixels().any(|pixel| pixel.0[3] == 0));
        assert!(rgba.pixels().any(|pixel| pixel.0[3] > 0));
    }
}
