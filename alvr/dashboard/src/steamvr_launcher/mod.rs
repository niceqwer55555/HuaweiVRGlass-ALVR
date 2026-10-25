#[cfg(target_os = "linux")]
mod linux_steamvr;
#[cfg(windows)]
mod windows_steamvr;

use crate::data_sources;
use alvr_adb::commands as adb;
use alvr_common::{
    anyhow::{Context, Result},
    debug,
    glam::bool,
    once_cell::sync::Lazy,
    parking_lot::Mutex,
    warn,
};
use alvr_filesystem as afs;
use serde_json::{self, json};
use std::{
    ffi::OsStr,
    fs,
    marker::PhantomData,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{ProcessesToUpdate, System};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const DRIVER_KEY: &str = "driver_alvr_server";
const BLOCKED_KEY: &str = "blocked_by_safe_mode";

// SteamVR path configured by the user in the dashboard. It is kept here so that
// every launch entry point (startup, drawer button, devices tab) uses the same
// value without threading it through the whole UI.
pub static MANUAL_STEAMVR_PATH: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

const STEAMVR_PATH_FILE: &str = "steamvr_path.txt";

/// Sidecar file next to the dashboard executable. SteamVR may be launched
/// before the dashboard UI loads its own storage, so the path is mirrored here.
fn steamvr_path_file() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    Some(dir.join(STEAMVR_PATH_FILE))
}

pub fn set_manual_steamvr_path(path: impl Into<String>) {
    let path = path.into();

    if let Some(file) = steamvr_path_file() {
        let _ = fs::write(file, &path);
    }

    *MANUAL_STEAMVR_PATH.lock() = path;
}

pub fn load_persisted_steamvr_path() {
    if let Some(file) = steamvr_path_file() {
        if let Ok(text) = fs::read_to_string(file) {
            *MANUAL_STEAMVR_PATH.lock() = text.trim().to_owned();
        }
    }
}

pub fn manual_steamvr_path() -> String {
    MANUAL_STEAMVR_PATH.lock().clone()
}

/// Resolves an arbitrary user supplied path (without touching the stored
/// setting). Used by the "Steam library" tab to show a live preview.
#[cfg(windows)]
pub fn resolve_steamvr_path(input: &str) -> Option<PathBuf> {
    windows_steamvr::resolve_steamvr_launch_path(input)
}

#[cfg(not(windows))]
pub fn resolve_steamvr_path(_input: &str) -> Option<PathBuf> {
    None
}

pub fn is_steamvr_running() -> bool {
    System::new_all()
        .processes_by_name(OsStr::new(&afs::exec_fname("vrserver")))
        .count()
        != 0
}

pub fn maybe_kill_steamvr() {
    let mut system = System::new_all();

    #[allow(unused_variables)]
    for process in system.processes_by_name(OsStr::new(&afs::exec_fname("vrmonitor"))) {
        debug!("Killing vrmonitor");

        #[cfg(target_os = "linux")]
        linux_steamvr::terminate_process(process);
        #[cfg(windows)]
        windows_steamvr::kill_process(process.pid().as_u32());

        thread::sleep(Duration::from_secs(1));
    }

    system.refresh_processes(ProcessesToUpdate::All, true);

    #[allow(unused_variables)]
    for process in system.processes_by_name(OsStr::new(&afs::exec_fname("vrserver"))) {
        debug!("Killing vrserver");

        #[cfg(target_os = "linux")]
        linux_steamvr::terminate_process(process);
        #[cfg(windows)]
        windows_steamvr::kill_process(process.pid().as_u32());

        thread::sleep(Duration::from_secs(1));
    }
}

fn unblock_alvr_driver() -> Result<()> {
    if !cfg!(target_os = "linux") {
        return Ok(());
    }

    let path = alvr_server_io::steamvr_settings_file_path()?;
    let text = fs::read_to_string(&path).with_context(|| format!("Failed to read {path:?}"))?;
    let new_text = unblock_alvr_driver_within_vrsettings(text.as_str())
        .with_context(|| "Failed to rewrite .vrsettings.")?;
    fs::write(&path, new_text)
        .with_context(|| "Failed to write .vrsettings back after changing it.")?;
    Ok(())
}

// Reads and writes back steamvr.vrsettings in order to
// ensure the ALVR driver is not blocked (safe mode).
fn unblock_alvr_driver_within_vrsettings(text: &str) -> Result<String> {
    let mut settings = serde_json::from_str::<serde_json::Value>(text)?;
    let values = settings
        .as_object_mut()
        .with_context(|| "Failed to parse .vrsettings.")?;
    let blocked = values
        .get(DRIVER_KEY)
        .and_then(|driver| driver.get(BLOCKED_KEY))
        .and_then(|blocked| blocked.as_bool())
        .unwrap_or(false);

    if blocked {
        debug!("Unblocking ALVR driver in SteamVR.");
        if !values.contains_key(DRIVER_KEY) {
            values.insert(DRIVER_KEY.into(), json!({}));
        }
        let driver = settings[DRIVER_KEY]
            .as_object_mut()
            .with_context(|| "Did not find ALVR key in settings.")?;
        driver.insert(BLOCKED_KEY.into(), json!(false)); // overwrites if present
    } else {
        debug!("ALVR is not blocked in SteamVR.");
    }

    Ok(serde_json::to_string_pretty(&settings)?)
}

/// Huawei VR Glass: the glasses have a single USB-C port, so ALVR and the official
/// wired driver can never work at the same time - whichever one SteamVR activates
/// decides whether you get a picture at all.
///
/// Policy: default to the official wired driver. Only fall back to ALVR when a
/// phone is actually attached over USB, because that is the only situation where
/// the glasses are on the phone instead of on the PC.
///
/// SteamVR reads the enable flags while starting up, so this must run before it
/// is launched.
fn apply_auto_driver_selection() {
    let adb_path = adb::get_adb_path(&crate::get_filesystem_layout());
    let usb_serials = adb_path
        .as_deref()
        .and_then(|path| adb::list_usb_device_serials(path).ok())
        .unwrap_or_default();
    let usb_phone = !usb_serials.is_empty();

    match alvr_server_io::apply_auto_driver_selection(usb_phone) {
        Ok(Some(winner)) => {
            debug!("Auto driver selection: using {winner} (phone on USB: {usb_phone})")
        }
        Ok(None) => debug!("Auto driver selection: skipped, no Huawei driver registered"),
        Err(err) => warn!("Auto driver selection failed: {err:?}"),
    }

    if usb_phone {
        if let Some(path) = adb_path.as_deref() {
            launch_huawei_vr_client(path, &usb_serials);
        }
    }
}

/// The wired client cannot be started by ALVR's own autolaunch: it uses
/// `monkey`, which opens the app without the VR action and the phone never
/// enters VR mode. Start it ourselves with the right intent, otherwise the
/// wired connection keeps reporting "No suitable ALVR client is installed".
fn launch_huawei_vr_client(adb_path: &str, serials: &[String]) {
    // Must match connection.wired_client_type in session.json.
    const CLIENT_PACKAGE: &str = "com.lht.huaweivr.alvr";
    const CLIENT_ACTIVITY: &str = ".HuaweiVrActivity";
    const VR_MAIN_ACTION: &str = "com.huawei.android.vr.action.MAIN";

    for serial in serials {
        match adb::start_application_with_action(
            adb_path,
            serial,
            CLIENT_PACKAGE,
            CLIENT_ACTIVITY,
            VR_MAIN_ACTION,
        ) {
            Ok(()) => debug!("Launched Huawei VR client on {serial}"),
            Err(err) => warn!("Failed to launch Huawei VR client on {serial}: {err:?}"),
        }
    }
}

pub struct Launcher {
    _phantom: PhantomData<()>,
}

impl Launcher {
    pub fn launch_steamvr(&self) {
        // The ADB server might be left running because of a unclean termination of SteamVR
        // Note that this will also kill a system wide ADB server not started by ALVR
        let wired_enabled = data_sources::get_read_only_local_session()
            .session()
            .client_connections
            .contains_key(alvr_sockets::WIRED_CLIENT_HOSTNAME);
        if wired_enabled {
            if let Some(path) = adb::get_adb_path(&crate::get_filesystem_layout()) {
                adb::kill_server(&path).ok();
            }
        }

        #[cfg(target_os = "linux")]
        linux_steamvr::linux_hardware_checks();

        let alvr_driver_dir = crate::get_filesystem_layout().openvr_driver_root_dir;

        // Make sure to unregister any other ALVR driver because it would cause a socket conflict
        let other_alvr_dirs = alvr_server_io::get_registered_drivers()
            .unwrap_or_default()
            .into_iter()
            .filter(|path| {
                path.to_string_lossy().to_lowercase().contains("alvr") && *path != alvr_driver_dir
            })
            .collect::<Vec<_>>();
        alvr_server_io::driver_registration(&other_alvr_dirs, false).ok();

        alvr_server_io::driver_registration(&[alvr_driver_dir], true).ok();

        if let Err(err) = unblock_alvr_driver() {
            warn!("Failed to unblock ALVR driver: {:?}", err);
        }

        apply_auto_driver_selection();

        #[cfg(target_os = "linux")]
        {
            let vrcompositor_wrap_result = linux_steamvr::maybe_wrap_vrcompositor_launcher();
            alvr_common::show_err(linux_steamvr::maybe_wrap_vrcompositor_launcher());
            if vrcompositor_wrap_result.is_err() {
                return;
            }
        }

        if !is_steamvr_running() {
            debug!("SteamVR is dead. Launching...");

            #[cfg(windows)]
            windows_steamvr::start_steamvr(&manual_steamvr_path());

            #[cfg(target_os = "linux")]
            linux_steamvr::start_steamvr();
        }
    }

    pub fn ensure_steamvr_shutdown(&self) {
        debug!("Waiting for SteamVR to shutdown...");
        let start_time = Instant::now();
        while start_time.elapsed() < SHUTDOWN_TIMEOUT && is_steamvr_running() {
            thread::sleep(Duration::from_millis(500));
        }

        maybe_kill_steamvr();
    }

    pub fn restart_steamvr(&self) {
        self.ensure_steamvr_shutdown();
        self.launch_steamvr();
    }
}

// Singleton with exclusive access
pub static LAUNCHER: Lazy<Mutex<Launcher>> = Lazy::new(|| {
    Mutex::new(Launcher {
        _phantom: PhantomData,
    })
});
