mod components;

use self::components::{
    DevicesTab, LogsTab, NotificationBar, SettingsTab, SetupWizard, SetupWizardRequest,
};
use crate::{
    dashboard::components::{CloseAction, NewVersionPopup, StatisticsTab},
    icons, DataSources,
};
use alvr_common::parking_lot::{Condvar, Mutex};
use alvr_events::EventType;
use alvr_gui_common::theme;
use alvr_packets::{PathValuePair, ServerRequest};
use alvr_session::SessionConfig;
use eframe::egui::{
    self, Align, Button, CentralPanel, Frame, Layout, Margin, RichText, SidePanel, Stroke,
};
use std::sync::Arc;

const THEME_STORAGE_KEY: &str = "huawei_dashboard_theme";

#[derive(Clone, Copy, PartialEq, Eq)]
enum DashboardTheme {
    Dark,
    Light,
}

impl DashboardTheme {
    fn is_dark(self) -> bool {
        self == Self::Dark
    }

    fn toggle(&mut self) {
        *self = if self.is_dark() {
            Self::Light
        } else {
            Self::Dark
        };
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Tab {
    Devices,
    Statistics,
    Settings,
    #[cfg(not(target_arch = "wasm32"))]
    Installation,
    Logs,
    Debug,
    About,
}

const TABS: &[Tab] = &[
    Tab::Devices,
    Tab::Statistics,
    Tab::Settings,
    #[cfg(not(target_arch = "wasm32"))]
    Tab::Installation,
    Tab::Logs,
    Tab::Debug,
    Tab::About,
];

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Self::Devices => "设备",
            Self::Statistics => "统计",
            Self::Settings => "设置",
            #[cfg(not(target_arch = "wasm32"))]
            Self::Installation => "安装",
            Self::Logs => "日志",
            Self::Debug => "调试",
            Self::About => "关于",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::Devices => "管理有线与无线客户端连接",
            Self::Statistics => "查看串流性能与网络状态",
            Self::Settings => "调整画质、延迟、音频与追踪",
            #[cfg(not(target_arch = "wasm32"))]
            Self::Installation => "维护 SteamVR 驱动与防火墙规则",
            Self::Logs => "查看运行日志与诊断信息",
            Self::Debug => "检查追踪、按键与触觉事件",
            Self::About => "版本信息、开源项目与许可证",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Devices => icons::DEVICES,
            Self::Statistics => icons::MONITORING,
            Self::Settings => icons::SETTINGS,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Installation => icons::INSTALL_DESKTOP,
            Self::Logs => icons::DESCRIPTION,
            Self::Debug => icons::BUG_REPORT,
            Self::About => icons::INFO,
        }
    }
}

pub struct Dashboard {
    data_sources: DataSources,
    just_opened: bool,
    server_restarting: Arc<Mutex<bool>>,
    server_restarting_condvar: Arc<Condvar>,
    theme: DashboardTheme,
    selected_tab: Tab,
    connections_tab: DevicesTab,
    statistics_tab: StatisticsTab,
    settings_tab: SettingsTab,
    #[cfg(not(target_arch = "wasm32"))]
    installation_tab: components::InstallationTab,
    logs_tab: LogsTab,
    notification_bar: NotificationBar,
    setup_wizard: SetupWizard,
    new_version_popup: Option<components::NewVersionPopup>,
    setup_wizard_open: bool,
    session: Option<SessionConfig>,
}

impl Dashboard {
    pub fn new(creation_context: &eframe::CreationContext<'_>, data_sources: DataSources) -> Self {
        let dashboard_theme = creation_context
            .storage
            .and_then(|storage| storage.get_string(THEME_STORAGE_KEY))
            .map(|value| {
                if value == "light" {
                    DashboardTheme::Light
                } else {
                    DashboardTheme::Dark
                }
            })
            .unwrap_or(DashboardTheme::Dark);
        alvr_gui_common::theme::set_theme_mode(
            &creation_context.egui_ctx,
            dashboard_theme.is_dark(),
        );
        crate::localization::install_chinese_font(&creation_context.egui_ctx);

        data_sources.request(ServerRequest::GetSession);

        Self {
            data_sources,
            just_opened: true,
            server_restarting: Arc::new(Mutex::new(false)),
            server_restarting_condvar: Arc::new(Condvar::new()),
            theme: dashboard_theme,
            selected_tab: Tab::Devices,
            connections_tab: DevicesTab::new(),
            statistics_tab: StatisticsTab::new(),
            settings_tab: SettingsTab::new(),
            #[cfg(not(target_arch = "wasm32"))]
            installation_tab: components::InstallationTab::new(),
            logs_tab: LogsTab::new(),
            notification_bar: NotificationBar::new(),
            setup_wizard: SetupWizard::new(),
            setup_wizard_open: false,
            session: None,
            new_version_popup: None,
        }
    }

    // This call may block
    fn restart_steamvr(&self, requests: &mut Vec<ServerRequest>) {
        requests.push(ServerRequest::RestartSteamvr);

        let mut server_restarting_lock = self.server_restarting.lock();

        if *server_restarting_lock {
            self.server_restarting_condvar
                .wait(&mut server_restarting_lock);
        }

        *server_restarting_lock = true;

        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn({
            let server_restarting = Arc::clone(&self.server_restarting);
            let condvar = Arc::clone(&self.server_restarting_condvar);
            move || {
                crate::steamvr_launcher::LAUNCHER.lock().restart_steamvr();

                *server_restarting.lock() = false;
                condvar.notify_one();
            }
        });
    }
}

impl eframe::App for Dashboard {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        let mut requests = vec![];

        let connected_to_server = self.data_sources.server_connected();
        let palette = theme::palette(context);

        while let Some(event) = self.data_sources.poll_event() {
            self.logs_tab.push_event(event.inner.clone());

            match event.inner.event_type {
                EventType::Log(log_event) => {
                    self.notification_bar
                        .push_notification(log_event, event.from_dashboard);
                }
                EventType::GraphStatistics(graph_statistics) => self
                    .statistics_tab
                    .update_graph_statistics(graph_statistics),
                EventType::StatisticsSummary(statistics) => {
                    self.statistics_tab.update_statistics(statistics)
                }
                EventType::Session(session) => {
                    let settings = session.to_settings();

                    self.connections_tab.update_client_list(&session);
                    self.settings_tab.update_session(&session.session_settings);
                    self.logs_tab.update_settings(&settings);
                    self.notification_bar.update_settings(&settings);
                    if self.just_opened {
                        if settings.extra.open_setup_wizard {
                            self.setup_wizard_open = true;
                        }

                        self.just_opened = false;
                    }

                    self.session = Some(*session);
                }
                EventType::ServerRequestsSelfRestart => self.restart_steamvr(&mut requests),
                #[cfg(not(target_arch = "wasm32"))]
                EventType::DriversList(list) => self.installation_tab.update_drivers(list),
                EventType::Adb(adb_event) => self
                    .connections_tab
                    .update_adb_download_progress(adb_event.download_progress),
                EventType::NewVersionFound { version, message } => {
                    self.new_version_popup = Some(NewVersionPopup::new(version, message));
                }
                EventType::DebugGroup { .. }
                | EventType::Tracking(_)
                | EventType::Buttons(_)
                | EventType::Haptics(_) => (),
            }
        }

        if *self.server_restarting.lock() {
            CentralPanel::default()
                .frame(Frame::new().fill(palette.background))
                .show(context, |ui| {
                    // todo: find a way to center both vertically and horizontally
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.label(icons::symbol(icons::RESTART_ALT, 42.0, palette.accent));
                        ui.add_space(8.0);
                        ui.heading(RichText::new("SteamVR 正在重启").size(28.0));
                        ui.label(
                            RichText::new("请稍候，控制台会自动恢复连接")
                                .color(palette.muted_foreground),
                        );
                    });
                });

            return;
        }

        self.notification_bar.ui(context);

        if self.setup_wizard_open {
            CentralPanel::default().show(context, |ui| {
                if let Some(request) = self.setup_wizard.ui(ui) {
                    match request {
                        SetupWizardRequest::ServerRequest(request) => {
                            requests.push(request);
                        }
                        SetupWizardRequest::Close { finished } => {
                            if finished {
                                requests.push(ServerRequest::SetValues(vec![PathValuePair {
                                    path: alvr_packets::parse_path(
                                        "session_settings.extra.open_setup_wizard",
                                    ),
                                    value: serde_json::Value::Bool(false),
                                }]))
                            }

                            self.setup_wizard_open = false;
                        }
                    }
                }
            });
        } else {
            SidePanel::left("side_panel")
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(palette.navigation)
                        .inner_margin(Margin::same(8))
                        .stroke(Stroke::new(1.0, palette.separator)),
                )
                .exact_width(224.0)
                .show(context, |ui| {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(icons::symbol(icons::VIEW_IN_AR, 31.0, palette.accent));
                        ui.vertical(|ui| {
                            ui.label(RichText::new("ALVR 华为专版").size(18.0).strong());
                            ui.label(
                                RichText::new("VR Glass 串流控制台")
                                    .size(11.5)
                                    .color(palette.muted_foreground),
                            );
                        });
                        egui::warn_if_debug_build(ui);
                    });
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.with_layout(Layout::top_down_justified(Align::Min), |ui| {
                        for tab in TABS {
                            let selected = self.selected_tab == *tab;
                            let label = if selected {
                                icons::label_colored(
                                    ui.style(),
                                    tab.icon(),
                                    tab.label(),
                                    15.0,
                                    palette.on_accent,
                                )
                            } else {
                                icons::label_colored(
                                    ui.style(),
                                    tab.icon(),
                                    tab.label(),
                                    15.0,
                                    palette.foreground,
                                )
                            };
                            let response = ui.add(
                                Button::new(label)
                                    .selected(selected)
                                    .corner_radius(4.0)
                                    .min_size(egui::vec2(ui.available_width(), 48.0)),
                            );
                            if response.clicked() {
                                self.selected_tab = *tab;
                            }
                        }
                    });

                    #[cfg(not(target_arch = "wasm32"))]
                    ui.with_layout(
                        Layout::bottom_up(Align::Center).with_cross_justify(true),
                        |ui| {
                            ui.add_space(5.0);

                            let (theme_icon, theme_label) = if self.theme.is_dark() {
                                (icons::LIGHT_MODE, "切换到浅色模式")
                            } else {
                                (icons::DARK_MODE, "切换到深色模式")
                            };
                            if ui
                                .add(
                                    Button::new(icons::label(
                                        ui.style(),
                                        theme_icon,
                                        theme_label,
                                        12.0,
                                    ))
                                    .min_size(egui::vec2(ui.available_width(), 36.0)),
                                )
                                .clicked()
                            {
                                self.theme.toggle();
                                theme::set_theme_mode(context, self.theme.is_dark());
                                context.request_repaint();
                            }

                            ui.separator();

                            if connected_to_server {
                                if ui
                                    .add(
                                        Button::new(icons::label(
                                            ui.style(),
                                            icons::RESTART_ALT,
                                            "重启 SteamVR",
                                            13.0,
                                        ))
                                        .min_size(egui::vec2(ui.available_width(), 38.0)),
                                    )
                                    .clicked()
                                {
                                    self.restart_steamvr(&mut requests);
                                }
                            } else if ui
                                .add(
                                    Button::new(icons::label(
                                        ui.style(),
                                        icons::PLAY_ARROW,
                                        "启动 SteamVR",
                                        13.0,
                                    ))
                                    .min_size(egui::vec2(ui.available_width(), 38.0)),
                                )
                                .clicked()
                            {
                                crate::steamvr_launcher::LAUNCHER.lock().launch_steamvr();
                            }

                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                let (status, color) = if connected_to_server {
                                    ("SteamVR 已连接", palette.ok)
                                } else {
                                    ("SteamVR 未连接", palette.error)
                                };
                                ui.label(icons::symbol(icons::RADIO_BUTTON_CHECKED, 12.0, color));
                                ui.add_space(-6.0);
                                ui.label(RichText::new(status).color(color).size(12.5));
                            })
                        },
                    )
                });

            CentralPanel::default()
                .frame(
                    Frame::new()
                        .inner_margin(Margin::same(24))
                        .fill(palette.background),
                )
                .show(context, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(icons::symbol(
                                self.selected_tab.icon(),
                                31.0,
                                palette.accent,
                            ));
                            ui.add_space(2.0);
                            ui.vertical(|ui| {
                                ui.heading(
                                    RichText::new(self.selected_tab.label()).size(24.0).strong(),
                                );
                                ui.label(
                                    RichText::new(self.selected_tab.subtitle())
                                        .size(12.5)
                                        .color(palette.muted_foreground),
                                );
                            });
                        });
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(6.0);
                        match self.selected_tab {
                            Tab::Devices => {
                                requests.extend(self.connections_tab.ui(ui, connected_to_server));
                            }
                            Tab::Statistics => {
                                if let Some(request) = self.statistics_tab.ui(ui) {
                                    requests.push(request);
                                }
                            }
                            Tab::Settings => {
                                requests.extend(self.settings_tab.ui(ui));
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            Tab::Installation => {
                                for request in self.installation_tab.ui(ui) {
                                    match request {
                                        components::InstallationTabRequest::OpenSetupWizard => {
                                            self.setup_wizard_open = true
                                        }
                                        components::InstallationTabRequest::ServerRequest(
                                            request,
                                        ) => {
                                            requests.push(request);
                                        }
                                    }
                                }
                            }
                            Tab::Logs => self.logs_tab.ui(ui),
                            Tab::Debug => {
                                if let Some(request) = components::debug_tab_ui(ui) {
                                    requests.push(request);
                                }
                            }
                            Tab::About => components::about_tab_ui(ui),
                        }
                    })
                });
        }

        let shutdown_alvr = || {
            self.data_sources.request(ServerRequest::ShutdownSteamvr);

            crate::steamvr_launcher::LAUNCHER
                .lock()
                .ensure_steamvr_shutdown();
        };

        if let Some(popup) = &self.new_version_popup {
            if let Some(action) = popup.ui(context, shutdown_alvr) {
                if let CloseAction::CloseWithRequest(request) = action {
                    requests.push(request);
                }

                self.new_version_popup = None;
            }
        }

        for request in requests {
            self.data_sources.request(request);
        }

        if context.input(|state| state.viewport().close_requested())
            && self.session.as_ref().is_some_and(|s| {
                s.to_settings()
                    .extra
                    .steamvr_launcher
                    .open_close_steamvr_with_dashboard
            })
        {
            shutdown_alvr();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            THEME_STORAGE_KEY,
            if self.theme.is_dark() {
                "dark".into()
            } else {
                "light".into()
            },
        );
    }
}
