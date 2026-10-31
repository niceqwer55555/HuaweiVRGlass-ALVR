use alvr_common::{
    anyhow::{bail, Context, Result},
    ToAny,
};
use encoding_rs_io::DecodeReaderBytes;
use serde_json as json;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

fn openvr_source_file_path() -> Result<PathBuf> {
    let path = openvr_paths_file_path()?;

    if path.exists() {
        Ok(path)
    } else {
        bail!("{} does not exist", path.to_string_lossy())
    }
}

pub fn steamvr_settings_file_path() -> Result<PathBuf> {
    let path = if cfg!(windows) {
        // SteamVR records its config directory inside openvrpaths.vrpath, so reading
        // it from there works no matter which drive Steam happens to live on.
        get_single_openvr_path("config")?
    } else {
        dirs::data_dir().to_any()?.join("Steam/config")
    }
    .join("steamvr.vrsettings");

    if path.exists() {
        Ok(path)
    } else {
        bail!("{} does not exist", path.to_string_lossy())
    }
}

pub fn load_openvr_paths_json() -> Result<json::Value> {
    let file = File::open(openvr_source_file_path()?)?;

    let mut file_content_decoded = String::new();
    DecodeReaderBytes::new(&file).read_to_string(&mut file_content_decoded)?;

    let value = json::from_str(&file_content_decoded)?;

    Ok(value)
}

pub fn save_openvr_paths_json(openvr_paths: &json::Value) -> Result<()> {
    let file_content = json::to_string_pretty(openvr_paths)?;

    fs::write(openvr_source_file_path()?, file_content)?;

    Ok(())
}

pub fn from_openvr_paths(paths: &json::Value) -> Vec<std::path::PathBuf> {
    let Some(paths_vec) = paths.as_array() else {
        return vec![];
    };

    paths_vec
        .iter()
        .filter_map(json::Value::as_str)
        .map(|s| PathBuf::from(s.replace(r"\\", r"\")))
        .collect()
}

pub fn to_openvr_paths(paths: &[PathBuf]) -> json::Value {
    let paths_vec = paths
        .iter()
        .map(|p| p.to_string_lossy().into())
        .map(json::Value::String) // backslashes gets duplicated here
        .collect::<Vec<_>>();

    json::Value::Array(paths_vec)
}

fn get_single_openvr_path(path_type: &str) -> Result<PathBuf> {
    let openvr_paths_json = load_openvr_paths_json()?;
    let paths_json = openvr_paths_json.get(path_type).to_any()?;
    from_openvr_paths(paths_json).first().cloned().to_any()
}

pub fn steamvr_root_dir() -> Result<PathBuf> {
    get_single_openvr_path("runtime")
}

/// Path of openvrpaths.vrpath without requiring the file to already exist
/// (unlike `openvr_source_file_path`, which bails on a fresh machine).
///
/// The `ALVR_OPENVR_PATHS_FILE` environment variable overrides the location
/// (portable setups and tests).
pub fn openvr_paths_file_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("ALVR_OPENVR_PATHS_FILE") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    let dir = if cfg!(windows) {
        dirs::cache_dir()
    } else {
        dirs::config_dir()
    }
    .to_any()?;

    Ok(dir.join("openvr/openvrpaths.vrpath"))
}

/// Makes sure openvrpaths.vrpath exists and points at `runtime_root`, so both
/// SteamVR and OpenVR applications work on a machine without the Steam client.
///
/// - File missing: created with a standalone layout (config and log folders
///   inside the runtime directory, empty driver and app lists).
/// - File present: only `runtime[0]` is (re)set to `runtime_root`; everything
///   else (Steam config location, external drivers, registered apps) is kept.
///
/// Returns the file path.
pub fn ensure_openvr_paths_file(runtime_root: &Path) -> Result<PathBuf> {
    let file_path = openvr_paths_file_path()?;

    let existing = fs::read_to_string(&file_path)
        .ok()
        .and_then(|text| json::from_str::<json::Value>(&text).ok());

    let (mut value, fresh) = match existing {
        Some(value) => (value, false),
        None => (
            json::json!({ "jsonid": "vrpathreg", "version": 1 }),
            true,
        ),
    };

    let object = value.as_object_mut().to_any()?;

    // Chosen runtime first, keep any previously known ones.
    let mut runtimes = from_openvr_paths(object.get("runtime").unwrap_or(&json::Value::Null));
    runtimes.retain(|path| {
        !path
            .to_string_lossy()
            .eq_ignore_ascii_case(&runtime_root.to_string_lossy())
    });
    runtimes.insert(0, runtime_root.to_path_buf());
    object.insert("runtime".to_owned(), to_openvr_paths(&runtimes));

    if fresh {
        let config_dir = runtime_root.join("config");
        let log_dir = runtime_root.join("logs");
        fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create {}", config_dir.display()))?;
        fs::create_dir_all(&log_dir)
            .with_context(|| format!("Failed to create {}", log_dir.display()))?;

        object.insert("config".to_owned(), to_openvr_paths(&[config_dir]));
        object.insert("log".to_owned(), to_openvr_paths(&[log_dir]));
        object.insert(
            "external_drivers".to_owned(),
            json::Value::Array(Vec::new()),
        );
        object.insert("apps".to_owned(), json::Value::Array(Vec::new()));
    }

    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {parent:?}"))?;
    }
    fs::write(&file_path, json::to_string_pretty(&value)?)
        .with_context(|| format!("Failed to write {file_path:?}"))?;

    Ok(file_path)
}

/// Adds or removes a `.vrmanifest` path in the `apps` array of
/// openvrpaths.vrpath. This is what `vrpathreg` does, without spawning it.
/// The file has to exist first (`ensure_openvr_paths_file`).
pub fn set_app_manifest_registered(manifest_path: &Path, register: bool) -> Result<()> {
    let file_path = openvr_paths_file_path()?;
    if !file_path.is_file() {
        bail!(
            "{} does not exist; create it first with ensure_openvr_paths_file",
            file_path.to_string_lossy()
        );
    }

    let mut value: json::Value = json::from_str(&fs::read_to_string(&file_path)?)?;
    let object = value.as_object_mut().to_any()?;

    let mut apps = from_openvr_paths(object.get("apps").unwrap_or(&json::Value::Null));
    if register {
        let already = apps.iter().any(|path| {
            path.as_os_str()
                .eq_ignore_ascii_case(manifest_path.as_os_str())
        });
        if !already {
            apps.insert(0, manifest_path.to_path_buf());
        }
    } else {
        apps.retain(|path| {
            !path
                .as_os_str()
                .eq_ignore_ascii_case(manifest_path.as_os_str())
        });
    }
    object.insert("apps".to_owned(), to_openvr_paths(&apps));

    fs::write(&file_path, json::to_string_pretty(&value)?)
        .with_context(|| format!("Failed to write {file_path:?}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_standalone_file_and_registers_apps() {
        let tmp =
            std::env::temp_dir().join(format!("alvr_openvrpaths_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let file_path = tmp.join("openvrpaths.vrpath");
        std::env::set_var("ALVR_OPENVR_PATHS_FILE", &file_path);

        let runtime = tmp.join("SteamVR Test");

        ensure_openvr_paths_file(&runtime).unwrap();

        let value: json::Value = json::from_str(&fs::read_to_string(&file_path).unwrap()).unwrap();
        assert_eq!(
            value["runtime"][0].as_str().unwrap().to_lowercase(),
            runtime.to_string_lossy().to_lowercase()
        );
        assert!(value["config"][0]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("config"));
        assert!(value["log"][0]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("logs"));
        assert!(runtime.join("config").is_dir());
        assert!(runtime.join("logs").is_dir());

        let manifest = tmp.join("game.vrmanifest");
        fs::write(&manifest, "{}").unwrap();

        set_app_manifest_registered(&manifest, true).unwrap();
        let value = load_openvr_paths_json().unwrap();
        assert_eq!(value["apps"].as_array().unwrap().len(), 1);

        // Registering twice must not duplicate.
        set_app_manifest_registered(&manifest, true).unwrap();
        let value = load_openvr_paths_json().unwrap();
        assert_eq!(value["apps"].as_array().unwrap().len(), 1);

        set_app_manifest_registered(&manifest, false).unwrap();
        let value = load_openvr_paths_json().unwrap();
        assert_eq!(value["apps"].as_array().unwrap().len(), 0);

        // Re-running ensure keeps the file and only reorders the runtime entry.
        ensure_openvr_paths_file(&runtime).unwrap();
        let value = load_openvr_paths_json().unwrap();
        assert!(value["apps"].as_array().unwrap().is_empty());

        std::env::remove_var("ALVR_OPENVR_PATHS_FILE");
        let _ = fs::remove_dir_all(&tmp);
    }
}
