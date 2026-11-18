//! Programs running in the background and how much they are using.

use std::collections::HashMap;
use std::time::Duration;

use sysinfo::System;

use crate::facts::Facts;
use crate::model::{ProgramCard, ProgramKind, Review};
use crate::report::text;

/// How long we watch the machine before measuring processor use per program.
const SAMPLE_TIME: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Browser,
    Chat,
    Launcher,
    CloudSync,
    Download,
    Capture,
    Peripherals,
    Heavy,
    Music,
}

impl Category {
    /// Plain-language reason shown to the streamer.
    pub fn tip(self) -> &'static str {
        match self {
            Category::Browser => {
                "Cada pestaña abierta ocupa memoria. Deja solo las que uses en el directo (chat, alertas) y cierra el resto."
            }
            Category::Chat => {
                "Si no lo vas a usar durante el directo, ciérralo. Si lo usas, desactiva su superposición dentro del juego."
            }
            Category::Launcher => {
                "Si el juego que vas a transmitir no es de esta tienda, ciérrala: puede ponerse a descargar actualizaciones en pleno directo."
            }
            Category::CloudSync => {
                "Ponlo en pausa mientras transmites: cuando sincroniza archivos usa tu disco y tu internet de subida."
            }
            Category::Download => {
                "Ciérralo sí o sí: compite con el directo por tu internet de subida y es la causa típica de cortes."
            }
            Category::Capture => {
                "Graba o captura la pantalla al mismo tiempo que OBS. Dos programas capturando a la vez se pisan: deja solo OBS."
            }
            Category::Peripherals => {
                "Los programas de luces y periféricos gastan procesador todo el tiempo. Configura tus luces y ciérralo."
            }
            Category::Heavy => {
                "Es un programa pesado. Si no lo necesitas para el directo, ciérralo y recuperas memoria y procesador."
            }
            Category::Music => "No es un problema, pero tenlo en cuenta: si no vas a poner música, puedes cerrarlo.",
        }
    }

    /// Worth mentioning even when it is barely using anything right now.
    fn always_report(self) -> bool {
        matches!(self, Category::Download | Category::CloudSync | Category::Capture)
    }
}

/// (executable name without `.exe`, friendly name, category)
const KNOWN: &[(&str, &str, Category)] = &[
    ("chrome", "Google Chrome", Category::Browser),
    ("msedge", "Microsoft Edge", Category::Browser),
    ("firefox", "Firefox", Category::Browser),
    ("opera", "Opera", Category::Browser),
    ("brave", "Brave", Category::Browser),
    ("vivaldi", "Vivaldi", Category::Browser),
    ("discord", "Discord", Category::Chat),
    ("slack", "Slack", Category::Chat),
    ("ms-teams", "Microsoft Teams", Category::Chat),
    ("teams", "Microsoft Teams", Category::Chat),
    ("whatsapp", "WhatsApp", Category::Chat),
    ("telegram", "Telegram", Category::Chat),
    ("zoom", "Zoom", Category::Chat),
    ("skype", "Skype", Category::Chat),
    ("steam", "Steam", Category::Launcher),
    ("steamwebhelper", "Steam", Category::Launcher),
    ("epicgameslauncher", "Epic Games Launcher", Category::Launcher),
    ("battle.net", "Battle.net", Category::Launcher),
    ("eadesktop", "EA App", Category::Launcher),
    ("galaxyclient", "GOG Galaxy", Category::Launcher),
    ("upc", "Ubisoft Connect", Category::Launcher),
    ("ubisoftconnect", "Ubisoft Connect", Category::Launcher),
    ("onedrive", "OneDrive", Category::CloudSync),
    ("dropbox", "Dropbox", Category::CloudSync),
    ("googledrivefs", "Google Drive", Category::CloudSync),
    ("megasync", "MEGA", Category::CloudSync),
    ("qbittorrent", "qBittorrent", Category::Download),
    ("utorrent", "uTorrent", Category::Download),
    ("bittorrent", "BitTorrent", Category::Download),
    ("transmission-qt", "Transmission", Category::Download),
    ("gamebar", "Xbox Game Bar", Category::Capture),
    ("medal", "Medal", Category::Capture),
    ("overwolf", "Overwolf", Category::Capture),
    ("outplayed", "Outplayed", Category::Capture),
    ("streamlabs obs", "Streamlabs", Category::Capture),
    ("streamlabs desktop", "Streamlabs", Category::Capture),
    ("action", "Mirillis Action!", Category::Capture),
    ("bandicam", "Bandicam", Category::Capture),
    ("icue", "Corsair iCUE", Category::Peripherals),
    ("lghub", "Logitech G HUB", Category::Peripherals),
    ("lghub_agent", "Logitech G HUB", Category::Peripherals),
    ("razer synapse 3", "Razer Synapse", Category::Peripherals),
    ("razerappengine", "Razer Synapse", Category::Peripherals),
    ("armourycrate", "ASUS Armoury Crate", Category::Peripherals),
    ("armourycrate.service", "ASUS Armoury Crate", Category::Peripherals),
    ("nzxt cam", "NZXT CAM", Category::Peripherals),
    ("signalrgb", "SignalRGB", Category::Peripherals),
    ("wallpaper32", "Wallpaper Engine", Category::Peripherals),
    ("wallpaper64", "Wallpaper Engine", Category::Peripherals),
    ("photoshop", "Photoshop", Category::Heavy),
    ("adobe premiere pro", "Premiere Pro", Category::Heavy),
    ("afterfx", "After Effects", Category::Heavy),
    ("creative cloud", "Adobe Creative Cloud", Category::Heavy),
    ("resolve", "DaVinci Resolve", Category::Heavy),
    ("blender", "Blender", Category::Heavy),
    ("code", "Visual Studio Code", Category::Heavy),
    ("devenv", "Visual Studio", Category::Heavy),
    ("docker desktop", "Docker Desktop", Category::Heavy),
    ("com.docker.backend", "Docker Desktop", Category::Heavy),
    ("vmmem", "Máquina virtual (WSL o Docker)", Category::Heavy),
    ("vmmemwsl", "Máquina virtual (WSL o Docker)", Category::Heavy),
    ("spotify", "Spotify", Category::Music),
];

/// Windows internals, security software and OBS itself: never suggest closing these.
const IGNORED: &[&str] = &[
    "system",
    "idle",
    "system idle process",
    "registry",
    "memory compression",
    "memcompression",
    "secure system",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "svchost",
    "dwm",
    "explorer",
    "fontdrvhost",
    "sihost",
    "taskhostw",
    "ctfmon",
    "conhost",
    "runtimebroker",
    "searchhost",
    "searchindexer",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "textinputhost",
    "audiodg",
    "spoolsv",
    "wmiprvse",
    "dllhost",
    "msmpeng",
    "nissrv",
    "securityhealthservice",
    "mpdefendercoreservice",
    "obs64",
    "obs32",
    "obs",
    "obs-browser-page",
    "obs-ffmpeg-mux",
    "obs-doctor",
    "windowsterminal",
    "openconsole",
    "cmd",
    "powershell",
    "pwsh",
];

const MIB: u64 = 1024 * 1024;
/// A known program is worth a line once it uses this much.
const KNOWN_MIN_MEM: u64 = 150 * MIB;
const KNOWN_MIN_CPU: f32 = 1.0;
/// An unknown program must be clearly heavy before we point at it.
const UNKNOWN_MIN_MEM: u64 = 700 * MIB;
const UNKNOWN_MIN_CPU: f32 = 8.0;
const MAX_UNKNOWN: usize = 5;

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    pub category: Option<Category>,
    pub process_count: usize,
    pub mem_bytes: u64,
    /// Share of the whole processor (0-100), not of a single core.
    pub cpu_pct: f32,
}

#[derive(Debug, Default)]
pub struct Apps {
    /// Programs we recognise, heaviest first.
    pub known: Vec<App>,
    /// Heavy programs we do not recognise (it may be the game).
    pub unknown: Vec<App>,
    pub obs_running: bool,
}

/// Watches the machine for a moment and reports what is running.
pub fn read(sys: &mut System) -> Apps {
    std::thread::sleep(SAMPLE_TIME);
    sys.refresh_all();
    let cores = sys.cpus().len().max(1) as f32;
    let samples = sys.processes().values().map(|p| Sample {
        exe: p.name().to_string_lossy().into_owned(),
        mem_bytes: p.memory(),
        cpu_pct: p.cpu_usage() / cores,
    });
    classify(samples)
}

/// The programs as the report shows them.
pub fn cards(facts: &Facts) -> Vec<ProgramCard> {
    let apps = &facts.apps;
    let known = apps.known.iter().map(|app| ProgramCard {
        name: app.name.clone(),
        kind: ProgramKind::Review,
        usage: usage(app),
        tip: app.category.map(Category::tip).unwrap_or_default().to_string(),
    });
    let unknown = apps.unknown.iter().map(|app| ProgramCard {
        name: app.name.clone(),
        kind: ProgramKind::Unknown,
        usage: usage(app),
        tip: String::new(),
    });
    known.chain(unknown).collect()
}

fn usage(app: &App) -> String {
    let mut parts = vec![format!("{} de memoria", text::mem(app.mem_bytes))];
    if app.cpu_pct >= 1.0 {
        parts.push(format!("{} % del procesador", app.cpu_pct.round()));
    }
    if app.process_count > 1 {
        parts.push(format!("{} procesos", app.process_count));
    }
    parts.join(", ")
}

pub fn advise(_facts: &Facts, _review: &mut Review) {}

pub struct Sample {
    pub exe: String,
    pub mem_bytes: u64,
    pub cpu_pct: f32,
}

/// Groups processes by program and keeps only the ones worth mentioning.
pub fn classify(samples: impl Iterator<Item = Sample>) -> Apps {
    let mut groups: HashMap<String, App> = HashMap::new();
    let mut obs_running = false;

    for s in samples {
        let key = normalize(&s.exe);
        if key.starts_with("obs64") || key == "obs32" || key == "obs" {
            obs_running = true;
        }
        if IGNORED.contains(&key.as_str()) {
            continue;
        }
        let known = KNOWN.iter().find(|(exe, _, _)| *exe == key);
        // Several executables can belong to one program (Steam, G HUB...).
        let (group_key, name, category) = match known {
            Some((_, friendly, cat)) => (friendly.to_string(), friendly.to_string(), Some(*cat)),
            None => (key.clone(), s.exe.clone(), None),
        };
        let app =
            groups.entry(group_key).or_insert(App { name, category, process_count: 0, mem_bytes: 0, cpu_pct: 0.0 });
        app.process_count += 1;
        app.mem_bytes += s.mem_bytes;
        app.cpu_pct += s.cpu_pct;
    }

    let mut known = Vec::new();
    let mut unknown = Vec::new();
    for app in groups.into_values() {
        match app.category {
            Some(cat) => {
                if cat.always_report() || app.mem_bytes >= KNOWN_MIN_MEM || app.cpu_pct >= KNOWN_MIN_CPU {
                    known.push(app);
                }
            }
            None => {
                if app.mem_bytes >= UNKNOWN_MIN_MEM || app.cpu_pct >= UNKNOWN_MIN_CPU {
                    unknown.push(app);
                }
            }
        }
    }
    known.sort_by_key(|a| std::cmp::Reverse(a.mem_bytes));
    unknown.sort_by_key(|a| std::cmp::Reverse(a.mem_bytes));
    unknown.truncate(MAX_UNKNOWN);

    Apps { known, unknown, obs_running }
}

fn normalize(exe: &str) -> String {
    let lower = exe.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(exe: &str, mem_mb: u64, cpu: f32) -> Sample {
        Sample { exe: exe.to_string(), mem_bytes: mem_mb * MIB, cpu_pct: cpu }
    }

    #[test]
    fn groups_all_processes_of_one_program() {
        let apps = classify(
            vec![sample("chrome.exe", 300, 0.5), sample("chrome.exe", 200, 0.5), sample("Chrome.exe", 100, 0.0)]
                .into_iter(),
        );
        assert_eq!(apps.known.len(), 1);
        let chrome = &apps.known[0];
        assert_eq!(chrome.name, "Google Chrome");
        assert_eq!(chrome.process_count, 3);
        assert_eq!(chrome.mem_bytes, 600 * MIB);
    }

    #[test]
    fn never_points_at_windows_or_antivirus() {
        let apps = classify(
            vec![sample("svchost.exe", 2000, 30.0), sample("MsMpEng.exe", 900, 20.0), sample("dwm.exe", 800, 10.0)]
                .into_iter(),
        );
        assert!(apps.known.is_empty());
        assert!(apps.unknown.is_empty());
    }

    #[test]
    fn torrents_are_reported_even_when_idle() {
        let apps = classify(vec![sample("qbittorrent.exe", 40, 0.0)].into_iter());
        assert_eq!(apps.known.len(), 1);
        assert_eq!(apps.known[0].category, Some(Category::Download));
    }

    #[test]
    fn light_known_programs_are_left_alone() {
        let apps = classify(vec![sample("Spotify.exe", 80, 0.1)].into_iter());
        assert!(apps.known.is_empty());
    }

    #[test]
    fn unknown_programs_only_show_up_when_heavy() {
        let apps = classify(vec![sample("juego.exe", 4000, 35.0), sample("cosita.exe", 50, 0.2)].into_iter());
        assert_eq!(apps.unknown.len(), 1);
        assert_eq!(apps.unknown[0].name, "juego.exe");
    }

    #[test]
    fn detects_obs_running_without_listing_it() {
        let apps = classify(vec![sample("obs64.exe", 900, 12.0)].into_iter());
        assert!(apps.obs_running);
        assert!(apps.known.is_empty() && apps.unknown.is_empty());
    }
}
