//! "有线投屏" tab - the glasses docked to the PC are just a DP monitor. This
//! tab turns that monitor into an immersive desktop instead of a plain second
//! screen, by driving the vendored desktop-mode renderer
//! (`desktop-mode/HwVRDesktop.exe`, MIT, deps/huawei-desktop).
//!
//! The renderer captures the Windows desktop with DXGI Desktop Duplication and
//! draws it as a curved, stereo-corrected image into the glasses panel. It does
//! not need SteamVR, the Huawei assistant, or the USB data link. This tab owns
//! the display setup, the renderer lifecycle and its `desktop_mode.ini`.

use alvr_server_io::{apply_cast, list_displays, CastLayout, DisplayInfo};
use alvr_gui_common::theme;
use eframe::egui::{self, RichText, ScrollArea, Ui};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// CREATE_NO_WINDOW: the dashboard is a GUI app; spawning tasklist/taskkill
/// without this flag flashes a black console window on every call. The UI calls
/// the process check repeatedly, which looked like an endless popup storm.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How often to re-check whether the renderer is running. Checking every frame
/// is both wasteful and (without the flag above) visually noisy.
const PROCESS_POLL_INTERVAL: Duration = Duration::from_secs(1);

const RENDERER_DIR: &str = "desktop-mode";
const RENDERER_EXE: &str = "HwVRDesktop.exe";
const RENDERER_CONFIG: &str = "desktop_mode.ini";

/// A single ini key exposed in the UI. `section.key` maps to the file. Numeric
/// fields get a slider over `range`; step/enum fields stay as a small editor.
struct IniField {
    section: &'static str,
    key: &'static str,
    label: &'static str,
    hint: &'static str,
    /// Some((min, max)) -> render as a slider; None -> plain text editor.
    range: Option<(f32, f32)>,
}

const fn num(section: &'static str, key: &'static str, label: &'static str, hint: &'static str, min: f32, max: f32) -> IniField {
    IniField { section, key, label, hint, range: Some((min, max)) }
}
const fn txt(section: &'static str, key: &'static str, label: &'static str, hint: &'static str) -> IniField {
    IniField { section, key, label, hint, range: None }
}

const FIELDS: &[IniField] = &[
    // 画面方向（画面上下/左右颠倒时调这里）
    txt("display", "flip180", "整幅翻转180° (0/1)", "整幅翻转；默认关(0)，每眼朝向请用下面「左右眼旋转」"),
    num("display", "eye_rot_left", "左眼贴屏角 (°)", "贴屏角基准 左0/右180（必须差180°）；一般不用动", 0.0, 360.0),
    num("display", "eye_rot_right", "右眼贴屏角 (°)", "与左眼相差180°（面板+镜片物理特性）", 0.0, 360.0),
    num("display", "roll_left", "左眼旋转 (°)", "每眼独立旋转画面；向下转90°填 -90（当前转正值）", -180.0, 180.0),
    num("display", "roll_right", "右眼旋转 (°)", "每眼独立旋转画面；与左眼同值 -90", -180.0, 180.0),
    txt("s3d", "swap_eyes", "左右眼交换 (0/1)", "左右眼画面反了就设为 1"),
    // 常规画面参数
    num("display", "distance", "距离 (m)", "画面离眼睛多远，0.5~10", 0.5, 10.0),
    num("display", "scale", "缩放", "画面大小", 0.5, 10.0),
    num("display", "curvature", "弯曲度", "0=平面，1=贴脸圆柱", 0.0, 1.0),
    num("display", "opacity", "不透明度", "0~1", 0.1, 1.0),
    num("display", "ipd", "瞳距 (mm)", "左右眼间距", 30.0, 80.0),
    num("display", "fov_tan_x", "水平视角", "tan 值，1.0=90°，越小画面越大", 0.5, 2.0),
    txt("mode", "follow", "跟随头部 (1/0)", "1=平滑跟随，0=悬停世界锁定"),
    txt("s3d", "mode", "3D 模式", "0=2D，1=SBS半宽，2=SBS全宽，3=上下"),
];

pub struct WiredDisplayTab {
    displays: Vec<DisplayInfo>,
    selected: Option<String>,
    status: Option<(bool, String)>,
    dirty: bool,
    /// Values loaded from / saved to desktop_mode.ini, parallel to FIELDS.
    values: Vec<String>,
    config_loaded: bool,
    /// Cached renderer-running state, refreshed at most once a second.
    renderer_running: bool,
    last_process_check: Option<Instant>,
}

impl Default for WiredDisplayTab {
    fn default() -> Self {
        Self::new()
    }
}

/// Directory holding the renderer + its ini, next to the dashboard executable.
fn renderer_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    Some(dir.join(RENDERER_DIR))
}

fn renderer_exe() -> Option<PathBuf> {
    renderer_dir().map(|dir| dir.join(RENDERER_EXE))
}

/// Read one `section.key` value from an ini string.
fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed[1..trimmed.len() - 1].trim().eq_ignore_ascii_case(section);
            continue;
        }
        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim().to_owned());
            }
        }
    }
    None
}

/// Set one `section.key` value in an ini string, preserving everything else.
fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut in_section = false;
    let mut found = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed[1..trimmed.len() - 1].trim().eq_ignore_ascii_case(section);
            continue;
        }
        if in_section && !found && !trimmed.starts_with('#') && !trimmed.starts_with(';') {
            if let Some((k, _)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    *line = format!("{}={}", k.trim(), value);
                    found = true;
                }
            }
        }
    }
    if !found {
        if !lines.iter().any(|l| l.trim().eq_ignore_ascii_case(&format!("[{section}]"))) {
            if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
        }
        // Insert right after the section header.
        let header = format!("[{section}]");
        if let Some(pos) = lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&header)) {
            lines.insert(pos + 1, format!("{key}={value}"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

impl WiredDisplayTab {
    pub fn new() -> Self {
        let mut tab = Self {
            displays: vec![],
            selected: None,
            status: None,
            dirty: true,
            values: vec![String::new(); FIELDS.len()],
            config_loaded: false,
            renderer_running: false,
            last_process_check: None,
        };
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        self.displays = list_displays();
        if self
            .selected
            .as_ref()
            .map(|name| !self.displays.iter().any(|d| &d.device_name == name))
            .unwrap_or(true)
        {
            self.selected = self
                .displays
                .iter()
                .find(|d| d.is_glasses())
                .or_else(|| self.displays.iter().find(|d| d.attached))
                .map(|d| d.device_name.clone());
        }
        self.load_config();
        self.dirty = false;
    }

    fn load_config(&mut self) {
        self.config_loaded = false;
        let Some(dir) = renderer_dir() else { return };
        let text = fs::read_to_string(dir.join(RENDERER_CONFIG)).unwrap_or_default();
        for (i, field) in FIELDS.iter().enumerate() {
            self.values[i] = ini_get(&text, field.section, field.key).unwrap_or_default();
        }
        self.config_loaded = !text.is_empty();
    }

    fn save_config(&mut self) {
        let Some(dir) = renderer_dir() else {
            self.set_status(false, "无法确定程序目录");
            return;
        };
        if let Err(err) = fs::create_dir_all(&dir) {
            self.set_status(false, format!("无法创建桌面模式目录：{err}"));
            return;
        }
        let path = dir.join(RENDERER_CONFIG);
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut updated = text;
        for (i, field) in FIELDS.iter().enumerate() {
            let value = self.values[i].trim();
            if !value.is_empty() {
                updated = ini_set(&updated, field.section, field.key, value);
            }
        }
        match fs::write(&path, updated) {
            Ok(()) => {
                self.config_loaded = true;
                self.set_status(true, format!("已保存配置：{}", path.display()));
            }
            Err(err) => self.set_status(false, format!("保存配置失败：{err}")),
        }
    }

    fn set_status(&mut self, ok: bool, message: impl Into<String>) {
        self.status = Some((ok, message.into()));
    }

    fn report(&mut self, result: alvr_common::anyhow::Result<()>, ok_text: &str) {
        self.status = Some(match result {
            Ok(()) => (true, ok_text.to_owned()),
            Err(err) => (false, err.to_string()),
        });
        self.dirty = true;
    }

    /// One-off check with no visible console. Called from the throttled refresh
    /// and from button handlers, never directly in the UI draw path.
    fn probe_renderer_running() -> bool {
        let mut command = Command::new("tasklist");
        command.args(["/FI", &format!("IMAGENAME eq {RENDERER_EXE}"), "/NH"]);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let Ok(out) = command.output() else {
            return false;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout.to_ascii_lowercase().contains(&RENDERER_EXE.to_ascii_lowercase())
    }

    /// Refreshes the cached state at most once per second.
    fn refresh_process_state(&mut self) {
        let due = self
            .last_process_check
            .map(|t| t.elapsed() >= PROCESS_POLL_INTERVAL)
            .unwrap_or(true);
        if due {
            self.renderer_running = Self::probe_renderer_running();
            self.last_process_check = Some(Instant::now());
        }
    }

    fn start_renderer(&mut self) {
        let Some(exe) = renderer_exe() else {
            self.set_status(false, "无法确定程序目录");
            return;
        };
        if !exe.is_file() {
            self.set_status(
                false,
                format!("找不到桌面模式渲染器：{}（请确认 desktop-mode 目录随包分发）", exe.display()),
            );
            return;
        }
        if Self::probe_renderer_running() {
            self.renderer_running = true;
            self.set_status(true, "桌面模式已经在运行");
            return;
        }
        // SteamVR and desktop mode fight over the glasses monitor; stop it.
        crate::steamvr_launcher::maybe_kill_steamvr();

        let mut command = Command::new(&exe);
        command
            .current_dir(exe.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        match command.spawn() {
            Ok(child) => {
                self.renderer_running = true;
                self.last_process_check = Some(Instant::now());
                let _ = child; // detached: renderer lives on its own
                self.set_status(true, "桌面模式已启动（退出快捷键见下方说明）");
            }
            Err(err) => self.set_status(false, format!("启动桌面模式失败：{err}")),
        }
    }

    fn stop_renderer(&mut self) {
        let mut command = Command::new("taskkill");
        command.args(["/IM", RENDERER_EXE, "/F"]);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let _ = command.output();
        self.renderer_running = false;
        self.last_process_check = Some(Instant::now());
        self.set_status(true, "已停止桌面模式");
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if self.dirty {
            self.refresh();
        }
        // Throttled to once a second: no per-frame tasklist spawn, no console
        // flash storm when this tab is open.
        self.refresh_process_state();

        let palette = theme::palette(ui.ctx());
        let glasses = self.displays.iter().find(|d| d.is_glasses()).cloned();
        let renderer_ready = renderer_exe().map(|p| p.is_file()).unwrap_or(false);

        // The tab stacks four cards plus parameter rows and a log block, which
        // easily exceeds a small window. Wrap everything in a vertical scroller
        // so the lower settings stay reachable.
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.render_content(ui, &palette, glasses.as_ref(), renderer_ready));
    }

    fn render_content(
        &mut self,
        ui: &mut Ui,
        palette: &theme::Palette,
        glasses: Option<&DisplayInfo>,
        renderer_ready: bool,
    ) {
        theme::card_frame(ui.ctx())
            .fill(palette.surface)
            .show(ui, |ui| {
                ui.label(RichText::new("有线投屏（桌面模式）").size(18.0).strong());
                ui.label(
                    RichText::new(
                        "眼镜直插 PC 时是一块 3200x1600 的 DP 显示器。桌面模式把 Windows 桌面直接画进眼镜，\
                         带曲面/3D/头动跟随，无需 SteamVR 或华为助手。使用「auto-vr-mode.ps1 -Mode usb」切到直连模式。",
                    )
                    .size(12.0)
                    .color(palette.muted_foreground),
                );
                ui.add_space(6.0);

                let (text, color) = match &glasses {
                    Some(g) => (
                        format!("已检测到眼镜：{} — {}", g.friendly_name, g.summary()),
                        palette.ok,
                    ),
                    None => (
                        "未检测到眼镜显示器（EDID 无 RTK2560/HWV/HUAWEI）——先插好 USB-C 并确认是扩展屏".to_owned(),
                        palette.accent_variant,
                    ),
                };
                ui.colored_label(color, text);

                let (rtext, rcolor) = if renderer_ready {
                    ("桌面模式渲染器：已就绪", palette.ok)
                } else {
                    ("桌面模式渲染器：缺失（desktop-mode/HwVRDesktop.exe）", palette.error)
                };
                ui.colored_label(rcolor, rtext);
            });

        ui.add_space(10.0);
        theme::card_frame(ui.ctx())
            .fill(palette.surface)
            .show(ui, |ui| {
                ui.label(RichText::new("启动 / 停止").size(16.0).strong());
                ui.label(
                    RichText::new(
                        "桌面模式默认关闭：它和 SteamVR 抢同一块眼镜屏，两者只能开一个。\
                         启动时会自动停掉 SteamVR；要用串流请点「停止桌面模式」再从设备页启动 SteamVR。",
                    )
                    .size(11.0)
                    .color(palette.muted_foreground),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(renderer_ready && glasses.is_some(), egui::Button::new("启动桌面模式"))
                        .clicked()
                    {
                        self.start_renderer();
                    }
                    if ui.button("停止桌面模式").clicked() {
                        self.stop_renderer();
                    }
                    ui.label(
                        RichText::new(format!(
                            "状态：{}",
                            if self.renderer_running { "运行中" } else { "未运行（默认）" }
                        ))
                        .weak(),
                    );
                });
                ui.label(
                    RichText::new(
                        "默认快捷键：Ctrl+Alt+Home 回中 / 方向键 平移 / +/- 缩放 / PageUp·PageDown 弯曲 / \
                         ]·[ 距离 / 3 切 3D / F 跟随·悬停 / H 帮助。可在 desktop_mode.ini 的 [hotkeys] 改。",
                    )
                    .size(11.0)
                    .color(palette.muted_foreground),
                );
            });

        ui.add_space(10.0);
        theme::card_frame(ui.ctx())
            .fill(palette.surface)
            .show(ui, |ui| {
                ui.label(RichText::new("画面参数").size(16.0).strong());
                ui.label(
                    RichText::new(if self.config_loaded {
                        "配置来自 desktop_mode.ini（运行中改动会自动重载）"
                    } else {
                        "未找到 desktop_mode.ini，将用渲染器默认值；点保存即生成"
                    })
                    .size(11.0)
                    .color(palette.muted_foreground),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("画面方向不对（上下颠倒 / 左右反）？调下面一组即可——改动实时保存到 ini，运行中渲染器自动重载，无需点保存。")
                        .size(11.0)
                        .color(palette.accent_variant),
                );
                for (i, field) in FIELDS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(field.label).on_hover_text(field.hint);
                        if let Some((min, max)) = field.range {
                            // Numeric field: slider + live value. Parse the
                            // stored string, fall back to the range midpoint.
                            let mut v = self.values[i].parse::<f32>().unwrap_or((min + max) / 2.0);
                            let changed = ui
                                .add(egui::Slider::new(&mut v, min..=max))
                                .changed();
                            if changed {
                                self.values[i] = format!("{v:.2}");
                                // Live-apply: write the ini immediately so the
                                // renderer's 600ms hot-reload picks it up. No
                                // need to click a separate "Save" button.
                                self.save_config();
                            }
                        } else {
                            let changed = ui.add(
                                egui::TextEdit::singleline(&mut self.values[i])
                                    .desired_width(60.0)
                                    .hint_text("默认"),
                            ).changed();
                            if changed {
                                // Live-apply text fields on edit-complete.
                                self.save_config();
                            }
                        }
                    });
                }
                if ui.button("保存参数").clicked() {
                    self.save_config();
                }
            });

        ui.add_space(10.0);
        theme::card_frame(ui.ctx())
            .fill(palette.surface)
            .show(ui, |ui| {
                ui.label(RichText::new("显示器布局（可选）").size(16.0).strong());
                ui.label(
                    RichText::new("桌面模式会在眼镜屏上开全屏窗口；下面这些只影响普通桌面镜像，可不用。")
                        .size(11.0)
                        .color(palette.muted_foreground),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(glasses.is_some(), egui::Button::new("镜像主屏到眼镜"))
                        .clicked()
                    {
                        let result = apply_cast(CastLayout::Mirror);
                        self.report(result, "已镜像桌面到眼镜");
                    }
                    if ui
                        .add_enabled(glasses.is_some(), egui::Button::new("扩展为第二屏"))
                        .clicked()
                    {
                        let result = apply_cast(CastLayout::ExtendRight);
                        self.report(result, "眼镜已扩展为主屏右侧");
                    }
                    if ui.button("刷新显示器").clicked() {
                        self.status = None;
                        self.dirty = true;
                    }
                });
            });

        if let Some((ok, message)) = &self.status {
            ui.add_space(8.0);
            ui.colored_label(if *ok { palette.ok } else { palette.error }, message);
        }

        // The renderer logs errors instead of showing a modal dialog (it is a
        // child process of the dashboard). Surface the last log lines here so
        // a failed start is diagnosable without opening files manually.
        if let Some(dir) = renderer_dir() {
            let log_path = dir.join("desktop_mode.log");
            if let Ok(text) = fs::read_to_string(&log_path) {
                let lines: Vec<&str> = text.lines().rev().take(5).collect();
                if !lines.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("渲染器日志（最近 5 行）：").weak().size(11.0));
                    for line in lines.iter().rev() {
                        ui.label(RichText::new(*line).weak().size(10.0));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ini_get, ini_set};

    #[test]
    fn ini_round_trip_preserves_other_keys() {
        let original = "[general]\nenabled=1\n\n[display]\ndistance=2.0\nscale=4.29\n";
        let updated = ini_set(original, "display", "distance", "3.5");
        assert_eq!(ini_get(&updated, "display", "distance").as_deref(), Some("3.5"));
        assert_eq!(ini_get(&updated, "display", "scale").as_deref(), Some("4.29"));
        assert_eq!(ini_get(&updated, "general", "enabled").as_deref(), Some("1"));
    }

    #[test]
    fn ini_set_adds_missing_section_and_key() {
        let updated = ini_set("", "s3d", "mode", "2");
        assert_eq!(ini_get(&updated, "s3d", "mode").as_deref(), Some("2"));
    }
}
