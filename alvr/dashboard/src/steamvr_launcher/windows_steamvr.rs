use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// Bundled SteamVR relative to the workspace root
const BUNDLED_STEAMVR_DIR: &str = "SteamVR 1.27.5";
const VRMONITOR_FNAME: &str = "vrmonitor.exe";

/// Turns whatever the user typed in the "SteamVR path" setting into a runnable
/// `vrmonitor.exe` (or `steam.exe`). Accepted inputs:
/// - the full path to `vrmonitor.exe`
/// - the SteamVR runtime folder (the one containing `bin\win64\vrmonitor.exe`)
/// - the folder containing a `SteamVR` folder (e.g. `SteamVR 1.27.5`, `steamapps\common`)
/// - the path to `steam.exe`
pub fn resolve_steamvr_launch_path(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);

    if path.is_file() {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if file_name == VRMONITOR_FNAME || file_name == "steam.exe" || file_name == "vrstartup.exe"
        {
            return Some(path);
        }

        // The user may have picked a file inside the runtime, fall back to its folder.
        return path
            .parent()
            .and_then(|parent| resolve_steamvr_launch_path(&parent.to_string_lossy()));
    }

    let candidates = [
        path.join(VRMONITOR_FNAME),
        path.join("bin").join("win64").join(VRMONITOR_FNAME),
        path.join("SteamVR")
            .join("bin")
            .join("win64")
            .join(VRMONITOR_FNAME),
        path.join("steam.exe"),
        path.join("SteamVR").join("steam.exe"),
        path.join("vrstartup.exe"),
    ];

    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Resolves the SteamVR runtime root (the folder containing
/// `bin\win64\vrmonitor.exe`) for the configured path, or the bundled runtime.
/// Used to write openvrpaths.vrpath for standalone operation.
pub fn resolve_steamvr_runtime_root(input: &str) -> Option<PathBuf> {
    if let Some(monitor) = resolve_steamvr_launch_path(input)
        .or_else(bundled_steamvr_monitor_path)
    {
        let name = monitor
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if name == VRMONITOR_FNAME {
            // <runtime>\bin\win64\vrmonitor.exe
            return monitor
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .map(Path::to_path_buf);
        }

        // steam.exe / vrstartup.exe: runtime lives in steamapps\common\SteamVR
        if let Some(parent) = monitor.parent() {
            let runtime = parent.join("steamapps").join("common").join("SteamVR");
            if runtime.join("bin").join("win64").join(VRMONITOR_FNAME).is_file() {
                return Some(runtime);
            }
        }
    }

    // Nothing configured or bundled: fall back to the runtime that
    // openvrpaths.vrpath already points at (e.g. a Steam-managed install).
    alvr_server_io::steamvr_root_dir().ok().and_then(|runtime| {
        runtime
            .join("bin")
            .join("win64")
            .join(VRMONITOR_FNAME)
            .is_file()
            .then_some(runtime)
    })
}

fn bundled_steamvr_monitor_path() -> Option<PathBuf> {
    let relative: PathBuf = [BUNDLED_STEAMVR_DIR, "SteamVR", "bin", "win64", VRMONITOR_FNAME]
        .iter()
        .collect();

    // Search upward from the dashboard executable (handles both the packaged
    // layout, where the exe sits next to "SteamVR 1.27.5", and dev builds in
    // target/<profile>/ inside the workspace).
    let mut dir = std::env::current_exe().ok()?;
    for _ in 0..8 {
        let candidate = dir.join(&relative);
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

fn spawn_detached(exe: &Path, args: &[&str]) {
    let _ = Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn launch_via_steam_protocol() {
    let _ = Command::new("cmd")
        .args(["/C", "start", "steam://rungameid/250820"])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub fn start_steamvr(manual_path: &str) {
    if let Some(path) = resolve_steamvr_launch_path(manual_path) {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if file_name == "steam.exe" {
            // Let the Steam client start the SteamVR app (app id 250820).
            spawn_detached(&path, &["-applaunch", "250820"]);
        } else {
            spawn_detached(&path, &[]);
        }
        return;
    }

    if let Some(vrmonitor_path) = bundled_steamvr_monitor_path() {
        spawn_detached(&vrmonitor_path, &[]);
        return;
    }

    // Fallback to the Steam client if the configured and bundled runtimes are missing
    launch_via_steam_protocol();
}

pub fn kill_process(pid: u32) {
    Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok();
}
