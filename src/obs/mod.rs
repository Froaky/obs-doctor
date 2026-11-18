//! Everything we learn from OBS Studio's own files. All of it is read-only.
//!
//! - [`config`]: the active profile (what the streamer has configured).
//! - [`rules`]: advice that follows from that configuration.
//! - [`logs`]: what OBS wrote down about this PC and about the last stream.
//! - [`scenes`]: the scene collection (sources, filters).

pub mod config;
pub mod logs;
pub mod menu;
pub mod rules;
pub mod scenes;

use std::fs;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// Where OBS keeps its things for this user.
#[derive(Debug, Clone, Default)]
pub struct ObsPaths {
    /// `%APPDATA%\obs-studio`, or the folder given with `--obs-dir`.
    pub config_dir: PathBuf,
    /// False when OBS is not installed or was never opened.
    pub exists: bool,
    /// Folder of the active profile (holds `basic.ini`, `streamEncoder.json`...).
    pub profile_dir: Option<PathBuf>,
    /// JSON file of the active scene collection.
    pub scene_file: Option<PathBuf>,
    /// Folder with one text log per OBS session. May not exist.
    pub logs_dir: PathBuf,
}

pub fn locate(dir_override: Option<PathBuf>) -> ObsPaths {
    let config_dir = dir_override.unwrap_or_else(default_dir);
    let exists = config_dir.is_dir();
    let selection = if exists { read_selection(&config_dir) } else { Selection::default() };

    ObsPaths {
        profile_dir: if exists { find_profile(&config_dir, selection.profile_dir.as_deref()) } else { None },
        scene_file: if exists { find_scene_file(&config_dir, selection.scene_file.as_deref()) } else { None },
        logs_dir: config_dir.join("logs"),
        exists,
        config_dir,
    }
}

fn default_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));

    base.unwrap_or_default().join("obs-studio")
}

/// What the streamer has selected in OBS's Profile and Scene Collection menus.
#[derive(Debug, Default, PartialEq)]
struct Selection {
    profile_dir: Option<String>,
    scene_file: Option<String>,
}

/// Newer OBS versions keep the selection in `user.ini`, older ones in `global.ini`.
fn read_selection(config_dir: &Path) -> Selection {
    let mut selection = Selection::default();
    for file in ["user.ini", "global.ini"] {
        let Ok(text) = fs::read_to_string(config_dir.join(file)) else {
            continue;
        };
        let found = parse_selection(&text);
        selection.profile_dir = selection.profile_dir.or(found.profile_dir);
        selection.scene_file = selection.scene_file.or(found.scene_file);
    }
    selection
}

fn parse_selection(ini_text: &str) -> Selection {
    let ini = Ini::parse(ini_text);
    let value = |key: &str| ini.get("Basic", key).filter(|v| !v.is_empty()).map(str::to_string);
    Selection {
        profile_dir: value("ProfileDir"),
        // Older versions only stored the file name without extension.
        scene_file: value("SceneCollectionFile")
            .map(|f| if f.to_lowercase().ends_with(".json") { f } else { format!("{f}.json") }),
    }
}

/// The selected profile, or the most recently changed one as a fallback.
fn find_profile(config_dir: &Path, selected: Option<&str>) -> Option<PathBuf> {
    let profiles = config_dir.join("basic").join("profiles");
    if let Some(name) = selected {
        let candidate = profiles.join(name);
        if candidate.join("basic.ini").is_file() {
            return Some(candidate);
        }
    }
    newest(&profiles, |p| p.join("basic.ini"))
}

/// The selected scene collection, or the most recently changed one.
fn find_scene_file(config_dir: &Path, selected: Option<&str>) -> Option<PathBuf> {
    let scenes = config_dir.join("basic").join("scenes");
    if let Some(name) = selected {
        let candidate = scenes.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    newest(&scenes, |p| p.to_path_buf()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
}

/// Among the entries of `dir`, the one whose marker file changed last.
fn newest(dir: &Path, marker: impl Fn(&Path) -> PathBuf) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| marker(path).is_file())
        // Backups such as "Escena.json.bak" are not scene collections.
        .filter(|path| !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bak")))
        .max_by_key(|path| fs::metadata(marker(path)).and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_selection_obs_writes_today() {
        let selection = parse_selection(
            "[Basic]\nProfile=Sin título\nProfileDir=Sin_título\nSceneCollection=Sin título\nSceneCollectionFile=Sin_título.json\n",
        );
        assert_eq!(selection.profile_dir.as_deref(), Some("Sin_título"));
        assert_eq!(selection.scene_file.as_deref(), Some("Sin_título.json"));
    }

    #[test]
    fn older_versions_store_the_scene_file_without_extension() {
        let selection = parse_selection("[Basic]\nSceneCollectionFile=Mi_escena\n");
        assert_eq!(selection.scene_file.as_deref(), Some("Mi_escena.json"));
    }

    #[test]
    fn nothing_selected_yet() {
        assert_eq!(parse_selection("[General]\nFirstRun=true\n"), Selection::default());
    }

    #[test]
    fn a_missing_folder_is_reported_not_guessed() {
        let paths = locate(Some(PathBuf::from("Z:/no/existe/obs-studio")));
        assert!(!paths.exists);
        assert!(paths.profile_dir.is_none());
        assert!(paths.scene_file.is_none());
    }
}
