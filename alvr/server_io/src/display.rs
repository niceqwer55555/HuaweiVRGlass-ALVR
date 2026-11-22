//! Windows display control used by the wired (glasses plugged straight into the
//! PC) mode.
//!
//! The Huawei VR Glass is a DP sink: once it is plugged in it is just another
//! monitor to Windows. That means "casting" to it does not need SteamVR, nor the
//! Huawei assistant, nor the USB data link - only the Win32 display API.
//!
//! Everything here is plain FFI. The project has no windows-sys dependency and
//! adding one breaks offline builds, so the few structs we need are declared
//! locally; user32 is linked by default on the msvc target.

use alvr_common::anyhow::{bail, Result};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

// ---------------------------------------------------------------- constants

const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;

const CDS_UPDATEREGISTRY: u32 = 0x0000_0001;
const CDS_NORESET: u32 = 0x1000_0000;

const DISP_CHANGE_SUCCESSFUL: i32 = 0;

const DM_POSITION: u32 = 0x0000_0020;
const DM_BITSPERPEL: u32 = 0x0004_0000;
const DM_PELSWIDTH: u32 = 0x0008_0000;
const DM_PELSHEIGHT: u32 = 0x0010_0000;
const DM_DISPLAYORIENTATION: u32 = 0x0000_0080;
const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;

const DISPLAY_DEVICE_ATTACHED_TO_DESKTOP: u32 = 0x0000_0002;
const DISPLAY_DEVICE_PRIMARY_DEVICE: u32 = 0x0000_0004;

const ORIENTATION: [u32; 4] = [0, 1, 2, 3]; // 0 / 90 / 180 / 270 degrees

// ---------------------------------------------------------------- raw types

#[repr(C)]
struct DisplayDeviceW {
    cb: u32,
    device_name: [u16; 32],
    device_string: [u16; 128],
    state_flags: u32,
    device_id: [u16; 128],
    device_key: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DevmodeW {
    device_name: [u16; 32],
    spec_version: u16,
    driver_version: u16,
    size: u16,
    driver_extra: u16,
    fields: u32,
    position_x: i32,
    position_y: i32,
    display_orientation: u32,
    display_fixed_output: u32,
    color: i16,
    duplex: i16,
    y_resolution: i16,
    tt_option: i16,
    collate: i16,
    form_name: [u16; 32],
    log_pixels: u16,
    bits_per_pel: u32,
    pels_width: u32,
    pels_height: u32,
    display_flags: u32,
    display_frequency: u32,
    icm_method: u32,
    icm_intent: u32,
    media_type: u32,
    dither_type: u32,
    reserved1: u32,
    reserved2: u32,
    panning_width: u32,
    panning_height: u32,
}

extern "system" {
    fn EnumDisplayDevicesW(
        device: *const u16,
        dev_num: u32,
        display_device: *mut DisplayDeviceW,
        flags: u32,
    ) -> i32;
    fn EnumDisplaySettingsExW(
        device_name: *const u16,
        mode_num: u32,
        dev_mode: *mut DevmodeW,
        flags: u32,
    ) -> i32;
    fn ChangeDisplaySettingsExW(
        device_name: *const u16,
        dev_mode: *const DevmodeW,
        hwnd: *const std::ffi::c_void,
        flags: u32,
        param: *const std::ffi::c_void,
    ) -> i32;
}

// ---------------------------------------------------------------- public api

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub bits_per_pel: u32,
}

impl DisplayMode {
    pub fn label(&self) -> String {
        format!("{} x {} @ {} Hz", self.width, self.height, self.refresh_rate)
    }
}

#[derive(Clone, Debug)]
pub struct DisplayInfo {
    /// Win32 device name, e.g. `\\.\DISPLAY1`
    pub device_name: String,
    /// Human readable name, e.g. "Generic PnP Monitor"
    pub friendly_name: String,
    /// Monitor hardware id, e.g. `MONITOR\RTK2560\{...}` - how we find the glasses
    pub monitor_id: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
    pub attached: bool,
    pub orientation: u32,
}

impl DisplayInfo {
    pub fn is_glasses(&self) -> bool {
        is_glasses_monitor_id(&self.monitor_id)
    }

    pub fn summary(&self) -> String {
        format!(
            "{} x {} @ {} Hz  at ({}, {}){}",
            self.width,
            self.height,
            self.refresh_rate,
            self.x,
            self.y,
            if self.primary { "  [primary]" } else { "" }
        )
    }
}

/// How the desktop should be laid out over the monitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastLayout {
    /// Glasses show exactly what the primary monitor shows.
    Mirror,
    /// Glasses sit to the right of the primary monitor (a normal second screen).
    ExtendRight,
    /// Glasses sit at (0,0), primary monitor moves to their right.
    GlassesLeft,
    /// Every monitor in a row, ordered left to right by their current position.
    Row,
}

/// Keywords identifying the glasses among the monitors.
const DEFAULT_GLASSES_EDID_KEYWORDS: &[&str] = &["RTK2560", "HUAWEI", "VRGLASS", "VR GLASS"];

pub fn is_glasses_monitor_id(monitor_id: &str) -> bool {
    let upper = monitor_id.to_uppercase();
    DEFAULT_GLASSES_EDID_KEYWORDS
        .iter()
        .any(|kw| upper.contains(kw))
}

// ---------------------------------------------------------------- internals

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn from_wstring(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn new_devmode() -> DevmodeW {
    DevmodeW {
        device_name: [0; 32],
        spec_version: 0,
        driver_version: 0,
        size: std::mem::size_of::<DevmodeW>() as u16,
        driver_extra: 0,
        fields: 0,
        position_x: 0,
        position_y: 0,
        display_orientation: 0,
        display_fixed_output: 0,
        color: 0,
        duplex: 0,
        y_resolution: 0,
        tt_option: 0,
        collate: 0,
        form_name: [0; 32],
        log_pixels: 0,
        bits_per_pel: 0,
        pels_width: 0,
        pels_height: 0,
        display_flags: 0,
        display_frequency: 0,
        icm_method: 0,
        icm_intent: 0,
        media_type: 0,
        dither_type: 0,
        reserved1: 0,
        reserved2: 0,
        panning_width: 0,
        panning_height: 0,
    }
}

fn check(result: i32, what: &str) -> Result<()> {
    match result {
        DISP_CHANGE_SUCCESSFUL => Ok(()),
        other => bail!("{what} failed: ChangeDisplaySettings error {other}"),
    }
}

// ---------------------------------------------------------------- queries

/// Every adapter that is currently attached to the desktop, with its live mode.
pub fn list_displays() -> Vec<DisplayInfo> {
    let mut out = Vec::new();
    let mut device = DisplayDeviceW {
        cb: std::mem::size_of::<DisplayDeviceW>() as u32,
        device_name: [0; 32],
        device_string: [0; 128],
        state_flags: 0,
        device_id: [0; 128],
        device_key: [0; 128],
    };

    for adapter_index in 0..16 {
        device.cb = std::mem::size_of::<DisplayDeviceW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), adapter_index, &mut device, 0) } == 0 {
            break;
        }
        let name = from_wstring(&device.device_name);
        if name.is_empty() {
            continue;
        }
        // The monitor (EDID) hangs off the adapter; the first one is the display.
        let mut monitor = DisplayDeviceW {
            cb: std::mem::size_of::<DisplayDeviceW>() as u32,
            device_name: [0; 32],
            device_string: [0; 128],
            state_flags: 0,
            device_id: [0; 128],
            device_key: [0; 128],
        };
        let mut monitor_id = String::new();
        let mut friendly = from_wstring(&device.device_string);
        if unsafe {
            EnumDisplayDevicesW(device.device_name.as_ptr(), 0, &mut monitor, 0)
        } != 0
        {
            monitor_id = from_wstring(&monitor.device_id);
            let monitor_name = from_wstring(&monitor.device_string);
            if !monitor_name.is_empty() {
                friendly = monitor_name;
            }
        }

        let mut mode = new_devmode();
        let (width, height, refresh, x, y, orientation) = if unsafe {
            EnumDisplaySettingsExW(
                device.device_name.as_ptr(),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
                0,
            )
        } != 0
        {
            (
                mode.pels_width,
                mode.pels_height,
                mode.display_frequency,
                mode.position_x,
                mode.position_y,
                mode.display_orientation,
            )
        } else {
            (0, 0, 0, 0, 0, 0)
        };

        out.push(DisplayInfo {
            device_name: name,
            friendly_name: friendly,
            monitor_id,
            width,
            height,
            refresh_rate: refresh,
            x,
            y,
            primary: device.state_flags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
            attached: device.state_flags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP != 0,
            orientation,
        });
    }

    out
}

/// Modes a display reports. Sorted biggest-first, then by refresh rate.
pub fn list_modes(device_name: &str) -> Vec<DisplayMode> {
    let wide = to_wstring(device_name);
    let mut out: Vec<DisplayMode> = Vec::new();

    for index in 0..512u32 {
        let mut mode = new_devmode();
        if unsafe { EnumDisplaySettingsExW(wide.as_ptr(), index, &mut mode, 0) } == 0 {
            break;
        }
        let candidate = DisplayMode {
            width: mode.pels_width,
            height: mode.pels_height,
            refresh_rate: mode.display_frequency,
            bits_per_pel: mode.bits_per_pel,
        };
        if candidate.width == 0 || candidate.height == 0 {
            continue;
        }
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }

    out.sort_by(|a, b| {
        (b.width * b.height)
            .cmp(&(a.width * a.height))
            .then(b.refresh_rate.cmp(&a.refresh_rate))
    });
    out
}

/// The display whose monitor id matches the glasses, if one is plugged in.
pub fn find_glasses() -> Option<DisplayInfo> {
    list_displays().into_iter().find(|d| d.is_glasses())
}

// ---------------------------------------------------------------- mutations

/// Set resolution / refresh rate on one display.
pub fn apply_mode(device_name: &str, mode: &DisplayMode) -> Result<()> {
    let wide = to_wstring(device_name);
    let mut dm = new_devmode();
    dm.fields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    dm.pels_width = mode.width;
    dm.pels_height = mode.height;
    dm.display_frequency = mode.refresh_rate;
    if mode.bits_per_pel > 0 {
        dm.fields |= DM_BITSPERPEL;
        dm.bits_per_pel = mode.bits_per_pel;
    }

    let result = unsafe {
        ChangeDisplaySettingsExW(
            wide.as_ptr(),
            &dm,
            std::ptr::null(),
            CDS_UPDATEREGISTRY | CDS_NORESET,
            std::ptr::null(),
        )
    };
    check(result, "apply display mode")
}

/// Move a display to a desktop position (this is what turns it into a second
/// screen, or into a mirror of the primary one).
pub fn apply_position(device_name: &str, x: i32, y: i32) -> Result<()> {
    let wide = to_wstring(device_name);
    let mut dm = new_devmode();
    dm.fields = DM_POSITION;
    dm.position_x = x;
    dm.position_y = y;

    let result = unsafe {
        ChangeDisplaySettingsExW(
            wide.as_ptr(),
            &dm,
            std::ptr::null(),
            CDS_UPDATEREGISTRY | CDS_NORESET,
            std::ptr::null(),
        )
    };
    check(result, "set display position")
}

/// Rotate a display. `degrees` must be one of 0 / 90 / 180 / 270.
pub fn apply_orientation(device_name: &str, degrees: u32) -> Result<()> {
    let index = match degrees {
        0 => 0,
        90 => 1,
        180 => 2,
        270 => 3,
        other => bail!("unsupported rotation: {other}"),
    };
    let wide = to_wstring(device_name);
    let mut dm = new_devmode();
    dm.fields = DM_DISPLAYORIENTATION;
    dm.display_orientation = ORIENTATION[index];

    let result = unsafe {
        ChangeDisplaySettingsExW(
            wide.as_ptr(),
            &dm,
            std::ptr::null(),
            CDS_UPDATEREGISTRY | CDS_NORESET,
            std::ptr::null(),
        )
    };
    check(result, "set display orientation")
}

/// Lay the desktop out over the available monitors.
///
/// `Mirror` puts the glasses on top of the primary monitor, so whatever is on
/// the desktop shows up in the glasses - that is the "cast my screen" case. The
/// others are ordinary multi-monitor layouts.
pub fn apply_cast(layout: CastLayout) -> Result<()> {
    let displays = list_displays();
    let attached: Vec<&DisplayInfo> = displays.iter().filter(|d| d.attached).collect();
    if attached.is_empty() {
        bail!("no monitor is attached to the desktop");
    }

    let primary = attached
        .iter()
        .find(|d| d.primary)
        .copied()
        .unwrap_or(attached[0]);
    let glasses = attached.iter().find(|d| d.is_glasses()).copied();

    match layout {
        CastLayout::Mirror => {
            let glasses = glasses.ok_or_else(|| {
                alvr_common::anyhow::anyhow!(
                    "the glasses are not attached - plug them in and try again"
                )
            })?;
            // Same origin as the primary, and the same resolution so the whole
            // desktop actually fits.
            let target = list_modes(&glasses.device_name)
                .into_iter()
                .find(|m| m.width == primary.width && m.height == primary.height)
                .or_else(|| list_modes(&glasses.device_name).into_iter().next())
                .ok_or_else(|| alvr_common::anyhow::anyhow!("no usable mode for the glasses"))?;
            apply_mode(&glasses.device_name, &target)?;
            apply_position(&glasses.device_name, primary.x, primary.y)
        }
        CastLayout::ExtendRight => {
            let glasses = glasses.ok_or_else(|| {
                alvr_common::anyhow::anyhow!(
                    "the glasses are not attached - plug them in and try again"
                )
            })?;
            apply_position(&glasses.device_name, primary.x + primary.width as i32, primary.y)
        }
        CastLayout::GlassesLeft => {
            let glasses = glasses.ok_or_else(|| {
                alvr_common::anyhow::anyhow!(
                    "the glasses are not attached - plug them in and try again"
                )
            })?;
            apply_position(&primary.device_name, glasses.width as i32, 0)?;
            apply_position(&glasses.device_name, 0, 0)
        }
        CastLayout::Row => {
            let mut ordered: Vec<&DisplayInfo> = attached.clone();
            ordered.sort_by_key(|d| d.x);
            let mut cursor = 0;
            for d in ordered {
                apply_position(&d.device_name, cursor, 0)?;
                cursor += d.width as i32;
            }
            Ok(())
        }
    }
}
