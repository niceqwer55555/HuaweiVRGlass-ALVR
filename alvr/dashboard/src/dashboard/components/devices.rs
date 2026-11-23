use crate::dashboard::ServerRequest;
use alvr_common::ConnectionState;
use alvr_gui_common::theme::{self, log_colors};
use alvr_packets::ClientListAction;
use alvr_server_io::list_displays;
use alvr_session::{ClientConnectionConfig, SessionConfig};
use alvr_sockets::WIRED_CLIENT_HOSTNAME;
use eframe::{
    egui::{self, Grid, Layout, ProgressBar, RichText, TextEdit, Ui, Window},
    emath::{Align, Align2},
    epaint::Color32,
};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const DESKTOP_RENDERER_EXE: &str = "HwVRDesktop.exe";

/// Locate the bundled desktop-mode renderer next to this exe.
fn desktop_renderer_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join("desktop-mode").join(DESKTOP_RENDERER_EXE);
    if candidate.is_file() { Some(candidate) } else { None }
}

/// Throttled-free, one-off process probe (called on button clicks / redraws).
fn desktop_renderer_running() -> bool {
    let mut command = Command::new("tasklist");
    command.args(["/FI", &format!("IMAGENAME eq {DESKTOP_RENDERER_EXE}"), "/NH"]);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let Ok(out) = command.output() else { return false; };
    String::from_utf8_lossy(&out.stdout)
        .to_ascii_lowercase()
        .contains(&DESKTOP_RENDERER_EXE.to_ascii_lowercase())
}

/// Start / stop the desktop renderer. Returns a short status message.
fn desktop_renderer_action(start: bool) -> String {
    if start {
        let Some(exe) = desktop_renderer_path() else {
            return "找不到 desktop-mode 渲染器".into();
        };
        if desktop_renderer_running() { return "桌面投屏已在运行".into(); }
        // SteamVR and desktop mode fight over the glasses monitor.
        let _ = Command::new("taskkill")
            .args(["/IM", "vrserver.exe", "/F"])
            .output();
        let mut command = Command::new(&exe);
        command.current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
            .stdin(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        match command.spawn() {
            Ok(_) => "桌面投屏已启动".into(),
            Err(err) => format!("启动失败：{err}"),
        }
    } else {
        let mut command = Command::new("taskkill");
        command.args(["/IM", DESKTOP_RENDERER_EXE, "/F"]);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        match command.output() {
            Ok(_) => "桌面投屏已停止".into(),
            Err(err) => format!("停止失败：{err}"),
        }
    }
}

struct EditPopupState {
    new_devices: bool,
    hostname: String,
    ips: Vec<String>,
}

pub struct DevicesTab {
    new_devices: Option<Vec<(String, ClientConnectionConfig)>>,
    trusted_devices: Option<Vec<(String, ClientConnectionConfig)>>,
    edit_popup_state: Option<EditPopupState>,
    adb_download_progress: Option<f32>,
}

impl DevicesTab {
    pub fn new() -> Self {
        Self {
            new_devices: None,
            trusted_devices: None,
            edit_popup_state: None,
            adb_download_progress: None,
        }
    }

    pub fn update_client_list(&mut self, session: &SessionConfig) {
        let (trusted_clients, untrusted_clients) =
            session
                .client_connections
                .clone()
                .into_iter()
                .partition::<Vec<_>, _>(|(_, data)| data.trusted);

        self.trusted_devices = Some(trusted_clients);
        self.new_devices = Some(untrusted_clients);
    }

    pub fn update_adb_download_progress(&mut self, progress: f32) {
        self.adb_download_progress = Some(progress);
    }

    pub fn ui(&mut self, ui: &mut Ui, connected_to_server: bool) -> Vec<ServerRequest> {
        let mut requests = vec![];

        if self.new_devices.is_none() {
            requests.push(ServerRequest::GetSession);
        }

        if !connected_to_server {
            theme::card_frame(ui.ctx())
                .fill(log_colors::WARNING_LIGHT)
                .show(ui, |ui| {
                    Grid::new(0).num_columns(2).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.heading(
                                RichText::new(format!(
                                    "ALVR 需要 SteamVR 正在运行！{}",
                                    "设备暂时无法被发现或连接"
                                ))
                                .color(Color32::BLACK)
                                .size(16.0),
                            );
                        });

                        #[cfg(not(target_arch = "wasm32"))]
                        ui.with_layout(Layout::right_to_left(eframe::emath::Align::Center), |ui| {
                            if ui.button("启动 SteamVR").clicked() {
                                crate::steamvr_launcher::LAUNCHER.lock().launch_steamvr();
                            }
                        });
                    })
                });
        }

        ui.vertical_centered_justified(|ui| {
            if let Some(clients) = &mut self.trusted_devices {
                let wired_client = clients
                    .iter()
                    .find(|(hostname, _)| hostname == WIRED_CLIENT_HOSTNAME);
                let _ = wired_client_section(ui, wired_client, self.adb_download_progress);
            }

            ui.add_space(10.0);

            if let Some(clients) = &self.new_devices {
                if let Some(request) = new_clients_section(ui, clients) {
                    requests.push(request);
                }
            }

            ui.add_space(10.0);

            if let Some(clients) = &mut self.trusted_devices {
                let wireless_clients: Vec<&(String, ClientConnectionConfig)> = clients
                    .iter()
                    .filter(|(hostname, _)| hostname != WIRED_CLIENT_HOSTNAME)
                    .collect();
                if let Some(request) = trusted_clients_section(
                    ui,
                    wireless_clients.as_slice(),
                    &mut self.edit_popup_state,
                ) {
                    requests.push(request);
                }
            }
        });

        if let Some(mut state) = self.edit_popup_state.take() {
            Window::new("编辑连接")
                .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
                .resizable(false)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.add_space(5.0);

                    ui.columns(2, |ui| {
                        ui[0].horizontal(|ui| {
                            ui.add_space(5.0);
                            ui.label("主机名：");
                        });
                        ui[1].add_enabled(
                            state.new_devices,
                            TextEdit::singleline(&mut state.hostname),
                        );

                        ui[0].horizontal(|ui| {
                            ui.add_space(5.0);
                            ui.label("IP 地址：");
                        });
                        for address in &mut state.ips {
                            ui[1].text_edit_singleline(address);
                        }
                        if ui[1].button("新增").clicked() {
                            state.ips.push("192.168.X.X".into());
                        }
                    });

                    ui.columns(2, |ui| {
                        if ui[0].button("取消").clicked() {
                            return;
                        }

                        if ui[1].button("保存").clicked() {
                            let manual_ips =
                                state.ips.iter().filter_map(|s| s.parse().ok()).collect();

                            if state.new_devices {
                                requests.push(ServerRequest::UpdateClientList {
                                    hostname: state.hostname,
                                    action: ClientListAction::AddIfMissing {
                                        trusted: true,
                                        manual_ips,
                                    },
                                });
                            } else {
                                requests.push(ServerRequest::UpdateClientList {
                                    hostname: state.hostname,
                                    action: ClientListAction::SetManualIps(manual_ips),
                                });
                            }
                        } else {
                            self.edit_popup_state = Some(state);
                        }
                    })
                });
        }

        requests
    }
}

fn wired_client_section(
    ui: &mut Ui,
    _maybe_client: Option<&(String, ClientConnectionConfig)>,
    _adb_download_progress: Option<f32>,
) {
    // USB mode on this build uses the bundled HwVRDesktop renderer (plain D3D
    // cast to the glasses DP monitor), NOT the ALVR wired streaming client.
    // Show the renderer's real state instead of an always-"Disconnected"
    // ALVR client that can never connect in this mode.
    let running = desktop_renderer_running();
    let glasses_res = list_displays()
        .into_iter().find(|d| d.is_glasses())
        .map(|d| format!("{}x{}", d.width, d.height))
        .unwrap_or_else(|| "未检测到眼镜".into());

    theme::card_frame(ui.ctx())
        .fill(theme::palette(ui.ctx()).surface)
        .inner_margin(egui::vec2(15.0, 12.0))
        .show(ui, |ui| {
            Grid::new("wired-client")
                .num_columns(2)
                .spacing(egui::vec2(8.0, 8.0))
                .show(ui, |ui| {
                    ui.heading("有线连接（USB 直连桌面投屏）");
                    ui.end_row();

                    ui.label("眼镜屏");
                    ui.label(&glasses_res);
                    ui.end_row();

                    ui.label("桌面投屏渲染器");
                    if running {
                        ui.colored_label(theme::palette(ui.ctx()).ok, "运行中");
                    } else {
                        ui.colored_label(Color32::GRAY, "未运行");
                    }
                    ui.end_row();

                    ui.label("");
                    ui.horizontal(|ui| {
                        if running {
                            if ui.button("停止投屏").clicked() {
                                let msg = desktop_renderer_action(false);
                                alvr_common::info!("{msg}");
                            }
                        } else if ui.button("启动投屏").clicked() {
                            let msg = desktop_renderer_action(true);
                            alvr_common::info!("{msg}");
                        }
                    });
                    ui.end_row();
                });
        });
}

fn new_clients_section(
    ui: &mut Ui,
    clients: &[(String, ClientConnectionConfig)],
) -> Option<ServerRequest> {
    let mut request = None;

    theme::card_frame(ui.ctx())
        .fill(theme::palette(ui.ctx()).surface)
        .show(ui, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.heading("新发现的无线设备");

                    // Extend to the right
                    ui.with_layout(Layout::right_to_left(Align::Center), |_| ());
                });

                if clients.is_empty() {
                    // for some reson any positive value adds too much space
                    ui.add_space(-10.0);
                }
            });
            for (hostname, _) in clients {
                theme::card_frame(ui.ctx())
                    .fill(theme::palette(ui.ctx()).inset_surface)
                    .inner_margin(egui::vec2(15.0, 12.0))
                    .show(ui, |ui| {
                        Grid::new(format!("{hostname}-new-clients"))
                            .num_columns(2)
                            .spacing(egui::vec2(8.0, 8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(hostname);
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("信任").clicked() {
                                        request = Some(ServerRequest::UpdateClientList {
                                            hostname: hostname.clone(),
                                            action: ClientListAction::Trust,
                                        });
                                    };
                                });
                                ui.end_row();
                            });
                    });
            }
        });

    request
}

fn trusted_clients_section(
    ui: &mut Ui,
    clients: &[&(String, ClientConnectionConfig)],
    edit_popup_state: &mut Option<EditPopupState>,
) -> Option<ServerRequest> {
    let mut request = None;

    theme::card_frame(ui.ctx())
        .fill(theme::palette(ui.ctx()).surface)
        .show(ui, |ui| {
            Grid::new(0).num_columns(2).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.heading("已信任的无线设备");
                });

                ui.with_layout(Layout::right_to_left(eframe::emath::Align::Center), |ui| {
                    if ui.button("手动添加设备").clicked() {
                        *edit_popup_state = Some(EditPopupState {
                            hostname: "XXXX.client.local.".into(),
                            new_devices: true,
                            ips: Vec::new(),
                        });
                    }
                });
            });

            for (hostname, data) in clients {
                theme::card_frame(ui.ctx())
                    .fill(theme::palette(ui.ctx()).inset_surface)
                    .inner_margin(egui::vec2(15.0, 12.0))
                    .show(ui, |ui| {
                        Grid::new(format!("{hostname}-clients"))
                            .num_columns(2)
                            .spacing(egui::vec2(8.0, 8.0))
                            .show(ui, |ui| {
                                ui.label(&data.display_name);
                                ui.horizontal(|ui| {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        connection_label(ui, &data.connection_state)
                                    });
                                });

                                ui.end_row();

                                ui.label(format!(
                                    "{hostname}: {}",
                                    data.current_ip
                                        .map_or_else(|| "未知 IP".into(), |ip| ip.to_string()),
                                ));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.button("移除").clicked() {
                                        request = Some(ServerRequest::UpdateClientList {
                                            hostname: hostname.clone(),
                                            action: ClientListAction::RemoveEntry,
                                        });
                                    }
                                    if ui.button("编辑").clicked() {
                                        *edit_popup_state = Some(EditPopupState {
                                            new_devices: false,
                                            hostname: hostname.to_owned(),
                                            ips: data
                                                .manual_ips
                                                .iter()
                                                .map(|addr| addr.to_string())
                                                .collect::<Vec<String>>(),
                                        });
                                    }
                                });
                            });
                    });
            }
        });

    request
}

fn connection_label(ui: &mut Ui, connection_state: &ConnectionState) {
    match connection_state {
        ConnectionState::Disconnected => ui.colored_label(Color32::GRAY, "未连接"),
        ConnectionState::Connecting => ui.colored_label(log_colors::WARNING_LIGHT, "连接中"),
        ConnectionState::Connected => ui.colored_label(theme::palette(ui.ctx()).ok, "已连接"),
        ConnectionState::Streaming => ui.colored_label(theme::palette(ui.ctx()).ok, "串流中"),
        ConnectionState::Disconnecting => ui.colored_label(log_colors::WARNING_LIGHT, "正在断开"),
    };
}
