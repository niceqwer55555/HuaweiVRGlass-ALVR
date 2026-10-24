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

#[cfg(test)]
mod tests {
    use super::split_args;

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
}
