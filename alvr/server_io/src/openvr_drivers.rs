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
/// Sidecar (next to the dashboard exe) remembering all discovered HMD driver
/// paths so switching the load list never loses the inactive driver's path.
const HMD_DRIVERS_FILE: &str = "hmd_drivers.json";

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

/// Whether the Huawei PC VR assistant is running.
///
/// The official wired driver needs the assistant to own the glasses' USB/DP
/// link: without it the driver reports `pNumDisplay = 0` and fails to load with
/// `VRInitError_Init_HmdNotFound`, leaving SteamVR with no HMD at all. Auto
/// driver selection must therefore NOT switch to huaweiVR on EDID evidence
/// alone - it also needs the assistant to be alive (or the user to have forced
/// `usb` explicitly via vr_mode.txt).
#[cfg(windows)]
pub fn is_huawei_assistant_running() -> bool {
    use std::process::Command;

    // sysinfo is a dashboard dependency, not server_io's. tasklist is always
    // available on Windows and keeps this crate dependency-free. Compare raw
    // bytes: tasklist output uses the OEM code page on zh-CN systems.
    let Ok(output) = Command::new("tasklist")
        .args(["/NH"])
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    ["hwpcvrassistant.exe", "pccastscreen.exe", "hwvrglasslite.exe"]
        .iter()
        .any(|name| stdout.contains(name))
}

#[cfg(not(windows))]
pub fn is_huawei_assistant_running() -> bool {
    false
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
/// Explicit driver choice - bypasses all auto-detection. Used when the user
/// forced a mode via `vr_mode.txt` and therefore knows what they want.
pub fn select_driver(use_huawei: bool) -> Result<()> {
    let (winner, loser) = if use_huawei {
        (HUAWEI_DRIVER_NAME, ALVR_DRIVER_NAME)
    } else {
        (ALVR_DRIVER_NAME, HUAWEI_DRIVER_NAME)
    };
    set_driver_enabled(winner, true)?;
    set_driver_enabled(loser, false)?;
    Ok(())
}

pub fn apply_auto_driver_selection(glasses_on_pc: bool) -> Result<Option<&'static str>> {
    if !is_huawei_driver_registered()? {
        return Ok(None);
    }

    // Physical topology decides: glasses on the PC (visible as a monitor /
    // USB device) -> official wired driver; glasses on the phone -> ALVR.
    // No dependency on the Huawei assistant process: if the official driver
    // needs it, the driver itself launches it via its named mutex. We only
    // own the enable flags here.
    let winner = if glasses_on_pc {
        HUAWEI_DRIVER_NAME
    } else {
        ALVR_DRIVER_NAME
    };
    select_driver(glasses_on_pc)?;

    Ok(Some(winner))
}

/// Register the bundled official Huawei driver (if present) as THE huaweiVR
/// driver, removing any other registered copy. SteamVR keys the enable flag on
/// the driver NAME, so several copies of `huaweiVR` all switch together and one
/// of them may be a stale/shim path. Keeping only our bundled copy makes the
/// USB/ALVR auto-switch deterministic and removes the need for an external
/// Huawei PC VR assistant install.
pub fn register_bundled_huawei_driver(dir: &std::path::Path) -> Result<()> {
    if is_temp_build_path(dir) {
        // Launched from a temp package dir: never persist that path. The stable
        // artifacts registration is handled by apply_driver_registration_for_mode.
        return Ok(());
    }
    if !dir.join("driver.vrdrivermanifest").is_file()
        || !dir.join("bin").join("win64").join("driver_huaweiVR.dll").is_file()
    {
        return Ok(()); // not bundled in this build; leave existing registration alone
    }

    let wanted = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let duplicates: Vec<PathBuf> = get_registered_drivers()?
        .into_iter()
        .filter(|p| {
            let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            resolved != wanted && driver_name_of(p).as_deref() == Some(HUAWEI_DRIVER_NAME)
        })
        .collect();
    if !duplicates.is_empty() {
        driver_registration(&duplicates, false)?;
    }
    driver_registration(&[wanted], true)
}

/// Which HMD driver should be present in SteamVR's external driver load list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HmdDriver {
    Alvr,
    Huawei,
}

/// True for paths inside a packaging output directory (`build/end/server*`).
/// The dashboard may be launched from a freshly built package for testing; the
/// driver paths derived from such an exe are temporary and must never be
/// persisted into the OpenVR load list - doing so breaks the registration as
/// soon as the package folder is replaced or deleted.
fn is_temp_build_path(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy().replace('/', "\\").to_lowercase();
    text.contains("\\build\\end\\")
}

/// Make SteamVR's load list (openvrpaths.vrpath `external_drivers`) contain only
/// the HMD driver matching the current physical connection. Third-party drivers
/// (driver4VR, etc.) are left untouched. Both HMD driver paths are remembered
/// in a sidecar file so the inactive one can be restored on the next switch,
/// even if it is currently absent from the load list.
///
/// `alvr_dir` and `huawei_dir` are the canonical paths we own; they are always
/// recorded so switching back works. SteamVR reads the load list at startup,
/// so callers should invoke this before launching it.
pub fn apply_driver_registration_for_mode(
    keep: HmdDriver,
    alvr_dir: &std::path::Path,
    huawei_dir: &std::path::Path,
    storage_dir: &std::path::Path,
) -> Result<()> {
    let keep_dir = match keep {
        HmdDriver::Alvr => alvr_dir,
        HmdDriver::Huawei => huawei_dir,
    };

    // Remember every HMD driver path ever seen, so switching the load list is
    // reversible even after the inactive driver was removed from it.
    let sidecar = storage_dir.join(HMD_DRIVERS_FILE);
    let mut known_alvr: Vec<PathBuf> = vec![];
    let mut known_huawei: Vec<PathBuf> = vec![];
    if let Ok(text) = fs::read_to_string(&sidecar) {
        if let Ok(v) = json::from_str::<json::Value>(&text) {
            for (key, out) in [("alvr", &mut known_alvr), ("huawei", &mut known_huawei)] {
                if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            let p = PathBuf::from(s);
                            if !is_temp_build_path(&p) {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut registered = get_registered_drivers()?;
    for p in &registered {
        if is_temp_build_path(p) { continue; }
        match driver_name_of(p).as_deref() {
            Some(ALVR_DRIVER_NAME) if !known_alvr.contains(p) => known_alvr.push(p.clone()),
            Some(HUAWEI_DRIVER_NAME) if !known_huawei.contains(p) => known_huawei.push(p.clone()),
            _ => {}
        }
    }
    for p in [alvr_dir, huawei_dir] {
        if is_temp_build_path(p) { continue; }
        let list = if p == alvr_dir { &mut known_alvr } else { &mut known_huawei };
        if !list.contains(&p.to_path_buf()) { list.push(p.to_path_buf()); }
    }
    let _ = fs::write(
        &sidecar,
        json::to_string_pretty(&json::json!({
            "alvr": known_alvr,
            "huawei": known_huawei,
        }))?,
    );

    // Drop every known inactive HMD path from the load list; keep third-party
    // drivers and all of the active driver's copies.
    let drop_paths = match keep {
        HmdDriver::Alvr => &known_huawei,
        HmdDriver::Huawei => &known_alvr,
    };
    registered.retain(|p| {
        if is_temp_build_path(p) { return false; }   // never persist temp package paths
        let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        !drop_paths.iter().any(|d| {
            let rd = std::fs::canonicalize(d).unwrap_or_else(|_| d.clone());
            rd == resolved
        })
    });
    // If the active path itself is a temp package path, do not register it. The
    // stable artifacts registration (if any) is already in `registered`.
    if !is_temp_build_path(keep_dir) {
        let keep_resolved = std::fs::canonicalize(keep_dir).unwrap_or_else(|_| keep_dir.to_path_buf());
        if !registered.iter().any(|p| {
            std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) == keep_resolved
        }) {
            registered.push(keep_dir.to_path_buf());
        }
    }

    let mut json = openvrpaths::load_openvr_paths_json()?;
    let list_ref = json.get_mut("external_drivers").to_any()?;
    *list_ref = openvrpaths::to_openvr_paths(&registered);
    openvrpaths::save_openvr_paths_json(&json)
}

pub fn get_driver_dir_from_registered() -> Result<PathBuf> {
    for dir in get_registered_drivers()? {
        if driver_name_of(&dir).as_deref() == Some(ALVR_DRIVER_NAME) {
            return Ok(dir);
        }
    }

    bail!("ALVR driver path not registered")
}
