use crate::icons;
use eframe::{
    egui::{Button, Layout, Response, Ui},
    emath::Align,
};

pub fn reset_button(ui: &mut Ui, enabled: bool, default_str: &str) -> Response {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(5.0);

        ui.add_enabled(
            enabled,
            Button::new(icons::symbol_auto(icons::REFRESH, 18.0)),
        )
        .on_hover_text(format!("重置为 {default_str}"))
    })
    .inner
}
