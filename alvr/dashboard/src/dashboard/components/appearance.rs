use crate::dashboard::DashboardTheme;
use alvr_gui_common::theme;
use eframe::egui::{ComboBox, RichText, Ui};

pub fn appearance_theme_ui(ui: &mut Ui, dashboard_theme: &mut DashboardTheme) {
    let palette = theme::palette(ui.ctx());

    theme::card_frame(ui.ctx()).show(ui, |ui| {
        ui.label(
            RichText::new("界面主题")
                .size(18.0)
                .strong()
                .color(palette.foreground),
        );
        ui.label(
            RichText::new("默认跟随 Windows 的颜色模式，也可以固定使用浅色或深色。")
                .color(palette.muted_foreground),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("主题模式");
            ComboBox::from_id_salt("dashboard_theme_mode")
                .selected_text(theme_label(*dashboard_theme))
                .show_ui(ui, |ui| {
                    ui.selectable_value(dashboard_theme, DashboardTheme::System, "跟随系统");
                    ui.selectable_value(dashboard_theme, DashboardTheme::Light, "始终浅色");
                    ui.selectable_value(dashboard_theme, DashboardTheme::Dark, "始终深色");
                });
        });
        ui.label(
            RichText::new(format!(
                "当前实际显示：{}",
                if dashboard_theme.resolved_is_dark(ui.ctx()) {
                    "深色"
                } else {
                    "浅色"
                }
            ))
            .size(12.0)
            .color(palette.muted_foreground),
        );
    });
}

fn theme_label(theme: DashboardTheme) -> &'static str {
    match theme {
        DashboardTheme::System => "跟随系统",
        DashboardTheme::Light => "始终浅色",
        DashboardTheme::Dark => "始终深色",
    }
}
