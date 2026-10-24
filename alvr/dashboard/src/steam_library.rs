//! Helpers to manage the Steam library from the dashboard: locating the Steam
//! installation and adding/removing non-Steam game shortcuts, which is what
//! makes an arbitrary executable show up in the Steam and SteamVR libraries.
//!
//! Shortcuts live in `<steam>/userdata/<account id>/config/shortcuts.vdf`, a
//! binary KeyValues file. This module parses it into a generic tree, mutates the
//! tree and writes it back, so unknown fields written by newer Steam builds are
//! preserved.

use alvr_common::{
    anyhow::{bail, Context, Result},
    debug,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

const SHORTCUTS_FILE: &str = "shortcuts.vdf";

// ---------------------------------------------------------------------------
// CRC32 (IEEE), used for the shortcut app id
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;

    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }

    !crc
}

// ---------------------------------------------------------------------------
// Binary VDF
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum VdfValue {
    Object(Vec<(String, VdfValue)>),
    String(String),
    Int(u32),
    Uint64(u64),
}

impl VdfValue {
    fn as_object(&self) -> Option<&Vec<(String, VdfValue)>> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn as_u32(&self) -> Option<u32> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn read_byte(&mut self) -> Result<u8> {
        let byte = *self
            .data
            .get(self.position)
            .context("Unexpected end of shortcuts.vdf")?;
        self.position += 1;
        Ok(byte)
    }

    fn read_cstring(&mut self) -> Result<String> {
        let start = self.position;
        while self.position < self.data.len() && self.data[self.position] != 0 {
            self.position += 1;
        }
        if self.position >= self.data.len() {
            bail!("Unterminated string in shortcuts.vdf");
        }
        let text = String::from_utf8_lossy(&self.data[start..self.position]).into_owned();
        self.position += 1;
        Ok(text)
    }

    fn read_u32(&mut self) -> Result<u32> {
        let end = self.position + 4;
        let slice = self
            .data
            .get(self.position..end)
            .context("Unexpected end of shortcuts.vdf")?;
        self.position = end;
        Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let end = self.position + 8;
        let slice = self
            .data
            .get(self.position..end)
            .context("Unexpected end of shortcuts.vdf")?;
        self.position = end;
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(slice);
        Ok(u64::from_le_bytes(bytes))
    }
}

fn parse_children(reader: &mut Reader<'_>) -> Result<Vec<(String, VdfValue)>> {
    let mut entries = Vec::new();

    loop {
        let value_type = reader.read_byte()?;
        match value_type {
            0x08 => break,
            0x00 => {
                let name = reader.read_cstring()?;
                let children = parse_children(reader)?;
                entries.push((name, VdfValue::Object(children)));
            }
            0x01 => {
                let name = reader.read_cstring()?;
                let value = reader.read_cstring()?;
                entries.push((name, VdfValue::String(value)));
            }
            0x02 => {
                let name = reader.read_cstring()?;
                let value = reader.read_u32()?;
                entries.push((name, VdfValue::Int(value)));
            }
            0x07 => {
                let name = reader.read_cstring()?;
                let value = reader.read_u64()?;
                entries.push((name, VdfValue::Uint64(value)));
            }
            other => bail!("Unsupported VDF value type 0x{other:02X}"),
        }
    }

    Ok(entries)
}

fn parse_file(bytes: &[u8]) -> Result<Vec<(String, VdfValue)>> {
    let mut reader = Reader {
        data: bytes,
        position: 0,
    };

    if reader.read_byte()? != 0x00 {
        bail!("shortcuts.vdf does not start with a root object");
    }
    // Root object name ("shortcuts")
    reader.read_cstring()?;

    parse_children(&mut reader)
}

fn push_cstring(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(value.as_bytes());
    out.push(0);
}

fn write_children(out: &mut Vec<u8>, entries: &[(String, VdfValue)]) {
    for (name, value) in entries {
        match value {
            VdfValue::Object(children) => {
                out.push(0x00);
                push_cstring(out, name);
                write_children(out, children);
                out.push(0x08);
            }
            VdfValue::String(text) => {
                out.push(0x01);
                push_cstring(out, name);
                push_cstring(out, text);
            }
            VdfValue::Int(number) => {
                out.push(0x02);
                push_cstring(out, name);
                out.extend_from_slice(&number.to_le_bytes());
            }
            VdfValue::Uint64(number) => {
                out.push(0x07);
                push_cstring(out, name);
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
    }
}

fn serialize_file(entries: &[(String, VdfValue)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x00);
    push_cstring(&mut out, "shortcuts");
    write_children(&mut out, entries);
    out.push(0x08);
    out.push(0x08);
    out
}

// ---------------------------------------------------------------------------
// Steam installation discovery
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SteamInstall {
    /// Steam client root, e.g. `D:\Program Files\Steam`
    pub root: PathBuf,
    /// `...\userdata\<account id>`
    pub userdata_dir: PathBuf,
    /// `...\userdata\<account id>\config\shortcuts.vdf`
    pub shortcuts_path: PathBuf,
}

impl SteamInstall {
    pub fn describe(&self) -> String {
        format!(
            "{}（账户目录 {}）",
            self.root.display(),
            self.userdata_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    }
}

fn candidate_steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    for env_name in ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"] {
        if let Ok(program_files) = std::env::var(env_name) {
            roots.push(PathBuf::from(program_files).join("Steam"));
        }
    }

    for letter in 'C'..='Z' {
        let drive = format!("{letter}:\\");
        if !Path::new(&drive).exists() {
            continue;
        }
        roots.push(PathBuf::from(format!("{drive}Steam")));
        roots.push(PathBuf::from(format!("{drive}Program Files (x86)\\Steam")));
        roots.push(PathBuf::from(format!("{drive}Program Files\\Steam")));
        roots.push(PathBuf::from(format!("{drive}Games\\Steam")));
    }

    roots
}

fn is_steam_root(path: &Path) -> bool {
    path.join("steam.exe").is_file() || path.join("userdata").is_dir()
}

fn newest_userdata_dir(root: &Path) -> Option<PathBuf> {
    let userdata = root.join("userdata");

    // Prefer the account Steam used last (config/loginusers.vdf, MostRecent "1").
    if let Some(account_id) = most_recent_account_id(root) {
        let candidate = userdata.join(account_id.to_string());
        if candidate.join("config").is_dir() {
            return Some(candidate);
        }
    }

    let read_dir = fs::read_dir(&userdata).ok()?;

    let mut candidates = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let shortcuts = path.join("config").join(SHORTCUTS_FILE);
        let modified = fs::metadata(&shortcuts)
            .and_then(|meta| meta.modified())
            .or_else(|_| fs::metadata(&path).and_then(|meta| meta.modified()))
            .unwrap_or(SystemTime::UNIX_EPOCH);

        candidates.push((modified, path));
    }

    candidates
        .into_iter()
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn most_recent_account_id(root: &Path) -> Option<u32> {
    const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

    let text = fs::read_to_string(root.join("config").join("loginusers.vdf")).ok()?;

    let mut current_id: Option<u64> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }

        let value = trimmed.trim_matches('"');
        if value.len() >= 17 && value.chars().all(|character| character.is_ascii_digit()) {
            current_id = value.parse::<u64>().ok();
        } else if trimmed.contains("MostRecent") && trimmed.ends_with("\"1\"") {
            if let Some(id) = current_id {
                return u32::try_from(id.saturating_sub(STEAM_ID64_BASE)).ok();
            }
        }
    }

    None
}

/// Finds the Steam installation. `manual_root` is used first when non empty, so
/// the user can point the dashboard at a Steam install that is not autodetected
/// (portable installs, secondary drives, etc).
pub fn find_steam_install(manual_root: &str) -> Option<SteamInstall> {
    let manual = manual_root.trim().trim_matches('"');
    let manual_path = (!manual.is_empty()).then(|| PathBuf::from(manual));

    let mut roots: Vec<PathBuf> = manual_path.into_iter().collect();
    roots.extend(candidate_steam_roots());

    for root in roots {
        if !is_steam_root(&root) {
            continue;
        }
        let Some(userdata_dir) = newest_userdata_dir(&root) else {
            continue;
        };

        let shortcuts_path = userdata_dir.join("config").join(SHORTCUTS_FILE);
        debug!("Found Steam at {:?}", root);

        return Some(SteamInstall {
            root,
            userdata_dir,
            shortcuts_path,
        });
    }

    None
}

// ---------------------------------------------------------------------------
// Shortcuts
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Shortcut {
    /// Key of the entry inside the `shortcuts` object (usually "0", "1", ...)
    pub key: String,
    pub name: String,
    pub exe: String,
    pub app_id: u32,
}

impl Shortcut {
    /// Executable path without the surrounding quotes Steam stores.
    pub fn clean_exe(&self) -> String {
        self.exe.trim_matches('"').to_owned()
    }
}

fn read_shortcuts_object(path: &Path) -> Result<Vec<(String, VdfValue)>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path).with_context(|| format!("Failed to read {path:?}"))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let root = parse_file(&bytes)?;

    // Normally the root contains a single "shortcuts" object.
    for (name, value) in &root {
        if name.eq_ignore_ascii_case("shortcuts") {
            if let Some(entries) = value.as_object() {
                return Ok(entries.clone());
            }
        }
    }

    Ok(root)
}

pub fn list_shortcuts(install: &SteamInstall) -> Result<Vec<Shortcut>> {
    let entries = read_shortcuts_object(&install.shortcuts_path)?;
    let mut shortcuts = Vec::new();

    for (key, value) in &entries {
        let Some(fields) = value.as_object() else {
            continue;
        };

        let get = |name: &str| {
            fields
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(name))
                .and_then(|(_, value)| value.as_str())
                .unwrap_or("")
                .to_owned()
        };

        let app_id = fields
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case("appid"))
            .and_then(|(_, value)| value.as_u32())
            .unwrap_or(0);

        shortcuts.push(Shortcut {
            key: key.clone(),
            name: get("AppName"),
            exe: get("Exe"),
            app_id,
        });
    }

    Ok(shortcuts)
}

fn shortcut_app_id(exe: &str, name: &str) -> u32 {
    // Same scheme Steam uses for non-Steam shortcuts: CRC32 of the lowercase
    // quoted executable followed by the lowercase display name, with the high
    // bit set.
    let key = format!("{exe}{name}").to_lowercase();
    crc32(key.as_bytes()) | 0x8000_0000
}

fn backup_file(path: &Path) -> Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }

    let backup = path.with_extension("vdf.bak");
    fs::copy(path, &backup).with_context(|| format!("Failed to back up {path:?}"))?;
    Ok(Some(backup))
}

fn write_shortcuts_object(
    install: &SteamInstall,
    entries: &[(String, VdfValue)],
) -> Result<Option<PathBuf>> {
    if let Some(parent) = install.shortcuts_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {parent:?}"))?;
    }

    let backup = backup_file(&install.shortcuts_path)?;

    let root = vec![(
        "shortcuts".to_owned(),
        VdfValue::Object(entries.to_vec()),
    )];
    let bytes = serialize_file(&root);

    fs::write(&install.shortcuts_path, bytes)
        .with_context(|| format!("Failed to write {:?}", install.shortcuts_path))?;

    Ok(backup)
}

/// Adds (or updates) a non-Steam game shortcut. Steam has to be restarted for
/// the entry to show up in the library.
pub fn add_shortcut(
    install: &SteamInstall,
    name: &str,
    exe: &Path,
    launch_options: &str,
) -> Result<(Shortcut, Option<PathBuf>)> {
    let name = name.trim();
    if name.is_empty() {
        bail!("游戏名称不能为空");
    }
    if !exe.is_file() {
        bail!("找不到可执行文件：{}", exe.display());
    }

    let exe_quoted = format!("\"{}\"", exe.display());
    let start_dir = format!(
        "{}\\",
        exe.parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default()
    );
    let app_id = shortcut_app_id(&exe_quoted, name);

    let mut entries = read_shortcuts_object(&install.shortcuts_path)?;

    // Update an existing shortcut pointing to the same executable instead of
    // adding a duplicate entry.
    let existing_key = entries.iter().find_map(|(key, value)| {
        let fields = value.as_object()?;
        let stored_exe = fields
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case("Exe"))
            .and_then(|(_, value)| value.as_str())?;

        stored_exe
            .trim_matches('"')
            .eq_ignore_ascii_case(&exe.display().to_string())
            .then(|| key.clone())
    });

    let key = match &existing_key {
        Some(key) => key.clone(),
        None => {
            let next_index = entries
                .iter()
                .filter_map(|(key, _)| key.parse::<u32>().ok())
                .max()
                .map(|max| max + 1)
                .unwrap_or(0);
            next_index.to_string()
        }
    };

    let mut fields = vec![
        ("appid".to_owned(), VdfValue::Int(app_id)),
        ("AppName".to_owned(), VdfValue::String(name.to_owned())),
        ("Exe".to_owned(), VdfValue::String(exe_quoted.clone())),
        ("StartDir".to_owned(), VdfValue::String(start_dir)),
        (
            "icon".to_owned(),
            VdfValue::String(exe.display().to_string()),
        ),
        ("ShortcutPath".to_owned(), VdfValue::String(String::new())),
        (
            "LaunchOptions".to_owned(),
            VdfValue::String(launch_options.trim().to_owned()),
        ),
        ("IsHidden".to_owned(), VdfValue::Int(0)),
        ("AllowDesktopConfig".to_owned(), VdfValue::Int(1)),
        ("AllowOverlay".to_owned(), VdfValue::Int(1)),
        // Marks the entry as a VR title so SteamVR lists it in its library.
        ("OpenVR".to_owned(), VdfValue::Int(1)),
        ("Devkit".to_owned(), VdfValue::Int(0)),
        ("DevkitGameID".to_owned(), VdfValue::String(String::new())),
        ("DevkitOverrideAppID".to_owned(), VdfValue::Int(0)),
        ("LastPlayTime".to_owned(), VdfValue::Int(0)),
        ("FlatpakAppID".to_owned(), VdfValue::String(String::new())),
        ("sortas".to_owned(), VdfValue::String(String::new())),
        ("tags".to_owned(), VdfValue::Object(Vec::new())),
    ];

    // Keep fields this dashboard does not know about (newer Steam versions).
    let preserved = entries
        .iter()
        .find(|(entry_key, _)| *entry_key == key)
        .and_then(|(_, value)| value.as_object())
        .cloned()
        .unwrap_or_default();
    for (field, value) in preserved {
        if !fields
            .iter()
            .any(|(existing, _)| existing.eq_ignore_ascii_case(&field))
        {
            fields.push((field, value));
        }
    }

    match entries.iter_mut().find(|(entry_key, _)| *entry_key == key) {
        Some((_, value)) => *value = VdfValue::Object(fields),
        None => entries.push((key.clone(), VdfValue::Object(fields))),
    }

    let backup = write_shortcuts_object(install, &entries)?;

    Ok((
        Shortcut {
            key,
            name: name.to_owned(),
            exe: exe_quoted,
            app_id,
        },
        backup,
    ))
}

pub fn remove_shortcut(install: &SteamInstall, key: &str) -> Result<Option<PathBuf>> {
    let mut entries = read_shortcuts_object(&install.shortcuts_path)?;
    entries.retain(|(entry_key, _)| entry_key != key);

    write_shortcuts_object(install, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_round_trip_keeps_fields() {
        let root = vec![(
            "shortcuts".to_owned(),
            VdfValue::Object(vec![
                (
                    "0".to_owned(),
                    VdfValue::Object(vec![
                        ("appid".to_owned(), VdfValue::Int(1234)),
                        (
                            "AppName".to_owned(),
                            VdfValue::String("My Game".to_owned()),
                        ),
                        ("IsHidden".to_owned(), VdfValue::Int(0)),
                        ("LastPlayTime".to_owned(), VdfValue::Uint64(42)),
                    ]),
                ),
                (
                    "1".to_owned(),
                    VdfValue::Object(vec![(
                        "AppName".to_owned(),
                        VdfValue::String("Second".to_owned()),
                    )]),
                ),
            ]),
        )];

        let bytes = serialize_file(&root);
        let parsed = parse_file(&bytes).unwrap();

        let shortcuts = parsed
            .iter()
            .find(|(name, _)| name == "shortcuts")
            .and_then(|(_, value)| value.as_object())
            .unwrap();

        assert_eq!(shortcuts.len(), 2);
        let first = shortcuts[0].1.as_object().unwrap();
        assert_eq!(first.len(), 4);
        assert_eq!(first[1].1.as_str().unwrap(), "My Game");
    }

    #[test]
    fn app_ids_have_high_bit_set() {
        let app_id = shortcut_app_id("\"C:\\Game\\game.exe\"", "Game");
        assert_eq!(app_id & 0x8000_0000, 0x8000_0000);
    }
}
