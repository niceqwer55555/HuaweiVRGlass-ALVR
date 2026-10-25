use crate::openvrpaths;
use alvr_common::{
    anyhow::{bail, Result},
    ToAny,
};
use serde_json as json;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

/// Name declared in ALVR's own driver.vrdrivermanifest
pub const ALVR_DRIVER_NAME: &str = "alvr_server";
/// Name declared in the Huawei VR Glass official (wired) driver manifest
pub const HUAWEI_DRIVER_NAME: &str = "huaweiVR";

pub fn get_registered_drivers() -> Result<Vec<PathBuf>> {
    Ok(openvrpaths::from_openvr_paths(
        openvrpaths::load_openvr_paths_json()?
            .get_mut("external_drivers")
            .to_any()?,
    ))
}

pub fn driver_registration(driver_paths: &[PathBuf], register: bool) -> Result<()> {
    let mut openvr_paths_json = openvrpaths::load_openvr_paths_json()?;
    let paths_json_ref = openvr_paths_json.get_mut("external_drivers").to_any()?;

    let mut paths: HashSet<_> = openvrpaths::from_openvr_paths(paths_json_ref)
        .into_iter()
        .collect();

    if register {
        paths.extend(driver_paths.iter().cloned());
    } else {
        for path in driver_paths {
            paths.remove(path);
        }
    }

    // write into openvr_paths_json, the other fields are preserved
    *paths_json_ref =
        openvrpaths::to_openvr_paths(paths.into_iter().collect::<Vec<_>>().as_slice());

    openvrpaths::save_openvr_paths_json(&openvr_paths_json)
}

/// Read the driver name out of `<dir>/driver.vrdrivermanifest`, if it has one.
fn driver_name_of(dir: &PathBuf) -> Option<String> {
    let manifest_string = fs::read_to_string(dir.join("driver.vrdrivermanifest")).ok()?;
    let mut manifest_map = json::from_str::<HashMap<String, json::Value>>(&manifest_string).ok()?;

    manifest_map
        .remove("name")
        .and_then(|name| name.as_str().map(str::to_owned))
}

fn load_steamvr_settings() -> Result<json::Value> {
    let path = openvrpaths::steamvr_settings_file_path()?;
    let text = fs::read_to_string(&path)?;

    Ok(json::from_str(&text)?)
}

fn save_steamvr_settings(settings: &json::Value) -> Result<()> {
    let path = openvrpaths::steamvr_settings_file_path()?;

    fs::write(&path, json::to_string_pretty(settings)?)?;

    Ok(())
}

/// SteamVR keeps a per-driver kill switch in steamvr.vrsettings:
/// `"driver_<name>" : { "enable" : true }`.
/// Returns `None` when the flag is absent (SteamVR then treats it as enabled).
pub fn get_driver_enabled(driver_name: &str) -> Result<Option<bool>> {
    let settings = load_steamvr_settings()?;

    Ok(settings
        .get(format!("driver_{driver_name}"))
        .and_then(|driver| driver.get("enable"))
        .and_then(json::Value::as_bool))
}

pub fn set_driver_enabled(driver_name: &str, enabled: bool) -> Result<()> {
    let mut settings = load_steamvr_settings()?;
    let values = settings.as_object_mut().to_any()?;

    let mut driver = json::Map::new();
    driver.insert("enable".into(), json::Value::Bool(enabled));
    values.insert(
        format!("driver_{driver_name}"),
        json::Value::Object(driver),
    );

    save_steamvr_settings(&settings)
}

/// Is Huawei's official wired driver registered with SteamVR?
pub fn is_huawei_driver_registered() -> Result<bool> {
    Ok(get_registered_drivers()?
        .iter()
        .any(|dir| driver_name_of(dir).as_deref() == Some(HUAWEI_DRIVER_NAME)))
}

/// Device IDs of every monitor attached to this machine, e.g. `DISPLAY\RTK2560\5&...`.
///
/// The HUAWEI VR Glass is a split design: the headset is only a DisplayPort panel
/// with no logic of its own, so it never appears as an HMD/HID device. It appears
/// as a monitor. That makes the monitor list the only reliable way to tell whether
/// the glasses are plugged into THIS PC.
///
/// Implemented with raw FFI on purpose - this crate has no windows API dependency
/// and user32 is already linked by default on the msvc targets.
#[cfg(windows)]
pub fn monitor_device_ids() -> Vec<String> {
    use std::{
        ffi::OsString,
        mem::size_of,
        os::windows::ffi::OsStringExt,
        ptr,
    };

    #[repr(C)]
    #[allow(non_snake_case)]
    struct DisplayDeviceW {
        cb: u32,
        DeviceName: [u16; 32],
        DeviceString: [u16; 128],
        StateFlags: u32,
        DeviceID: [u16; 128],
        DeviceKey: [u16; 128],
    }

    extern "system" {
        fn EnumDisplayDevicesW(
            lpDevice: *const u16,
            iDevNum: u32,
            lpDisplayDevice: *mut DisplayDeviceW,
            dwFlags: u32,
        ) -> i32;
    }

    fn wide_to_string(slice: &[u16]) -> String {
        let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
        OsString::from_wide(&slice[..end])
            .to_string_lossy()
            .into_owned()
    }

    fn blank() -> DisplayDeviceW {
        DisplayDeviceW {
            cb: size_of::<DisplayDeviceW>() as u32,
            DeviceName: [0; 32],
            DeviceString: [0; 128],
            StateFlags: 0,
            DeviceID: [0; 128],
            DeviceKey: [0; 128],
        }
    }

    let mut out = Vec::new();

    unsafe {
        // First level: display adapters (\\.\DISPLAY1, ...). Passing null
        // enumerates adapters; passing an adapter name enumerates its monitors.
        for adapter_index in 0..16u32 {
            let mut adapter = blank();
            if EnumDisplayDevicesW(ptr::null(), adapter_index, &mut adapter, 0) == 0 {
                break;
            }

            for monitor_index in 0..16u32 {
                let mut monitor = blank();
                if EnumDisplayDevicesW(adapter.DeviceName.as_ptr(), monitor_index, &mut monitor, 0)
                    == 0
                {
                    break;
                }

                let id = wide_to_string(&monitor.DeviceID);
                let name = wide_to_string(&monitor.DeviceString);
                if !id.is_empty() {
                    out.push(id);
                }
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
    }

    out
}

#[cfg(not(windows))]
pub fn monitor_device_ids() -> Vec<String> {
    Vec::new()
}

/// True when any attached monitor matches one of the EDID keywords.
///
/// Keywords are matched case-insensitively against both the monitor device id
/// (`DISPLAY\RTK2560\...`) and its display name.
pub fn is_glasses_attached_to_pc(keywords: &[String]) -> bool {
    let ids = monitor_device_ids();

    ids.iter().any(|id| {
        let upper = id.to_uppercase();
        keywords
            .iter()
            .any(|k| !k.is_empty() && upper.contains(&k.to_uppercase()))
    })
}

/// Choose which HMD driver SteamVR activates, from where the glasses actually are.
///
/// The Huawei VR Glass has a single USB-C port, and so does the phone. That makes
/// ALVR's "wired" mode (adb forward over a PC<->phone USB cable) physically
/// impossible for this headset: once the glasses occupy the phone's only port,
/// the phone can only reach the PC over WiFi. So the two real cases are:
/// * glasses plugged into the PC  -> official wired driver must win
/// * glasses plugged into the phone -> ALVR (over WiFi) must win
///
/// The deciding signal is therefore the glasses themselves, not the phone: when
/// they are on the PC they show up as a monitor (see `monitor_device_ids`).
///
/// SteamVR activates a single HMD, and both drivers declare `hmd_presence: ["*.*"]`,
/// so the loser has to be disabled explicitly rather than left to load order.
///
/// Returns `None` if Huawei's driver is not installed, in which case nothing is
/// touched (otherwise this would disable ALVR on machines that have no alternative).
pub fn apply_auto_driver_selection(glasses_on_pc: bool) -> Result<Option<&'static str>> {
    if !is_huawei_driver_registered()? {
        return Ok(None);
    }

    let (winner, loser) = if glasses_on_pc {
        (HUAWEI_DRIVER_NAME, ALVR_DRIVER_NAME)
    } else {
        (ALVR_DRIVER_NAME, HUAWEI_DRIVER_NAME)
    };

    set_driver_enabled(winner, true)?;
    set_driver_enabled(loser, false)?;

    Ok(Some(winner))
}

pub fn get_driver_dir_from_registered() -> Result<PathBuf> {
    for dir in get_registered_drivers()? {
        if driver_name_of(&dir).as_deref() == Some(ALVR_DRIVER_NAME) {
            return Ok(dir);
        }
    }

    bail!("ALVR driver path not registered")
}
