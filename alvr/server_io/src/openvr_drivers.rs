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

/// Choose which HMD driver SteamVR activates, from what is actually plugged in.
///
/// The Huawei VR Glass has a single USB-C port, so only one of the two drivers
/// can ever work:
/// * phone wired to the PC -> the glasses are on the phone -> ALVR must win
/// * no phone              -> the glasses are on the PC    -> official wired wins
///
/// SteamVR activates a single HMD, and both drivers declare `hmd_presence: ["*.*"]`,
/// so the loser has to be disabled explicitly rather than left to load order.
///
/// Returns `None` if Huawei's driver is not installed, in which case nothing is
/// touched (otherwise this would disable ALVR on machines that have no alternative).
pub fn apply_auto_driver_selection(usb_phone_connected: bool) -> Result<Option<&'static str>> {
    if !is_huawei_driver_registered()? {
        return Ok(None);
    }

    let (winner, loser) = if usb_phone_connected {
        (ALVR_DRIVER_NAME, HUAWEI_DRIVER_NAME)
    } else {
        (HUAWEI_DRIVER_NAME, ALVR_DRIVER_NAME)
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
