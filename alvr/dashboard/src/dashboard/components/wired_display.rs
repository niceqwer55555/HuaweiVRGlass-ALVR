//! "有线投屏" tab - wired mode headset setup.
//!
//! In wired mode the glasses are a plain DP monitor to Windows, so screen
//! casting needs neither SteamVR nor the Huawei assistant: pick the monitor,
//! pick a mode, choose a layout.

use alvr_server_io::{display, CastLayout, DisplayInfo, DisplayMode};
use eframe::egui::{self, Ui};

pub struct WiredDisplayTab {
    displays: Vec<DisplayInfo>,
    selected: Option<String>,
    modes: Vec<DisplayMode>,
    pending_mode: Option<DisplayMode>,
    status: Option<(bool, String)>,
    /// Bumped whenever something changed, so the next frame refreshes the list.
    dirty: bool,
}

impl Default for WiredDisplayTab {
    fn default() -> Self {
        Self::new()
    }
}

impl WiredDisplayTab {
    pub fn new() -> Self {
        let mut tab = Self {
            displays: vec![],
            selected: None,
            modes: vec![],
            pending_mode: None,
            status: None,
            dirty: true,
        };
        tab.refresh();
        tab
    }

    fn refresh(&mut self) {
        self.displays = display::list_displays();

        // Default to the glasses; fall back to the first attached monitor.
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

        self.modes = self
            .selected
            .as_deref()
            .map(display::list_modes)
            .unwrap_or_default();

        // Keep the combo box on the mode that is actually in use.
        self.pending_mode = self.selected.as_ref().and_then(|name| {
            self.displays
                .iter()
                .find(|d| &d.device_name == name)
                .and_then(|d| {
                    self.modes.iter().find_map(|m| {
                        (m.width == d.width
                            && m.height == d.height
                            && m.refresh_rate == d.refresh_rate)
                            .then(|| m.clone())
                    })
                })
        });

        self.dirty = false;
    }

    fn selected_display(&self) -> Option<&DisplayInfo> {
        self.selected
            .as_ref()
            .and_then(|n| self.displays.iter().find(|d| &d.device_name == n))
    }

    fn report(&mut self, result: alvr_common::anyhow::Result<()>, ok_text: &str) {
        self.status = Some(match result {
            Ok(()) => (true, ok_text.to_owned()),
            Err(e) => (false, e.to_string()),
        });
        self.dirty = true;
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        if self.dirty {
            self.refresh();
        }

        let glasses = self.displays.iter().find(|d| d.is_glasses()).cloned();

        // ---- status line ----
        ui.horizontal(|ui| {
            let (text, color) = match &glasses {
                Some(g) => (
                    format!("已检测到眼镜：{}", g.friendly_name),
                    egui::Color32::from_rgb(80, 200, 120),
                ),
                None => (
                    "未检测到眼镜（显示器 EDID 里没有 RTK2560 / HUAWEI）".to_owned(),
                    egui::Color32::from_rgb(230, 160, 60),
                ),
            };
            ui.colored_label(color, text);
        });
        if let Some(g) = &glasses {
            ui.label(
                egui::RichText::new(format!("   {}", g.summary()))
                    .weak()
                    .size(12.0),
            );
        }
        ui.add_space(4.0);

        if let Some((ok, msg)) = &self.status {
            let color = if *ok {
                egui::Color32::from_rgb(80, 200, 120)
            } else {
                egui::Color32::from_rgb(230, 90, 90)
            };
            ui.colored_label(color, msg);
            ui.add_space(4.0);
        }

        ui.separator();

        // ---- monitor list ----
        ui.heading("显示器");
        for d in &self.displays {
            let is_selected = self.selected.as_deref() == Some(d.device_name.as_str());
            ui.horizontal(|ui| {
                let tag = if d.is_glasses() {
                    "眼镜"
                } else if d.primary {
                    "主屏"
                } else if d.attached {
                    "扩展"
                } else {
                    "未启用"
                };
                if ui.selectable_label(is_selected, tag).clicked() {
                    self.selected = Some(d.device_name.clone());
                    self.dirty = true;
                }
                ui.label(egui::RichText::new(&d.friendly_name).strong());
                ui.label(egui::RichText::new(d.summary()).weak().size(12.0));
            });
            ui.label(
                egui::RichText::new(format!("   EDID {}", d.monitor_id))
                    .weak()
                    .size(11.0),
            );
        }

        ui.add_space(6.0);
        ui.separator();

        // ---- settings ----
        ui.heading("设置");
        let Some(current) = self.selected_display().cloned() else {
            ui.label("没有可设置的显示器。");
            return;
        };

        ui.label(egui::RichText::new(&current.device_name).weak().size(12.0));

        // resolution / refresh rate
        ui.horizontal(|ui| {
            ui.label("分辨率 / 刷新率");
            let mut chosen = self.pending_mode.clone();
            egui::ComboBox::from_id_salt("wired_mode")
                .width(220.0)
                .selected_text(
                    chosen
                        .as_ref()
                        .map(|m| m.label())
                        .unwrap_or_else(|| "未选择".to_owned()),
                )
                .show_ui(ui, |ui| {
                    for m in &self.modes {
                        let label = m.label();
                        ui.selectable_value(&mut chosen, Some(m.clone()), label);
                    }
                });
            self.pending_mode = chosen;

            let unchanged = self.pending_mode.as_ref().map(|m| {
                m.width == current.width
                    && m.height == current.height
                    && m.refresh_rate == current.refresh_rate
            });
            if ui
                .add_enabled(
                    self.pending_mode.is_some() && !unchanged.unwrap_or(true),
                    egui::Button::new("应用"),
                )
                .clicked()
            {
                if let Some(mode) = self.pending_mode.clone() {
                    let name = current.device_name.clone();
                    let result = display::apply_mode(&name, &mode);
                    self.report(result, "已应用显示模式");
                }
            }
        });

        // rotation - the glasses panel can be mounted rotated
        ui.horizontal(|ui| {
            ui.label("旋转");
            for deg in [0u32, 90, 180, 270] {
                let active = current.orientation
                    == match deg {
                        0 => 0,
                        90 => 1,
                        180 => 2,
                        _ => 3,
                    };
                if ui.selectable_label(active, format!("{deg}°")).clicked() && !active {
                    let name = current.device_name.clone();
                    let result = display::apply_orientation(&name, deg);
                    self.report(result, &format!("已旋转到 {deg}°"));
                    return;
                }
            }
        });

        ui.add_space(6.0);
        ui.separator();

        // ---- cast ----
        ui.heading("投屏");
        ui.label(
            egui::RichText::new("有线模式下眼镜就是一块 DP 显示器，投屏不需要 SteamVR。")
                .weak()
                .size(12.0),
        );
        ui.add_space(4.0);

        ui.columns(2, |ui| {
            if ui[0]
                .add_enabled(glasses.is_some(), egui::Button::new("镜像桌面（眼镜显示主屏内容）"))
                .clicked()
            {
                let result = display::apply_cast(CastLayout::Mirror);
                self.report(result, "已镜像桌面到眼镜");
            }
            if ui[1]
                .add_enabled(glasses.is_some(), egui::Button::new("扩展（眼镜放主屏右侧）"))
                .clicked()
            {
                let result = display::apply_cast(CastLayout::ExtendRight);
                self.report(result, "眼镜已扩展为主屏右侧的第二块屏");
            }
            if ui[0]
                .add_enabled(glasses.is_some(), egui::Button::new("眼镜放左侧（主屏移到右边）"))
                .clicked()
            {
                let result = display::apply_cast(CastLayout::GlassesLeft);
                self.report(result, "眼镜已移到 (0,0)");
            }
            if ui[1].button("恢复横向排列").clicked() {
                let result = display::apply_cast(CastLayout::Row);
                self.report(result, "所有显示器已横向排列");
            }
        });

        ui.add_space(6.0);
        if ui.button("刷新").clicked() {
            self.status = None;
            self.dirty = true;
        }
    }
}
