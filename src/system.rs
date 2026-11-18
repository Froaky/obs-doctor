//! Windows settings and state that affect a stream (power, game features,
//! graphics drivers, disks, startup programs...). Read-only: nothing here
//! changes a setting.
//!
//! Everything is read without administrator rights: `GetSystemPowerStatus`,
//! display and disk enumeration, and the registry. The game feature keys are
//! the same ones OBS writes to its log at startup (libobs/obs-windows.c,
//! `log_gaming_features`), so both sources agree; the log only fills gaps.
//!
//! Privacy: registry value names and data may hold paths with the Windows
//! user name (startup programs, per-app graphics preferences). Only program
//! names and numbers are kept, never a path.

use crate::facts::Facts;
use crate::hardware::{Gpu, GpuVendor};
use crate::model::{Advice, Area, Level, Review, Row, Section, Tone};
use crate::obs::logs::GameMode;
use crate::report::text;

/// What we read from Windows. `Default` means "could not read anything".
///
/// CONTRACT: other areas read the fields below. Keep their names, types and
/// meaning; add more fields freely.
#[derive(Debug, Clone, Default)]
pub struct SystemFacts {
    /// A laptop running on its battery right now. `None` when unknown or
    /// when the PC has no battery.
    pub on_battery: Option<bool>,
    /// The PC has a battery (a laptop or tablet). `None` when unknown.
    pub has_battery: Option<bool>,
    /// Charge left, 0 to 100.
    pub battery_pct: Option<u8>,
    /// "Ahorro de energía" (called "Ahorro de batería" before 24H2) is on.
    pub energy_saver: Option<bool>,
    /// The classic power plan of "Opciones de energía".
    pub power_plan: Option<PowerPlan>,
    /// Windows 11 "Modo de energía" for the current power source. `None`
    /// when the streamer never moved it (the maker's default applies).
    pub power_mode: Option<PowerMode>,
    pub game_mode: Option<GameMode>,
    /// Game Bar captures are allowed.
    pub game_bar: Option<bool>,
    /// Windows keeps recording the last minutes of games in the background.
    pub background_recording: Option<bool>,
    /// Hardware-accelerated GPU scheduling, as chosen in Settings. `None`
    /// when the setting was never touched and the driver decides.
    pub hags: Option<bool>,
    /// Installed graphics drivers, one per card Windows knows about.
    pub drivers: Vec<GpuDriver>,
    /// "Preferencia de GPU" stored for obs64.exe, if any.
    pub obs_gpu_preference: Option<GpuPreference>,
    pub displays: Vec<Display>,
    pub windows: Option<WindowsVersion>,
    /// Fixed drives with their free space.
    pub disks: Vec<Disk>,
    pub page_file: Option<PageFile>,
    /// Names of the programs that start with Windows and are not disabled.
    /// `None` when the lists could not be read.
    pub startup_apps: Option<Vec<String>>,
    /// Delivery Optimization download mode. Usually `None`: the setting
    /// lives in a part of the registry only administrators can read.
    pub delivery_mode: Option<DeliveryMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPlan {
    Balanced,
    HighPerformance,
    PowerSaver,
    Ultimate,
    /// Made by the PC maker or the user.
    Other,
}

impl PowerPlan {
    /// Built-in plan GUIDs, the ones `powercfg /list` shows on a clean install.
    /// https://learn.microsoft.com/en-us/windows-hardware/design/device-experiences/powercfg-command-line-options
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn from_guid(guid: &str) -> Self {
        match guid.trim().trim_matches(['{', '}']).to_ascii_lowercase().as_str() {
            "381b4222-f694-41f0-9685-ff5bb260df2e" => PowerPlan::Balanced,
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c" => PowerPlan::HighPerformance,
            "a1841308-3541-4fab-bc81-f71556f20b4a" => PowerPlan::PowerSaver,
            "e9a42b02-d5df-448d-aa00-03f14749eb61" => PowerPlan::Ultimate,
            _ => PowerPlan::Other,
        }
    }

    fn label(self) -> &'static str {
        match self {
            PowerPlan::Balanced => "Equilibrado",
            PowerPlan::HighPerformance => "Alto rendimiento",
            PowerPlan::PowerSaver => "Economizador",
            PowerPlan::Ultimate => "Máximo rendimiento",
            PowerPlan::Other => "Personalizado",
        }
    }
}

/// Positions of the Windows power slider ("Modo de energía").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    BestEfficiency,
    Balanced,
    BestPerformance,
}

impl PowerMode {
    /// Overlay GUIDs stored in `ActiveOverlay{Ac,Dc}PowerScheme`.
    /// https://learn.microsoft.com/en-us/windows-hardware/customize/desktop/customize-power-slider
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn from_guid(guid: &str) -> Option<Self> {
        match guid.trim().trim_matches(['{', '}']).to_ascii_lowercase().as_str() {
            "961cc777-2547-4f9d-8174-7d86181b8a7a" => Some(PowerMode::BestEfficiency),
            // "Better performance": the Windows default. The all-zero GUID is
            // what Windows 11 writes when the slider goes back to it.
            "3af9b8d9-7c97-431d-ad78-34a8bfea439f"
            | "381b4222-f694-41f0-9685-ff5bb260df2e"
            | "00000000-0000-0000-0000-000000000000" => Some(PowerMode::Balanced),
            "ded574b5-45a0-4f42-8737-46345c09c238" => Some(PowerMode::BestPerformance),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            PowerMode::BestEfficiency => "ahorro de energía",
            PowerMode::Balanced => "equilibrado",
            PowerMode::BestPerformance => "máximo rendimiento",
        }
    }
}

/// A graphics driver as Windows registered it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuDriver {
    /// Card name, as DXGI also reports it ("NVIDIA GeForce GTX 1660").
    pub name: String,
    /// "32.0.101.7076".
    pub version: Option<String>,
    /// Release date of the driver, as days since 1970-01-01.
    pub date_days: Option<i64>,
    /// How old the driver was when we looked, in days.
    pub age_days: Option<i64>,
    /// Windows' own "Microsoft Basic Display Adapter" (display.inf): the
    /// card has no real driver installed.
    pub basic: bool,
}

/// "Preferencia de GPU" of an app in Settings, System, Display, Graphics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPreference {
    LetWindowsDecide,
    PowerSaving,
    HighPerformance,
}

impl GpuPreference {
    /// Parses the value data Windows stores, e.g. `"GpuPreference=2;"`.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn parse(data: &str) -> Option<Self> {
        let value = data
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(key, _)| key.trim().eq_ignore_ascii_case("GpuPreference"))?
            .1;
        match value.trim() {
            "0" => Some(GpuPreference::LetWindowsDecide),
            "1" => Some(GpuPreference::PowerSaving),
            "2" => Some(GpuPreference::HighPerformance),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            GpuPreference::LetWindowsDecide => "Windows decide",
            GpuPreference::PowerSaving => "Ahorro de energía",
            GpuPreference::HighPerformance => "Alto rendimiento",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsVersion {
    /// 26100 and up is Windows 11 24H2, 22000 and up Windows 11.
    pub build: u32,
    /// "25H2". Missing on old Windows 10.
    pub display_version: Option<String>,
}

impl WindowsVersion {
    pub fn is_windows_11(&self) -> bool {
        self.build >= 22000
    }

    fn label(&self) -> String {
        let name = if self.is_windows_11() { "Windows 11" } else { "Windows 10" };
        match &self.display_version {
            Some(v) => format!("{name} {v}"),
            None => name.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Disk {
    /// Drive letter, e.g. 'C'.
    pub letter: char,
    pub free_bytes: u64,
    pub total_bytes: u64,
    /// Windows is installed here.
    pub system: bool,
}

/// What `PagingFiles` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFile {
    /// No paging file at all.
    Off,
    /// Windows chooses the size.
    SystemManaged,
    /// Fixed sizes chosen by hand; the largest maximum in MB.
    Custom { max_mb: u32 },
}

impl PageFile {
    /// Parses the `PagingFiles` multi-string: one entry per drive, either
    /// `"?:\pagefile.sys"` (Windows chooses drive and size), `"C:\pagefile.sys
    /// 0 0"` (Windows chooses the size) or `"C:\pagefile.sys 1024 4096"`
    /// (initial and maximum MB). No entries means the paging file is off.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn parse(entries: &[String]) -> Self {
        let mut custom_max: Option<u32> = None;
        let mut any = false;
        for entry in entries.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            any = true;
            let mut parts = entry.split_whitespace().skip(1);
            let initial = parts.next().and_then(|v| v.parse::<u32>().ok());
            let max = parts.next().and_then(|v| v.parse::<u32>().ok());
            match (initial, max) {
                (Some(0), Some(0)) | (None, _) => return PageFile::SystemManaged,
                (_, Some(max)) => custom_max = Some(custom_max.map_or(max, |m| m.max(max))),
                (Some(_), None) => return PageFile::SystemManaged,
            }
        }
        match (any, custom_max) {
            (false, _) => PageFile::Off,
            (true, Some(0)) => PageFile::Off,
            (true, Some(max_mb)) => PageFile::Custom { max_mb },
            (true, None) => PageFile::SystemManaged,
        }
    }
}

/// Delivery Optimization download modes.
/// https://learn.microsoft.com/en-us/windows/deployment/do/waas-delivery-optimization-reference#download-mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// Downloads only from Microsoft (0, 99, 100).
    Off,
    /// Shares with PCs on the same home network (1) or group (2).
    LocalNetwork,
    /// Shares with PCs on the internet too (3): uses the streamer's upload.
    Internet,
}

impl DeliveryMode {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn from_value(value: u32) -> Option<Self> {
        match value {
            0 | 99 | 100 => Some(DeliveryMode::Off),
            1 | 2 => Some(DeliveryMode::LocalNetwork),
            3 => Some(DeliveryMode::Internet),
            _ => None,
        }
    }
}

/// Days from 1970-01-01 to a civil date (proleptic Gregorian).
/// Howard Hinnant's algorithm: http://howardhinnant.github.io/date_algorithms.html
#[cfg_attr(not(windows), allow(dead_code))]
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count, as (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Parses the registry `DriverDate`, always written month first: "9-12-2025".
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_driver_date(raw: &str) -> Option<i64> {
    let mut parts = raw.trim().split(['-', '/']);
    let month: u32 = parts.next()?.trim().parse().ok()?;
    let day: u32 = parts.next()?.trim().parse().ok()?;
    let year: i64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(1990..=2200).contains(&year)
    {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Task Manager and Settings mark a disabled startup entry by setting the
/// lowest bit of the first byte of its `StartupApproved` value (02 and 06
/// are on, 03 and 07 are off). No value at all means it was never touched.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn startup_enabled(approved: Option<&[u8]>) -> bool {
    match approved.and_then(|bytes| bytes.first()) {
        Some(first) => first & 1 == 0,
        None => true,
    }
}

/// A presentable program name from a Run value name or a startup shortcut:
/// drops the extension and the random suffix some installers add
/// ("MicrosoftEdgeAutoLaunch_0A1B2C...").
#[cfg_attr(not(windows), allow(dead_code))]
pub fn startup_name(raw: &str) -> Option<String> {
    let mut name = raw.trim();
    if let Some(stem) = name.strip_suffix(".lnk").or_else(|| name.strip_suffix(".LNK")) {
        name = stem;
    }
    if let Some((head, tail)) = name.rsplit_once('_')
        && tail.len() >= 16
        && tail.chars().all(|c| c.is_ascii_hexdigit())
    {
        name = head;
    }
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("desktop.ini") {
        return None;
    }
    Some(name.to_string())
}

pub fn read() -> SystemFacts {
    #[cfg(windows)]
    {
        win::read()
    }
    #[cfg(not(windows))]
    {
        SystemFacts::default()
    }
}

// ---------------------------------------------------------------- rules

/// Free space under which Windows updates fail and OBS recordings stop
/// early. Microsoft asks for room to update; 10 GB is a conservative floor.
/// https://support.microsoft.com/en-us/windows/free-up-drive-space-in-windows-85529ccb-c365-490d-b548-831022bc9b32
const LOW_SYSTEM_DISK_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// A driver older than this misses years of capture and encoder fixes.
/// A heuristic, so the advice is only a tip.
const OLD_DRIVER_DAYS: i64 = 2 * 365;

/// More programs than this starting with Windows is worth a look.
const MANY_STARTUP_APPS: usize = 15;

/// Share of frames lost to rendering or encoding that counts as "OBS was
/// struggling" for the HAGS troubleshooting tip.
const STRUGGLING_PCT: f32 = 1.0;

/// Hardware-accelerated GPU scheduling: the registry choice when the
/// streamer made one, else what OBS saw at startup.
fn hags_on(facts: &Facts) -> Option<bool> {
    if facts.system.hags.is_some() {
        return facts.system.hags;
    }
    let startup = facts.logs.startup.as_ref()?;
    let states: Vec<bool> = startup.adapters.iter().filter_map(|a| a.hags_enabled).collect();
    if states.is_empty() { None } else { Some(states.iter().any(|on| *on)) }
}

fn game_mode(facts: &Facts) -> Option<GameMode> {
    facts.system.game_mode.or_else(|| facts.logs.startup.as_ref()?.game_mode)
}

fn background_recording(facts: &Facts) -> Option<bool> {
    facts.system.background_recording.or_else(|| facts.logs.startup.as_ref()?.game_dvr_background_recording)
}

fn system_disk(facts: &Facts) -> Option<&Disk> {
    facts.system.disks.iter().find(|d| d.system)
}

/// The card from `hw.gpus` with the same name as the driver.
fn card_of<'a>(gpus: &'a [Gpu], driver: &GpuDriver) -> Option<&'a Gpu> {
    gpus.iter().find(|g| g.name.trim().eq_ignore_ascii_case(driver.name.trim()))
}

/// A laptop-style pair: a card built into the processor plus a real one.
fn has_two_cards(gpus: &[Gpu]) -> bool {
    let integrated = gpus.iter().any(Gpu::is_integrated);
    let dedicated = gpus.iter().any(|g| !g.is_integrated() && g.vendor != GpuVendor::Other);
    integrated && dedicated
}

pub fn advise(facts: &Facts, review: &mut Review) {
    let sys = &facts.system;
    if sys.windows.is_none() && sys.power_plan.is_none() && sys.disks.is_empty() {
        review.note("No pude revisar la configuración de Windows.");
        return;
    }
    advise_power(sys, review);
    advise_games(facts, review);
    advise_graphics(facts, review);
    advise_storage(facts, review);
    advise_background(sys, review);
}

fn advise_power(sys: &SystemFacts, review: &mut Review) {
    if sys.on_battery == Some(true) {
        review.add(Advice {
            id: "win.power.battery",
            level: Level::Recommended,
            area: Area::Windows,
            title: "Conecta el cargador antes de salir en directo".to_string(),
            why: "Tu notebook está usando la batería. Con batería, Windows y el procesador bajan la velocidad para que dure más, y el directo puede perder cuadros.".to_string(),
            steps: vec![
                "Conecta el cargador y déjalo puesto todo el directo.".to_string(),
                "Vuelve a correr OBS Doctor para ver el cambio.".to_string(),
            ],
        });
    } else if sys.on_battery == Some(false) && sys.has_battery == Some(true) {
        review.ok("Tu notebook está conectada al cargador.");
    }

    if sys.energy_saver == Some(true) {
        review.add(Advice {
            id: "win.power.energy-saver",
            level: Level::Recommended,
            area: Area::Windows,
            title: "Apaga el ahorro de energía de Windows".to_string(),
            why: "Con el ahorro de energía activado, Windows frena al procesador y a los programas en segundo plano. OBS puede quedarse corto y perder cuadros.".to_string(),
            steps: vec![
                "Abre Configuración, Sistema, Energía y batería (en una PC de escritorio se llama Energía).".to_string(),
                "Apaga Ahorro de energía. En versiones anteriores de Windows se llama Ahorro de batería.".to_string(),
            ],
        });
    }

    let slow_mode = sys.power_mode == Some(PowerMode::BestEfficiency);
    let slow_plan = sys.power_plan == Some(PowerPlan::PowerSaver);
    if slow_mode || slow_plan {
        let steps = if slow_mode {
            vec![
                "Abre Configuración, Sistema, Energía y batería (en una PC de escritorio se llama Energía)."
                    .to_string(),
                "En Modo de energía, elige la opción de más rendimiento.".to_string(),
                "Si la notebook se calienta mucho, prueba con la opción del medio.".to_string(),
            ]
        } else {
            vec![
                "Abre el Panel de control y entra en Hardware y sonido, Opciones de energía.".to_string(),
                "Elige el plan Equilibrado o Alto rendimiento.".to_string(),
            ]
        };
        review.add(Advice {
            id: "win.power.mode",
            level: Level::Recommended,
            area: Area::Windows,
            title: "Prueba Windows en un modo de energía de más rendimiento".to_string(),
            why: "Windows está configurado para gastar poca energía, así que el procesador trabaja más lento. Para jugar y transmitir a la vez conviene darle todo.".to_string(),
            steps,
        });
    }
}

fn advise_games(facts: &Facts, review: &mut Review) {
    // OBS used to say "turn Game Mode off". Since Windows 10 1809 it says it
    // can stay on, and Windows turns it on by default.
    // https://obsproject.com/kb/encoding-performance-troubleshooting
    match game_mode(facts) {
        Some(GameMode::Off) => review.add(Advice {
            id: "win.game-mode",
            level: Level::Tip,
            area: Area::Windows,
            title: "Prueba con el Modo de juego activado".to_string(),
            why: "El Modo de juego evita que Windows instale actualizaciones o drivers mientras juegas. La guía de OBS dice que en Windows actual puede quedar activado.".to_string(),
            steps: vec![
                "Abre Configuración, Juegos, Modo de juego y actívalo.".to_string(),
                "Haz un directo de prueba y compara.".to_string(),
            ],
        }),
        Some(GameMode::On | GameMode::ProbablyOn) => review.ok("El Modo de juego de Windows está activado."),
        None => {}
    }

    // OBS asks to turn off Game DVR, the background game recording: it
    // keeps the graphics card encoding video all the time, on top of OBS.
    // https://obsproject.com/kb/encoding-performance-troubleshooting
    match background_recording(facts) {
        Some(true) => review.add(Advice {
            id: "win.game-dvr.background",
            level: Level::Recommended,
            area: Area::Windows,
            title: "Apaga la grabación en segundo plano de Windows".to_string(),
            why: "Windows está grabando tus juegos todo el tiempo para que puedas guardar los últimos minutos. Esa grabación usa la misma placa de video que OBS y le quita fuerza al directo.".to_string(),
            steps: vec![
                "Abre Configuración, Juegos, Capturas.".to_string(),
                "Apaga la opción que graba lo que ha pasado mientras juegas.".to_string(),
            ],
        }),
        Some(false) => review.ok("La grabación en segundo plano de Windows está apagada."),
        None => {}
    }
}

fn advise_graphics(facts: &Facts, review: &mut Review) {
    let sys = &facts.system;
    let gpus = &facts.hw.gpus;

    // OBS only suggests turning HAGS off as a troubleshooting step when it
    // causes problems, so we only bring it up when the last stream lost
    // frames while preparing or drawing the video.
    // https://obsproject.com/kb/hags (2023-05-07)
    let struggled = facts.logs.last_stream.as_ref().filter(|s| !s.too_short()).is_some_and(|s| {
        s.lagged_render_pct.is_some_and(|p| p >= STRUGGLING_PCT)
            || s.skipped_encoding_pct.is_some_and(|p| p >= STRUGGLING_PCT)
    });
    if struggled && hags_on(facts) == Some(true) {
        review.add(Advice {
            id: "win.hags.try-off",
            level: Level::Tip,
            area: Area::Windows,
            title: "Si se siguen perdiendo cuadros, prueba sin la programación de GPU acelerada".to_string(),
            why: "En tu último directo OBS perdió cuadros al dibujar o preparar el video. La guía de OBS sugiere probar con esta opción de Windows apagada cuando pasa eso.".to_string(),
            steps: vec![
                "Abre Configuración, Sistema, Pantalla, Gráficos.".to_string(),
                "Busca \"Programación de GPU acelerada por hardware\" (en algunas versiones está dentro de \"Cambiar la configuración de gráficos predeterminada\") y apágala.".to_string(),
                "Reinicia la PC y haz un directo de prueba. Si no mejora, vuelve a activarla.".to_string(),
            ],
        });
    }

    for driver in &sys.drivers {
        let Some(card) = card_of(gpus, driver) else {
            // Leftover entry of a card that is not plugged in: ignore it.
            continue;
        };
        let name = text::product_name(&card.name);
        if driver.basic {
            review.add(Advice {
                id: "win.driver.missing",
                level: Level::Important,
                area: Area::Windows,
                title: "Instala el driver de tu placa de video".to_string(),
                why: format!(
                    "Windows está usando un driver básico para {name}. Así la placa no puede preparar el video del directo y todo se ve lento."
                ),
                steps: driver_steps(card.vendor),
            });
        } else if driver.age_days.is_some_and(|days| days > OLD_DRIVER_DAYS) {
            review.add(Advice {
                id: "win.driver.old",
                level: Level::Tip,
                area: Area::Windows,
                title: "Actualiza el driver de tu placa de video".to_string(),
                why: format!(
                    "El driver de {name} tiene más de dos años. Los drivers nuevos suelen corregir fallas de captura y de la preparación del video."
                ),
                steps: driver_steps(card.vendor),
            });
        }
    }

    // On laptops with two cards, OBS should draw with the strong one for
    // game and window capture.
    // https://github.com/obsproject/obs-studio/wiki/laptop-troubleshooting
    if has_two_cards(gpus) {
        let log_says_integrated = facts
            .logs
            .startup
            .as_ref()
            .and_then(|s| s.render_card())
            .and_then(|adapter| gpus.iter().find(|g| g.name.trim().eq_ignore_ascii_case(adapter.name.trim())))
            .is_some_and(Gpu::is_integrated);
        let on_weak_card = match sys.obs_gpu_preference {
            Some(GpuPreference::PowerSaving) => true,
            Some(GpuPreference::HighPerformance) => false,
            Some(GpuPreference::LetWindowsDecide) | None => log_says_integrated,
        };
        if on_weak_card {
            review.add(Advice {
                id: "win.gpu.obs-power-saving",
                level: Level::Recommended,
                area: Area::Windows,
                title: "Haz que OBS use tu placa de video más potente".to_string(),
                why: "Tu notebook tiene dos placas de video y OBS está usando la más débil, la que viene con el procesador. Con el juego en la otra placa, la captura puede salir lenta o con saltos.".to_string(),
                steps: vec![
                    "Cierra OBS.".to_string(),
                    "Abre Configuración, Sistema, Pantalla, Gráficos.".to_string(),
                    "Busca OBS Studio en la lista. Si no está, agrégalo con Examinar: es obs64.exe, dentro de la carpeta bin\\64bit de OBS.".to_string(),
                    "Elige Alto rendimiento y guarda.".to_string(),
                    "Si después la Captura de pantalla sale en negro, vuelve a dejarlo como estaba y usa Captura de juego o Captura de ventana.".to_string(),
                ],
            });
        }
    }
}

fn driver_steps(vendor: GpuVendor) -> Vec<String> {
    let first = match vendor {
        GpuVendor::Nvidia => {
            "Abre la aplicación NVIDIA (o GeForce Experience), entra en Controladores y descarga el más nuevo."
        }
        GpuVendor::Amd => {
            "Abre AMD Software: Adrenalin Edition y busca actualizaciones. Si no lo tienes, descárgalo desde amd.com."
        }
        GpuVendor::Intel => {
            "Descarga el Asistente de controladores y soporte de Intel desde intel.com y sigue sus pasos."
        }
        GpuVendor::Other => "Descarga el driver más nuevo desde la página del fabricante de tu placa de video.",
    };
    vec![
        first.to_string(),
        "En una notebook, si el instalador no reconoce la placa, busca el driver en la página del fabricante de la notebook.".to_string(),
        "Reinicia la PC al terminar.".to_string(),
    ]
}

fn advise_storage(facts: &Facts, review: &mut Review) {
    let sys = &facts.system;
    if let Some(disk) = system_disk(facts)
        && disk.free_bytes < LOW_SYSTEM_DISK_BYTES
    {
        review.add(Advice {
                id: "win.disk.system-low",
                level: Level::Recommended,
                area: Area::Windows,
                title: format!("Libera espacio en el disco {}:", disk.letter),
                why: format!(
                    "Al disco donde está Windows le quedan {} GB. Con tan poco espacio Windows se pone lento, las actualizaciones fallan y una grabación de OBS se corta cuando se llena.",
                    text::gb(disk.free_bytes)
                ),
                steps: vec![
                    "Abre Configuración, Sistema, Almacenamiento.".to_string(),
                    "Entra en Recomendaciones de limpieza y borra lo que no necesites.".to_string(),
                    "Si grabas tus directos, mueve las grabaciones viejas a otro disco.".to_string(),
                ],
            });
    }

    // Without a paging file, a program that runs out of memory is closed
    // by Windows on the spot, OBS or the game included.
    // https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/introduction-to-the-page-file
    if sys.page_file == Some(PageFile::Off) {
        review.add(Advice {
            id: "win.pagefile.off",
            level: Level::Recommended,
            area: Area::Windows,
            title: "Vuelve a activar el archivo de paginación".to_string(),
            why: "El archivo de paginación está apagado. Si la memoria se llena en pleno directo, Windows puede cerrar OBS o el juego sin aviso.".to_string(),
            steps: vec![
                "Abre Configuración, Sistema, Información y entra en Configuración avanzada del sistema.".to_string(),
                "En la pestaña Opciones avanzadas, dentro de Rendimiento, haz clic en Configuración.".to_string(),
                "En la pestaña Opciones avanzadas, dentro de Memoria virtual, haz clic en Cambiar.".to_string(),
                "Marca \"Administrar automáticamente el tamaño del archivo de paginación para todas las unidades\", acepta y reinicia la PC.".to_string(),
            ],
        });
    }
}

fn advise_background(sys: &SystemFacts, review: &mut Review) {
    if let Some(apps) = &sys.startup_apps
        && apps.len() > MANY_STARTUP_APPS
    {
        review.add(Advice {
            id: "win.startup.many",
            level: Level::Tip,
            area: Area::Windows,
            title: "Revisa los programas que arrancan con Windows".to_string(),
            why: format!(
                "Hay {} programas que se abren solos al prender la PC. Muchos quedan en segundo plano usando memoria y procesador durante el directo.",
                apps.len()
            ),
            steps: vec![
                "Abre Configuración, Aplicaciones, Inicio.".to_string(),
                "Apaga los que no uses todos los días. No se borran: solo dejan de abrirse solos.".to_string(),
            ],
        });
    }

    // Mode 3 lets Windows send updates to PCs on the internet, using the
    // same upload the stream needs.
    // https://learn.microsoft.com/en-us/windows/deployment/do/waas-delivery-optimization-reference#download-mode
    match sys.delivery_mode {
        Some(DeliveryMode::Internet) => review.add(Advice {
            id: "win.delivery-optimization.internet",
            level: Level::Tip,
            area: Area::Windows,
            title: "Haz que Windows no comparta actualizaciones por internet".to_string(),
            why: "Windows está enviando partes de sus actualizaciones a otras PC de internet. Eso usa la subida de tu internet, la misma que necesita el directo.".to_string(),
            steps: vec![
                "Abre Configuración, Windows Update, Opciones avanzadas, Optimización de distribución.".to_string(),
                "Apaga la opción que permite descargas desde otros equipos, o deja que solo comparta con los equipos de tu red local.".to_string(),
            ],
        }),
        Some(_) => {}
        None => review.note(
            "No pude ver si Windows comparte actualizaciones con otras PC de internet: ese ajuste solo se puede leer como administrador. Puedes mirarlo en Configuración, Windows Update, Opciones avanzadas, Optimización de distribución.",
        ),
    }
}

// ---------------------------------------------------------------- display

pub fn sections(facts: &Facts) -> Vec<Section> {
    let sys = &facts.system;
    let mut section = Section::new("windows", "Windows");
    if sys.windows.is_none() && sys.power_plan.is_none() && sys.disks.is_empty() {
        return vec![section.intro("No pude leer la configuración de Windows.")];
    }

    if let Some(version) = &sys.windows {
        section.push(Row::new("Versión", version.label()));
    }

    let mut power = sys.power_plan.map(PowerPlan::label).unwrap_or("No pude leerlo").to_string();
    if let Some(mode) = sys.power_mode {
        power = format!("{power}, modo {}", mode.label());
    }
    let power_tone = if matches!(sys.power_plan, Some(PowerPlan::PowerSaver))
        || sys.power_mode == Some(PowerMode::BestEfficiency)
        || sys.energy_saver == Some(true)
    {
        Tone::Warn
    } else {
        Tone::Neutral
    };
    let mut row = Row::new("Energía", power).tone(power_tone);
    if sys.energy_saver == Some(true) {
        row = row.note("El ahorro de energía está activado.");
    }
    section.push(row);

    if sys.has_battery == Some(true) {
        let (state, tone) = match sys.on_battery {
            Some(true) => ("Usando la batería", Tone::Warn),
            Some(false) => ("Conectada al cargador", Tone::Good),
            None => ("No pude saberlo", Tone::Neutral),
        };
        let value = match sys.battery_pct {
            Some(pct) => format!("{state} ({pct} %)"),
            None => state.to_string(),
        };
        section.push(Row::new("Batería", value).tone(tone));
    }

    if let Some(mode) = game_mode(facts) {
        let (value, tone) = match mode {
            GameMode::On => ("Activado", Tone::Good),
            GameMode::ProbablyOn => ("Activado (como viene Windows)", Tone::Good),
            GameMode::Off => ("Apagado", Tone::Warn),
        };
        section.push(Row::new("Modo de juego", value).tone(tone));
    }
    if let Some(on) = background_recording(facts) {
        let (value, tone) = if on { ("Activada", Tone::Bad) } else { ("Apagada", Tone::Good) };
        section.push(
            Row::new("Grabación de juegos", value)
                .tone(tone)
                .note("La grabación en segundo plano de Windows, que guarda los últimos minutos de juego."),
        );
    }
    if let Some(on) = hags_on(facts) {
        section.push(
            Row::new("Programación de GPU", if on { "Activada" } else { "Apagada" })
                .note("\"Programación de GPU acelerada por hardware\": la placa de video organiza su propio trabajo."),
        );
    }

    for driver in sys.drivers.iter().filter(|d| card_of(&facts.hw.gpus, d).is_some() || facts.hw.gpus.is_empty()) {
        let mut value = driver.version.clone().unwrap_or_else(|| "versión desconocida".to_string());
        if let Some(days) = driver.date_days {
            let (y, m, d) = civil_from_days(days);
            value = format!("{value}, del {d:02}/{m:02}/{y}");
        }
        let card = text::product_name(&driver.name);
        let tone = if driver.basic {
            value = "El básico de Windows".to_string();
            Tone::Bad
        } else if driver.age_days.is_some_and(|a| a > OLD_DRIVER_DAYS) {
            Tone::Warn
        } else {
            Tone::Neutral
        };
        section.push(Row::new("Driver de video", value).tone(tone).note(card));
    }

    if has_two_cards(&facts.hw.gpus) {
        let value = sys.obs_gpu_preference.map(GpuPreference::label).unwrap_or("Sin elegir (Windows decide)");
        let tone = if sys.obs_gpu_preference == Some(GpuPreference::PowerSaving) { Tone::Warn } else { Tone::Neutral };
        section.push(Row::new("Placa para OBS", value).tone(tone));
    }

    for (index, display) in sys.displays.iter().enumerate() {
        let label = if sys.displays.len() == 1 {
            "Pantalla".to_string()
        } else if display.primary {
            format!("Pantalla {} (principal)", index + 1)
        } else {
            format!("Pantalla {}", index + 1)
        };
        section.push(Row::new(label, format!("{} × {} a {} Hz", display.width, display.height, display.refresh_hz)));
    }

    for disk in &sys.disks {
        let used = if disk.total_bytes > 0 { 1.0 - disk.free_bytes as f32 / disk.total_bytes as f32 } else { 0.0 };
        let low = disk.system && disk.free_bytes < LOW_SYSTEM_DISK_BYTES;
        let mut row = Row::new(
            format!("Disco {}:", disk.letter),
            format!("{} GB libres de {} GB", text::gb(disk.free_bytes), text::gb(disk.total_bytes)),
        )
        .meter(used)
        .tone(if low { Tone::Bad } else { Tone::Neutral });
        if disk.system {
            row = row.note("Aquí está Windows.");
        }
        section.push(row);
    }

    if let Some(page) = sys.page_file {
        let (value, tone) = match page {
            PageFile::Off => ("Apagado".to_string(), Tone::Bad),
            PageFile::SystemManaged => ("Lo maneja Windows".to_string(), Tone::Good),
            PageFile::Custom { max_mb } => {
                (format!("Tamaño fijo, hasta {}", text::mem(u64::from(max_mb) * 1024 * 1024)), Tone::Neutral)
            }
        };
        section.push(Row::new("Archivo de paginación", value).tone(tone));
    }

    if let Some(apps) = &sys.startup_apps {
        let mut row = Row::new("Arrancan con Windows", text::plural(apps.len(), "programa", "programas"))
            .tone(if apps.len() > MANY_STARTUP_APPS { Tone::Warn } else { Tone::Neutral });
        if !apps.is_empty() {
            const SHOWN: usize = 8;
            let mut list = apps.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
            if apps.len() > SHOWN {
                list.push_str(&format!(" y {} más", apps.len() - SHOWN));
            }
            row = row.note(list);
        }
        section.push(row);
    }

    if let Some(mode) = sys.delivery_mode {
        let (value, tone) = match mode {
            DeliveryMode::Off => ("No comparte", Tone::Good),
            DeliveryMode::LocalNetwork => ("Solo con tu red local", Tone::Good),
            DeliveryMode::Internet => ("Comparte con PC de internet", Tone::Warn),
        };
        section.push(
            Row::new("Actualizaciones", value).tone(tone).note("Optimización de distribución de Windows Update."),
        );
    }

    vec![section]
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod win {
    use std::time::{SystemTime, UNIX_EPOCH};

    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, WIN32_ERROR};
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW,
        ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW,
    };
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD,
        RRF_RT_REG_MULTI_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    };
    use windows::core::{HSTRING, PCWSTR, PWSTR};

    use super::*;

    const GAME_BAR_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\GameDVR";
    const GAME_MODE_KEY: &str = r"Software\Microsoft\GameBar";
    const POWER_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes";
    const GRAPHICS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";
    /// The "Display adapters" device class.
    const DISPLAY_CLASS_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    const GPU_PREFS_KEY: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
    const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    const MEMORY_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN32_KEY: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
    const DO_POLICY_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization";
    /// The Network Service hive, where the Settings app stores the choice.
    const DO_SETTINGS_KEY: &str = r"S-1-5-20\Software\Microsoft\Windows\CurrentVersion\DeliveryOptimization\Settings";

    pub fn read() -> SystemFacts {
        let mut facts = SystemFacts::default();
        read_power(&mut facts);

        // Same reading as OBS: AutoGameModeEnabled, else AllowAutoGameMode,
        // else "probably on" from Windows 10 20H2 (19042) onwards.
        let windows = windows_version();
        facts.game_mode = match reg_dword(HKEY_CURRENT_USER, GAME_MODE_KEY, "AutoGameModeEnabled")
            .or_else(|| reg_dword(HKEY_CURRENT_USER, GAME_MODE_KEY, "AllowAutoGameMode"))
        {
            Some(0) => Some(GameMode::Off),
            Some(_) => Some(GameMode::On),
            None if windows.as_ref().is_some_and(|w| w.build >= 19042) => Some(GameMode::ProbablyOn),
            None => None,
        };
        facts.game_bar = reg_dword(HKEY_CURRENT_USER, GAME_BAR_KEY, "AppCaptureEnabled").map(|v| v != 0);
        facts.background_recording = reg_dword(HKEY_CURRENT_USER, GAME_BAR_KEY, "HistoricalCaptureEnabled")
            .map(|v| v != 0 && facts.game_bar != Some(false));
        facts.windows = windows;

        // 2 = on, 1 = off; anything else means the driver's default.
        facts.hags = match reg_dword(HKEY_LOCAL_MACHINE, GRAPHICS_KEY, "HwSchMode") {
            Some(2) => Some(true),
            Some(1) => Some(false),
            _ => None,
        };
        facts.drivers = drivers();
        facts.obs_gpu_preference = obs_gpu_preference();
        facts.displays = displays();
        facts.disks = disks();
        facts.page_file = reg_multi_sz(HKEY_LOCAL_MACHINE, MEMORY_KEY, "PagingFiles").map(|e| PageFile::parse(&e));
        facts.startup_apps = startup_apps();
        facts.delivery_mode = reg_dword(HKEY_LOCAL_MACHINE, DO_POLICY_KEY, "DODownloadMode")
            .or_else(|| reg_dword(HKEY_USERS, DO_SETTINGS_KEY, "DownloadMode"))
            .and_then(DeliveryMode::from_value);
        facts
    }

    fn read_power(facts: &mut SystemFacts) {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: `status` is a valid, writable SYSTEM_POWER_STATUS.
        if unsafe { GetSystemPowerStatus(&mut status) }.is_ok() {
            // BatteryFlag 128 = no battery, 255 = unknown; ACLineStatus 0 =
            // offline, 1 = online, 255 = unknown; SystemStatusFlag 1 = saver on.
            // https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-system_power_status
            let has_battery = match status.BatteryFlag {
                255 => None,
                flag => Some(flag & 128 == 0),
            };
            facts.has_battery = has_battery;
            if has_battery == Some(true) {
                facts.on_battery = match status.ACLineStatus {
                    0 => Some(true),
                    1 => Some(false),
                    _ => None,
                };
                facts.battery_pct = (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent);
            }
            facts.energy_saver = Some(status.SystemStatusFlag == 1);
        }

        facts.power_plan =
            reg_string(HKEY_LOCAL_MACHINE, POWER_KEY, "ActivePowerScheme").map(|g| PowerPlan::from_guid(&g));
        let overlay =
            if facts.on_battery == Some(true) { "ActiveOverlayDcPowerScheme" } else { "ActiveOverlayAcPowerScheme" };
        facts.power_mode = reg_string(HKEY_LOCAL_MACHINE, POWER_KEY, overlay).and_then(|g| PowerMode::from_guid(&g));
    }

    fn windows_version() -> Option<WindowsVersion> {
        let build = reg_string(HKEY_LOCAL_MACHINE, VERSION_KEY, "CurrentBuild")?.trim().parse().ok()?;
        let display_version = reg_string(HKEY_LOCAL_MACHINE, VERSION_KEY, "DisplayVersion").filter(|v| !v.is_empty());
        Some(WindowsVersion { build, display_version })
    }

    fn drivers() -> Vec<GpuDriver> {
        let Some(class) = Key::open(HKEY_LOCAL_MACHINE, DISPLAY_CLASS_KEY) else {
            return Vec::new();
        };
        let today = SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| (d.as_secs() / 86_400) as i64);
        let mut found = Vec::new();
        // Device instances are "0000", "0001"...; skip "Configuration" and "Properties".
        for sub in class.subkeys().into_iter().filter(|s| s.len() == 4 && s.chars().all(|c| c.is_ascii_digit())) {
            let path = format!(r"{DISPLAY_CLASS_KEY}\{sub}");
            let Some(name) = reg_string(HKEY_LOCAL_MACHINE, &path, "DriverDesc").filter(|n| !n.trim().is_empty())
            else {
                continue;
            };
            let date_days = reg_string(HKEY_LOCAL_MACHINE, &path, "DriverDate").and_then(|d| parse_driver_date(&d));
            let basic = reg_string(HKEY_LOCAL_MACHINE, &path, "InfPath")
                .is_some_and(|inf| inf.trim().eq_ignore_ascii_case("display.inf"));
            found.push(GpuDriver {
                name: name.trim().to_string(),
                version: reg_string(HKEY_LOCAL_MACHINE, &path, "DriverVersion").filter(|v| !v.is_empty()),
                date_days,
                age_days: date_days.zip(today).map(|(date, now)| now - date),
                basic,
            });
        }
        found
    }

    /// The preference stored for obs64.exe. The value name is the full
    /// path of the program (with the user name in it), so only the parsed
    /// preference leaves this function.
    fn obs_gpu_preference() -> Option<GpuPreference> {
        let key = Key::open(HKEY_CURRENT_USER, GPU_PREFS_KEY)?;
        let mut found = None;
        key.for_each_value(|name, data| {
            if found.is_none() && name.to_ascii_lowercase().ends_with(r"\obs64.exe") {
                found = GpuPreference::parse(&utf16_bytes_to_string(data));
            }
        });
        found
    }

    fn displays() -> Vec<Display> {
        let mut found = Vec::new();
        // A PC rarely has more than a handful of outputs; the cap only
        // guards against a misbehaving driver.
        for index in 0..32 {
            let mut device = DISPLAY_DEVICEW { cb: size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
            // SAFETY: `device` is a valid DISPLAY_DEVICEW with `cb` set as
            // the API requires; a null device name enumerates adapters.
            if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), index, &mut device, 0) }.as_bool() {
                break;
            }
            if !device.StateFlags.contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) {
                continue;
            }
            let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
            // SAFETY: `DeviceName` is a NUL-terminated array inside `device`,
            // which outlives the call; `mode` is valid with `dmSize` set.
            let ok =
                unsafe { EnumDisplaySettingsW(PCWSTR(device.DeviceName.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode) };
            if ok.as_bool() && mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0 {
                found.push(Display {
                    width: mode.dmPelsWidth,
                    height: mode.dmPelsHeight,
                    refresh_hz: mode.dmDisplayFrequency,
                    primary: device.StateFlags.contains(DISPLAY_DEVICE_PRIMARY_DEVICE),
                });
            }
        }
        found
    }

    fn disks() -> Vec<Disk> {
        let system_letter = std::env::var("SystemDrive")
            .ok()
            .and_then(|d| d.chars().next())
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('C');
        // SAFETY: no arguments; returns a bit mask of drive letters.
        let mask = unsafe { GetLogicalDrives() };
        let mut found = Vec::new();
        for (bit, letter) in ('A'..='Z').enumerate() {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let root = HSTRING::from(format!(r"{letter}:\"));
            // SAFETY: `root` is a valid NUL-terminated wide string.
            // 3 = DRIVE_FIXED: skips USB sticks, card readers and network drives.
            if unsafe { GetDriveTypeW(&root) } != 3 {
                continue;
            }
            let (mut free, mut total) = (0u64, 0u64);
            // SAFETY: `root` is valid and both out pointers are live locals.
            if unsafe { GetDiskFreeSpaceExW(&root, Some(&mut free), Some(&mut total), None) }.is_ok() && total > 0 {
                found.push(Disk { letter, free_bytes: free, total_bytes: total, system: letter == system_letter });
            }
        }
        found
    }

    /// Run keys plus the Startup folders, minus what the streamer disabled
    /// in Task Manager or Settings.
    fn startup_apps() -> Option<Vec<String>> {
        let mut names: Vec<String> = Vec::new();
        let mut read_any = false;
        let sources = [
            (HKEY_CURRENT_USER, RUN_KEY, "Run"),
            (HKEY_LOCAL_MACHINE, RUN_KEY, "Run"),
            (HKEY_LOCAL_MACHINE, RUN32_KEY, "Run32"),
        ];
        for (root, run, approved) in sources {
            let Some(key) = Key::open(root, run) else { continue };
            read_any = true;
            let approvals = approvals(root, approved);
            for name in key.value_names() {
                let approved = approvals.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)).map(|(_, b)| b.as_slice());
                if super::startup_enabled(approved) {
                    names.extend(super::startup_name(&name));
                }
            }
        }

        let folders =
            [(std::env::var_os("APPDATA"), HKEY_CURRENT_USER), (std::env::var_os("ProgramData"), HKEY_LOCAL_MACHINE)];
        for (base, root) in folders {
            let Some(base) = base else { continue };
            let dir = std::path::Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            read_any = true;
            let approvals = approvals(root, "StartupFolder");
            for entry in entries.flatten() {
                // Only the file name is kept: the folder path holds the user name.
                let file = entry.file_name().to_string_lossy().into_owned();
                let approved = approvals.iter().find(|(n, _)| n.eq_ignore_ascii_case(&file)).map(|(_, b)| b.as_slice());
                if super::startup_enabled(approved) {
                    names.extend(super::startup_name(&file));
                }
            }
        }

        if !read_any {
            return None;
        }
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Some(names)
    }

    fn approvals(root: HKEY, sub: &str) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        if let Some(key) = Key::open(root, &format!(r"{APPROVED_KEY}\{sub}")) {
            key.for_each_value(|name, data| out.push((name.to_string(), data.to_vec())));
        }
        out
    }

    // ------------------------------------------------------------ registry

    /// An open registry key, closed on drop.
    struct Key(HKEY);

    impl Key {
        fn open(root: HKEY, path: &str) -> Option<Self> {
            let mut key = HKEY::default();
            // SAFETY: `path` is a valid wide string for the call and `key`
            // is a live out pointer. On success we own the handle.
            let status = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) };
            (status == ERROR_SUCCESS).then_some(Key(key))
        }

        fn subkeys(&self) -> Vec<String> {
            let mut out = Vec::new();
            let mut buffer = [0u16; 256];
            for index in 0..1024 {
                let mut len = buffer.len() as u32;
                // SAFETY: open key; `buffer` and `len` describe a writable
                // buffer of `len` characters.
                let status = unsafe {
                    RegEnumKeyExW(self.0, index, Some(PWSTR(buffer.as_mut_ptr())), &mut len, None, None, None, None)
                };
                if status != ERROR_SUCCESS {
                    break;
                }
                out.push(String::from_utf16_lossy(&buffer[..(len as usize).min(buffer.len())]));
            }
            out
        }

        fn value_names(&self) -> Vec<String> {
            let mut out = Vec::new();
            self.for_each_value(|name, _| out.push(name.to_string()));
            out
        }

        /// Calls `f` with the name and raw data of every value. Values with
        /// more data than the buffer holds are skipped: none of the ones we
        /// read (flags, a short preference string) comes close.
        fn for_each_value(&self, mut f: impl FnMut(&str, &[u8])) {
            // 16383 characters is the registry's limit for a value name.
            let mut name = vec![0u16; 16_384];
            let mut data = vec![0u8; 1024];
            for index in 0..4096 {
                let mut name_len = name.len() as u32;
                let mut data_len = data.len() as u32;
                // SAFETY: open key; each buffer is paired with its length
                // and both outlive the call.
                let status: WIN32_ERROR = unsafe {
                    RegEnumValueW(
                        self.0,
                        index,
                        Some(PWSTR(name.as_mut_ptr())),
                        &mut name_len,
                        None,
                        None,
                        Some(data.as_mut_ptr()),
                        Some(&mut data_len),
                    )
                };
                if status == ERROR_MORE_DATA {
                    continue;
                }
                if status == ERROR_NO_MORE_ITEMS || status != ERROR_SUCCESS {
                    break;
                }
                let name = String::from_utf16_lossy(&name[..(name_len as usize).min(name.len())]);
                f(&name, &data[..(data_len as usize).min(data.len())]);
            }
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful RegOpenKeyExW and is
            // closed exactly once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn reg_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: wide strings live for the call; `value` has room for the
        // DWORD and `size` says so.
        let status = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(path),
                &HSTRING::from(name),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut value as *mut u32).cast()),
                Some(&mut size),
            )
        };
        (status == ERROR_SUCCESS).then_some(value)
    }

    /// Raw UTF-16 data of a string value, sized with a first query.
    fn reg_wide(root: HKEY, path: &str, name: &str, flags: REG_ROUTINE_FLAGS) -> Option<Vec<u16>> {
        let (path, name) = (HSTRING::from(path), HSTRING::from(name));
        for _ in 0..2 {
            let mut size = 0u32;
            // SAFETY: a null data pointer asks only for the size in bytes.
            let status = unsafe { RegGetValueW(root, &path, &name, flags, None, None, Some(&mut size)) };
            if status != ERROR_SUCCESS || size == 0 || size > 1 << 20 {
                return None;
            }
            let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
            // SAFETY: `buffer` holds `size` bytes, as `size` tells the API.
            let status = unsafe {
                RegGetValueW(root, &path, &name, flags, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size))
            };
            if status == ERROR_MORE_DATA {
                continue; // the value grew between the two calls
            }
            if status != ERROR_SUCCESS {
                return None;
            }
            buffer.truncate((size as usize / 2).min(buffer.len()));
            return Some(buffer);
        }
        None
    }

    fn reg_string(root: HKEY, path: &str, name: &str) -> Option<String> {
        let wide = reg_wide(root, path, name, RRF_RT_REG_SZ)?;
        let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        Some(String::from_utf16_lossy(&wide[..end]).trim().to_string())
    }

    fn reg_multi_sz(root: HKEY, path: &str, name: &str) -> Option<Vec<String>> {
        let wide = reg_wide(root, path, name, RRF_RT_REG_MULTI_SZ)?;
        Some(wide.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
    }

    fn utf16_bytes_to_string(data: &[u8]) -> String {
        let wide: Vec<u16> =
            data.as_chunks::<2>().0.iter().map(|&pair| u16::from_le_bytes(pair)).take_while(|&c| c != 0).collect();
        String::from_utf16_lossy(&wide)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::{gpu, pc};
    use crate::obs::logs::{Adapter, LogFacts, Startup, StreamStats};

    const GB: u64 = 1024 * 1024 * 1024;

    fn base() -> SystemFacts {
        SystemFacts {
            windows: Some(WindowsVersion { build: 26200, display_version: Some("25H2".into()) }),
            power_plan: Some(PowerPlan::Balanced),
            delivery_mode: Some(DeliveryMode::LocalNetwork),
            disks: vec![Disk { letter: 'C', free_bytes: 200 * GB, total_bytes: 500 * GB, system: true }],
            ..SystemFacts::default()
        }
    }

    fn facts(system: SystemFacts) -> Facts {
        Facts { system, ..Facts::default() }
    }

    fn review_of(facts: &Facts) -> Review {
        let mut review = Review::default();
        advise(facts, &mut review);
        review
    }

    fn laptop() -> crate::hardware::Hardware {
        pc(
            8,
            16,
            vec![
                gpu("Intel(R) UHD Graphics", GpuVendor::Intel, 0),
                gpu("NVIDIA GeForce RTX 3050 Laptop GPU", GpuVendor::Nvidia, 4),
            ],
        )
    }

    #[test]
    fn nothing_read_is_only_a_note() {
        let review = review_of(&facts(SystemFacts::default()));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert_eq!(review.notes.len(), 1);
        assert!(sections(&facts(SystemFacts::default()))[0].intro.is_some());
    }

    #[test]
    fn clean_pc_gets_no_advice() {
        let review = review_of(&facts(base()));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert!(review.notes.is_empty());
    }

    #[test]
    fn battery_and_saver() {
        let sys = SystemFacts { has_battery: Some(true), on_battery: Some(true), energy_saver: Some(true), ..base() };
        let review = review_of(&facts(sys));
        assert_eq!(review.get("win.power.battery").map(|a| a.level), Some(Level::Recommended));
        assert!(review.has("win.power.energy-saver"));

        let plugged = SystemFacts { has_battery: Some(true), on_battery: Some(false), ..base() };
        assert!(!review_of(&facts(plugged)).has("win.power.battery"));
    }

    #[test]
    fn efficiency_mode_or_saver_plan() {
        let mode = SystemFacts { power_mode: Some(PowerMode::BestEfficiency), ..base() };
        assert!(review_of(&facts(mode)).has("win.power.mode"));
        let plan = SystemFacts { power_plan: Some(PowerPlan::PowerSaver), ..base() };
        assert!(review_of(&facts(plan)).has("win.power.mode"));
        let fast = SystemFacts { power_mode: Some(PowerMode::BestPerformance), ..base() };
        assert!(!review_of(&facts(fast)).has("win.power.mode"));
    }

    #[test]
    fn game_mode_off_is_a_tip_and_log_fills_the_gap() {
        let off = SystemFacts { game_mode: Some(GameMode::Off), ..base() };
        assert_eq!(review_of(&facts(off)).get("win.game-mode").map(|a| a.level), Some(Level::Tip));

        let mut f = facts(base());
        f.logs.startup = Some(Startup { game_mode: Some(GameMode::Off), ..Startup::default() });
        assert!(review_of(&f).has("win.game-mode"));

        let on = SystemFacts { game_mode: Some(GameMode::ProbablyOn), ..base() };
        assert!(!review_of(&facts(on)).has("win.game-mode"));
    }

    #[test]
    fn background_recording() {
        let on = SystemFacts { background_recording: Some(true), ..base() };
        assert_eq!(review_of(&facts(on)).get("win.game-dvr.background").map(|a| a.level), Some(Level::Recommended));
        // Windows says off: an older log that said on does not count.
        let mut f = facts(SystemFacts { background_recording: Some(false), ..base() });
        f.logs.startup = Some(Startup { game_dvr_background_recording: Some(true), ..Startup::default() });
        assert!(!review_of(&f).has("win.game-dvr.background"));
    }

    fn struggling_stream() -> LogFacts {
        LogFacts {
            last_stream: Some(StreamStats {
                duration_secs: Some(3600),
                lagged_render_pct: Some(4.0),
                ..StreamStats::default()
            }),
            ..LogFacts::default()
        }
    }

    #[test]
    fn hags_only_with_evidence() {
        let mut f = facts(SystemFacts { hags: Some(true), ..base() });
        assert!(!review_of(&f).has("win.hags.try-off"), "no evidence, no advice");
        f.logs = struggling_stream();
        assert_eq!(review_of(&f).get("win.hags.try-off").map(|a| a.level), Some(Level::Tip));

        // Registry silent: the log's per-card state is used.
        let mut g = facts(base());
        g.logs = struggling_stream();
        g.logs.startup = Some(Startup {
            adapters: vec![Adapter { name: "A".into(), hags_enabled: Some(true), ..Adapter::default() }],
            ..Startup::default()
        });
        assert!(review_of(&g).has("win.hags.try-off"));

        let mut off = facts(SystemFacts { hags: Some(false), ..base() });
        off.logs = struggling_stream();
        assert!(!review_of(&off).has("win.hags.try-off"));
    }

    #[test]
    fn drivers_missing_old_and_stale_entries() {
        let mut sys = base();
        sys.drivers = vec![
            GpuDriver {
                name: "NVIDIA GeForce RTX 3050 Laptop GPU".into(),
                age_days: Some(900),
                ..GpuDriver::default()
            },
            GpuDriver { name: "Intel(R) UHD Graphics".into(), basic: true, ..GpuDriver::default() },
            // Not in hw.gpus: a card that was removed.
            GpuDriver { name: "AMD Radeon RX 580".into(), basic: true, ..GpuDriver::default() },
        ];
        let mut f = facts(sys);
        f.hw = laptop();
        let review = review_of(&f);
        assert_eq!(review.get("win.driver.missing").map(|a| a.level), Some(Level::Important));
        assert_eq!(review.get("win.driver.old").map(|a| a.level), Some(Level::Tip));
        assert_eq!(review.advice.iter().filter(|a| a.id == "win.driver.missing").count(), 1);

        let mut fresh = base();
        fresh.drivers =
            vec![GpuDriver { name: "Intel(R) UHD Graphics".into(), age_days: Some(100), ..GpuDriver::default() }];
        let mut f = facts(fresh);
        f.hw = laptop();
        assert!(!review_of(&f).has("win.driver.old"));
    }

    #[test]
    fn obs_on_the_weak_card() {
        let mut f = facts(SystemFacts { obs_gpu_preference: Some(GpuPreference::PowerSaving), ..base() });
        f.hw = laptop();
        assert_eq!(review_of(&f).get("win.gpu.obs-power-saving").map(|a| a.level), Some(Level::Recommended));

        // No preference stored: the OBS log says which card it drew with.
        let mut g = facts(base());
        g.hw = laptop();
        g.logs.startup = Some(Startup {
            adapters: vec![Adapter { name: "Intel(R) UHD Graphics".into(), ..Adapter::default() }],
            render_adapter: Some(0),
            ..Startup::default()
        });
        assert!(review_of(&g).has("win.gpu.obs-power-saving"));

        // High performance chosen: trust it over an older log.
        g.system.obs_gpu_preference = Some(GpuPreference::HighPerformance);
        assert!(!review_of(&g).has("win.gpu.obs-power-saving"));

        // One card only: nothing to choose.
        let mut h = facts(SystemFacts { obs_gpu_preference: Some(GpuPreference::PowerSaving), ..base() });
        h.hw = pc(8, 16, vec![gpu("NVIDIA GeForce RTX 3060", GpuVendor::Nvidia, 12)]);
        assert!(!review_of(&h).has("win.gpu.obs-power-saving"));
    }

    #[test]
    fn disk_pagefile_startup_and_delivery() {
        let mut sys = base();
        sys.disks = vec![
            Disk { letter: 'C', free_bytes: 3 * GB, total_bytes: 250 * GB, system: true },
            Disk { letter: 'D', free_bytes: GB, total_bytes: 1000 * GB, system: false },
        ];
        sys.page_file = Some(PageFile::Off);
        sys.startup_apps = Some((0..20).map(|i| format!("App {i}")).collect());
        sys.delivery_mode = Some(DeliveryMode::Internet);
        let review = review_of(&facts(sys));
        assert_eq!(review.get("win.disk.system-low").map(|a| a.level), Some(Level::Recommended));
        assert_eq!(review.advice.iter().filter(|a| a.id == "win.disk.system-low").count(), 1);
        assert!(review.has("win.pagefile.off"));
        assert_eq!(review.get("win.startup.many").map(|a| a.level), Some(Level::Tip));
        assert!(review.has("win.delivery-optimization.internet"));

        let unknown = SystemFacts { delivery_mode: None, ..base() };
        assert_eq!(review_of(&facts(unknown)).notes.len(), 1);
    }

    #[test]
    fn page_file_parsing() {
        let p = |v: &[&str]| PageFile::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(p(&[]), PageFile::Off);
        assert_eq!(p(&["", "  "]), PageFile::Off);
        assert_eq!(p(&[r"?:\pagefile.sys"]), PageFile::SystemManaged);
        assert_eq!(p(&[r"C:\pagefile.sys 0 0"]), PageFile::SystemManaged);
        assert_eq!(p(&[r"C:\pagefile.sys 1024 4096", r"D:\pagefile.sys 2048 8192"]), PageFile::Custom { max_mb: 8192 });
        assert_eq!(p(&[r"C:\pagefile.sys 0 garbage"]), PageFile::SystemManaged);
    }

    #[test]
    fn guids_and_values() {
        assert_eq!(PowerPlan::from_guid("381B4222-F694-41F0-9685-FF5BB260DF2E"), PowerPlan::Balanced);
        assert_eq!(PowerPlan::from_guid("{a1841308-3541-4fab-bc81-f71556f20b4a}"), PowerPlan::PowerSaver);
        assert_eq!(PowerPlan::from_guid("basura"), PowerPlan::Other);
        assert_eq!(PowerMode::from_guid("961cc777-2547-4f9d-8174-7d86181b8a7a"), Some(PowerMode::BestEfficiency));
        assert_eq!(PowerMode::from_guid("00000000-0000-0000-0000-000000000000"), Some(PowerMode::Balanced));
        assert_eq!(PowerMode::from_guid(""), None);
        assert_eq!(DeliveryMode::from_value(3), Some(DeliveryMode::Internet));
        assert_eq!(DeliveryMode::from_value(99), Some(DeliveryMode::Off));
        assert_eq!(DeliveryMode::from_value(7), None);
        assert_eq!(GpuPreference::parse("GpuPreference=2;"), Some(GpuPreference::HighPerformance));
        assert_eq!(
            GpuPreference::parse("SwapEffectUpgradeEnable=1;GpuPreference=1;"),
            Some(GpuPreference::PowerSaving)
        );
        assert_eq!(GpuPreference::parse("SwapEffectUpgradeEnable=1;"), None);
        assert_eq!(GpuPreference::parse(""), None);
    }

    #[test]
    fn driver_dates() {
        assert_eq!(parse_driver_date("1-1-2000"), Some(10_957));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // Placeholder dates some drivers carry are not real release dates.
        assert_eq!(parse_driver_date("1-1-1970"), None);
        let day = parse_driver_date("9-12-2025").unwrap_or_default();
        assert_eq!(civil_from_days(day), (2025, 9, 12));
        assert_eq!(parse_driver_date("2-29-2024").map(civil_from_days), Some((2024, 2, 29)));
        assert_eq!(parse_driver_date("13-1-2025"), None);
        assert_eq!(parse_driver_date("2025"), None);
        assert_eq!(parse_driver_date(""), None);
        assert_eq!(parse_driver_date("1-2-3-4"), None);
    }

    #[test]
    fn startup_entries() {
        assert!(startup_enabled(None));
        assert!(startup_enabled(Some(&[2, 0, 0])));
        assert!(startup_enabled(Some(&[6, 0])));
        assert!(!startup_enabled(Some(&[3, 0, 0])));
        assert!(!startup_enabled(Some(&[7])));
        assert!(startup_enabled(Some(&[])));
        assert_eq!(startup_name("Discord.lnk").as_deref(), Some("Discord"));
        assert_eq!(
            startup_name("MicrosoftEdgeAutoLaunch_0A1B2C3D4E5F60718293A4B5C6D7E8F9").as_deref(),
            Some("MicrosoftEdgeAutoLaunch")
        );
        assert_eq!(startup_name("Steam_Helper").as_deref(), Some("Steam_Helper"));
        assert_eq!(startup_name("desktop.ini"), None);
        assert_eq!(startup_name("  "), None);
    }

    #[test]
    fn section_lists_what_was_read() {
        let mut sys = base();
        sys.page_file = Some(PageFile::SystemManaged);
        sys.displays = vec![Display { width: 1920, height: 1080, refresh_hz: 144, primary: true }];
        let rows = &sections(&facts(sys))[0].rows;
        assert!(rows.len() >= 5);
        assert!(rows.iter().any(|r| r.tone == Tone::Good));
    }
}
