use glyph_brush_layout::{
    ab_glyph::{Font, FontRef, ScaleFont},
    FontId, GlyphPositioner, HorizontalAlign, Layout, SectionGeometry, SectionText, VerticalAlign,
};

pub const HUAWEI_LOBBY_TEXTURE_SIDE: usize = 1552;
const HUAWEI_LOBBY_DESIGN_SIDE: f32 = 1024.0;
const HUAWEI_LOBBY_FONT_SIZE: f32 = 41.0;
const HUAWEI_LOBBY_CONTENT_SCALE: f32 = 0.62;

pub fn render_huawei_lobby_texture(message: &str) -> Vec<u8> {
    let side = HUAWEI_LOBBY_TEXTURE_SIDE;
    let layout_scale = side as f32 / HUAWEI_LOBBY_DESIGN_SIDE * HUAWEI_LOBBY_CONTENT_SCALE;
    let scaled_coord =
        |value: f32| side as f32 / 2.0 + (value - HUAWEI_LOBBY_DESIGN_SIDE / 2.0) * layout_scale;
    let scaled_size = |value: f32| value * layout_scale;
    let mut buffer = vec![0_u8; side * side * 4];

    for y in 0..side {
        let blend = y as f32 / side as f32;
        let color = background_color(blend);
        for x in 0..side {
            let index = (y * side + x) * 4;
            buffer[index..index + 3].copy_from_slice(&color);
            buffer[index + 3] = 255;
        }
    }

    fill_rounded_rect(
        &mut buffer,
        scaled_coord(92.0).round() as usize,
        scaled_coord(116.0).round() as usize,
        scaled_size(840.0).round() as usize,
        scaled_size(792.0).round() as usize,
        scaled_size(42.0).round() as usize,
        [17, 42, 56],
    );
    fill_rounded_rect(
        &mut buffer,
        scaled_coord(92.0).round() as usize,
        scaled_coord(116.0).round() as usize,
        scaled_size(12.0).round() as usize,
        scaled_size(792.0).round() as usize,
        scaled_size(6.0).round() as usize,
        [44, 178, 199],
    );
    fill_circle(
        &mut buffer,
        scaled_coord(151.0).round() as usize,
        scaled_coord(187.0).round() as usize,
        scaled_size(14.0).round() as usize,
        [44, 178, 199],
    );

    let font =
        FontRef::try_from_slice(include_bytes!("../resources/NotoSansCJKsc-HuaweiLobby.otf"))
            .unwrap();
    draw_text(
        &mut buffer,
        &font,
        "HUAWEI VR GLASS · ALVR",
        (scaled_coord(184.0), scaled_coord(170.0)),
        (scaled_size(690.0), scaled_size(62.0)),
        scaled_size(31.0),
        [121, 218, 230],
        HorizontalAlign::Left,
        VerticalAlign::Top,
    );
    draw_text(
        &mut buffer,
        &font,
        "正在准备电脑 VR 串流",
        (scaled_coord(146.0), scaled_coord(240.0)),
        (scaled_size(730.0), scaled_size(88.0)),
        scaled_size(54.0),
        [241, 248, 250],
        HorizontalAlign::Left,
        VerticalAlign::Top,
    );
    draw_text(
        &mut buffer,
        &font,
        message,
        (scaled_coord(146.0), scaled_coord(348.0)),
        (scaled_size(730.0), scaled_size(360.0)),
        scaled_size(HUAWEI_LOBBY_FONT_SIZE),
        [216, 232, 237],
        HorizontalAlign::Left,
        VerticalAlign::Top,
    );
    fill_rounded_rect(
        &mut buffer,
        scaled_coord(146.0).round() as usize,
        scaled_coord(768.0).round() as usize,
        scaled_size(730.0).round() as usize,
        scaled_size(1.0).round().max(1.0) as usize,
        0,
        [52, 79, 91],
    );
    draw_text(
        &mut buffer,
        &font,
        "请保持电脑端 ALVR Dashboard 与 SteamVR 运行",
        (scaled_coord(146.0), scaled_coord(803.0)),
        (scaled_size(730.0), scaled_size(64.0)),
        scaled_size(29.0),
        [174, 202, 211],
        HorizontalAlign::Left,
        VerticalAlign::Top,
    );

    buffer
}

fn background_color(blend: f32) -> [u8; 3] {
    [
        (7.0 + blend * 3.0) as u8,
        (20.0 + blend * 13.0) as u8,
        (30.0 + blend * 18.0) as u8,
    ]
}

fn draw_text(
    buffer: &mut [u8],
    font: &FontRef,
    text: &str,
    position: (f32, f32),
    bounds: (f32, f32),
    size: f32,
    color: [u8; 3],
    horizontal_align: HorizontalAlign,
    vertical_align: VerticalAlign,
) {
    let glyphs = Layout::default()
        .h_align(horizontal_align)
        .v_align(vertical_align)
        .calculate_glyphs(
            &[font],
            &SectionGeometry {
                screen_position: position,
                bounds,
            },
            &[SectionText {
                text,
                scale: size.into(),
                font_id: FontId(0),
            }],
        );
    let scaled_font = font.as_scaled(size);

    for glyph in glyphs {
        if let Some(outlined) = scaled_font.outline_glyph(glyph.glyph) {
            let pixel_bounds = outlined.px_bounds();
            outlined.draw(|x, y, alpha| {
                let px = x as i32 + pixel_bounds.min.x as i32;
                let py = y as i32 + pixel_bounds.min.y as i32;
                if px < 0
                    || py < 0
                    || px >= HUAWEI_LOBBY_TEXTURE_SIDE as i32
                    || py >= HUAWEI_LOBBY_TEXTURE_SIDE as i32
                {
                    return;
                }

                let index = (py as usize * HUAWEI_LOBBY_TEXTURE_SIDE + px as usize) * 4;
                for channel in 0..3 {
                    let background = buffer[index + channel] as f32;
                    buffer[index + channel] =
                        (background + (color[channel] as f32 - background) * alpha) as u8;
                }
            });
        }
    }
}

fn fill_rounded_rect(
    buffer: &mut [u8],
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    radius: usize,
    color: [u8; 3],
) {
    let right = left + width;
    let bottom = top + height;
    let radius_squared = (radius * radius) as isize;
    for y in top..bottom.min(HUAWEI_LOBBY_TEXTURE_SIDE) {
        for x in left..right.min(HUAWEI_LOBBY_TEXTURE_SIDE) {
            let dx = if x < left + radius {
                left + radius - x
            } else if x >= right.saturating_sub(radius) {
                x - right.saturating_sub(radius) + 1
            } else {
                0
            };
            let dy = if y < top + radius {
                top + radius - y
            } else if y >= bottom.saturating_sub(radius) {
                y - bottom.saturating_sub(radius) + 1
            } else {
                0
            };
            if dx > 0 && dy > 0 && (dx * dx + dy * dy) as isize > radius_squared {
                continue;
            }
            let index = (y * HUAWEI_LOBBY_TEXTURE_SIDE + x) * 4;
            buffer[index..index + 3].copy_from_slice(&color);
            buffer[index + 3] = 255;
        }
    }
}

fn fill_circle(buffer: &mut [u8], center_x: usize, center_y: usize, radius: usize, color: [u8; 3]) {
    let radius_squared = (radius * radius) as isize;
    for y in center_y.saturating_sub(radius)..=(center_y + radius) {
        for x in center_x.saturating_sub(radius)..=(center_x + radius) {
            let dx = x as isize - center_x as isize;
            let dy = y as isize - center_y as isize;
            if dx * dx + dy * dy > radius_squared
                || x >= HUAWEI_LOBBY_TEXTURE_SIDE
                || y >= HUAWEI_LOBBY_TEXTURE_SIDE
            {
                continue;
            }
            let index = (y * HUAWEI_LOBBY_TEXTURE_SIDE + x) * 4;
            buffer[index..index + 3].copy_from_slice(&color);
            buffer[index + 3] = 255;
        }
    }
}
