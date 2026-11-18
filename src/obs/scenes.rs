//! The scene collection: which sources and filters the streamer has set up,
//! and which of them make the PC work hardest while live.
//!
//! Field names come from OBS 33's own save code (`libobs/obs.c`,
//! `obs_save_source`; `libobs/obs-scene.c`, `scene_save_item`) and from each
//! plugin's settings keys: `plugins/win-capture`, `plugins/win-dshow/win-dshow.cpp`,
//! `plugins/obs-ffmpeg/obs-ffmpeg-source.c`, `plugins/nv-filters` and
//! obs-browser's `obs-browser-plugin.cpp` (`browser_source_get_defaults`,
//! <https://github.com/obsproject/obs-browser>).
//!
//! OBS only writes settings that differ from their default, so every missing
//! key below falls back to the plugin's own default.
//!
//! Privacy: sources keep web addresses and file paths in their settings. We
//! read only the numbers and switches the rules need and never keep a text
//! value from `settings`. The one thing taken from a path is whether a media
//! file is a video, judged by its extension and then dropped. Names the
//! streamer gave to scenes, sources and filters are kept (advice quotes them),
//! except when a name itself looks like an address or a path.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use super::ObsPaths;
use super::logs::NOTICEABLE_LOSS_PCT;
use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review, Row, Section, Tone};

/// A scene collection bigger than this is not something OBS wrote; reading
/// it would only waste memory.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Browser sources drawing a page larger than Full HD. obs-browser lets the
/// page go up to 8192 pixels a side and draws every pixel of it, even when
/// the source is scaled down on screen.
const MAX_BROWSER_WIDTH: u32 = 1920;
const MAX_BROWSER_HEIGHT: u32 = 1080;

/// Browser sources that stay open all the time before we suggest trimming.
/// There is no official number: each one is a separate Chromium page with
/// its own memory and timers. This is our own threshold, so the advice is
/// worded as something to try and never rated Important.
const MANY_BROWSERS: usize = 5;

/// A webcam or capture card set by hand above Full HD (`res_type` =
/// `ResType_Custom`). Above 1080 lines the frames are usually compressed
/// (MJPEG) and decoding them is real work for the processor.
const MAX_CAMERA_HEIGHT: u32 = 1080;

/// Nested scenes and groups deeper than this are almost certainly a loop.
const MAX_NESTING: usize = 16;

/// Extensions OBS's media source plays as video.
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "webm", "avi", "flv", "ts", "m4v", "wmv", "mpg", "mpeg"];

/// Whether the collection could be read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SceneStatus {
    /// No scene collection file (OBS missing, never opened, or `--obs-dir`
    /// pointing elsewhere).
    #[default]
    NotFound,
    /// The file is there but is not a scene collection we understand.
    Unreadable,
    Read,
}

/// What the scene collection holds. `Default` means "nothing read".
#[derive(Debug, Clone, Default)]
pub struct SceneFacts {
    pub status: SceneStatus,
    /// Scene names in the order of OBS's Escenas panel.
    pub scenes: Vec<String>,
    /// The scene OBS had on screen (the program scene) when it last saved.
    pub current_scene: Option<String>,
    /// Every source except scenes and groups, in file order.
    pub sources: Vec<SourceInfo>,
}

/// The kinds of source that matter for the rules. Audio and anything we do
/// not single out are grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// `browser_source`, "Navegador".
    Browser,
    /// `dshow_input`, "Dispositivo de captura de video": webcams and capture cards.
    Camera,
    /// `game_capture`, "Captura de juego".
    GameCapture,
    /// `window_capture`, "Captura de ventana".
    WindowCapture,
    /// `monitor_capture`, "Captura de pantalla".
    DisplayCapture,
    /// `ffmpeg_source` and `vlc_source`.
    Media,
    Image,
    Text,
    Audio,
    Other,
}

impl SourceKind {
    fn from_id(id: &str) -> Self {
        match id {
            "browser_source" => Self::Browser,
            "dshow_input" => Self::Camera,
            "game_capture" => Self::GameCapture,
            "window_capture" => Self::WindowCapture,
            "monitor_capture" => Self::DisplayCapture,
            "ffmpeg_source" | "vlc_source" => Self::Media,
            "image_source" | "slideshow" | "slideshow_v2" => Self::Image,
            "text_gdiplus" | "text_ft2_source" => Self::Text,
            "wasapi_input_capture" | "wasapi_output_capture" | "wasapi_process_output_capture" => Self::Audio,
            _ => Self::Other,
        }
    }

    /// Plain label for the report, with OBS's own name in parentheses where
    /// the streamer would look for it.
    fn label(self) -> &'static str {
        match self {
            Self::Browser => "Páginas web (Navegador)",
            Self::Camera => "Cámaras y capturadoras",
            Self::GameCapture => "Captura de juego",
            Self::WindowCapture => "Captura de ventana",
            Self::DisplayCapture => "Captura de pantalla",
            Self::Media => "Videos y sonidos",
            Self::Image => "Imágenes",
            Self::Text => "Textos",
            Self::Audio => "Audio",
            Self::Other => "Otras",
        }
    }
}

/// One source, reduced to what the rules use.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    pub name: String,
    pub kind: SourceKind,
    /// False when the whole source was turned off (not just hidden).
    pub enabled: bool,
    /// First scene, in panel order, whose Fuentes list shows it (directly or
    /// inside a group). `None` when no scene uses it.
    pub scene: Option<String>,
    /// Its eye is open in at least one scene or group.
    pub visible_somewhere: bool,
    /// Visible in the current scene, also through nested scenes and groups.
    pub in_current_scene: bool,
    pub filters: Vec<FilterInfo>,
    /// Browser: size of the page it draws, in pixels.
    pub page_size: Option<(u32, u32)>,
    /// Browser: "Apagar fuente cuando no sea visible".
    pub shutdown_when_hidden: bool,
    /// Camera: resolution picked by hand under "Personalizado".
    pub custom_resolution: Option<(u32, u32)>,
    /// Media: plays a video file from this PC (not a sound, not a stream).
    pub local_video: bool,
    pub looping: bool,
    /// Media: "Utilizar la decodificación por hardware cuando esté disponible".
    pub hw_decode: bool,
}

impl SourceInfo {
    /// In a scene and switched on: OBS keeps it loaded.
    pub fn in_use(&self) -> bool {
        self.enabled && self.scene.is_some()
    }

    /// A browser page that keeps running even while nobody sees it.
    fn browser_always_on(&self) -> bool {
        self.kind == SourceKind::Browser && self.in_use() && !self.shutdown_when_hidden
    }
}

/// A video or audio filter on a source.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterInfo {
    pub name: String,
    /// Plugin id, e.g. `"nv_background_blur_filter"`. Not personal.
    pub id: String,
    pub enabled: bool,
}

impl FilterInfo {
    /// Filters that redo every frame with heavy maths: NVIDIA's background
    /// effects (OBS 33 `plugins/nv-filters/nvidia-videofx-filter.c`, ids
    /// `nv_greenscreen_filter`, `nv_blur_filter`, `nv_background_blur_filter`)
    /// and the blur, shader, upscaling and background-removal filters of the
    /// popular third-party plugins (StreamFX, Composite Blur, Shaderfilter,
    /// Background Removal).
    pub fn is_heavy(&self) -> bool {
        const EXACT: &[&str] = &["shader_filter", "streamfx-filter-shader", "streamfx-filter-sdf-effects"];
        const PARTS: &[&str] = &["blur", "greenscreen", "background_removal", "upscal", "superres", "denois"];
        let id = self.id.to_ascii_lowercase();
        EXACT.contains(&id.as_str()) || PARTS.iter().any(|part| id.contains(part))
    }
}

pub fn read(paths: &ObsPaths) -> SceneFacts {
    let Some(file) = &paths.scene_file else {
        return SceneFacts::default();
    };
    let too_big = fs::metadata(file).map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(true);
    if too_big {
        return SceneFacts { status: SceneStatus::Unreadable, ..SceneFacts::default() };
    }
    match fs::read(file) {
        Ok(bytes) => parse(&String::from_utf8_lossy(&bytes)),
        Err(_) => SceneFacts { status: SceneStatus::Unreadable, ..SceneFacts::default() },
    }
}

/// One scene or group: its items, as (index into `sources` or containers, visible).
struct Container {
    name: String,
    is_group: bool,
    items: Vec<(Target, bool)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Target {
    Source(usize),
    Container(usize),
}

/// Reads a scene collection file. Pure; tolerant of old and partial files.
pub fn parse(text: &str) -> SceneFacts {
    let unreadable = SceneFacts { status: SceneStatus::Unreadable, ..SceneFacts::default() };
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
        return unreadable;
    };
    // Groups live in their own list since OBS 25; before that they were
    // among the sources.
    let raw: Vec<&Map<String, Value>> = ["sources", "groups"]
        .iter()
        .filter_map(|key| root.get(*key).and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_object)
        .collect();

    let mut sources = Vec::new();
    let mut containers = Vec::new();
    // Items point at their source by uuid (OBS 30 and newer) or by name.
    let mut by_key: HashMap<String, Target> = HashMap::new();
    let mut raw_items = Vec::new();
    for object in &raw {
        let id = str_of(object, "id").unwrap_or_default();
        let raw_name = str_of(object, "name").unwrap_or_default();
        let target = if id == "scene" || id == "group" {
            containers.push(Container { name: display_name(raw_name), is_group: id == "group", items: Vec::new() });
            raw_items.push(object.get("settings").and_then(|s| s.get("items")).and_then(Value::as_array));
            Target::Container(containers.len() - 1)
        } else {
            sources.push(source_info(object, id, raw_name));
            Target::Source(sources.len() - 1)
        };
        if let Some(uuid) = str_of(object, "uuid") {
            by_key.entry(format!("u:{uuid}")).or_insert(target);
        }
        by_key.entry(format!("n:{raw_name}")).or_insert(target);
    }

    for (index, items) in raw_items.iter().enumerate() {
        let resolved = items
            .iter()
            .flat_map(|list| list.iter())
            .filter_map(Value::as_object)
            .filter_map(|item| {
                let by_uuid = str_of(item, "source_uuid").and_then(|u| by_key.get(&format!("u:{u}")));
                let by_name = || str_of(item, "name").and_then(|n| by_key.get(&format!("n:{n}")));
                let target = by_uuid.or_else(by_name).copied()?;
                Some((target, bool_of(item, "visible").unwrap_or(true)))
            })
            .collect();
        if let Some(container) = containers.get_mut(index) {
            container.items = resolved;
        }
    }

    let scenes = scene_order(&root, &containers);
    let current = ["current_program_scene", "current_scene"]
        .iter()
        .filter_map(|key| str_of(&root, key))
        .map(display_name)
        .find(|name| scenes.iter().any(|s| s == name));

    // Where each source shows up, as the Fuentes panel lists it.
    for scene_name in &scenes {
        let Some(scene) = containers.iter().position(|c| !c.is_group && &c.name == scene_name) else {
            continue;
        };
        for (target, _) in panel_items(&containers, scene) {
            if let Target::Source(i) = target
                && let Some(source) = sources.get_mut(i)
                && source.scene.is_none()
            {
                source.scene = Some(scene_name.clone());
            }
        }
    }
    for container in &containers {
        for (target, visible) in &container.items {
            if let (Target::Source(i), true) = (target, visible)
                && let Some(source) = sources.get_mut(*i)
            {
                source.visible_somewhere = true;
            }
        }
    }
    if let Some(start) =
        current.as_ref().and_then(|name| containers.iter().position(|c| !c.is_group && &c.name == name))
    {
        for i in visible_from(&containers, start) {
            if let Some(source) = sources.get_mut(i) {
                source.in_current_scene = true;
            }
        }
    }

    SceneFacts { status: SceneStatus::Read, scenes, current_scene: current, sources }
}

/// Scene names in panel order: `scene_order` first, then any scene it misses.
fn scene_order(root: &Map<String, Value>, containers: &[Container]) -> Vec<String> {
    let known: Vec<&str> = containers.iter().filter(|c| !c.is_group).map(|c| c.name.as_str()).collect();
    let mut order: Vec<String> = root
        .get("scene_order")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(Value::as_str))
        .map(display_name)
        .filter(|name| known.contains(&name.as_str()))
        .collect();
    for name in known {
        if !order.iter().any(|o| o == name) {
            order.push(name.to_string());
        }
    }
    order.dedup();
    order
}

/// The items a scene's Fuentes panel shows: its own, plus those inside its groups.
fn panel_items(containers: &[Container], scene: usize) -> Vec<(Target, bool)> {
    let mut out = Vec::new();
    let Some(container) = containers.get(scene) else {
        return out;
    };
    for &(target, visible) in &container.items {
        out.push((target, visible));
        if let Target::Container(c) = target
            && let Some(group) = containers.get(c).filter(|g| g.is_group)
        {
            out.extend(group.items.iter().map(|&(t, v)| (t, v && visible)));
        }
    }
    out
}

/// Sources that end up on screen when `start` is shown, following visible
/// nested scenes and groups.
fn visible_from(containers: &[Container], start: usize) -> HashSet<usize> {
    let mut found = HashSet::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(start, 0usize)];
    while let Some((index, depth)) = stack.pop() {
        if depth > MAX_NESTING || !seen.insert(index) {
            continue;
        }
        let Some(container) = containers.get(index) else {
            continue;
        };
        for &(target, visible) in &container.items {
            match (target, visible) {
                (Target::Source(i), true) => {
                    found.insert(i);
                }
                (Target::Container(c), true) => stack.push((c, depth + 1)),
                _ => {}
            }
        }
    }
    found
}

/// Reads one source, keeping no text from its settings.
fn source_info(object: &Map<String, Value>, id: &str, raw_name: &str) -> SourceInfo {
    let kind = SourceKind::from_id(id);
    let empty = Map::new();
    let settings = object.get("settings").and_then(Value::as_object).unwrap_or(&empty);
    let filters = object
        .get("filters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|f| FilterInfo {
            name: display_name(str_of(f, "name").unwrap_or_default()),
            id: str_of(f, "id").unwrap_or_default().chars().take(64).collect(),
            enabled: bool_of(f, "enabled").unwrap_or(true),
        })
        .collect();

    let mut info = SourceInfo {
        name: display_name(raw_name),
        kind,
        enabled: bool_of(object, "enabled").unwrap_or(true),
        scene: None,
        visible_somewhere: false,
        in_current_scene: false,
        filters,
        page_size: None,
        shutdown_when_hidden: false,
        custom_resolution: None,
        local_video: false,
        looping: false,
        hw_decode: false,
    };
    match kind {
        SourceKind::Browser => {
            // obs-browser defaults: 800 x 600, "shutdown" off.
            let width = u32_of(settings, "width").unwrap_or(800);
            let height = u32_of(settings, "height").unwrap_or(600);
            info.page_size = Some((width, height));
            info.shutdown_when_hidden = bool_of(settings, "shutdown").unwrap_or(false);
        }
        SourceKind::Camera => {
            // win-dshow: res_type 0 = ResType_Preferred, 1 = ResType_Custom.
            if u32_of(settings, "res_type") == Some(1) {
                info.custom_resolution = str_of(settings, "resolution").and_then(parse_resolution);
            }
        }
        SourceKind::Media if id == "ffmpeg_source" => {
            // obs-ffmpeg-source.c defaults: is_local_file on, looping and hw_decode off.
            let local = bool_of(settings, "is_local_file").unwrap_or(true);
            info.local_video = local && str_of(settings, "local_file").is_some_and(is_video_file);
            info.looping = bool_of(settings, "looping").unwrap_or(false);
            info.hw_decode = bool_of(settings, "hw_decode").unwrap_or(false);
        }
        _ => {}
    }
    info
}

fn str_of<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn bool_of(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(Value::as_bool)
}

fn u32_of(object: &Map<String, Value>, key: &str) -> Option<u32> {
    let value = object.get(key)?;
    let number = value.as_u64().or_else(|| value.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))?;
    u32::try_from(number).ok()
}

/// "1920x1080" as win-dshow stores it.
fn parse_resolution(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.trim().split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn is_video_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The name the streamer gave, unless it looks like an address or a path:
/// some people name a browser source after its link, and that link may
/// carry a key or their user folder.
fn display_name(raw: &str) -> String {
    let name = raw.trim();
    let lower = name.to_lowercase();
    let looks_private = lower.contains("://")
        || lower.contains("www.")
        || lower.contains(":\\")
        || lower.contains(":/")
        || lower.starts_with('\\')
        || lower.starts_with('/')
        || lower.contains("users\\")
        || lower.contains("users/")
        || lower.contains('?');
    if name.is_empty() || looks_private {
        return "(sin nombre)".to_string();
    }
    let mut short: String = name.chars().take(60).collect();
    if short.len() < name.len() {
        short.push('…');
    }
    short
}

pub fn sections(facts: &Facts) -> Vec<Section> {
    let scenes = &facts.scenes;
    let section = Section::new("scenes", "Tus escenas");
    let section = match scenes.status {
        SceneStatus::NotFound => return Vec::new(),
        SceneStatus::Unreadable => section.intro("No pude leer tu colección de escenas de OBS."),
        SceneStatus::Read => scene_rows(scenes, section),
    };
    vec![section]
}

fn scene_rows(scenes: &SceneFacts, mut section: Section) -> Section {
    section.push(
        Row::new("Escenas", scenes.scenes.len().to_string()).note(name_list(scenes.scenes.iter().map(String::as_str))),
    );
    if let Some(current) = &scenes.current_scene {
        section.push(Row::new("Escena en pantalla", current.clone()));
    }
    let used: Vec<&SourceInfo> = scenes.sources.iter().filter(|s| s.in_use()).collect();
    if used.is_empty() {
        return section.intro("Tus escenas todavía no tienen fuentes.");
    }
    for kind in [
        SourceKind::GameCapture,
        SourceKind::DisplayCapture,
        SourceKind::WindowCapture,
        SourceKind::Camera,
        SourceKind::Browser,
        SourceKind::Media,
        SourceKind::Image,
        SourceKind::Text,
        SourceKind::Audio,
        SourceKind::Other,
    ] {
        let of_kind: Vec<&str> = used.iter().filter(|s| s.kind == kind).map(|s| s.name.as_str()).collect();
        if of_kind.is_empty() {
            continue;
        }
        let mut row = Row::new(kind.label(), of_kind.len().to_string()).note(name_list(of_kind.iter().copied()));
        if kind == SourceKind::Browser {
            let always_on = used.iter().filter(|s| s.browser_always_on()).count();
            if always_on >= MANY_BROWSERS {
                row = row.tone(Tone::Warn);
            }
        }
        section.push(row);
    }
    let heavy = heavy_filters(scenes);
    if !heavy.is_empty() {
        let labels: Vec<String> =
            heavy.iter().map(|(source, filter)| format!("{} en {}", filter.name, source.name)).collect();
        section.push(
            Row::new("Filtros pesados", heavy.len().to_string())
                .tone(Tone::Warn)
                .note(name_list(labels.iter().map(String::as_str))),
        );
    }
    section
}

/// "A, B, C y 4 más".
fn name_list<'a>(names: impl Iterator<Item = &'a str>) -> String {
    const SHOWN: usize = 4;
    let names: Vec<&str> = names.collect();
    let rest = names.len().saturating_sub(SHOWN);
    let head = names.iter().take(SHOWN).copied().collect::<Vec<_>>().join(", ");
    if rest > 0 { format!("{head} y {rest} más") } else { head }
}

/// Quoted names for advice: "\"A\"", "\"A\" y \"B\"", "\"A\", \"B\" y 2 más".
fn quoted(names: &[&str]) -> String {
    const SHOWN: usize = 3;
    let q: Vec<String> = names.iter().take(SHOWN).map(|n| format!("\"{n}\"")).collect();
    let rest = names.len().saturating_sub(SHOWN);
    match (q.len(), rest) {
        (0, _) => String::new(),
        (1, _) => q.join(""),
        (_, 0) => {
            let (last, first) = q.split_last().map(|(l, f)| (l.clone(), f.join(", "))).unwrap_or_default();
            format!("{first} y {last}")
        }
        _ => format!("{} y {rest} más", q.join(", ")),
    }
}

fn heavy_filters(scenes: &SceneFacts) -> Vec<(&SourceInfo, &FilterInfo)> {
    scenes
        .sources
        .iter()
        .filter(|s| s.in_use())
        .flat_map(|s| s.filters.iter().filter(|f| f.enabled && f.is_heavy()).map(move |f| (s, f)))
        .collect()
}

/// How much the PC is already struggling to draw the picture. It decides
/// how strongly the heuristic advice below is worded, never more than
/// Recommended: we cannot tell which source caused a lag.
struct Pressure {
    /// The last stream (long enough to judge) lost frames because drawing
    /// the picture took too long.
    render_lag: bool,
    /// The best graphics in this PC is the one built into the processor.
    integrated: bool,
}

impl Pressure {
    fn of(facts: &Facts) -> Self {
        let render_lag = facts
            .logs
            .last_stream
            .as_ref()
            .is_some_and(|s| !s.too_short() && s.lagged_render_pct.is_some_and(|pct| pct >= NOTICEABLE_LOSS_PCT));
        let integrated = facts.hw.best_gpu().is_some_and(|gpu| gpu.is_integrated());
        Self { render_lag, integrated }
    }

    fn any(&self) -> bool {
        self.render_lag || self.integrated
    }

    fn level(&self) -> Level {
        if self.any() { Level::Recommended } else { Level::Tip }
    }

    /// A sentence tying the advice to what we saw, or nothing.
    fn reason(&self) -> &'static str {
        if self.render_lag {
            " En tu último directo OBS no llegó a dibujar todos los cuadros a tiempo."
        } else if self.integrated {
            " Tu PC usa la placa de video integrada en el procesador, que tiene poco margen."
        } else {
            ""
        }
    }
}

fn pick_scene(source: &SourceInfo) -> String {
    match &source.scene {
        Some(scene) => format!("En OBS, en el panel Escenas, elige \"{scene}\"."),
        None => "En OBS, busca la escena que la usa en el panel Escenas.".to_string(),
    }
}

fn open_properties(source: &SourceInfo) -> String {
    format!("En el panel Fuentes haz doble clic en \"{}\" para abrir sus Propiedades.", source.name)
}

fn others_too(rest: &[&SourceInfo]) -> Option<String> {
    let names: Vec<&str> = rest.iter().map(|s| s.name.as_str()).collect();
    (!names.is_empty()).then(|| format!("Repite lo mismo con {}.", quoted(&names)))
}

pub fn advise(facts: &Facts, review: &mut Review) {
    let scenes = &facts.scenes;
    match scenes.status {
        SceneStatus::NotFound => return,
        SceneStatus::Unreadable => {
            review.note("No pude leer tu colección de escenas de OBS, así que no revisé tus fuentes.");
            return;
        }
        SceneStatus::Read => {}
    }
    let pressure = Pressure::of(facts);
    let before = review.advice.len();

    current_scene_empty(scenes, review);
    oversized_browsers(scenes, &pressure, review);
    hidden_browsers(scenes, review);
    many_browsers(scenes, &pressure, review);
    heavy_filter_advice(scenes, &pressure, review);
    big_cameras(scenes, &pressure, review);
    media_without_hw_decode(scenes, review);
    display_capture_for_game(facts, review);

    let added = review.advice[before..].iter().any(|a| a.id.starts_with("scenes."));
    if !added && scenes.sources.iter().any(SourceInfo::in_use) {
        review.ok("Tus escenas no tienen fuentes que exijan de más a tu PC.");
    }
}

/// The scene on screen shows nothing but sound: viewers would see black.
fn current_scene_empty(scenes: &SceneFacts, review: &mut Review) {
    let Some(current) = &scenes.current_scene else {
        return;
    };
    let shows_something = scenes.sources.iter().any(|s| s.in_current_scene && s.enabled && s.kind != SourceKind::Audio);
    if shows_something {
        return;
    }
    review.add(Advice {
        id: "scenes.current-empty",
        level: Level::Tip,
        area: Area::Scenes,
        title: format!("Agrega lo que quieres mostrar a la escena \"{current}\""),
        why: "Esa escena no muestra nada todavía. Si sales en directo así, tu público verá la pantalla en negro.".to_string(),
        steps: vec![
            format!("En OBS, en el panel Escenas, elige \"{current}\"."),
            "En el panel Fuentes haz clic en + y elige qué mostrar, por ejemplo Captura de juego o Dispositivo de captura de video.".to_string(),
            "Mira la vista previa para comprobar que se ve.".to_string(),
        ],
    });
}

fn oversized_browsers(scenes: &SceneFacts, pressure: &Pressure, review: &mut Review) {
    let big: Vec<&SourceInfo> = scenes
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::Browser && s.in_use())
        .filter(|s| s.page_size.is_some_and(|(w, h)| w > MAX_BROWSER_WIDTH || h > MAX_BROWSER_HEIGHT))
        .collect();
    let Some((first, rest)) = big.split_first() else {
        return;
    };
    let (w, h) = first.page_size.unwrap_or_default();
    let names: Vec<&str> = big.iter().map(|s| s.name.as_str()).collect();
    let mut steps = vec![
        pick_scene(first),
        open_properties(first),
        format!(
            "Baja Ancho y Alto al tamaño que de verdad ocupa en pantalla, como máximo {MAX_BROWSER_WIDTH} y {MAX_BROWSER_HEIGHT}, y haz clic en Aceptar."
        ),
        "Si se ve chica, agrándala en la vista previa arrastrando sus bordes.".to_string(),
    ];
    steps.extend(others_too(rest));
    review.add(Advice {
        id: "scenes.browser.oversized",
        level: pressure.level(),
        area: Area::Scenes,
        title: format!("Achica la página web de {}", quoted(&names)),
        why: format!(
            "Dibuja una página de {w} x {h}, más grande que la imagen Full HD de tu directo. OBS dibuja cada punto aunque en pantalla se vea chica, y eso le suma trabajo a la placa de video.{}",
            pressure.reason()
        ),
        steps,
    });
}

/// Hidden in every scene, yet "Apagar fuente cuando no sea visible" is off:
/// the page keeps running in the background for nothing.
fn hidden_browsers(scenes: &SceneFacts, review: &mut Review) {
    let hidden: Vec<&SourceInfo> =
        scenes.sources.iter().filter(|s| s.browser_always_on() && !s.visible_somewhere).collect();
    let Some((first, rest)) = hidden.split_first() else {
        return;
    };
    let names: Vec<&str> = hidden.iter().map(|s| s.name.as_str()).collect();
    let mut steps = vec![
        pick_scene(first),
        format!("Si ya no usas \"{}\", selecciónala en el panel Fuentes y bórrala con la tecla Supr.", first.name),
        format!(
            "Si la quieres guardar, {} y marca \"Apagar fuente cuando no sea visible\".",
            open_properties(first).to_lowercase_first()
        ),
    ];
    steps.extend(others_too(rest));
    review.add(Advice {
        id: "scenes.browser.hidden",
        level: Level::Tip,
        area: Area::Scenes,
        title: format!("Apaga la página web oculta {}", quoted(&names)),
        why: "Está oculta en todas tus escenas, pero sigue abierta por detrás y usa memoria y procesador sin que nadie la vea.".to_string(),
        steps,
    });
}

fn many_browsers(scenes: &SceneFacts, pressure: &Pressure, review: &mut Review) {
    let always_on: Vec<&SourceInfo> = scenes.sources.iter().filter(|s| s.browser_always_on()).collect();
    if always_on.len() < MANY_BROWSERS {
        return;
    }
    let partial: Vec<&SourceInfo> =
        always_on.iter().copied().filter(|s| s.visible_somewhere && !s.in_current_scene).collect();
    let mut steps = vec![
        "Piensa cuáles de estas páginas usas de verdad en directo y borra las que sobran.".to_string(),
        "En las que solo aparecen en algunas escenas, abre sus Propiedades con doble clic en el panel Fuentes y marca \"Apagar fuente cuando no sea visible\". Al volver a mostrarse tardan un segundo en cargar.".to_string(),
    ];
    if let Some(first) = partial.first() {
        steps.push(format!("Puedes empezar por \"{}\", que no está en la escena que tienes en pantalla.", first.name));
    }
    steps.push("Haz un directo de prueba y compara.".to_string());
    review.add(Advice {
        id: "scenes.browser.many",
        level: pressure.level(),
        area: Area::Scenes,
        title: format!("Prueba con menos páginas web abiertas a la vez (tienes {})", always_on.len()),
        why: format!(
            "Cada fuente de Navegador es una página web completa que sigue funcionando aunque no esté en pantalla. Varias juntas le quitan memoria y procesador al juego y a OBS.{}",
            pressure.reason()
        ),
        steps,
    });
}

/// Only when the PC is short of graphics power: on a strong card these
/// filters are fine.
fn heavy_filter_advice(scenes: &SceneFacts, pressure: &Pressure, review: &mut Review) {
    if !pressure.any() {
        return;
    }
    let heavy = heavy_filters(scenes);
    let Some(((source, filter), rest)) = heavy.split_first() else {
        return;
    };
    let title = match rest.len() {
        0 => format!("Prueba sin el filtro \"{}\" de \"{}\"", filter.name, source.name),
        n => format!("Prueba sin el filtro \"{}\" de \"{}\" y {n} más", filter.name, source.name),
    };
    let mut steps = vec![
        pick_scene(source),
        format!("En el panel Fuentes haz clic derecho en \"{}\" y elige Filtros.", source.name),
        format!("Haz clic en el ojo junto a \"{}\" para apagarlo.", filter.name),
        "Haz un directo de prueba. Si va mejor, déjalo apagado o busca una versión más liviana del efecto.".to_string(),
    ];
    if !rest.is_empty() {
        steps.push("Si no alcanza, prueba lo mismo con los otros filtros de la lista.".to_string());
    }
    review.add(Advice {
        id: "scenes.filters.heavy",
        level: Level::Recommended,
        area: Area::Scenes,
        title,
        why: format!(
            "Los efectos como desenfocar o quitar el fondo recalculan cada cuadro y exigen mucho a la placa de video.{}",
            pressure.reason()
        ),
        steps,
    });
}

fn big_cameras(scenes: &SceneFacts, pressure: &Pressure, review: &mut Review) {
    let big: Vec<&SourceInfo> = scenes
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::Camera && s.in_use())
        .filter(|s| s.custom_resolution.is_some_and(|(_, h)| h > MAX_CAMERA_HEIGHT))
        .collect();
    let Some((first, rest)) = big.split_first() else {
        return;
    };
    let (w, h) = first.custom_resolution.unwrap_or_default();
    let names: Vec<&str> = big.iter().map(|s| s.name.as_str()).collect();
    let mut steps = vec![
        pick_scene(first),
        open_properties(first),
        "En \"Tipo de resolución/FPS\" deja \"Personalizado\" y en Resolución elige 1920x1080 o menos.".to_string(),
        "Haz clic en Aceptar y mira que la imagen siga viéndose bien.".to_string(),
    ];
    steps.extend(others_too(rest));
    review.add(Advice {
        id: "scenes.webcam.high-res",
        level: pressure.level(),
        area: Area::Scenes,
        title: format!("Baja la resolución de {} a 1080p", quoted(&names)),
        why: format!(
            "Está en {w}x{h}, más que la imagen de tu directo. Procesar esos cuadros tan grandes le cuesta al procesador y no se nota en el directo.{}",
            pressure.reason()
        ),
        steps,
    });
}

fn media_without_hw_decode(scenes: &SceneFacts, review: &mut Review) {
    let videos: Vec<&SourceInfo> = scenes
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::Media && s.in_use() && s.local_video && s.looping && !s.hw_decode)
        .collect();
    let Some((first, rest)) = videos.split_first() else {
        return;
    };
    let names: Vec<&str> = videos.iter().map(|s| s.name.as_str()).collect();
    let mut steps = vec![
        pick_scene(first),
        open_properties(first),
        "Marca \"Utilizar la decodificación por hardware cuando esté disponible\" y haz clic en Aceptar.".to_string(),
    ];
    steps.extend(others_too(rest));
    review.add(Advice {
        id: "scenes.media.hw-decode",
        level: Level::Tip,
        area: Area::Scenes,
        title: format!("Deja que la placa de video reproduzca {}", quoted(&names)),
        why: "Es un video en bucle y hoy lo reproduce el procesador. La placa de video lo puede hacer casi gratis y le deja más margen a OBS.".to_string(),
        steps,
    });
}

/// A game is running, the scene on screen captures the whole display and
/// the collection has no Captura de juego. OBS's guide recommends Game
/// Capture for games: <https://obsproject.com/kb/game-capture-sources>.
fn display_capture_for_game(facts: &Facts, review: &mut Review) {
    let scenes = &facts.scenes;
    let game_running = facts.apps.known.iter().any(|app| app.is_game);
    let has_game_capture = scenes.sources.iter().any(|s| s.kind == SourceKind::GameCapture && s.in_use());
    let display =
        scenes.sources.iter().find(|s| s.kind == SourceKind::DisplayCapture && s.in_current_scene && s.enabled);
    let (true, false, Some(display)) = (game_running, has_game_capture, display) else {
        return;
    };
    review.add(Advice {
        id: "scenes.capture.display-for-game",
        level: Level::Tip,
        area: Area::Scenes,
        title: "Prueba Captura de juego en vez de capturar toda la pantalla".to_string(),
        why: format!(
            "Muestras el juego con \"{}\", que copia la pantalla entera. Captura de juego toma la imagen directo del juego, suele ir más fluida y no muestra tus notificaciones.",
            display.name
        ),
        steps: vec![
            "Con el juego abierto, en el panel Fuentes de OBS haz clic en + y elige Captura de juego.".to_string(),
            "En Modo elige \"Capturar ventana específica\", elige tu juego en Ventana y haz clic en Aceptar.".to_string(),
            format!("Oculta \"{}\" con el ojo del panel Fuentes y mira que el juego se vea bien.", display.name),
        ],
    });
}

trait LowercaseFirst {
    fn to_lowercase_first(&self) -> String;
}

impl LowercaseFirst for String {
    /// "En el panel..." -> "en el panel...", to embed a step in a sentence.
    fn to_lowercase_first(&self) -> String {
        let trimmed = self.trim_end_matches('.');
        let mut chars = trimmed.chars();
        match chars.next() {
            Some(first) => first.to_lowercase().chain(chars).collect(),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::GpuVendor;
    use crate::hardware::test_support::{gpu, pc};
    use crate::obs::logs::{LogFacts, StreamStats};

    const TYPICAL: &str = include_str!("../../tests/fixtures/scenes/typical.json");
    const FRESH: &str = include_str!("../../tests/fixtures/scenes/fresh.json");
    const LEGACY: &str = include_str!("../../tests/fixtures/scenes/legacy.json");

    fn source<'a>(facts: &'a SceneFacts, name: &str) -> &'a SourceInfo {
        facts.sources.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no source {name}"))
    }

    fn review_of(facts: &Facts) -> Review {
        let mut review = Review::default();
        advise(facts, &mut review);
        review
    }

    fn with_scenes(text: &str) -> Facts {
        Facts { scenes: parse(text), ..Facts::default() }
    }

    fn render_lag(pct: f32) -> LogFacts {
        LogFacts {
            last_stream: Some(StreamStats {
                duration_secs: Some(3600),
                lagged_render_pct: Some(pct),
                ..StreamStats::default()
            }),
            ..LogFacts::default()
        }
    }

    #[test]
    fn reads_scenes_in_panel_order_and_the_current_one() {
        let facts = parse(TYPICAL);
        assert_eq!(facts.status, SceneStatus::Read);
        assert_eq!(facts.scenes, vec!["Juego", "Charla", "Pausa"]);
        assert_eq!(facts.current_scene.as_deref(), Some("Juego"));
        // Scenes and groups are not sources.
        assert!(facts.sources.iter().all(|s| s.name != "Superposiciones" && s.name != "Juego"));
    }

    #[test]
    fn classifies_sources_and_reads_their_settings() {
        let facts = parse(TYPICAL);
        let chat = source(&facts, "Chat");
        assert_eq!(chat.kind, SourceKind::Browser);
        assert_eq!(chat.page_size, Some((3840, 2160)));
        assert!(chat.shutdown_when_hidden);
        // Missing keys fall back to obs-browser's defaults.
        let goal = source(&facts, "Meta de seguidores");
        assert_eq!(goal.page_size, Some((800, 600)));
        assert!(!goal.shutdown_when_hidden);

        let camera = source(&facts, "Camara");
        assert_eq!(camera.kind, SourceKind::Camera);
        assert_eq!(camera.custom_resolution, Some((3840, 2160)));
        assert_eq!(camera.filters.len(), 2);
        assert!(camera.filters[0].is_heavy());
        assert!(!camera.filters[1].is_heavy());

        let video = source(&facts, "Video de pausa");
        assert!(video.local_video && video.looping && !video.hw_decode);
        assert!(!source(&facts, "Musica").local_video);
        assert_eq!(source(&facts, "Mic/Aux").kind, SourceKind::Audio);
        assert_eq!(source(&facts, "Texto").kind, SourceKind::Text);
    }

    #[test]
    fn follows_groups_nested_scenes_and_hidden_items() {
        let facts = parse(TYPICAL);
        // Inside a group: listed under the scene that holds the group.
        assert_eq!(source(&facts, "Ticker").scene.as_deref(), Some("Juego"));
        assert!(source(&facts, "Ticker").in_current_scene);
        // Hidden in its only scene.
        let old = source(&facts, "Widget viejo");
        assert_eq!(old.scene.as_deref(), Some("Juego"));
        assert!(!old.visible_somewhere);
        assert!(!old.in_current_scene);
        // Only in another scene.
        assert_eq!(source(&facts, "Chat").scene.as_deref(), Some("Charla"));
        assert!(!source(&facts, "Chat").in_current_scene);
        // In no scene at all, or switched off.
        assert!(!source(&facts, "Mic/Aux").in_use());
        assert!(!source(&facts, "Navegador apagado").in_use());
    }

    #[test]
    fn nested_scenes_count_as_on_screen() {
        let mut facts = parse(TYPICAL);
        assert!(!source(&facts, "Chat").in_current_scene);
        // "Charla" shows "Juego" inside it.
        let text = TYPICAL.replace("\"current_program_scene\": \"Juego\"", "\"current_program_scene\": \"Charla\"");
        facts = parse(&text);
        assert_eq!(facts.current_scene.as_deref(), Some("Charla"));
        assert!(source(&facts, "Chat").in_current_scene);
        assert!(source(&facts, "Alertas").in_current_scene);
        assert!(!source(&facts, "Contador").in_current_scene);
    }

    #[test]
    fn older_files_link_items_by_name() {
        let facts = parse(LEGACY);
        assert_eq!(facts.status, SceneStatus::Read);
        assert_eq!(facts.scenes, vec!["Principal"]);
        let browser = source(&facts, "Navegador");
        assert!(browser.in_current_scene);
        assert_eq!(browser.page_size, Some((2560, 1440)));
        assert_eq!(source(&facts, "Ventana").kind, SourceKind::WindowCapture);
    }

    #[test]
    fn bad_input_is_unreadable_not_a_panic() {
        for text in ["", "no es json", "[]", "42", "{\"sources\": 5, \"groups\": {}}"] {
            let facts = parse(text);
            if text.starts_with('{') {
                assert_eq!(facts.status, SceneStatus::Read, "{text}");
                assert!(facts.sources.is_empty());
            } else {
                assert_eq!(facts.status, SceneStatus::Unreadable, "{text}");
            }
        }
        // Wrong types inside an otherwise valid file.
        let odd = r#"{"sources":[{"id":"browser_source","name":"B","settings":{"width":"ancho","height":-5,"shutdown":"si"}},
            {"id":"scene","name":"S","settings":{"items":[{"name":"B","visible":"si"},{"name":"falta"},7]}},
            {"id":"dshow_input","name":"C","settings":{"res_type":1,"resolution":"grande"}}],
            "scene_order":"no","current_scene":3}"#;
        let facts = parse(odd);
        let browser = source(&facts, "B");
        assert_eq!(browser.page_size, Some((800, 600)));
        assert!(browser.visible_somewhere);
        assert_eq!(source(&facts, "C").custom_resolution, None);
        assert_eq!(facts.scenes, vec!["S"]);
        assert_eq!(facts.current_scene, None);
    }

    #[test]
    fn a_missing_file_reads_as_not_found() {
        let paths = ObsPaths { scene_file: Some("Z:/no/existe/escenas.json".into()), ..ObsPaths::default() };
        assert_eq!(read(&paths).status, SceneStatus::Unreadable);
        assert_eq!(read(&ObsPaths::default()).status, SceneStatus::NotFound);
    }

    #[test]
    fn addresses_and_paths_are_never_kept_or_shown() {
        let leaky = TYPICAL
            .replace("\"name\": \"Ticker\"", "\"name\": \"https://ticker.example.invalid/?k=CLAVE-EN-NOMBRE\"")
            .replace("\"name\": \"Ticker\", \"source_uuid\"", "\"name\": \"x\", \"source_uuid\"");
        let mut facts = with_scenes(&leaky);
        facts.logs = render_lag(4.0);
        let review = review_of(&facts);
        let mut shown = format!("{:?}", facts.scenes);
        for section in sections(&facts) {
            shown.push_str(&format!("{section:?}"));
        }
        for advice in &review.advice {
            shown.push_str(&format!("{advice:?}"));
        }
        for secret in [
            "example.invalid",
            "SECRETO",
            "CLAVE-EN-NOMBRE",
            "StreamerDePrueba",
            "pausa-en-bucle",
            "ticker.html",
            "usb#vid",
            "DISPLAY#",
            "juego.exe",
            "https",
        ] {
            assert!(!shown.contains(secret), "leaked {secret}");
        }
        assert!(source(&facts.scenes, "(sin nombre)").in_use());
    }

    #[test]
    fn typical_collection_without_pressure_gets_tips_only() {
        let review = review_of(&with_scenes(TYPICAL));
        let mut ids = review.ids();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "scenes.browser.hidden",
                "scenes.browser.many",
                "scenes.browser.oversized",
                "scenes.media.hw-decode",
                "scenes.webcam.high-res",
            ]
        );
        assert!(review.advice.iter().all(|a| a.level == Level::Tip && a.area == Area::Scenes));
        assert!(review.title_of("scenes.browser.oversized").contains("\"Chat\""));
        assert!(review.title_of("scenes.browser.hidden").contains("\"Widget viejo\""));
        assert!(review.title_of("scenes.browser.many").contains('5'));
    }

    #[test]
    fn render_lag_raises_the_heuristics_but_never_to_important() {
        let mut facts = with_scenes(TYPICAL);
        facts.logs = render_lag(2.0);
        let review = review_of(&facts);
        assert!(review.has("scenes.filters.heavy"));
        for id in ["scenes.browser.oversized", "scenes.browser.many", "scenes.webcam.high-res", "scenes.filters.heavy"]
        {
            assert_eq!(review.get(id).map(|a| a.level), Some(Level::Recommended), "{id}");
        }
        assert_eq!(review.get("scenes.browser.hidden").map(|a| a.level), Some(Level::Tip));
        assert_eq!(review.count(Level::Important), 0);
    }

    #[test]
    fn short_or_clean_streams_are_not_evidence() {
        let mut facts = with_scenes(TYPICAL);
        facts.logs = render_lag(0.5);
        assert!(!review_of(&facts).has("scenes.filters.heavy"));
        facts.logs = render_lag(20.0);
        if let Some(stream) = facts.logs.last_stream.as_mut() {
            stream.duration_secs = Some(10);
        }
        assert!(!review_of(&facts).has("scenes.filters.heavy"));
    }

    #[test]
    fn integrated_graphics_raises_the_heuristics() {
        let mut facts = with_scenes(TYPICAL);
        facts.hw = pc(4, 8, vec![gpu("Intel(R) UHD Graphics 620", GpuVendor::Intel, 1)]);
        let review = review_of(&facts);
        assert_eq!(review.get("scenes.filters.heavy").map(|a| a.level), Some(Level::Recommended));
        assert_eq!(review.get("scenes.browser.oversized").map(|a| a.level), Some(Level::Recommended));

        facts.hw = pc(8, 32, vec![gpu("NVIDIA GeForce RTX 3060", GpuVendor::Nvidia, 12)]);
        let review = review_of(&facts);
        assert!(!review.has("scenes.filters.heavy"));
        assert_eq!(review.get("scenes.browser.oversized").map(|a| a.level), Some(Level::Tip));
    }

    #[test]
    fn a_fresh_collection_only_flags_the_empty_scene() {
        let facts = with_scenes(FRESH);
        assert_eq!(facts.scenes.scenes, vec!["Escena"]);
        assert!(facts.scenes.sources.is_empty());
        let review = review_of(&facts);
        assert_eq!(review.ids(), vec!["scenes.current-empty"]);
        assert!(review.good.is_empty());
        assert_eq!(sections(&facts).len(), 1);
    }

    #[test]
    fn a_light_collection_is_praised() {
        let text = r#"{"current_scene":"S","sources":[
            {"id":"scene","name":"S","settings":{"items":[{"name":"Juego","visible":true},{"name":"Cam","visible":true}]}},
            {"id":"game_capture","name":"Juego"},
            {"id":"dshow_input","name":"Cam","settings":{"res_type":1,"resolution":"1920x1080"}}]}"#;
        let review = review_of(&with_scenes(text));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert_eq!(review.good.len(), 1);
    }

    #[test]
    fn display_capture_with_a_game_open_suggests_game_capture() {
        use crate::processes::{App, Apps};
        let text = r#"{"current_scene":"S","sources":[
            {"id":"scene","name":"S","settings":{"items":[{"name":"Pantalla","visible":true}]}},
            {"id":"monitor_capture","name":"Pantalla"}]}"#;
        let mut facts = with_scenes(text);
        assert!(!review_of(&facts).has("scenes.capture.display-for-game"));
        facts.apps = Apps {
            known: vec![App {
                name: "Juego".to_string(),
                category: None,
                is_game: true,
                note: None,
                process_count: 1,
                mem_bytes: 0,
                cpu_pct: 0.0,
            }],
            ..Apps::default()
        };
        let review = review_of(&facts);
        assert_eq!(review.get("scenes.capture.display-for-game").map(|a| a.level), Some(Level::Tip));
        // With a Captura de juego already set up, nothing to say.
        let with_game = text
            .replace(
                r#"{"id":"monitor_capture","name":"Pantalla"}"#,
                r#"{"id":"monitor_capture","name":"Pantalla"},{"id":"game_capture","name":"G"}"#,
            )
            .replace(
                r#"{"name":"Pantalla","visible":true}"#,
                r#"{"name":"Pantalla","visible":true},{"name":"G","visible":false}"#,
            );
        facts.scenes = parse(&with_game);
        assert!(!review_of(&facts).has("scenes.capture.display-for-game"));
    }

    #[test]
    fn unreadable_collection_leaves_a_note() {
        let facts = Facts { scenes: parse("basura"), ..Facts::default() };
        let review = review_of(&facts);
        assert!(review.advice.is_empty());
        assert_eq!(review.notes.len(), 1);
        assert!(review_of(&Facts::default()).notes.is_empty());
    }

    #[test]
    fn long_names_are_cut_and_lists_are_short() {
        assert_eq!(display_name(&"a".repeat(80)).chars().count(), 61);
        assert_eq!(display_name("  "), "(sin nombre)");
        assert_eq!(display_name("C:\\Users\\x\\a.png"), "(sin nombre)");
        assert_eq!(quoted(&["a"]), "\"a\"");
        assert_eq!(quoted(&["a", "b"]), "\"a\" y \"b\"");
        assert_eq!(quoted(&["a", "b", "c", "d", "e"]), "\"a\", \"b\", \"c\" y 2 más");
        assert_eq!(name_list(["a", "b", "c", "d", "e", "f"].into_iter()), "a, b, c, d y 2 más");
        assert_eq!(parse_resolution("1280x720"), Some((1280, 720)));
        assert_eq!(parse_resolution("x"), None);
    }

    trait TitleOf {
        fn title_of(&self, id: &str) -> String;
    }

    impl TitleOf for Review {
        fn title_of(&self, id: &str) -> String {
            self.get(id).map(|a| a.title.clone()).unwrap_or_default()
        }
    }
}
