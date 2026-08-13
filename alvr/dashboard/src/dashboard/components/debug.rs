use alvr_packets::ServerRequest;
use eframe::egui::Ui;

pub fn debug_tab_ui(ui: &mut Ui) -> Option<ServerRequest> {
    let mut request = None;

    ui.label(
        "下方按钮提供的 ALVR 录制功能不适合录制游戏过程。\n如需录制游戏，请使用头显或桌面 VR 镜像等其他方式。",
    );

    ui.columns(4, |ui| {
        if ui[0].button("捕获一帧").clicked() {
            request = Some(ServerRequest::CaptureFrame);
        }

        if ui[1].button("插入 IDR 帧").clicked() {
            request = Some(ServerRequest::InsertIdr);
        }

        if ui[2].button("开始录制").clicked() {
            request = Some(ServerRequest::StartRecording);
        }

        if ui[3].button("停止录制").clicked() {
            request = Some(ServerRequest::StopRecording);
        }
    });

    request
}
