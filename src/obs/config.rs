//! Reads how OBS is set up, straight from its config files.
//!
//! Nothing is modified, and the stream key in `service.json` is never read
//! into our own data: only the name of the platform and whether it is a
//! custom server are kept. Server addresses are not kept either.
//!
//! OBS only writes a value to disk once it differs from its default, so every
//! missing value falls back to the default OBS 33 uses. The defaults come
//! from the OBS 33.0.0 source: `frontend/widgets/OBSBasic.cpp`
//! (`InitBasicConfigDefaults` and `InitBasicConfigDefaults2`) for
//! `basic.ini`, and each encoder plugin's `get_defaults` for the JSON files.

use std::fs;
use std::path::{Path, PathBuf};

use super::ObsPaths;
use crate::facts::Facts;
use crate::ini::Ini;
use crate::model::{Row, Section};
use crate::obs::logs::LogFacts;
use crate::platforms;
use crate::report::text;

/// `SimpleOutput/VBitrate` default (OBSBasic.cpp).
const DEFAULT_SIMPLE_BITRATE_KBPS: u32 = 6000;
/// `SimpleOutput/ABitrate` and `AdvOut/Track1Bitrate` default.
const DEFAULT_AUDIO_KBPS: u32 = 160;
/// `Video/FPSCommon` default.
const DEFAULT_FPS: f32 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncoderKind {
    /// x264 and friends: the processor does the work.
    #[default]
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

/// The encoder id OBS really uses for a name stored by the simple output
/// mode (`SimpleOutput/StreamEncoder`, `SimpleOutput/RecEncoder`). Same
/// table as `get_simple_output_encoder` in frontend/utility/SimpleOutput.cpp,
/// which OBS 28 to 33 share. NVENC maps to the OBS 31+ id; older versions
/// used `jim_nvenc`, which classifies the same way.
pub fn simple_encoder_id(name: &str) -> &'static str {
    match name {
        "qsv" => "obs_qsv11_v2",
        "qsv_av1" => "obs_qsv11_av1",
        "amd" => "h264_texture_amf",
        "amd_hevc" => "h265_texture_amf",
        "amd_av1" => "av1_texture_amf",
        "nvenc" => "obs_nvenc_h264_tex",
        "nvenc_hevc" => "obs_nvenc_hevc_tex",
        "nvenc_av1" => "obs_nvenc_av1_tex",
        "apple_h264" => "com.apple.videotoolbox.videoencoder.ave.avc",
        "apple_hevc" => "com.apple.videotoolbox.videoencoder.ave.hevc",
        // "x264", "x264_lowcpu" and anything unknown: OBS falls back to x264.
        _ => "obs_x264",
    }
}

/// A short name for an encoder id, as the streamer can find it in OBS's list.
pub fn encoder_name(id: &str) -> String {
    let lower = id.to_lowercase();
    let codec = if lower.contains("av1") {
        "AV1"
    } else if lower.contains("hevc") || lower.contains("h265") {
        "HEVC"
    } else {
        "H.264"
    };
    let family = match EncoderKind::from_id(id) {
        EncoderKind::Nvidia => "NVENC",
        EncoderKind::Intel => "QuickSync",
        EncoderKind::Amd => "AMD",
        EncoderKind::Software if lower.contains("x264") => return "x264".to_string(),
        EncoderKind::Software if lower.contains("svt") => return "SVT-AV1".to_string(),
        EncoderKind::Software if lower.contains("aom") => return "AOM AV1".to_string(),
        EncoderKind::Software if lower.contains("openh264") => return "OpenH264".to_string(),
        _ => return id.to_string(),
    };
    format!("{family} {codec}")
}

/// Default video bitrate of each encoder plugin in OBS 33, used in advanced
/// mode when `streamEncoder.json` has no bitrate.
fn default_encoder_bitrate(id: &str) -> u32 {
    match id {
        // obs-x264.c
        "obs_x264" => 6000,
        // obs-nvenc/nvenc-properties.c (OBS 31+ native NVENC)
        id if id.starts_with("obs_nvenc_") && (id.ends_with("_tex") || id.ends_with("_soft")) => 10000,
        // obs-nvenc/nvenc-compat.c: jim_nvenc, ffmpeg_nvenc and the *_cuda ids
        id if id.contains("nvenc") => 2500,
        // obs-qsv11.c
        id if id.contains("qsv") => 5000,
        // obs-ffmpeg: texture-amf.cpp, obs-ffmpeg-av1.c, obs-ffmpeg-openh264.c
        id if id.contains("amf") || id.contains("svt") || id.contains("aom") || id.contains("openh264") => 6000,
        _ => 2500,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Simple,
    Advanced,
}

/// How OBS shrinks the canvas to the size it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFilter {
    Point,
    Bilinear,
    Bicubic,
    Lanczos,
    Area,
}

impl ScaleFilter {
    /// `Video/ScaleType` in basic.ini (default "bicubic").
    fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "point" => Some(ScaleFilter::Point),
            "bilinear" => Some(ScaleFilter::Bilinear),
            "bicubic" => Some(ScaleFilter::Bicubic),
            "lanczos" => Some(ScaleFilter::Lanczos),
            "area" => Some(ScaleFilter::Area),
            _ => None,
        }
    }

    /// `AdvOut/RescaleFilter`, an `enum obs_scale_type` (libobs/obs.h);
    /// 0 means the encoder does not rescale.
    fn from_obs_scale_type(value: u32) -> Option<Self> {
        match value {
            1 => Some(ScaleFilter::Point),
            2 => Some(ScaleFilter::Bicubic),
            3 => Some(ScaleFilter::Bilinear),
            4 => Some(ScaleFilter::Lanczos),
            5 => Some(ScaleFilter::Area),
            _ => None,
        }
    }

    /// As OBS 33 shows it in Spanish.
    pub fn label(self) -> &'static str {
        match self {
            ScaleFilter::Point => "Punto",
            ScaleFilter::Bilinear => "Bilineal",
            ScaleFilter::Bicubic => "Bicúbico",
            ScaleFilter::Lanczos => "Lanczos",
            ScaleFilter::Area => "Área",
        }
    }
}

/// Encoder options that change how much work encoding takes. Each is `None`
/// when OBS's default depends on the hardware or the mode does not expose it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EncoderOptions {
    /// x264 "preset", NVENC "preset"/"preset2", QSV "target_usage", AMF "preset".
    pub preset: Option<String>,
    /// NVENC "tune" (hq, ll, ull).
    pub tuning: Option<String>,
    /// NVENC "multipass" (disabled, qres, fullres).
    pub multipass: Option<String>,
    pub lookahead: Option<bool>,
    /// NVENC "psycho_aq" (older) or "adaptive_quantization" (OBS 31+).
    pub psycho_visual: Option<bool>,
    pub bframes: Option<u32>,
}

impl EncoderOptions {
    /// Reads the options from an encoder's JSON settings, filling in the
    /// defaults of that encoder's plugin.
    fn from_json(id: &str, json: &serde_json::Value) -> Self {
        let kind = EncoderKind::from_id(id);
        let native_nvenc = id.starts_with("obs_nvenc_") && (id.ends_with("_tex") || id.ends_with("_soft"));
        let compat_nvenc = kind == EncoderKind::Nvidia && !native_nvenc;
        let preset = match kind {
            // obs-x264.c: "veryfast".
            EncoderKind::Software if id.contains("x264") => json_str(json, "preset").or(Some("veryfast".into())),
            // nvenc-properties.c uses "preset"; nvenc-compat.c "preset2"
            // (and before OBS 28, "preset" with names such as "hq" or "mq").
            EncoderKind::Nvidia => json_str(json, "preset2").or_else(|| json_str(json, "preset")).or(Some("p5".into())),
            // obs-qsv11.c: "TU4".
            EncoderKind::Intel => json_str(json, "target_usage").or(Some("TU4".into())),
            // texture-amf.cpp: "quality" for H.264; other codecs vary.
            EncoderKind::Amd => json_str(json, "preset").or_else(|| id.contains("h264").then(|| "quality".into())),
            _ => json_str(json, "preset"),
        };
        if kind != EncoderKind::Nvidia {
            return Self { preset, bframes: json_u32(json, "bf"), ..Self::default() };
        }
        Self {
            preset,
            tuning: json_str(json, "tune").or(Some("hq".into())),
            multipass: json_str(json, "multipass").or(Some("qres".into())),
            // The native encoder's default depends on the card; the compat
            // one leaves it off.
            lookahead: json_bool(json, "lookahead").or(compat_nvenc.then_some(false)),
            psycho_visual: json_bool(json, "adaptive_quantization")
                .or_else(|| json_bool(json, "psycho_aq"))
                .or(Some(true)),
            bframes: json_u32(json, "bf"),
        }
    }
}

/// What happens with recordings.
#[derive(Debug, Clone, PartialEq)]
pub enum Recording {
    /// The recording reuses the stream's encoder: no extra work.
    SameAsStream,
    /// A second encoder prepares the recording.
    Separate { encoder_id: String, encoder: EncoderKind, preset: Option<String> },
    /// "Sin pérdida": an enormous file encoded by the processor.
    Lossless,
    /// Advanced mode with a custom FFmpeg output: we do not judge it.
    Custom,
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

    // ---- added in v0.2 ----
    /// True when simple mode has no encoder written down. OBS then picks
    /// NVENC when it is available and x264 otherwise.
    pub encoder_is_default: bool,
    /// Expensive encoder options, with OBS's defaults filled in.
    pub options: EncoderOptions,
    /// `service.json` points at a server typed by hand instead of a platform
    /// from OBS's list. The address itself is never kept.
    pub custom_server: bool,
    /// "Ignorar las recomendaciones de configuración del servicio de
    /// streaming" (`Stream1/IgnoreRecommended`). Off by default.
    pub ignore_platform_limits: bool,
    /// Advanced mode lets the streamer stop OBS from applying the platform's
    /// keyframe and bitrate settings (`AdvOut/ApplyServiceSettings`).
    /// Always true in simple mode.
    pub apply_service_settings: bool,
    /// Keyframe interval in seconds, 0 meaning "auto". Only advanced mode
    /// lets the streamer set it.
    pub keyint_sec: Option<u32>,
    /// `Video/ScaleType`: how the canvas is shrunk to the output size.
    pub scale_filter: ScaleFilter,
    /// Advanced mode can shrink the picture again in the encoder
    /// ("Cambiar la escala de salida"): size and filter.
    pub rescale: Option<((u32, u32), ScaleFilter)>,
    pub audio_kbps: u32,
    pub recording: Recording,
}

impl ObsConfig {
    /// The size that actually goes to the platform.
    pub fn stream_size(&self) -> Option<(u32, u32)> {
        self.rescale.map(|(size, _)| size).or(self.output)
    }

    /// The platform from the doctor's table, when OBS names one.
    pub fn platform(&self) -> Option<&'static platforms::Platform> {
        if self.custom_server {
            return None;
        }
        platforms::find(self.service.as_deref())
    }

    /// Whether OBS itself holds the bitrate and keyframes to the platform's
    /// limits (rtmp-common.c `apply_video_encoder_settings`, called from
    /// SimpleOutput.cpp and AdvancedOutput.cpp).
    pub fn obs_enforces_platform_limits(&self) -> bool {
        self.platform().is_some_and(|p| p.listed_in_obs) && self.apply_service_settings
    }

    /// The highest video bitrate this platform takes for this picture.
    pub fn platform_max_kbps(&self) -> Option<u32> {
        self.platform()?.max_video_kbps_for(self.stream_size(), self.fps)
    }

    /// The video bitrate that really leaves OBS: OBS lowers it to the
    /// platform's limit unless told to ignore it.
    pub fn effective_bitrate_kbps(&self) -> u32 {
        match self.platform_max_kbps() {
            Some(max) if self.obs_enforces_platform_limits() && !self.ignore_platform_limits => {
                self.bitrate_kbps.min(max)
            }
            _ => self.bitrate_kbps,
        }
    }

    /// The encoder kind, resolving simple mode's default with what OBS said
    /// it could use on this PC.
    pub fn effective_encoder(&self, logs: &LogFacts) -> EncoderKind {
        if self.encoder_is_default && logs.can_encode_with(EncoderKind::Nvidia) == Some(true) {
            EncoderKind::Nvidia
        } else {
            self.encoder
        }
    }
}

// There is exactly one of these per run, so boxing the big variant would
// only make every match on it noisier.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum ObsStatus {
    /// No config folder: OBS is not installed or was never opened.
    NotFound {
        looked_in: PathBuf,
    },
    /// The folder exists but there is no usable profile inside.
    NoProfile {
        dir: PathBuf,
    },
    Found(ObsConfig),
}

impl Default for ObsStatus {
    fn default() -> Self {
        ObsStatus::NotFound { looked_in: PathBuf::new() }
    }
}

pub fn read(paths: &ObsPaths) -> ObsStatus {
    if !paths.exists {
        return ObsStatus::NotFound { looked_in: paths.config_dir.clone() };
    }
    match paths.profile_dir.as_deref().and_then(read_profile) {
        Some(config) => ObsStatus::Found(config),
        None => ObsStatus::NoProfile { dir: paths.config_dir.clone() },
    }
}

/// The "Tu OBS" block of the report.
pub fn sections(facts: &Facts) -> Vec<Section> {
    let mut section = Section::new("obs", "Tu OBS");
    match &facts.obs {
        ObsStatus::NotFound { looked_in } => {
            section = section.intro(
                "No encontré la configuración de OBS. Si ya lo instalaste, ábrelo una vez y vuelve a ejecutar el doctor. \
                 Mientras tanto, más abajo te dejo con qué configuración empezar.",
            );
            if !looked_in.as_os_str().is_empty() {
                section.push(Row::new("Busqué en", looked_in.display().to_string()));
            }
        }
        ObsStatus::NoProfile { dir } => {
            section = section.intro(
                "Encontré OBS, pero todavía no tiene ningún perfil guardado. Ábrelo una vez y vuelve a ejecutar el doctor.",
            );
            section.push(Row::new("Busqué en", dir.display().to_string()));
        }
        ObsStatus::Found(config) => {
            section.push(Row::new("Perfil", config.profile.clone()));
            section.push(Row::new("Transmite a", destination_text(config)));
            section.push(picture_row(config));
            section.push(bitrate_row(config));
            section.push(Row::new("Quién prepara el video", encoder_text(config, &facts.logs)));
            if let Some(keyint) = config.keyint_sec.filter(|_| !config.obs_enforces_platform_limits()) {
                let value = if keyint == 0 { "automático".to_string() } else { format!("cada {keyint} s") };
                section.push(Row::new("Fotogramas clave", value));
            }
            section.push(Row::new("Audio", format!("{} kbps", config.audio_kbps)));
            section.push(Row::new("Grabación", recording_text(&config.recording)));
            section.push(Row::new(
                "Modo",
                match config.mode {
                    OutputMode::Simple => "Sencillo",
                    OutputMode::Advanced => "Avanzado",
                },
            ));
        }
    }
    vec![section]
}

fn destination_text(config: &ObsConfig) -> String {
    if config.custom_server {
        return "Un servidor propio (no muestro su dirección)".to_string();
    }
    match (config.platform(), config.service.as_deref()) {
        (Some(p), _) => p.name.to_string(),
        (None, Some(name)) => name.to_string(),
        (None, None) => "Sin plataforma elegida".to_string(),
    }
}

fn picture_row(config: &ObsConfig) -> Row {
    let size = config.stream_size();
    let picture = match size {
        Some((w, h)) => format!("{w}x{h} a {} fps", text::fps(config.fps)),
        None => format!("tamaño sin definir, a {} fps", text::fps(config.fps)),
    };
    let mut row = Row::new("Imagen", picture);
    if let (Some(base), Some(out)) = (config.base, size)
        && base != out
    {
        let filter = config.rescale.map(|(_, f)| f).unwrap_or(config.scale_filter);
        row = row.note(format!(
            "Tu lienzo en OBS es de {}x{}; se achica antes de enviarlo (filtro {}).",
            base.0,
            base.1,
            filter.label()
        ));
    }
    row
}

fn bitrate_row(config: &ObsConfig) -> Row {
    let row = Row::new("Calidad de envío", format!("{} kbps", config.bitrate_kbps));
    let effective = config.effective_bitrate_kbps();
    match config.platform() {
        Some(p) if effective < config.bitrate_kbps => row
            .note(format!("OBS la baja a {effective} kbps al transmitir, porque es lo máximo que acepta {}.", p.name)),
        _ => row,
    }
}

fn encoder_text(config: &ObsConfig, logs: &LogFacts) -> String {
    let kind = config.effective_encoder(logs);
    let who = match kind {
        EncoderKind::Software => "el procesador",
        EncoderKind::Nvidia => "tu placa NVIDIA",
        EncoderKind::Amd => "tu placa AMD",
        EncoderKind::Intel => "tu gráfica Intel",
        EncoderKind::Other => "un codificador que no reconozco",
    };
    if kind != config.encoder {
        return format!("{who} (NVENC, el que OBS elige si no cambiaste nada)");
    }
    format!("{who} ({})", encoder_name(&config.encoder_id))
}

fn recording_text(recording: &Recording) -> String {
    match recording {
        Recording::SameAsStream => "usa la misma preparación que el directo".to_string(),
        Recording::Separate { encoder_id, .. } => format!("con un codificador aparte: {}", encoder_name(encoder_id)),
        Recording::Lossless => "sin pérdida (archivos enormes, mucho trabajo para el procesador)".to_string(),
        Recording::Custom => "salida personalizada (FFmpeg)".to_string(),
    }
}

/// The raw text of each profile file. `None` when the file is missing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProfileFiles<'a> {
    pub basic_ini: &'a str,
    pub stream_encoder_json: Option<&'a str>,
    pub record_encoder_json: Option<&'a str>,
    pub service_json: Option<&'a str>,
}

fn read_profile(profile_dir: &Path) -> Option<ObsConfig> {
    let basic = fs::read_to_string(profile_dir.join("basic.ini")).ok()?;
    let stream_encoder = fs::read_to_string(profile_dir.join("streamEncoder.json")).ok();
    let record_encoder = fs::read_to_string(profile_dir.join("recordEncoder.json")).ok();
    let service = fs::read_to_string(profile_dir.join("service.json")).ok();
    let fallback_name = profile_dir.file_name()?.to_string_lossy().into_owned();

    let files = ProfileFiles {
        basic_ini: &basic,
        stream_encoder_json: stream_encoder.as_deref(),
        record_encoder_json: record_encoder.as_deref(),
        service_json: service.as_deref(),
    };
    Some(parse(&files, &fallback_name))
}

/// Shorthand for tests that only need the three main files.
#[cfg(test)]
pub fn parse_profile(
    basic_ini: &str,
    stream_encoder_json: Option<&str>,
    service_json: Option<&str>,
    fallback_name: &str,
) -> ObsConfig {
    let files = ProfileFiles { basic_ini, stream_encoder_json, record_encoder_json: None, service_json };
    parse(&files, fallback_name)
}

/// Pure parsing, separated from the filesystem so it can be tested.
pub fn parse(files: &ProfileFiles, fallback_name: &str) -> ObsConfig {
    let ini = Ini::parse(files.basic_ini);
    let stream_json = parse_json(files.stream_encoder_json);
    let record_json = parse_json(files.record_encoder_json);
    let service = files.service_json.map(service_info).unwrap_or_default();

    let mode = match ini.get("Output", "Mode") {
        Some(m) if m.eq_ignore_ascii_case("Advanced") => OutputMode::Advanced,
        _ => OutputMode::Simple,
    };

    let simple_name = ini.get("SimpleOutput", "StreamEncoder").filter(|s| !s.is_empty());
    let encoder_id = match mode {
        OutputMode::Simple => simple_encoder_id(simple_name.unwrap_or("x264")).to_string(),
        OutputMode::Advanced => {
            ini.get("AdvOut", "Encoder").filter(|s| !s.is_empty()).unwrap_or("obs_x264").to_string()
        }
    };

    let (bitrate_kbps, options, rate_control, keyint_sec, audio_kbps) = match mode {
        OutputMode::Simple => (
            ini.get_u32("SimpleOutput", "VBitrate").unwrap_or(DEFAULT_SIMPLE_BITRATE_KBPS),
            simple_options(&ini, simple_name.unwrap_or("x264")),
            None,
            None,
            ini.get_u32("SimpleOutput", "ABitrate").unwrap_or(DEFAULT_AUDIO_KBPS),
        ),
        OutputMode::Advanced => {
            let track = ini.get_u32("AdvOut", "TrackIndex").filter(|t| (1..=6).contains(t)).unwrap_or(1);
            (
                json_u32(&stream_json, "bitrate").unwrap_or_else(|| default_encoder_bitrate(&encoder_id)),
                EncoderOptions::from_json(&encoder_id, &stream_json),
                Some(json_str(&stream_json, "rate_control").unwrap_or_else(|| "CBR".to_string())),
                Some(json_u32(&stream_json, "keyint_sec").unwrap_or(0)),
                ini.get_u32("AdvOut", &format!("Track{track}Bitrate")).unwrap_or(DEFAULT_AUDIO_KBPS),
            )
        }
    };

    let recording = match mode {
        OutputMode::Simple => simple_recording(&ini),
        OutputMode::Advanced => advanced_recording(&ini, &record_json),
    };

    ObsConfig {
        profile: ini.get("General", "Name").unwrap_or(fallback_name).to_string(),
        service: service.name,
        mode,
        base: resolution(&ini, "BaseCX", "BaseCY"),
        output: resolution(&ini, "OutputCX", "OutputCY"),
        fps: fps(&ini),
        encoder: EncoderKind::from_id(&encoder_id),
        encoder_is_default: mode == OutputMode::Simple && simple_name.is_none(),
        encoder_id,
        bitrate_kbps,
        preset: options.preset.clone(),
        rate_control,
        options,
        custom_server: service.custom,
        ignore_platform_limits: ini_bool(&ini, "Stream1", "IgnoreRecommended").unwrap_or(false),
        apply_service_settings: mode == OutputMode::Simple
            || ini_bool(&ini, "AdvOut", "ApplyServiceSettings").unwrap_or(true),
        keyint_sec,
        scale_filter: ini.get("Video", "ScaleType").and_then(ScaleFilter::from_name).unwrap_or(ScaleFilter::Bicubic),
        rescale: if mode == OutputMode::Advanced { advanced_rescale(&ini) } else { None },
        audio_kbps,
        recording,
    }
}

/// Simple mode stores one preset per encoder family (SimpleOutput.cpp,
/// `SimpleOutput::Update`). Only x264 and NVENC have a default.
fn simple_options(ini: &Ini, simple_name: &str) -> EncoderOptions {
    let key = match simple_name {
        "qsv" | "qsv_av1" => "QSVPreset",
        "amd" | "amd_hevc" => "AMDPreset",
        "amd_av1" => "AMDAV1Preset",
        "nvenc" | "nvenc_hevc" | "nvenc_av1" => "NVENCPreset2",
        _ => "Preset",
    };
    let default = match key {
        "Preset" => Some("veryfast"),
        "NVENCPreset2" => Some("p5"),
        _ => None,
    };
    let preset = ini.get("SimpleOutput", key).filter(|s| !s.is_empty()).or(default).map(str::to_string);
    EncoderOptions { preset, ..EncoderOptions::default() }
}

/// `SimpleOutput/RecQuality`: "Stream" (default), "Small", "HQ" or "Lossless".
fn simple_recording(ini: &Ini) -> Recording {
    match ini.get("SimpleOutput", "RecQuality").unwrap_or("Stream") {
        "Lossless" => Recording::Lossless,
        "Small" | "HQ" => {
            // Its default follows the stream encoder's (OBSBasic.cpp).
            let name = ini
                .get("SimpleOutput", "RecEncoder")
                .or_else(|| ini.get("SimpleOutput", "StreamEncoder"))
                .unwrap_or("x264");
            let encoder_id = simple_encoder_id(name).to_string();
            Recording::Separate { encoder: EncoderKind::from_id(&encoder_id), encoder_id, preset: None }
        }
        _ => Recording::SameAsStream,
    }
}

/// `AdvOut/RecType` "Standard" with `AdvOut/RecEncoder` "none" (default)
/// reuses the stream's encoder.
fn advanced_recording(ini: &Ini, record_json: &serde_json::Value) -> Recording {
    if ini.get("AdvOut", "RecType").is_some_and(|t| t.eq_ignore_ascii_case("FFmpeg")) {
        return Recording::Custom;
    }
    match ini.get("AdvOut", "RecEncoder").filter(|e| !e.is_empty() && *e != "none") {
        None => Recording::SameAsStream,
        Some(id) => Recording::Separate {
            encoder: EncoderKind::from_id(id),
            preset: EncoderOptions::from_json(id, record_json).preset,
            encoder_id: id.to_string(),
        },
    }
}

/// "Cambiar la escala de salida" in advanced mode. OBS 33 stores the filter
/// in `AdvOut/RescaleFilter` (0 = off) and the size in `AdvOut/RescaleRes`;
/// profiles from before the GPU rescaler only have `AdvOut/Rescale=true`,
/// which OBS migrates to bilinear (OBSBasic.cpp).
fn advanced_rescale(ini: &Ini) -> Option<((u32, u32), ScaleFilter)> {
    let filter = match ini.get_u32("AdvOut", "RescaleFilter") {
        Some(value) => ScaleFilter::from_obs_scale_type(value)?,
        None if ini_bool(ini, "AdvOut", "Rescale") == Some(true) => ScaleFilter::Bilinear,
        None => return None,
    };
    let (w, h) = ini.get("AdvOut", "RescaleRes")?.split_once('x')?;
    let size = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some((size, filter))
}

fn parse_json(text: Option<&str>) -> serde_json::Value {
    text.and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or(serde_json::Value::Null)
}

fn json_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn json_u32(value: &serde_json::Value, key: &str) -> Option<u32> {
    value.get(key)?.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key)?.as_bool()
}

fn ini_bool(ini: &Ini, section: &str, key: &str) -> Option<bool> {
    match ini.get(section, key)?.to_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
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
        _ => ini.get("Video", "FPSCommon").and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok()),
    }
    .filter(|f: &f32| f.is_finite() && *f > 0.0)
    .unwrap_or(DEFAULT_FPS)
}

/// What we keep from `service.json`.
#[derive(Debug, Default, PartialEq)]
struct ServiceInfo {
    name: Option<String>,
    custom: bool,
}

/// Picks only the platform name and the kind of service out of
/// `service.json`. The stream key and the server address that live next to
/// them are deliberately not kept anywhere.
fn service_info(json: &str) -> ServiceInfo {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}')) else {
        return ServiceInfo::default();
    };
    // "rtmp_common" is a platform from OBS's list; "rtmp_custom" and
    // "whip_custom" are servers typed by hand.
    let custom = value.get("type").and_then(|t| t.as_str()).is_some_and(|t| t.contains("custom"));
    let name = if custom {
        None
    } else {
        value
            .get("settings")
            .and_then(|s| s.get("service"))
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    ServiceInfo { name, custom }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_DEFAULTS: &str = include_str!("../../tests/fixtures/config/simple_defaults.ini");
    const ADVANCED_NVENC: &str = include_str!("../../tests/fixtures/config/advanced_nvenc.ini");
    const ADVANCED_NVENC_STREAM: &str = include_str!("../../tests/fixtures/config/advanced_nvenc_stream.json");
    const ADVANCED_NVENC_RECORD: &str = include_str!("../../tests/fixtures/config/advanced_nvenc_record.json");
    const OLD_OBS_28: &str = include_str!("../../tests/fixtures/config/old_obs28.ini");
    const OLD_OBS_28_STREAM: &str = include_str!("../../tests/fixtures/config/old_obs28_stream.json");
    const SERVICE_TWITCH: &str = include_str!("../../tests/fixtures/config/service_twitch.json");
    const SERVICE_CUSTOM: &str = include_str!("../../tests/fixtures/config/service_custom.json");
    const GARBAGE: &str = include_str!("../../tests/fixtures/config/garbage.ini");

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
        assert!(cfg.encoder_is_default);
        assert_eq!(cfg.bitrate_kbps, 6000);
        assert_eq!(cfg.audio_kbps, 160);
        assert_eq!(cfg.output, Some((1280, 720)));
        assert_eq!(cfg.fps, 30.0);
        assert_eq!(cfg.service, None);
        assert_eq!(cfg.preset.as_deref(), Some("veryfast"));
        assert_eq!(cfg.recording, Recording::SameAsStream);
        assert_eq!(cfg.scale_filter, ScaleFilter::Bicubic);
        assert!(cfg.apply_service_settings && !cfg.ignore_platform_limits);
        assert_eq!(cfg.keyint_sec, None);
    }

    #[test]
    fn a_profile_as_obs_33_writes_it() {
        let cfg = parse_profile(SIMPLE_DEFAULTS, None, None, "p");
        assert_eq!(cfg.encoder_id, "obs_x264");
        assert!(cfg.encoder_is_default);
        assert_eq!((cfg.bitrate_kbps, cfg.audio_kbps), (6000, 160));
        assert_eq!((cfg.base, cfg.output), (Some((1366, 768)), Some((1092, 614))));
        assert_eq!(cfg.preset.as_deref(), Some("veryfast"));
        assert_eq!(cfg.recording, Recording::SameAsStream);
    }

    #[test]
    fn simple_mode_with_hardware_encoder() {
        let cfg = parse_profile(
            "[Output]\nMode=Simple\n[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=6000\nNVENCPreset2=p7\n[Video]\nFPSCommon=60\n",
            None,
            Some(SERVICE_TWITCH),
            "p",
        );
        assert_eq!(cfg.encoder, EncoderKind::Nvidia);
        assert_eq!(cfg.encoder_id, "obs_nvenc_h264_tex");
        assert!(!cfg.encoder_is_default);
        assert_eq!(cfg.preset.as_deref(), Some("p7"));
        assert_eq!(cfg.bitrate_kbps, 6000);
        assert_eq!(cfg.fps, 60.0);
        assert_eq!(cfg.service.as_deref(), Some("Twitch"));
        assert_eq!(cfg.platform().map(|p| p.name), Some("Twitch"));
    }

    #[test]
    fn simple_mode_names_translate_to_obs_ids() {
        assert_eq!(simple_encoder_id("x264_lowcpu"), "obs_x264");
        assert_eq!(simple_encoder_id("qsv"), "obs_qsv11_v2");
        assert_eq!(simple_encoder_id("amd"), "h264_texture_amf");
        assert_eq!(simple_encoder_id("nvenc_av1"), "obs_nvenc_av1_tex");
        assert_eq!(simple_encoder_id("algo_raro"), "obs_x264");
        assert_eq!(EncoderKind::from_id(simple_encoder_id("qsv_av1")), EncoderKind::Intel);
    }

    #[test]
    fn advanced_mode_reads_the_encoder_file() {
        let cfg = parse_profile(
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\n[Video]\nFPSType=2\nFPSNum=60000\nFPSDen=1001\n",
            Some(r#"{"bitrate":4500,"preset":"medium","rate_control":"VBR","keyint_sec":2}"#),
            None,
            "p",
        );
        assert_eq!(cfg.mode, OutputMode::Advanced);
        assert_eq!(cfg.encoder, EncoderKind::Software);
        assert_eq!(cfg.bitrate_kbps, 4500);
        assert_eq!(cfg.preset.as_deref(), Some("medium"));
        assert_eq!(cfg.rate_control.as_deref(), Some("VBR"));
        assert_eq!(cfg.keyint_sec, Some(2));
        assert!((cfg.fps - 59.94).abs() < 0.01);
    }

    #[test]
    fn advanced_nvenc_with_every_option() {
        let files = ProfileFiles {
            basic_ini: ADVANCED_NVENC,
            stream_encoder_json: Some(ADVANCED_NVENC_STREAM),
            record_encoder_json: Some(ADVANCED_NVENC_RECORD),
            service_json: Some(SERVICE_TWITCH),
        };
        let cfg = parse(&files, "p");
        assert_eq!(cfg.encoder_id, "obs_nvenc_h264_tex");
        assert_eq!(cfg.bitrate_kbps, 8000);
        let o = &cfg.options;
        assert_eq!(o.preset.as_deref(), Some("p7"));
        assert_eq!(o.tuning.as_deref(), Some("hq"));
        assert_eq!(o.multipass.as_deref(), Some("fullres"));
        assert_eq!(o.lookahead, Some(true));
        assert_eq!(o.psycho_visual, Some(true));
        assert_eq!(o.bframes, Some(4));
        assert_eq!(cfg.keyint_sec, Some(0));
        assert!(!cfg.apply_service_settings);
        assert_eq!(cfg.audio_kbps, 320, "track 2 is the stream's track");
        assert_eq!(cfg.rescale, Some(((1280, 720), ScaleFilter::Lanczos)));
        assert_eq!(cfg.stream_size(), Some((1280, 720)));
        assert_eq!(cfg.scale_filter, ScaleFilter::Lanczos);
        assert_eq!(
            cfg.recording,
            Recording::Separate {
                encoder_id: "obs_x264".into(),
                encoder: EncoderKind::Software,
                preset: Some("slow".into())
            }
        );
    }

    #[test]
    fn an_obs_28_profile_still_reads() {
        let cfg = parse_profile(OLD_OBS_28, Some(OLD_OBS_28_STREAM), None, "p");
        assert_eq!(cfg.encoder, EncoderKind::Nvidia);
        assert_eq!(cfg.encoder_id, "jim_nvenc");
        assert_eq!(cfg.preset.as_deref(), Some("p6"), "preset2 wins over the old preset");
        assert_eq!(cfg.options.lookahead, Some(false));
        assert_eq!(cfg.bitrate_kbps, 2500, "nvenc-compat default");
        // The old "Rescale" checkbox becomes bilinear, as OBS migrates it.
        assert_eq!(cfg.rescale, Some(((1280, 720), ScaleFilter::Bilinear)));
    }

    #[test]
    fn encoder_defaults_follow_each_plugin() {
        let advanced =
            |id: &str| parse_profile(&format!("[Output]\nMode=Advanced\n[AdvOut]\nEncoder={id}\n"), None, None, "p");
        assert_eq!(advanced("obs_x264").bitrate_kbps, 6000);
        assert_eq!(advanced("obs_nvenc_h264_tex").bitrate_kbps, 10000);
        assert_eq!(advanced("ffmpeg_nvenc").bitrate_kbps, 2500);
        assert_eq!(advanced("obs_qsv11_v2").bitrate_kbps, 5000);
        assert_eq!(advanced("obs_qsv11_v2").preset.as_deref(), Some("TU4"));
        assert_eq!(advanced("h264_texture_amf").preset.as_deref(), Some("quality"));
        assert_eq!(advanced("obs_nvenc_h264_tex").options.lookahead, None, "depends on the card");
    }

    #[test]
    fn simple_recording_qualities() {
        let rec = |q: &str| parse_profile(&format!("[SimpleOutput]\nRecQuality={q}\n"), None, None, "p").recording;
        assert_eq!(rec("Stream"), Recording::SameAsStream);
        assert_eq!(rec("Lossless"), Recording::Lossless);
        assert!(matches!(rec("HQ"), Recording::Separate { encoder: EncoderKind::Software, .. }));
        let nvenc = parse_profile("[SimpleOutput]\nStreamEncoder=nvenc\nRecQuality=Small\n", None, None, "p").recording;
        assert!(matches!(nvenc, Recording::Separate { encoder: EncoderKind::Nvidia, .. }));
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
        assert_eq!(encoder_name("obs_nvenc_hevc_tex"), "NVENC HEVC");
        assert_eq!(encoder_name("obs_qsv11_av1"), "QuickSync AV1");
        assert_eq!(encoder_name("obs_x264"), "x264");
        assert_eq!(encoder_name("algo_nuevo"), "algo_nuevo");
    }

    #[test]
    fn platform_limits_are_applied_like_obs() {
        let twitch = |extra: &str| {
            parse_profile(&format!("[SimpleOutput]\nVBitrate=9000\n{extra}"), None, Some(SERVICE_TWITCH), "p")
        };
        assert_eq!(twitch("").effective_bitrate_kbps(), 6000);
        assert_eq!(twitch("[Stream1]\nIgnoreRecommended=true\n").effective_bitrate_kbps(), 9000);
        let custom = parse_profile("[SimpleOutput]\nVBitrate=9000\n", None, Some(SERVICE_CUSTOM), "p");
        assert!(custom.custom_server);
        assert_eq!(custom.platform(), None);
        assert_eq!(custom.effective_bitrate_kbps(), 9000);
    }

    #[test]
    fn broken_files_fall_back_to_defaults() {
        for basic in ["", GARBAGE, "\u{feff}", "[Video]\nOutputCX=0\nOutputCY=abc\nFPSType=2\nFPSDen=0\n"] {
            let cfg = parse_profile(basic, Some("{no es json"), Some("[]"), "p");
            assert_eq!(cfg.mode, OutputMode::Simple);
            assert_eq!(cfg.encoder_id, "obs_x264");
            assert_eq!(cfg.output, None);
            assert_eq!(cfg.fps, 30.0);
            assert_eq!(cfg.service, None);
            assert!(!cfg.custom_server);
        }
        let advanced = parse_profile(
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=\nRescaleFilter=4\nRescaleRes=nada\nTrackIndex=9\n",
            Some(r#"{"bitrate":-5,"keyint_sec":"dos","lookahead":"si"}"#),
            None,
            "p",
        );
        assert_eq!(advanced.encoder_id, "obs_x264");
        assert_eq!(advanced.bitrate_kbps, 6000);
        assert_eq!(advanced.keyint_sec, Some(0));
        assert_eq!(advanced.rescale, None);
        assert_eq!(advanced.audio_kbps, 160);
    }

    #[test]
    fn the_stream_key_never_ends_up_in_the_config() {
        for service in [SERVICE_TWITCH, SERVICE_CUSTOM] {
            let cfg = parse_profile("", None, Some(service), "p");
            let dump = format!("{cfg:?}");
            assert!(!dump.contains("SECRETO"), "{dump}");
            assert!(!dump.contains("example.invalid"), "{dump}");
        }
    }
}
