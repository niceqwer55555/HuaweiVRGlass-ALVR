#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
pub use android::*;

use alvr_common::settings_schema::SettingsSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

pub const PACKAGE_NAME_STORE: &str = "alvr.client";
pub const PACKAGE_NAME_GITHUB_DEV: &str = "alvr.client.dev";
pub const PACKAGE_NAME_GITHUB_STABLE: &str = "alvr.client.stable";

// Platform of the device. It is used to match the VR runtime and enable features conditionally.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Platform {
    Quest1,
    Quest2,
    Quest3,
    Quest3S,
    QuestPro,
    QuestUnknown,
    PicoNeo3,
    Pico4,
    Pico4Pro,
    Pico4Enterprise,
    Pico4Ultra,
    PicoG3,
    PicoUnknown,
    Focus3,
    FocusVision,
    XRElite,
    ViveUnknown,
    Yvr,
    PlayForDreamMR,
    Lynx,
    HuaweiVrGlass,
    AndroidUnknown,
    AppleHeadset,
    WindowsPc,
    LinuxPc,
    Macos,
    Unknown,
}

impl Platform {
    pub const fn is_quest(&self) -> bool {
        matches!(
            self,
            Platform::Quest1
                | Platform::Quest2
                | Platform::Quest3
                | Platform::Quest3S
                | Platform::QuestPro
                | Platform::QuestUnknown
        )
    }

    pub const fn is_pico(&self) -> bool {
        matches!(
            self,
            Platform::PicoG3
                | Platform::PicoNeo3
                | Platform::Pico4
                | Platform::Pico4Pro
                | Platform::Pico4Enterprise
                | Platform::Pico4Ultra
                | Platform::PicoUnknown
        )
    }

    pub const fn is_vive(&self) -> bool {
        matches!(
            self,
            Platform::Focus3 | Platform::FocusVision | Platform::XRElite | Platform::ViveUnknown
        )
    }

    pub const fn is_yvr(&self) -> bool {
        matches!(self, Platform::Yvr | Platform::PlayForDreamMR)
    }

    pub const fn is_huawei(&self) -> bool {
        matches!(self, Platform::HuaweiVrGlass)
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Platform::Quest1 => "Quest 1",
            Platform::Quest2 => "Quest 2",
            Platform::Quest3 => "Quest 3",
            Platform::Quest3S => "Quest 3S",
            Platform::QuestPro => "Quest Pro",
            Platform::QuestUnknown => "Quest (unknown)",
            Platform::PicoNeo3 => "Pico Neo 3",
            Platform::Pico4 => "Pico 4",
            Platform::Pico4Pro => "Pico 4 Pro",
            Platform::Pico4Enterprise => "Pico 4 Enterprise",
            Platform::Pico4Ultra => "Pico 4 Ultra",
            Platform::PicoG3 => "Pico G3",
            Platform::PicoUnknown => "Pico (unknown)",
            Platform::Focus3 => "VIVE Focus 3",
            Platform::FocusVision => "VIVE Focus Vision",
            Platform::XRElite => "VIVE XR Elite",
            Platform::ViveUnknown => "HTC VIVE (unknown)",
            Platform::Yvr => "YVR",
            Platform::PlayForDreamMR => "Play For Dream MR",
            Platform::Lynx => "Lynx Headset",
            Platform::HuaweiVrGlass => "HUAWEI VR Glass",
            Platform::AndroidUnknown => "Android (unknown)",
            Platform::AppleHeadset => "Apple Headset",
            Platform::WindowsPc => "Windows PC",
            Platform::LinuxPc => "Linux PC",
            Platform::Macos => "macOS",
            Platform::Unknown => "Unknown",
        };
        write!(f, "{name}")
    }
}

pub fn platform() -> Platform {
    #[cfg(target_os = "android")]
    {
        let manufacturer = android::manufacturer_name();
        let model = android::model_name();
        let device = android::device_name();
        let product = android::product_name();

        alvr_common::info!(
            "manufacturer: {manufacturer}, model: {model}, device: {device}, product: {product}"
        );

        // 允许把非华为设备（例如把 VR Glass 直接挂在 HDMI 上的 RK3566 盒子）
        // 强制切到华为分支：
        //   adb shell "echo huawei > /sdcard/Android/data/com.lht.huaweivr.alvr/files/platform"
        // 用文件而不是系统属性，是因为 android.os.Build 的静态字段在 zygote 里
        // 就初始化完了，resetprop 改 ro.product.manufacturer 对已运行进程无效。
        if let Ok(forced) = std::fs::read_to_string(
            "/sdcard/Android/data/com.lht.huaweivr.alvr/files/platform",
        ) {
            let forced = forced.trim().to_ascii_lowercase();
            if matches!(forced.as_str(), "huawei" | "huaweivrglass" | "huawei_vr_glass") {
                alvr_common::info!("platform forced to HUAWEI VR Glass by override file");
                return Platform::HuaweiVrGlass;
            }
        }

        match (
            manufacturer.as_str(),
            model.as_str(),
            device.as_str(),
            product.as_str(),
        ) {
            ("Oculus", _, "monterey", _) => Platform::Quest1,
            ("Oculus", _, "hollywood", _) => Platform::Quest2,
            ("Oculus", _, "eureka", _) => Platform::Quest3,
            ("Oculus", _, "panther", _) => Platform::Quest3S,
            ("Oculus", _, "seacliff", _) => Platform::QuestPro,
            ("Oculus", _, _, _) => Platform::QuestUnknown,
            ("Pico", "Pico Neo 3" | "Pico Neo3 Link", _, _) => Platform::PicoNeo3,
            ("Pico", _, _, "PICO 4 Pro") => Platform::Pico4Pro,
            ("Pico", _, _, "PICO 4 Enterprise") => Platform::Pico4Enterprise,
            ("Pico", _, _, "PICO 4") => Platform::Pico4,
            ("Pico", _, _, "PICO 4 Ultra") => Platform::Pico4Ultra,
            ("Pico", _, _, "PICO G3") => Platform::PicoG3,
            ("Pico", _, _, _) => Platform::PicoUnknown,
            ("HTC", "VIVE Focus 3", _, _) => Platform::Focus3,
            ("HTC", "VIVE Focus Vision", _, _) => Platform::FocusVision,
            ("HTC", "VIVE XR Series", _, _) => Platform::XRElite,
            ("HTC", _, _, _) => Platform::ViveUnknown,
            ("YVR", _, _, _) => Platform::Yvr,
            ("Play For Dream", _, _, _) => Platform::PlayForDreamMR,
            ("Lynx Mixed Reality", _, _, _) => Platform::Lynx,
            (manufacturer, _, _, _)
                if manufacturer.eq_ignore_ascii_case("HUAWEI")
                    || manufacturer.eq_ignore_ascii_case("HONOR") =>
            {
                Platform::HuaweiVrGlass
            }
            _ => Platform::AndroidUnknown,
        }
    }
    #[cfg(target_os = "ios")]
    {
        Platform::AppleHeadset
    }
    #[cfg(windows)]
    {
        Platform::WindowsPc
    }
    #[cfg(target_os = "linux")]
    {
        Platform::LinuxPc
    }
    #[cfg(target_os = "macos")]
    {
        Platform::Macos
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "ios",
        windows,
        target_os = "linux",
        target_os = "macos"
    )))]
    {
        Platform::Unknown
    }
}

#[cfg(not(target_os = "android"))]
pub fn local_ip() -> std::net::IpAddr {
    use std::net::{IpAddr, Ipv4Addr};

    local_ip_address::local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

#[derive(SettingsSchema, Serialize, Deserialize, Clone)]
pub enum ClientFlavor {
    Store,
    Github,
    Custom(String),
}
