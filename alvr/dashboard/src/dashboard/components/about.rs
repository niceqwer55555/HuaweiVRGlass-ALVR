use alvr_common::ALVR_VERSION;
use alvr_gui_common::theme;
use eframe::egui::{self, Frame, RichText, ScrollArea, Ui};

pub fn about_tab_ui(ui: &mut Ui) {
    ui.label(RichText::new(format!("ALVR 华为 VR Glass 串流专版 v{}", *ALVR_VERSION)).size(26.0));
    ui.add_space(10.0);
    ui.hyperlink_to("访问 ALVR GitHub", "https://github.com/alvr-org/ALVR");
    ui.hyperlink_to("加入 ALVR Discord", "https://discord.gg/ALVR");
    ui.hyperlink_to(
        "ALVR 上游最新版本",
        "https://github.com/alvr-org/ALVR/releases/latest",
    );
    ui.hyperlink_to(
        "在 Open Collective 向 ALVR 捐赠",
        "https://opencollective.com/alvr",
    );
    ui.add_space(10.0);
    ui.label("许可证：");
    Frame::group(ui.style())
        .fill(theme::DARKER_BG)
        .inner_margin(egui::vec2(15.0, 12.0))
        .show(ui, |ui| {
            ScrollArea::new([false, true])
                .id_salt("license_scroll")
                .show(ui, |ui| ui.label(include_str!("../../../../../LICENSE")))
        });
}
