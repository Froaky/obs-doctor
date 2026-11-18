//! Reads how OBS is set up, straight from its config files.
//!
//! Nothing is modified, and the stream key in `service.json` is never read
//! into our own data: only the name of the platform is kept.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// OBS only writes a value to disk once you change it; these are its defaults.
const DEFAULT_BITRATE_KBPS: u32 = 2500;
const DEFAULT_FPS: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderKind {
    /// x264 and friends: the processor does the work.
    Software,
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl EncoderKind {
    pub fn from_id(id: &str) -> Self {
        let id = id.to_lowercase();
        if id.contains("nvenc") {
            EncoderKind::Nvidia
        } else if id.contains("qsv") {
            EncoderKind::Intel
        } else if id.contains("amf") || id.contains("amd") {
            EncoderKind::Amd
        } else if id.contains("x264") || id.contains("svt") || id.contains("aom") || id.contains("openh264") {
            EncoderKind::Software
        } else {
            EncoderKind::Other
        }
    }

    pub fn is_hardware(self) -> bool {
        matches!(self, EncoderKind::Nvidia | EncoderKind::Amd | EncoderKind::Intel)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Simple,
    Advanced,
}

#[derive(Debug, Clone)]
pub struct ObsConfig {
    pub profile: String,
    /// Twitch, YouTube... `None` when it is a custom server or not set yet.
    pub service: Option<String>,
    pub mode: OutputMode,
    pub base: Option<(u32, u32)>,
    pub output: Option<(u32, u32)>,
    pub fps: f32,
    pub encoder_id: String,
    pub encoder: EncoderKind,
    pub bitrate_kbps: u32,
    pub preset: Option<String>,
    /// Only known in advanced mode; simple mode always streams in CBR.
    pub rate_control: Option<String>,
}

#[derive(Debug)]
pub enum ObsStatus {
    /// No config folder: OBS is not installed or was never opened.
    NotFound { looked_in: PathBuf },
    /// The folder exists but there is no usable profile inside.
    NoProfile { dir: PathBuf },
    Found(ObsConfig),
}

pub fn read(dir_override: Option<PathBuf>) -> ObsStatus {
    let dir = dir_override.unwrap_or_else(default_dir);
    if !dir.is_dir() {
        return ObsStatus::NotFound { looked_in: dir };
    }
    match find_profile(&dir) {
        Some(profile_dir) => match read_profile(&profile_dir) {
            Some(cfg) => ObsStatus::Found(cfg),
            None => ObsStatus::NoProfile { dir },
        },
        None => ObsStatus::NoProfile { dir },
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

/// The profile OBS has selected, or the most recently used one as a fallback.
fn find_profile(dir: &Path) -> Option<PathBuf> {
    let profiles = dir.join("basic").join("profiles");

    // Newer OBS versions keep the selection in user.ini, older ones in global.ini.
    for file in ["user.ini", "global.ini"] {
        let Ok(text) = fs::read_to_string(dir.join(file)) else {
            continue;
        };
        let ini = Ini::parse(&text);
        if let Some(name) = ini.get("Basic", "ProfileDir") {
            let candidate = profiles.join(name);
            if candidate.join("basic.ini").is_file() {
                return Some(candidate);
            }
        }
    }

    fs::read_dir(&profiles)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("basic.ini").is_file())
        .max_by_key(|p| {
            fs::metadata(p.join("basic.ini"))
                .and_then(|m| m.modified())
                .ok()
        })
}

fn read_profile(profile_dir: &Path) -> Option<ObsConfig> {
    let basic = fs::read_to_string(profile_dir.join("basic.ini")).ok()?;
    let stream_encoder = fs::read_to_string(profile_dir.join("streamEncoder.json")).ok();
    let service = fs::read_to_string(profile_dir.join("service.json")).ok();
    let fallback_name = profile_dir.file_name()?.to_string_lossy().into_owned();

    Some(parse_profile(
        &basic,
        stream_encoder.as_deref(),
        service.as_deref(),
        &fallback_name,
    ))
}

/// Pure parsing, separated from the filesystem so it can be tested.
pub fn parse_profile(
    basic_ini: &str,
    stream_encoder_json: Option<&str>,
    service_json: Option<&str>,
    fallback_name: &str,
) -> ObsConfig {
    let ini = Ini::parse(basic_ini);
    let encoder_settings: serde_json::Value = stream_encoder_json
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(serde_json::Value::Null);

    let mode = match ini.get("Output", "Mode") {
        Some(m) if m.eq_ignore_ascii_case("Advanced") => OutputMode::Advanced,
        _ => OutputMode::Simple,
    };

    let (encoder_id, bitrate_kbps, preset, rate_control) = match mode {
        OutputMode::Simple => (
            ini.get("SimpleOutput", "StreamEncoder").unwrap_or("x264").to_string(),
            ini.get_u32("SimpleOutput", "VBitrate").unwrap_or(DEFAULT_BITRATE_KBPS),
            ini.get("SimpleOutput", "Preset").map(str::to_string),
            None,
        ),
        OutputMode::Advanced => (
            ini.get("AdvOut", "Encoder").unwrap_or("obs_x264").to_string(),
            encoder_settings
                .get("bitrate")
                .and_then(|v| v.as_u64())
                .map(|b| b as u32)
                .unwrap_or(DEFAULT_BITRATE_KBPS),
            json_str(&encoder_settings, "preset"),
            Some(json_str(&encoder_settings, "rate_control").unwrap_or_else(|| "CBR".to_string())),
        ),
    };

    ObsConfig {
        profile: ini.get("General", "Name").unwrap_or(fallback_name).to_string(),
        service: service_json.and_then(service_name),
        mode,
        base: resolution(&ini, "BaseCX", "BaseCY"),
        output: resolution(&ini, "OutputCX", "OutputCY"),
        fps: fps(&ini),
        encoder: EncoderKind::from_id(&encoder_id),
        encoder_id,
        bitrate_kbps,
        preset,
        rate_control,
    }
}

fn json_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn resolution(ini: &Ini, x: &str, y: &str) -> Option<(u32, u32)> {
    match (ini.get_u32("Video", x)?, ini.get_u32("Video", y)?) {
        (0, _) | (_, 0) => None,
        size => Some(size),
    }
}

fn fps(ini: &Ini) -> f32 {
    match ini.get_u32("Video", "FPSType").unwrap_or(0) {
        1 => ini.get_u32("Video", "FPSInt").map(|f| f as f32),
        2 => match (ini.get_u32("Video", "FPSNum"), ini.get_u32("Video", "FPSDen")) {
            (Some(n), Some(d)) if d > 0 => Some(n as f32 / d as f32),
            _ => None,
        },
        // "Common" values are text such as "60", "59.94" or "29.97 NTSC".
        _ => ini
            .get("Video", "FPSCommon")
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok()),
    }
    .filter(|f: &f32| *f > 0.0)
    .unwrap_or(DEFAULT_FPS)
}

/// Picks only the platform name out of `service.json`. The stream key that
/// lives next to it is deliberately not kept anywhere.
fn service_name(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json.trim_start_matches('\u{feff}')).ok()?;
    let name = value.get("settings")?.get("service")?.as_str()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_mode_with_defaults() {
        let cfg = parse_profile(
            "[General]\nName=Sin Título\n[Video]\nBaseCX=1920\nBaseCY=1080\nOutputCX=1280\nOutputCY=720\n",
            None,
            None,
            "Sin_Titulo",
        );
        assert_eq!(cfg.profile, "Sin Título");
        assert_eq!(cfg.mode, OutputMode::Simple);
        assert_eq!(cfg.encoder, EncoderKind::Software);
        assert_eq!(cfg.bitrate_kbps, 2500);
        assert_eq!(cfg.output, Some((1280, 720)));
        assert_eq!(cfg.fps, 30.0);
        assert_eq!(cfg.service, None);
    }

    #[test]
    fn simple_mode_with_hardware_encoder() {
        let cfg = parse_profile(
            "[Output]\nMode=Simple\n[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=6000\n[Video]\nFPSCommon=60\n",
            None,
            Some(r#"{"type":"rtmp_common","settings":{"service":"Twitch","server":"auto","key":"live_secreto"}}"#),
            "p",
        );
        assert_eq!(cfg.encoder, EncoderKind::Nvidia);
        assert_eq!(cfg.bitrate_kbps, 6000);
        assert_eq!(cfg.fps, 60.0);
        assert_eq!(cfg.service.as_deref(), Some("Twitch"));
    }

    #[test]
    fn advanced_mode_reads_the_encoder_file() {
        let cfg = parse_profile(
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\n[Video]\nFPSType=2\nFPSNum=60000\nFPSDen=1001\n",
            Some(r#"{"bitrate":4500,"preset":"medium","rate_control":"VBR"}"#),
            None,
            "p",
        );
        assert_eq!(cfg.mode, OutputMode::Advanced);
        assert_eq!(cfg.encoder, EncoderKind::Software);
        assert_eq!(cfg.bitrate_kbps, 4500);
        assert_eq!(cfg.preset.as_deref(), Some("medium"));
        assert_eq!(cfg.rate_control.as_deref(), Some("VBR"));
        assert!((cfg.fps - 59.94).abs() < 0.01);
    }

    #[test]
    fn encoder_ids_are_classified() {
        assert_eq!(EncoderKind::from_id("obs_x264"), EncoderKind::Software);
        assert_eq!(EncoderKind::from_id("jim_nvenc"), EncoderKind::Nvidia);
        assert_eq!(EncoderKind::from_id("obs_nvenc_h264_tex"), EncoderKind::Nvidia);
        assert_eq!(EncoderKind::from_id("obs_qsv11_v2"), EncoderKind::Intel);
        assert_eq!(EncoderKind::from_id("h264_texture_amf"), EncoderKind::Amd);
        assert_eq!(EncoderKind::from_id("amd"), EncoderKind::Amd);
        assert_eq!(EncoderKind::from_id("algo_nuevo"), EncoderKind::Other);
    }

    #[test]
    fn the_stream_key_never_ends_up_in_the_config() {
        let cfg = parse_profile(
            "",
            None,
            Some(r#"{"settings":{"service":"YouTube - RTMPS","key":"abcd-1234-secreta"}}"#),
            "p",
        );
        assert!(!format!("{cfg:?}").contains("secreta"));
    }
}
