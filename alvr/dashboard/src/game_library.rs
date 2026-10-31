//! Local game library used to start arbitrary (non-Steam) VR games directly,
//! without involving the Steam client: the dashboard only needs a running
//! SteamVR runtime, the game is spawned as a normal Windows process and picks up
//! the VR session by itself.
//!
//! The list is stored in a JSON sidecar next to the dashboard executable so it
//! survives upgrades (the release archive does not contain it).

use alvr_common::anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const LIBRARY_FILE: &str = "game_library.json";
const MANIFEST_DIR: &str = "vrapps";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalGame {
    pub name: String,
    pub exe: String,
    #[serde(default)]
    pub args: String,
}

fn library_file() -> Option<PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    Some(dir.join(LIBRARY_FILE))
}

pub fn load_games() -> Vec<LocalGame> {
    let Some(file) = library_file() else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };

    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_games(games: &[LocalGame]) {
    let Some(file) = library_file() else {
        return;
    };

    if let Ok(text) = serde_json::to_string_pretty(games) {
        let _ = fs::write(file, text);
    }
}

fn split_args(args: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quoted = false;

    for character in args.chars() {
        match character {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Spawns the game in its own directory. Returns the created process id.
pub fn launch_game(game: &LocalGame) -> Result<u32> {
    let exe = Path::new(game.exe.trim().trim_matches('"'));
    if !exe.is_file() {
        bail!("找不到可执行文件：{}", exe.display());
    }

    let mut command = Command::new(exe);
    if let Some(parent) = exe.parent() {
        command.current_dir(parent);
    }
    for arg in split_args(&game.args) {
        command.arg(arg);
    }

    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("启动失败：{}", exe.display()))?;

    Ok(child.id())
}

// ---------------------------------------------------------------------------
// SteamVR app manifests
//
// Registering a .vrmanifest makes the game show up inside the SteamVR library
// itself, which is what makes SteamVR usable completely without the Steam
// client. The manifest lives next to the dashboard (so it survives runtime
// changes) and is registered in openvrpaths.vrpath, exactly what `vrpathreg`
// does.
// ---------------------------------------------------------------------------

/// Stable identifier for a local game. ASCII names become a slug, everything
/// else gets a hash suffix so two Chinese names never collide.
pub fn manifest_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_dash = false;
        } else if !slug.is_empty() && !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() || !name.is_ascii() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);

        slug = format!("game-{:08x}", hasher.finish() as u32);
    }

    slug
}

/// Where the manifest for `game` is stored (next to the dashboard executable).
pub fn manifest_path(game: &LocalGame) -> Option<PathBuf> {
    let mut dir = std::env::current_exe().ok()?;
    dir.pop();
    Some(dir.join(MANIFEST_DIR).join(format!(
        "local.{}.vrmanifest",
        manifest_slug(&game.name)
    )))
}

/// Writes the SteamVR manifest for `game`. Returns the manifest path.
pub fn write_manifest(game: &LocalGame) -> Result<PathBuf> {
    let exe = Path::new(game.exe.trim().trim_matches('"'));
    if !exe.is_file() {
        bail!("找不到可执行文件：{}", exe.display());
    }

    let mut dir = std::env::current_exe().context("无法确定程序目录")?;
    dir.pop();
    let dir = dir.join(MANIFEST_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;

    let path = dir.join(format!(
        "local.{}.vrmanifest",
        manifest_slug(&game.name)
    ));

    let working_dir = exe
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    let name = game.name.clone();

    let manifest = serde_json::json!({
        "app_key": format!("local.{}", manifest_slug(&game.name)),
        "launch_type": "binary",
        "binary_path_windows": exe.display().to_string(),
        "arguments": game.args.clone(),
        "working_directory": working_dir,
        "name": { "en_US": name, "zh_CN": name },
        "sort_order": 0,
        "is_overlay": false,
        "should_run_in_steamvr": true,
    });

    fs::write(&path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("无法写入 {}", path.display()))?;

    Ok(path)
}

/// Writes (or removes registration of) the manifest and updates
/// openvrpaths.vrpath, so SteamVR lists the game in its own library.
pub fn set_manifest_registered(game: &LocalGame, register: bool) -> Result<PathBuf> {
    if register {
        let path = write_manifest(game)?;
        alvr_server_io::set_app_manifest_registered(&path, true)?;
        Ok(path)
    } else {
        let path = manifest_path(game).context("无法确定程序目录")?;
        alvr_server_io::set_app_manifest_registered(&path, false)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{manifest_slug, split_args};

    #[test]
    fn splits_quoted_arguments() {
        assert_eq!(
            split_args(r#"-vr -path "C:\My Games\x" -windowed"#),
            vec!["-vr", "-path", r"C:\My Games\x", "-windowed"]
        );
    }

    #[test]
    fn handles_empty_arguments() {
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn slugs_ascii_names() {
        assert_eq!(manifest_slug(" Half-Life 2 VR "), "half-life-2-vr");
        assert_eq!(manifest_slug("My_Game (VR)"), "my-game-vr");
    }

    #[test]
    fn slugs_non_ascii_names_are_unique() {
        let a = manifest_slug("节奏光剑");
        let b = manifest_slug("Beat Saber 中文版");
        assert!(a.starts_with("game-"));
        assert!(b.starts_with("game-"));
        assert_ne!(a, b);
        assert_eq!(manifest_slug("节奏光剑"), a);
    }
}
