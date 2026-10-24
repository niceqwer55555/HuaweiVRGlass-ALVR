mod components;
mod drawer;

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
use eframe::egui::{self, Align, CentralPanel, Frame, Layout, Margin, RichText};
use std::sync::Arc;

const THEME_STORAGE_KEY: &str = "huawei_dashboard_theme";
const DRAWER_STORAGE_KEY: &str = "huawei_dashboard_drawer";
#[cfg(not(target_arch = "wasm32"))]
const STEAMVR_PATH_STORAGE_KEY: &str = "huawei_dashboard_steamvr_path";
#[cfg(not(target_arch = "wasm32"))]
const STEAM_ROOT_STORAGE_KEY: &str = "huawei_dashboard_steam_root";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DashboardTheme {
    System,
    Dark,
    Light,
}

impl DashboardTheme {
    fn resolved_is_dark(self, context: &egui::Context) -> bool {
        match self {
            Self::System => context
                .system_theme()
                .map(|theme| theme == egui::Theme::Dark)
                .unwrap_or(true),
            Self::Dark => true,
            Self::Light => false,
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Dark => egui::ThemePreference::Dark,
            Self::Light => egui::ThemePreference::Light,
        }
    }

    fn storage_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Tab {
    Devices,
    Statistics,
    Settings,
    #[cfg(not(target_arch = "wasm32"))]
    GameLibrary,
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
    Tab::GameLibrary,
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
            Self::GameLibrary => "游戏库",
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
            Self::GameLibrary => "设置 SteamVR 路径、免 Steam 直接启动游戏",
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
            Self::GameLibrary => icons::INSTALL_DESKTOP,
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
    title_bar_logo: egui::TextureHandle,
    theme: DashboardTheme,
    applied_dark_mode: bool,
    drawer_expanded: bool,
    selected_tab: Tab,
    connections_tab: DevicesTab,
    statistics_tab: StatisticsTab,
    settings_tab: SettingsTab,
    #[cfg(not(target_arch = "wasm32"))]
    game_library_tab: components::GameLibraryTab,
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
            .map(|value| match value.as_str() {
                "light" => DashboardTheme::Light,
                "dark" => DashboardTheme::Dark,
                _ => DashboardTheme::System,
            })
            .unwrap_or(DashboardTheme::System);
        let drawer_expanded = creation_context
            .storage
            .and_then(|storage| storage.get_string(DRAWER_STORAGE_KEY))
            .map(|value| value != "collapsed")
            .unwrap_or(true);
        let applied_dark_mode = dashboard_theme.resolved_is_dark(&creation_context.egui_ctx);
        alvr_gui_common::theme::set_theme_preference(
            &creation_context.egui_ctx,
            dashboard_theme.preference(),
            applied_dark_mode,
        );
        crate::localization::install_chinese_font(&creation_context.egui_ctx);
        let title_bar_logo = icons::load_alvr_logo(&creation_context.egui_ctx);

        data_sources.request(ServerRequest::GetSession);

        #[cfg(not(target_arch = "wasm32"))]
        let (steamvr_path, steam_root) = {
            let steamvr_path = creation_context
                .storage
                .and_then(|storage| storage.get_string(STEAMVR_PATH_STORAGE_KEY))
                .unwrap_or_default();
            let steam_root = creation_context
                .storage
                .and_then(|storage| storage.get_string(STEAM_ROOT_STORAGE_KEY))
                .unwrap_or_default();

            // Make the configured path effective for every launch entry point.
            crate::steamvr_launcher::set_manual_steamvr_path(steamvr_path.clone());

            (steamvr_path, steam_root)
        };

        Self {
            data_sources,
            just_opened: true,
            server_restarting: Arc::new(Mutex::new(false)),
            server_restarting_condvar: Arc::new(Condvar::new()),
            title_bar_logo,
            theme: dashboard_theme,
            applied_dark_mode,
            drawer_expanded,
            selected_tab: Tab::Devices,
            connections_tab: DevicesTab::new(),
            statistics_tab: StatisticsTab::new(),
            settings_tab: SettingsTab::new(),
            #[cfg(not(target_arch = "wasm32"))]
            game_library_tab: components::GameLibraryTab::new(steamvr_path, steam_root),
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

    fn should_shutdown_steamvr_with_dashboard(&self) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session
                .to_settings()
                .extra
                .steamvr_launcher
                .open_close_steamvr_with_dashboard
        })
    }

    fn shutdown_alvr(&self) {
        self.data_sources.request(ServerRequest::ShutdownSteamvr);

        crate::steamvr_launcher::LAUNCHER
            .lock()
            .ensure_steamvr_shutdown();
    }
}

impl eframe::App for Dashboard {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        let mut requests = vec![];

        let resolved_dark_mode = self.theme.resolved_is_dark(context);
        if resolved_dark_mode != self.applied_dark_mode {
            theme::set_theme_preference(context, self.theme.preference(), resolved_dark_mode);
            self.applied_dark_mode = resolved_dark_mode;
        }

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

        for action in drawer::show_title_bar(
            context,
            &self.title_bar_logo,
            self.drawer_expanded,
            connected_to_server,
            palette,
        ) {
            match action {
                drawer::TitleBarAction::ToggleExpanded => {
                    self.drawer_expanded = !self.drawer_expanded;
                }
                drawer::TitleBarAction::StartDrag => {
                    context.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                #[cfg(not(target_arch = "wasm32"))]
                drawer::TitleBarAction::Minimize => {
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                #[cfg(not(target_arch = "wasm32"))]
                drawer::TitleBarAction::ToggleMaximize => {
                    let maximized =
                        context.input(|input| input.viewport().maximized.unwrap_or(false));
                    context.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                #[cfg(not(target_arch = "wasm32"))]
                drawer::TitleBarAction::Close => {
                    if self.should_shutdown_steamvr_with_dashboard() {
                        self.shutdown_alvr();
                    }
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
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
                        ui.heading(
                            RichText::new("SteamVR 正在重启")
                                .size(28.0)
                                .color(palette.foreground),
                        );
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
            for action in drawer::show_navigation_drawer(
                context,
                self.drawer_expanded,
                self.selected_tab,
                connected_to_server,
                palette,
            ) {
                match action {
                    drawer::NavigationAction::SelectTab(tab) => self.selected_tab = tab,
                    #[cfg(not(target_arch = "wasm32"))]
                    drawer::NavigationAction::RestartSteamvr => self.restart_steamvr(&mut requests),
                    #[cfg(not(target_arch = "wasm32"))]
                    drawer::NavigationAction::LaunchSteamvr => {
                        crate::steamvr_launcher::LAUNCHER.lock().launch_steamvr();
                    }
                }
            }

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
                                    RichText::new(self.selected_tab.label())
                                        .size(24.0)
                                        .strong()
                                        .color(palette.foreground),
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
                                let previous_theme = self.theme;
                                requests.extend(self.settings_tab.ui(ui, &mut self.theme));
                                if self.theme != previous_theme {
                                    let resolved_dark_mode = self.theme.resolved_is_dark(context);
                                    theme::set_theme_preference(
                                        context,
                                        self.theme.preference(),
                                        resolved_dark_mode,
                                    );
                                    self.applied_dark_mode = resolved_dark_mode;
                                    context.request_repaint();
                                }
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            Tab::GameLibrary => {
                                self.game_library_tab.ui(ui);
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
            && self.should_shutdown_steamvr_with_dashboard()
        {
            self.shutdown_alvr();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(THEME_STORAGE_KEY, self.theme.storage_value().into());
        storage.set_string(
            DRAWER_STORAGE_KEY,
            if self.drawer_expanded {
                "expanded".into()
            } else {
                "collapsed".into()
            },
        );
        #[cfg(not(target_arch = "wasm32"))]
        {
            storage.set_string(
                STEAMVR_PATH_STORAGE_KEY,
                self.game_library_tab.steamvr_path(),
            );
            storage.set_string(STEAM_ROOT_STORAGE_KEY, self.game_library_tab.steam_root());
        }
    }
}
