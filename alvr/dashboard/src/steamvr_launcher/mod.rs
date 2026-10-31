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

/// SteamVR runtime root for the configured path (or the bundled runtime), used
/// to write openvrpaths.vrpath so SteamVR works without the Steam client.
#[cfg(windows)]
pub fn resolve_steamvr_runtime_root(input: &str) -> Option<PathBuf> {
    windows_steamvr::resolve_steamvr_runtime_root(input)
}

#[cfg(not(windows))]
pub fn resolve_steamvr_runtime_root(_input: &str) -> Option<PathBuf> {
    None
}

/// Makes openvrpaths.vrpath exist and point at the configured SteamVR runtime.
/// Without this file, driver registration fails silently and OpenVR games
/// cannot find the runtime on machines without a Steam client installation.
fn ensure_standalone_openvr_paths() {
    #[cfg(windows)]
    {
        let Some(runtime_root) = resolve_steamvr_runtime_root(&manual_steamvr_path()) else {
            warn!("Standalone config skipped: no SteamVR runtime found");
            return;
        };

        match alvr_server_io::ensure_openvr_paths_file(&runtime_root) {
            Ok(path) => debug!(
                "openvrpaths ready: {} (runtime {})",
                path.display(),
                runtime_root.display()
            ),
            Err(err) => warn!("Failed to prepare openvrpaths.vrpath: {err:?}"),
        }
    }
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

/// Sidecar file next to the dashboard executable. One EDID keyword per line (or
/// comma separated). When it is missing the built-in list is used.
const GLASSES_EDID_FILE: &str = "glasses_edid.txt";

/// The glasses are a plain DisplayPort panel, so they are identified by the
/// monitor EDID they report. `RTK2560` is the Realtek bridge inside the HUAWEI
/// VR Glass; the rest are fallbacks for other reports.
const DEFAULT_GLASSES_KEYWORDS: &[&str] = &["RTK2560", "HUAWEI", "VRGLASS", "VR GLASS"];

fn glasses_edid_keywords() -> Vec<String> {
    let mut dir = match std::env::current_exe().ok() {
        Some(dir) => dir,
        None => return DEFAULT_GLASSES_KEYWORDS.iter().map(|s| s.to_string()).collect(),
    };
    dir.pop();

    if let Ok(text) = fs::read_to_string(dir.join(GLASSES_EDID_FILE)) {
        let keywords = text
            .split(|c: char| c == ',' || c == '\n' || c == '\r')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();

        if !keywords.is_empty() {
            return keywords;
        }
    }

    DEFAULT_GLASSES_KEYWORDS.iter().map(|s| s.to_string()).collect()
}

/// Huawei VR Glass: the glasses have a single USB-C port, so ALVR and the official
/// wired driver can never work at the same time - whichever one SteamVR activates
/// decides whether you get a picture at all.
///
/// Policy: ask the hardware, not the phone. The headset is only a DisplayPort
/// panel (the phone is the computer), so when it is plugged into this PC it shows
/// up as a monitor and EDID says so. Found -> official wired wins; not found ->
/// the glasses must be on the phone, so ALVR (over WiFi) wins.
///
/// Note that ALVR's own "wired" mode (adb forward over a PC<->phone USB cable)
/// cannot apply here: the glasses occupy the phone's only USB-C port, so the
/// phone can only reach the PC over WiFi.
///
/// SteamVR reads the enable flags while starting up, so this must run before it
/// is launched.
fn apply_auto_driver_selection() {
    let keywords = glasses_edid_keywords();
    let glasses_on_pc = alvr_server_io::is_glasses_attached_to_pc(&keywords);

    match alvr_server_io::apply_auto_driver_selection(glasses_on_pc) {
        Ok(Some(winner)) => {
            debug!("Auto driver selection: using {winner} (glasses on PC: {glasses_on_pc})")
        }
        Ok(None) => debug!("Auto driver selection: skipped, no Huawei driver registered"),
        Err(err) => warn!("Auto driver selection failed: {err:?}"),
    }

    // The phone normally is NOT on USB (its port is taken by the glasses), so
    // this is usually empty. Keep it for the rare case where adb is reachable.
    let adb_path = adb::get_adb_path(&crate::get_filesystem_layout());
    let usb_serials = adb_path
        .as_deref()
        .and_then(|path| adb::list_usb_device_serials(path).ok())
        .unwrap_or_default();

    if !usb_serials.is_empty() {
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

        // Must run before driver registration and before SteamVR starts: it
        // creates/repairs openvrpaths.vrpath for standalone (no Steam) setups.
        ensure_standalone_openvr_paths();

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
