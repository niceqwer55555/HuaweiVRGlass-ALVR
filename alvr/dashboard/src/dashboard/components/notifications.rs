use alvr_common::{LogEntry, LogSeverity};
use alvr_gui_common::theme::{self, log_colors};
use alvr_session::Settings;
use eframe::{
    egui::{self, Frame, Label, Layout, RichText, TopBottomPanel},
    emath::Align,
    epaint::{Color32, Stroke},
};
use rand::seq::IndexedRandom;
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use instant::Instant;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

const TIMEOUT: Duration = Duration::from_secs(5);
const NO_NOTIFICATIONS_MESSAGE: &str = "暂无新通知";
const NOTIFICATION_TIPS: &[&str] = &[
    // The following tips are ordered roughtly in the order settings appear
    "若修改设置后发生崩溃，请在“安装”页重新运行设置向导并选择“重置设置”。",
    "部分设置默认隐藏，点击设置旁的“展开”可显示子菜单。",
    "建议保留 ALVR 的默认音频设置，并从任务栏托盘切换系统默认音频设备。",
    "增大“最大缓冲”可减少卡顿，但会增加延迟。",
    "某些显卡需要在 H.264 与 HEVC 之间切换，以解决崩溃或退回软件编码的问题。",
    "NVIDIA 显卡通常适合高码率 H.264；AMD 显卡使用 HEVC 的画质可能更好。",
    "若画面出现白色雪花闪烁，请将分辨率降至“低”并尝试关闭“注视点编码”。",
    "提高“颜色校正”中的“锐化”可能改善主观画质。",
    "若外部控制器或 Tracker 无法与 ALVR 追踪空间同步，可在“附加 OpenVR 属性”中增加条目并设置自定义追踪系统名称。",
    "若要改变控制器的外观模型，请调整“头显 → 控制器 → 模拟模式”。",
    "ALVR 支持自定义按键映射；需要帮助时可前往项目 Discord。",
    "ALVR 支持手势按键（“预设 → 手势交互 → ALVR 按键映射”）。详细用法请查阅项目 Wiki。",
    "若手势容易误触，可在“控制器 → 手势”中禁用，或开启“仅触摸”。",
    "可通过“控制器 → 预测”微调控制器响应与平滑度。",
    "若虚拟控制器或手部模型与实体不一致，可调整“头显 → 控制器 → 左控制器位置/旋转偏移”（会镜像到右手）。",
    "使用外部 Tracker 或控制器时，建议将位置与旋转重定位模式都设为“禁用”。",
    "躺姿使用时，可将位置重定位设为“本地”，旋转重定位设为“倾斜模式”。",
    "若经常花屏，可开启“避免视频花屏”，代价是可能出现重复帧卡顿。",
    "可使用连接/断开脚本，在头显连接或断开时运行自定义命令。",
    "报告问题前可开启“附加设置 → 日志 → 日志写入磁盘”，日志位于 session_log.txt。",
    "调试时可开启追踪、按键和触觉日志，并通过 ws://localhost:8082/api/events 获取数据。",
    "分享崩溃日志前，建议开启“附加设置 → 日志 → 优先输出调用栈”。",
    "切换“显示操作提示”可快速浏览此类提示。",
    "若要同时使用身体 Tracker 或其他 SteamVR 驱动，请注意避免驱动注册冲突。",
    "启用“附加设置 → SteamVR 启动器 → 随控制台启动和关闭 SteamVR”会更方便。",
    "报告问题需要分享录像时，可启用“附加设置 → 捕获 → 滚动录像文件”来限制文件大小。",
    // Miscellaneous
    "若头显未出现在设备列表中，可能位于不同子网；请用头显显示的 IP 尝试“手动添加设备”。",
];

pub struct NotificationBar {
    message: String,
    current_level: LogSeverity,
    receive_instant: Instant,
    min_notification_level: LogSeverity,
    tip_message: Option<String>,
    expanded: bool,
}

impl NotificationBar {
    pub fn new() -> Self {
        Self {
            message: NO_NOTIFICATIONS_MESSAGE.into(),
            current_level: LogSeverity::Debug,
            receive_instant: Instant::now(),
            min_notification_level: LogSeverity::Debug,
            tip_message: None,
            expanded: false,
        }
    }

    pub fn update_settings(&mut self, settings: &Settings) {
        self.min_notification_level = settings.extra.logging.notification_level;

        if settings.extra.logging.show_notification_tip {
            if self.tip_message.is_none() {
                self.tip_message = NOTIFICATION_TIPS
                    .choose(&mut rand::rng())
                    .map(|s| format!("提示：{s}"));
            }
        } else {
            self.tip_message = None;
        }
    }

    pub fn push_notification(&mut self, event: LogEntry, from_dashboard: bool) {
        let now = Instant::now();
        let min_severity = if from_dashboard {
            if cfg!(debug_assertions) {
                LogSeverity::Debug
            } else {
                LogSeverity::Info
            }
        } else {
            self.min_notification_level
        };

        if event.severity >= min_severity
            && (now > self.receive_instant + TIMEOUT || event.severity >= self.current_level)
        {
            self.message = event.content;
            self.current_level = event.severity;
            self.receive_instant = now;
        }
    }

    pub fn ui(&mut self, context: &egui::Context) {
        let now = Instant::now();
        if now > self.receive_instant + TIMEOUT {
            self.message = self
                .tip_message
                .clone()
                .unwrap_or_else(|| NO_NOTIFICATIONS_MESSAGE.into());
            self.current_level = LogSeverity::Debug;
        }

        let (fg, bg) = match self.current_level {
            LogSeverity::Error => (Color32::BLACK, log_colors::ERROR_LIGHT),
            LogSeverity::Warning => (Color32::BLACK, log_colors::WARNING_LIGHT),
            LogSeverity::Info => (Color32::BLACK, log_colors::INFO_LIGHT),
            LogSeverity::Debug => (theme::FG, theme::LIGHTER_BG),
        };

        let mut bottom_bar = TopBottomPanel::bottom("bottom_panel").frame(
            Frame::default()
                .inner_margin(egui::vec2(10.0, 5.0))
                .fill(bg)
                .stroke(Stroke::new(1.0, theme::SEPARATOR_BG)),
        );
        let alignment = if !self.expanded {
            bottom_bar = bottom_bar.max_height(26.0);

            Align::TOP
        } else {
            Align::Center
        };

        bottom_bar.show(context, |ui| {
            ui.with_layout(Layout::right_to_left(alignment), |ui| {
                if !self.expanded {
                    if ui.small_button("展开").clicked() {
                        self.expanded = true;
                    }
                } else if ui.button("收起").clicked() {
                    self.expanded = false;
                }
                ui.with_layout(Layout::left_to_right(alignment), |ui| {
                    ui.add(Label::new(RichText::new(&self.message).color(fg).size(12.0)).wrap());
                })
            })
        });
    }
}
