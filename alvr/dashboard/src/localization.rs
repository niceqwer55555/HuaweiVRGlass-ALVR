use alvr_common::once_cell::sync::Lazy;
use eframe::egui::{Context, FontData, FontDefinitions, FontFamily};
use std::{collections::HashMap, sync::Arc};

static SETTINGS_NAMES: Lazy<HashMap<String, String>> =
    Lazy::new(|| serde_json::from_str(include_str!("../resources/zh_cn_settings.json")).unwrap());

static SETTINGS_HELP: Lazy<HashMap<String, String>> =
    Lazy::new(|| serde_json::from_str(include_str!("../resources/zh_cn_help.json")).unwrap());

pub fn install_chinese_font(context: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "noto_sans_cjk_sc".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../resources/NotoSansCJKsc-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        crate::icons::FAMILY_NAME.into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../resources/MaterialSymbolsSharp-subset.ttf"
        ))),
    );
    fonts.families.insert(
        crate::icons::family(),
        vec![crate::icons::FAMILY_NAME.into()],
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "noto_sans_cjk_sc".into());
    }

    context.set_fonts(fonts);
}

pub fn setting_name(
    id: &str,
    strings: &HashMap<String, String>,
    fallback: impl FnOnce() -> String,
) -> String {
    if let Some(display_name) = strings.get("display_name") {
        ui_text(display_name)
    } else if let Some(display_name) = SETTINGS_NAMES.get(id) {
        display_name.clone()
    } else {
        ui_text(&fallback())
    }
}

pub fn schema_help(text: &str) -> String {
    SETTINGS_HELP
        .get(text)
        .cloned()
        .unwrap_or_else(|| ui_text(text))
}

pub fn ui_text(text: &str) -> String {
    let translated = match text {
        "Disabled" => "禁用",
        "Enabled" => "启用",
        "Default" => "默认",
        "Set" => "设置",
        "Automatic" => "自动",
        "Maximum" => "最大",
        "Custom" => "自定义",
        "Static" => "静态",
        "Dynamic" => "动态",
        "Constant" => "固定",
        "Adaptive" => "自适应",
        "Absolute" => "绝对分辨率",
        "Scale" => "缩放比例",
        "Blend" => "混合",
        "Passthrough" => "透视",
        "Hysteresis threshold" => "滞回阈值",
        "Binary to scalar" => "二值转标量",
        "Remap" => "重新映射",
        "Local floor" => "本地地面",
        "Local" => "本地",
        "Yaw" => "仅偏航角",
        "Tilted" => "倾斜模式",
        "Best effort" => "尽力而为",
        "Class selector" => "类别选择器",
        "Assured forwarding" => "保证转发",
        "Expedited forwarding" => "加速转发",
        "Body tracking" | "Body Tracking" => "身体追踪",
        "Object tracking" | "Object Tracking" => "物体追踪",
        "Full body" => "全身",
        "Fake Vive tracker" | "Fake Vive Trackers" => "模拟 Vive Tracker",
        "By index" => "按索引",
        "By name (substring)" => "按名称（包含文本）",
        "Main" => "主配置",
        "Baseline" => "兼容配置",
        "Average buffering" => "平均缓冲",
        "Batch size" => "批处理大小",
        "Duration" => "持续时间",
        "Encoder thread count" => "编码线程数",
        "Maximum bitrate" => "最大码率",
        "Minimum bitrate" => "最小码率",
        "Maximum buffering" => "最大缓冲",
        "Maximum decoder latency" => "最大解码延迟",
        "Maximum network latency" => "最大网络延迟",
        "Minimum duration" => "最短持续时间",
        "Minimum IDR interval" => "最短 IDR 间隔",
        "Prediction" => "预测",
        "Preferred FPS" => "首选帧率",
        "Quality preset" => "画质预设",
        "Type" => "类型",
        "1/4 resolution" => "四分之一分辨率",
        "10-bit encoding" => "10 位编码",
        "Center region height" => "中心区域高度",
        "Center region width" => "中心区域宽度",
        "Center shift X" => "中心区域水平偏移",
        "Center shift Y" => "中心区域垂直偏移",
        "Clamp HDR extended range" => "限制 HDR 扩展范围",
        "Client receive buffer size" => "客户端接收缓冲区",
        "Client send buffer size" => "客户端发送缓冲区",
        "Client-side post-processing" => "客户端后处理",
        "Enable HDR" => "启用 HDR",
        "Enable High-Motion Quality Boost" => "启用高运动画质增强",
        "Enable Pre-analysis" => "启用预分析",
        "Enable VBAQ/CAQ" => "启用 VBAQ/CAQ",
        "Encoding Gamma" => "编码伽马",
        "Force HDR sRGB Correction" => "强制 HDR sRGB 校正",
        "Force software encoding" => "强制软件编码",
        "Foveation offset" => "注视点偏移",
        "FPS reset threshold multiplier" => "帧率重置阈值倍数",
        "Full range color" => "全范围色彩",
        "Headset microphone" => "头显麦克风",
        "Headset speaker" => "头显扬声器",
        "Horizontal edge ratio" => "水平边缘比例",
        "HSV Chroma Key" => "HSV 色度键",
        "RGB Chroma Key" => "RGB 色度键",
        "Mute desktop audio when streaming" => "串流时静音桌面音频",
        "Open and close SteamVR with dashboard" => "随控制台启动和关闭 SteamVR",
        "Override for 10-bit encoding" => "覆盖客户端 10 位编码设置",
        "Override for encoding gamma" => "覆盖客户端编码伽马",
        "Override for full range color" => "覆盖客户端全范围色彩",
        "Override for HDR" => "覆盖客户端 HDR 设置",
        "Software (CPU) encoding" => "软件（CPU）编码",
        "Start video recording at client connection" => "客户端连接后开始录像",
        "Streamer receive buffer size" => "串流端接收缓冲区",
        "Streamer send buffer size" => "串流端发送缓冲区",
        "Vertical edge ratio" => "垂直边缘比例",
        "Very Low (width: 3072)" => "极低（宽度：3072）",
        "Low (width: 3712)" => "低（宽度：3712）",
        "Medium (width: 4288)" => "中（宽度：4288）",
        "High (width: 5184)" => "高（宽度：5184）",
        "Ultra (width: 5632)" => "超高（宽度：5632）",
        "Extreme (width: 6080)" => "极致（宽度：6080）",
        "Speed" => "速度优先",
        "Balanced" => "均衡",
        "Quality" => "画质优先",
        "Light" => "轻度",
        "Medium" => "中等",
        "High" => "高",
        "System Default" => "系统默认",
        "ALVR bindings" => "ALVR 按键映射",
        "Hand tracking interaction" => "手势交互",
        "Eye and face tracking" => "眼动与面部追踪",
        "Preset not applied" => "未套用预设",
        "Resolution" => "分辨率",
        "Preferred framerate" => "首选帧率",
        "Codec preset" => "编码格式预设",
        "Encoder preset" => "编码器预设",
        "Foveation preset" => "注视点编码预设",
        "Choosing too high resolution (commonly 'High (width: 5184)') may result in high latency or black screen." => {
            "分辨率过高（通常为“高（宽度：5184）”）可能导致高延迟或黑屏。"
        }
        "AV1 is only supported on newer gpus (AMD RX 7xxx+ , NVIDIA RTX 30xx+, Intel ARC)!" => {
            "AV1 仅受较新的显卡支持（AMD RX 7xxx+、NVIDIA RTX 30xx+、Intel ARC）！"
        }
        "Selecting a quality too high may result in stuttering or still image!" => {
            "画质预设过高可能导致卡顿或画面静止！"
        }
        "Foveation affects pixelation on the edges of the screen and significantly reduces codec latency. \nIt is not recommended to fully disable it, as it may cause shutterring and high encode/decode latency!" => {
            "注视点编码会影响画面边缘的像素感，并显著降低编解码延迟。\n不建议完全禁用，否则可能引起卡顿和较高的编解码延迟！"
        }
        "Disabled: hands cannot emulate buttons. Useful for using Joy-Cons or other non-native controllers.\nSteamVR Input 2.0: create separate SteamVR devices for hand tracking.\nALVR bindings: use ALVR hand tracking button bindings. Check the wiki for help.\n" => {
            "禁用：双手不能模拟按键，适用于 Joy-Con 等非原生控制器。\nSteamVR Input 2.0：为手部追踪创建独立的 SteamVR 设备。\nALVR 按键映射：使用 ALVR 的手势按键映射。\n"
        }
        "You can change the default audio device from the system taskbar tray (bottom right)" => {
            "可在系统任务栏右下角切换默认音频设备"
        }
        "Disabling foveated encoding may result in significantly higher encode/decode times and stuttering, or even crashing.\nIf you want to reduce the amount of pixelation on the edges, increase the center region width and height" => {
            "禁用注视点编码可能显著增加编解码耗时，引起卡顿甚至崩溃。\n若要减轻画面边缘的像素感，请增大中心区域的宽度和高度。"
        }
        "To be able to use the microphone on Windows, you need to install Virtual Audio Cable" => {
            "要在 Windows 上使用麦克风，需要安装 Virtual Audio Cable。"
        }
        _ => text,
    };

    translated.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_resources_are_complete_and_parseable() {
        assert_eq!(SETTINGS_NAMES.len(), 235);
        assert_eq!(SETTINGS_HELP.len(), 93);
        assert!(include_bytes!("../resources/NotoSansCJKsc-Regular.otf").len() > 1_000_000);
        assert!(include_bytes!("../resources/MaterialSymbolsSharp-subset.ttf").len() > 50_000);
    }

    #[test]
    fn representative_ui_strings_are_translated() {
        assert_eq!(ui_text("Disabled"), "禁用");
        assert_eq!(ui_text("Headset speaker"), "头显扬声器");
        assert_ne!(
            schema_help("Force enable on smartphone clients"),
            "Force enable on smartphone clients"
        );
    }
}
