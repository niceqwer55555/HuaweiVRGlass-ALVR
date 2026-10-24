use crate::{
    game_library::{self, LocalGame},
    steam_library::{self, SteamInstall, Shortcut},
};
use alvr_gui_common::theme;
use eframe::egui::{self, Grid, Label, Layout, RichText, ScrollArea, TextEdit, Ui, Window};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const STEAMVR_BOOT_DELAY: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickMode {
    /// SteamVR runtime: folders plus `vrmonitor.exe` / `steam.exe`
    SteamvrPath,
    /// Steam client folder
    SteamRoot,
    /// Game executable
    GameExe,
}

impl PickMode {
    fn title(self) -> &'static str {
        match self {
            Self::SteamvrPath => "选择 SteamVR 路径",
            Self::SteamRoot => "选择 Steam 安装目录",
            Self::GameExe => "选择游戏可执行文件",
        }
    }

    fn accepts_file(self, path: &Path) -> bool {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        match self {
            Self::SteamvrPath => {
                name == "vrmonitor.exe" || name == "steam.exe" || name == "vrstartup.exe"
            }
            Self::SteamRoot => false,
            Self::GameExe => name.ends_with(".exe"),
        }
    }
}

enum PickerOutcome {
    Open,
    Selected(PathBuf),
    Cancelled,
}

struct FilePicker {
    mode: PickMode,
    current_dir: PathBuf,
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
    drives: Vec<String>,
    error: Option<String>,
}

impl FilePicker {
    fn new(mode: PickMode, start: &str) -> Self {
        let mut start_dir = PathBuf::from(start.trim().trim_matches('"'));
        if start_dir.is_file() {
            if let Some(parent) = start_dir.parent() {
                start_dir = parent.to_path_buf();
            }
        }
        if !start_dir.is_dir() {
            start_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("C:\\"));
        }

        let drives = ('C'..='Z')
            .filter_map(|letter| {
                let drive = format!("{letter}:\\");
                Path::new(&drive).is_dir().then_some(drive)
            })
            .collect();

        let mut picker = Self {
            mode,
            current_dir: start_dir,
            folders: Vec::new(),
            files: Vec::new(),
            drives,
            error: None,
        };
        picker.refresh();

        picker
    }

    fn refresh(&mut self) {
        self.folders.clear();
        self.files.clear();
        self.error = None;

        match std::fs::read_dir(&self.current_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        self.folders.push(path);
                    } else if self.mode.accepts_file(&path) {
                        self.files.push(path);
                    }
                }
            }
            Err(err) => self.error = Some(format!("无法读取目录：{err}")),
        }

        self.folders
            .sort_by_key(|path| path.file_name().map(|name| name.to_owned()));
        self.files
            .sort_by_key(|path| path.file_name().map(|name| name.to_owned()));
    }

    fn navigate_to(&mut self, dir: PathBuf) {
        self.current_dir = dir;
        self.refresh();
    }

    fn ui(&mut self, ui: &mut Ui) -> PickerOutcome {
        let mut outcome = PickerOutcome::Open;
        let mut open = true;

        Window::new(self.mode.title())
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([620.0, 460.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for drive in self.drives.clone() {
                        let label = drive.trim_end_matches('\\').to_owned();
                        if ui
                            .selectable_label(self.current_dir.starts_with(&drive), label)
                            .clicked()
                        {
                            self.navigate_to(PathBuf::from(drive));
                        }
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("上一级").clicked() {
                        if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
                            self.navigate_to(parent);
                        }
                    }
                    ui.add(
                        Label::new(RichText::new(self.current_dir.display().to_string()))
                            .truncate()
                            .selectable(true),
                    );
                });

                if let Some(error) = &self.error {
                    ui.colored_label(theme::palette(ui.ctx()).error, error);
                }

                ui.separator();

                ScrollArea::vertical()
                    .id_salt("game_library_file_picker")
                    .auto_shrink([false, true])
                    .max_height(300.0)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        for folder in self.folders.clone() {
                            let name = folder
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if ui.selectable_label(false, format!("[目录] {name}")).clicked() {
                                self.navigate_to(folder);
                            }
                        }

                        for file in self.files.clone() {
                            let name = file
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if ui.selectable_label(false, format!("[文件] {name}")).clicked() {
                                outcome = PickerOutcome::Selected(file);
                            }
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("选择当前文件夹").clicked() {
                        outcome = PickerOutcome::Selected(self.current_dir.clone());
                    }
                    if ui.button("取消").clicked() {
                        outcome = PickerOutcome::Cancelled;
                    }
                });
            });

        if !open {
            return PickerOutcome::Cancelled;
        }

        outcome
    }
}

#[derive(Clone, Copy)]
enum StatusKind {
    Ok,
    Error,
}

pub struct GameLibraryTab {
    steamvr_path: String,
    steam_root: String,
    games: Vec<LocalGame>,
    game_name: String,
    game_exe: String,
    game_args: String,
    sync_to_steam: bool,
    install: Option<SteamInstall>,
    shortcuts: Vec<Shortcut>,
    status: Option<(StatusKind, String)>,
    picker: Option<FilePicker>,
    last_refresh: Instant,
}

impl GameLibraryTab {
    pub fn new(steamvr_path: String, steam_root: String) -> Self {
        let install = steam_library::find_steam_install(&steam_root);
        let shortcuts = install
            .as_ref()
            .and_then(|install| steam_library::list_shortcuts(install).ok())
            .unwrap_or_default();

        Self {
            steamvr_path,
            steam_root,
            games: game_library::load_games(),
            game_name: String::new(),
            game_exe: String::new(),
            game_args: String::new(),
            sync_to_steam: false,
            install,
            shortcuts,
            status: None,
            picker: None,
            last_refresh: Instant::now(),
        }
    }

    pub fn steamvr_path(&self) -> String {
        self.steamvr_path.clone()
    }

    pub fn steam_root(&self) -> String {
        self.steam_root.clone()
    }

    fn refresh_install(&mut self) {
        self.install = steam_library::find_steam_install(&self.steam_root);
        self.refresh_shortcuts();
    }

    fn refresh_shortcuts(&mut self) {
        self.shortcuts = self
            .install
            .as_ref()
            .and_then(|install| steam_library::list_shortcuts(install).ok())
            .unwrap_or_default();
        self.last_refresh = Instant::now();
    }

    fn set_status(&mut self, kind: StatusKind, message: impl Into<String>) {
        self.status = Some((kind, message.into()));
    }

    fn resolved_steamvr(&self) -> Option<PathBuf> {
        crate::steamvr_launcher::resolve_steamvr_path(&self.steamvr_path)
    }

    fn save_games(&mut self) {
        game_library::save_games(&self.games);
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if self.last_refresh.elapsed() > REFRESH_INTERVAL {
            self.refresh_shortcuts();
        }

        if let Some(mut picker) = self.picker.take() {
            let mode = picker.mode;
            match picker.ui(ui) {
                PickerOutcome::Open => self.picker = Some(picker),
                PickerOutcome::Cancelled => {}
                PickerOutcome::Selected(selected) => {
                    let path_text = selected.display().to_string();

                    match mode {
                        PickMode::SteamvrPath => {
                            self.steamvr_path.clone_from(&path_text);
                            crate::steamvr_launcher::set_manual_steamvr_path(
                                self.steamvr_path.clone(),
                            );
                        }
                        PickMode::SteamRoot => {
                            self.steam_root.clone_from(&path_text);
                            self.refresh_install();
                        }
                        PickMode::GameExe => {
                            self.game_exe.clone_from(&path_text);
                            if self.game_name.trim().is_empty() {
                                if let Some(stem) = selected.file_stem() {
                                    self.game_name = stem.to_string_lossy().into_owned();
                                }
                            }
                        }
                    }
                }
            }
        }

        ScrollArea::vertical()
            .id_salt("game_library_tab")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::card_frame(ui.ctx())
                    .fill(theme::palette(ui.ctx()).surface)
                    .show(ui, |ui| self.steamvr_section(ui));

                ui.add_space(10.0);

                theme::card_frame(ui.ctx())
                    .fill(theme::palette(ui.ctx()).surface)
                    .show(ui, |ui| self.game_list_section(ui));

                ui.add_space(10.0);
                theme::card_frame(ui.ctx())
                    .fill(theme::palette(ui.ctx()).surface)
                    .show(ui, |ui| self.steam_shortcuts_section(ui));

                if let Some((kind, message)) = &self.status {
                    ui.add_space(8.0);
                    let palette = theme::palette(ui.ctx());
                    let color = match kind {
                        StatusKind::Ok => palette.ok,
                        StatusKind::Error => palette.error,
                    };
                    ui.colored_label(color, message);
                }
            });
    }

    fn steamvr_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("SteamVR 启动路径").size(18.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "留空则自动使用内置的 SteamVR 1.27.5；也可以填 SteamVR 安装目录、vrmonitor.exe 或 steam.exe 的完整路径。无需安装 Steam 客户端。",
            )
            .size(12.0)
            .color(theme::palette(ui.ctx()).muted_foreground),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.steamvr_path)
                    .hint_text("例如 F:\\game\\VR\\SteamVR 1.27.5")
                    .desired_width(360.0),
            );
            if ui.button("浏览…").clicked() {
                self.picker = Some(FilePicker::new(PickMode::SteamvrPath, &self.steamvr_path));
            }
            if ui.button("自动检测").clicked() {
                match auto_detect_steamvr_path() {
                    Some(path) => {
                        self.steamvr_path = path.display().to_string();
                        crate::steamvr_launcher::set_manual_steamvr_path(
                            self.steamvr_path.clone(),
                        );
                        self.set_status(
                            StatusKind::Ok,
                            format!("已找到 SteamVR：{}", path.display()),
                        );
                    }
                    None => self.set_status(StatusKind::Error, "未找到 SteamVR，请手动选择路径"),
                }
            }
            if ui.button("应用").clicked() {
                crate::steamvr_launcher::set_manual_steamvr_path(self.steamvr_path.clone());

                match self.resolved_steamvr() {
                    Some(path) => self.set_status(
                        StatusKind::Ok,
                        format!("已保存，启动时将执行：{}", path.display()),
                    ),
                    None if self.steamvr_path.trim().is_empty() => self.set_status(
                        StatusKind::Ok,
                        "已保存：将使用内置 SteamVR",
                    ),
                    None => self.set_status(
                        StatusKind::Error,
                        "该路径下没有找到 vrmonitor.exe / steam.exe",
                    ),
                }
            }
        });

        ui.add_space(4.0);
        match self.resolved_steamvr() {
            Some(path) => {
                ui.colored_label(
                    theme::palette(ui.ctx()).ok,
                    format!("当前生效：{}", path.display()),
                );
            }
            None => {
                ui.colored_label(
                    theme::palette(ui.ctx()).muted_foreground,
                    "当前生效：未设置自定义路径（将回退到内置 SteamVR）",
                );
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("启动 SteamVR").clicked() {
                crate::steamvr_launcher::LAUNCHER.lock().launch_steamvr();
                self.set_status(StatusKind::Ok, "已发送启动命令");
            }
            if ui.button("重启 SteamVR").clicked() {
                crate::steamvr_launcher::LAUNCHER.lock().restart_steamvr();
                self.set_status(StatusKind::Ok, "已重启 SteamVR");
            }
        });
    }

    fn game_list_section(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("我的游戏（免 Steam）").size(18.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "添加后可直接在这里启动，不需要安装 Steam。启动时会先确保 SteamVR 正在运行，再把游戏作为普通程序拉起，游戏自己连接 VR 运行时。",
            )
            .size(12.0)
            .color(theme::palette(ui.ctx()).muted_foreground),
        );
        ui.add_space(8.0);

        Grid::new("game_library_add")
            .num_columns(3)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                ui.label("游戏名称");
                ui.add(
                    TextEdit::singleline(&mut self.game_name)
                        .hint_text("显示在列表里的名字")
                        .desired_width(320.0),
                );
                ui.label("");
                ui.end_row();

                ui.label("游戏程序");
                ui.add(
                    TextEdit::singleline(&mut self.game_exe)
                        .hint_text("例如 D:\\Games\\MyVrGame\\game.exe")
                        .desired_width(320.0),
                );
                if ui.button("浏览…").clicked() {
                    self.picker = Some(FilePicker::new(PickMode::GameExe, &self.game_exe));
                }
                ui.end_row();

                ui.label("启动参数");
                ui.add(
                    TextEdit::singleline(&mut self.game_args)
                        .hint_text("可选，例如 -vr -windowed")
                        .desired_width(320.0),
                );
                ui.label("");
                ui.end_row();
            });

        if self.install.is_some() {
            ui.checkbox(&mut self.sync_to_steam, "同时写入 Steam 库（可选，需已安装 Steam）");
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let can_add = !self.game_exe.trim().is_empty() && !self.game_name.trim().is_empty();
            if ui
                .add_enabled(can_add, egui::Button::new("添加游戏"))
                .clicked()
            {
                let game = LocalGame {
                    name: self.game_name.trim().to_owned(),
                    exe: self.game_exe.trim().to_owned(),
                    args: self.game_args.trim().to_owned(),
                };

                let mut message = format!("已添加「{}」", game.name);

                if self.sync_to_steam {
                    if let Some(install) = self.install.clone() {
                        match steam_library::add_shortcut(
                            &install,
                            &game.name,
                            Path::new(game.exe.trim_matches('"')),
                            &game.args,
                        ) {
                            Ok((shortcut, _)) => {
                                message.push_str(&format!(
                                    "；已写入 Steam 库（AppID {}，重启 Steam 后可见）",
                                    shortcut.app_id
                                ));
                                self.refresh_shortcuts();
                            }
                            Err(err) => {
                                message.push_str(&format!("；写入 Steam 库失败：{err}"))
                            }
                        }
                    }
                }

                self.games.push(game);
                self.save_games();

                self.game_name.clear();
                self.game_exe.clear();
                self.game_args.clear();

                self.set_status(StatusKind::Ok, message);
            }
            if ui.button("清空输入").clicked() {
                self.game_name.clear();
                self.game_exe.clear();
                self.game_args.clear();
            }
        });

        ui.add_space(8.0);

        if self.games.is_empty() {
            ui.label(
                RichText::new("还没有添加游戏，填好名称和程序路径后点「添加游戏」")
                    .size(12.0)
                    .color(theme::palette(ui.ctx()).muted_foreground),
            );
            return;
        }

        let mut launch_index = None;
        let mut remove_index = None;

        Grid::new("game_library_list")
            .num_columns(3)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("名称").strong());
                ui.label(RichText::new("程序").strong());
                ui.label("");
                ui.end_row();

                for (index, game) in self.games.iter().enumerate() {
                    ui.label(&game.name);

                    ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(game.exe.trim_matches('"'))
                                    .size(12.0)
                                    .color(theme::palette(ui.ctx()).muted_foreground),
                            )
                            .truncate(),
                        );
                    });

                    ui.horizontal(|ui| {
                        if ui.button("启动").clicked() {
                            launch_index = Some(index);
                        }
                        if ui.button("删除").clicked() {
                            remove_index = Some(index);
                        }
                    });
                    ui.end_row();
                }
            });

        if let Some(index) = launch_index {
            if let Some(game) = self.games.get(index).cloned() {
                self.launch(game);
            }
        }

        if let Some(index) = remove_index {
            if index < self.games.len() {
                let removed = self.games.remove(index);
                self.save_games();
                self.set_status(StatusKind::Ok, format!("已删除「{}」", removed.name));
            }
        }
    }

    fn launch(&mut self, game: LocalGame) {
        let steamvr_running = crate::steamvr_launcher::is_steamvr_running();

        if !steamvr_running {
            crate::steamvr_launcher::LAUNCHER.lock().launch_steamvr();

            let name = game.name.clone();
            std::thread::spawn(move || {
                std::thread::sleep(STEAMVR_BOOT_DELAY);
                match game_library::launch_game(&game) {
                    Ok(pid) => alvr_common::info!("Launched {} (pid {})", game.name, pid),
                    Err(err) => alvr_common::warn!("Failed to launch {}: {err}", game.name),
                }
            });

            self.set_status(
                StatusKind::Ok,
                format!("SteamVR 未运行，已先启动 SteamVR，约 4 秒后启动「{name}」"),
            );
            return;
        }

        match game_library::launch_game(&game) {
            Ok(pid) => self.set_status(
                StatusKind::Ok,
                format!("已启动「{}」（进程 {}）", game.name, pid),
            ),
            Err(err) => self.set_status(StatusKind::Error, format!("启动失败：{err}")),
        }
    }

    fn steam_shortcuts_section(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("可选：同步到 Steam 库").size(18.0));
            if ui.button("刷新").clicked() {
                self.refresh_shortcuts();
            }
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "只有需要让游戏出现在 Steam / SteamVR 自带库里时才用这一节。免 Steam 使用可以直接忽略。",
            )
            .size(12.0)
            .color(theme::palette(ui.ctx()).muted_foreground),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Steam 目录");
            ui.add(
                TextEdit::singleline(&mut self.steam_root)
                    .hint_text("留空自动检测")
                    .desired_width(300.0),
            );
            if ui.button("浏览…").clicked() {
                self.picker = Some(FilePicker::new(PickMode::SteamRoot, &self.steam_root));
            }
            if ui.button("重新检测").clicked() {
                self.refresh_install();
            }
        });

        ui.add_space(4.0);
        match &self.install {
            Some(install) => {
                ui.label(
                    RichText::new(format!("已检测到 Steam：{}", install.describe()))
                        .size(12.0)
                        .color(theme::palette(ui.ctx()).muted_foreground),
                );
            }
            None => {
                ui.label(
                    RichText::new("未检测到 Steam（不影响免 Steam 使用）")
                        .size(12.0)
                        .color(theme::palette(ui.ctx()).muted_foreground),
                );
            }
        }

        if self.shortcuts.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new("还没有写入任何 Steam 快捷方式")
                    .size(12.0)
                    .color(theme::palette(ui.ctx()).muted_foreground),
            );
            return;
        }

        ui.add_space(6.0);

        let mut remove_key = None;

        Grid::new("game_library_steam_shortcuts")
            .num_columns(3)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for shortcut in &self.shortcuts {
                    ui.label(&shortcut.name);

                    ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(shortcut.clean_exe())
                                    .size(12.0)
                                    .color(theme::palette(ui.ctx()).muted_foreground),
                            )
                            .truncate(),
                        );
                    });

                    if ui.button("移除").clicked() {
                        remove_key = Some(shortcut.key.clone());
                    }
                    ui.end_row();
                }
            });

        if let Some(key) = remove_key {
            if let Some(install) = self.install.clone() {
                match steam_library::remove_shortcut(&install, &key) {
                    Ok(_) => {
                        self.set_status(StatusKind::Ok, "已从 Steam 库移除该条目");
                        self.refresh_shortcuts();
                    }
                    Err(err) => self.set_status(
                        StatusKind::Error,
                        format!("移除失败：{err}（请先关闭 Steam 后重试）"),
                    ),
                }
            }
        }
    }
}

fn auto_detect_steamvr_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(install) = steam_library::find_steam_install("") {
        candidates.push(
            install
                .root
                .join("steamapps")
                .join("common")
                .join("SteamVR"),
        );
    }

    for letter in 'C'..='Z' {
        let drive = format!("{letter}:\\");
        if !Path::new(&drive).is_dir() {
            continue;
        }
        candidates.push(PathBuf::from(format!("{drive}SteamVR")));
        candidates.push(PathBuf::from(format!("{drive}SteamVR 1.27.5")));
        candidates.push(PathBuf::from(format!(
            "{drive}Program Files (x86)\\Steam\\steamapps\\common\\SteamVR"
        )));
        candidates.push(PathBuf::from(format!(
            "{drive}Program Files\\Steam\\steamapps\\common\\SteamVR"
        )));
        candidates.push(PathBuf::from(format!("{drive}game\\VR\\SteamVR 1.27.5")));
    }

    // The runtime bundled next to the dashboard
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..8 {
            let Some(current) = dir else { break };
            candidates.push(current.join("SteamVR 1.27.5"));
            dir = current.parent().map(Path::to_path_buf);
        }
    }

    candidates.into_iter().find(|candidate| {
        candidate
            .join("SteamVR")
            .join("bin")
            .join("win64")
            .join("vrmonitor.exe")
            .is_file()
            || candidate
                .join("bin")
                .join("win64")
                .join("vrmonitor.exe")
                .is_file()
    })
}
