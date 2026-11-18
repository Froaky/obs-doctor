//! Programs running in the background and how much they are using.
//!
//! We only look: nothing here closes, pauses or touches any program. The
//! executable path of each process is read once, turned into a yes/no ("is it
//! inside a game library?") and dropped on the spot: it is never stored,
//! shown or sent.

pub mod catalog;

use std::collections::HashMap;
use std::time::Duration;

use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::facts::Facts;
use crate::model::{Advice, Area, Level, ProgramCard, ProgramKind, Review};
use crate::report::text;
pub use catalog::Category;

/// We take a few snapshots instead of one: a single reading catches programs
/// in the middle of a spike (or a lull) and misjudges them.
const ROUNDS: usize = 3;
/// sysinfo needs some time between two readings to measure processor use.
const ROUND_GAP: Duration = Duration::from_millis(1000);

const MIB: u64 = 1024 * 1024;
/// A known program is worth a line once it uses this much.
const KNOWN_MIN_MEM: u64 = 150 * MIB;
const KNOWN_MIN_CPU: f32 = 1.0;
/// An unknown program must be clearly heavy before we point at it.
const UNKNOWN_MIN_MEM: u64 = 700 * MIB;
const UNKNOWN_MIN_CPU: f32 = 8.0;
const MAX_UNKNOWN: usize = 5;
/// Something inside a game library is only "the game" when it is busy: game
/// folders also hold small updaters and helpers.
const GAME_MIN_MEM: u64 = 300 * MIB;
const GAME_MIN_CPU: f32 = 3.0;

/// OBS itself: noticed, never listed.
const OBS: &[&str] = &["obs64", "obs32", "obs", "obs-browser-page", "obs-ffmpeg-mux"];

/// Folders where the game stores install games. Lowercase, with `\` on both
/// sides so that "Epic Games" never matches "My Epic Games Clips".
const GAME_LIBRARIES: &[&str] = &[
    r"\steamapps\common\",
    r"\epic games\",
    r"\gog galaxy\games\",
    r"\gog games\",
    r"\riot games\",
    r"\ea games\",
    r"\origin games\",
    r"\ubisoft game launcher\games\",
    r"\xboxgames\",
    r"\rockstar games\",
];

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    /// `None` for programs we do not recognise.
    pub category: Option<Category>,
    /// The game library or the game table says it is a game.
    pub is_game: bool,
    /// OBS knowledge-base fix that replaces the category tip.
    pub note: Option<&'static str>,
    pub process_count: usize,
    pub mem_bytes: u64,
    /// Share of the whole processor (0-100), not of a single core.
    pub cpu_pct: f32,
}

impl App {
    pub fn kind(&self) -> ProgramKind {
        if self.is_game {
            return ProgramKind::Game;
        }
        self.category.and_then(Category::kind).unwrap_or(ProgramKind::Unknown)
    }
}

#[derive(Debug, Default)]
pub struct Apps {
    /// Programs we recognise or detected as the game, heaviest first.
    pub known: Vec<App>,
    /// Heavy programs we do not recognise.
    pub unknown: Vec<App>,
    /// For cross-area rules in `diagnosis.rs`, e.g. "OBS is open, so its own
    /// load is part of what the PC is carrying right now".
    #[allow(dead_code)]
    pub obs_running: bool,
    /// How many processes we saw. Zero means we could not look at all.
    pub seen: usize,
}

impl Apps {
    fn in_category(&self, category: Category) -> impl Iterator<Item = &App> {
        self.known.iter().filter(move |a| a.category == Some(category))
    }
}

/// One process as measured, already stripped of anything private.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Executable file name, e.g. `chrome.exe`. Never the full path.
    pub exe: String,
    /// Executable file name of the parent process, if it is still running.
    pub parent_exe: Option<String>,
    /// Lives inside a game store library folder.
    pub in_game_library: bool,
    pub mem_bytes: u64,
    pub cpu_pct: f32,
}

/// Watches the machine for a few seconds and reports what is running.
pub fn read(sys: &mut System) -> Apps {
    let cores = sys.cpus().len().max(1) as f32;
    let mut tally = Tally::default();
    for _ in 0..ROUNDS {
        std::thread::sleep(ROUND_GAP);
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let processes = sys.processes();
        for (pid, process) in processes {
            let parent_exe = process
                .parent()
                .and_then(|parent| processes.get(&parent))
                .map(|parent| parent.name().to_string_lossy().into_owned());
            // The path is only used for this yes/no and goes out of scope here.
            let in_game_library = process.exe().is_some_and(|path| is_game_library(&path.to_string_lossy()));
            tally.add(
                *pid,
                Sample {
                    exe: process.name().to_string_lossy().into_owned(),
                    parent_exe,
                    in_game_library,
                    mem_bytes: process.memory(),
                    cpu_pct: process.cpu_usage() / cores,
                },
            );
        }
    }
    classify(tally.finish(ROUNDS).into_iter())
}

/// Folds several snapshots into one reading per process: average processor
/// use (a process that appeared halfway counts as idle before it existed) and
/// peak memory.
#[derive(Debug, Default)]
pub struct Tally {
    by_pid: HashMap<Pid, Sample>,
}

impl Tally {
    pub fn add(&mut self, pid: Pid, sample: Sample) {
        match self.by_pid.get_mut(&pid) {
            // A reused pid with another program is a different process; the
            // first one is gone, so the newest reading wins.
            Some(seen) if seen.exe == sample.exe => {
                seen.cpu_pct += sample.cpu_pct;
                seen.mem_bytes = seen.mem_bytes.max(sample.mem_bytes);
                seen.in_game_library |= sample.in_game_library;
                if seen.parent_exe.is_none() {
                    seen.parent_exe = sample.parent_exe;
                }
            }
            _ => {
                self.by_pid.insert(pid, sample);
            }
        }
    }

    pub fn finish(self, rounds: usize) -> Vec<Sample> {
        let rounds = rounds.max(1) as f32;
        self.by_pid
            .into_values()
            .map(|mut s| {
                s.cpu_pct /= rounds;
                s
            })
            .collect()
    }
}

/// Whether an executable path sits inside a game store library. Pure, so the
/// path never needs to leave the caller.
pub fn is_game_library(path: &str) -> bool {
    let path = path.to_lowercase().replace('/', "\\");
    GAME_LIBRARIES.iter().any(|lib| path.contains(lib))
}

/// The programs as the report shows them.
pub fn cards(facts: &Facts) -> Vec<ProgramCard> {
    let apps = &facts.apps;
    apps.known
        .iter()
        .chain(apps.unknown.iter())
        .map(|app| ProgramCard {
            name: app.name.clone(),
            kind: app.kind(),
            usage: usage(app),
            tip: tip(app).to_string(),
        })
        .collect()
}

fn tip(app: &App) -> &'static str {
    if app.is_game {
        return "";
    }
    app.note.or(app.category.map(Category::tip)).unwrap_or_default()
}

fn usage(app: &App) -> String {
    let mut parts = vec![format!("{} de memoria", text::mem(app.mem_bytes))];
    if app.cpu_pct >= 1.0 {
        parts.push(format!("{} del procesador", text::pct(app.cpu_pct)));
    }
    if app.process_count > 1 {
        parts.push(format!("{} procesos", app.process_count));
    }
    parts.join(", ")
}

/// Rules only for what can really hurt the stream; everything else is a tip
/// on the program's own card.
pub fn advise(facts: &Facts, review: &mut Review) {
    let apps = &facts.apps;
    if apps.seen == 0 {
        review.note("No pude ver qué programas tienes abiertos.");
        return;
    }

    let downloads = names(apps.in_category(Category::Download));
    if !downloads.is_empty() {
        review.add(Advice {
            id: "apps.download.open",
            level: Level::Recommended,
            area: Area::Programs,
            title: format!("Cierra {} antes del directo", list(&downloads)),
            why: "Los programas de torrents y descargas pueden usar la subida de tu internet en segundo plano. Le quitan espacio al directo y causan cortes."
                .to_string(),
            steps: vec![
                "Si tienes una descarga a medias, ponla en pausa o espera a que termine.".to_string(),
                "Busca su icono junto al reloj de Windows, haz clic derecho y elige la opción para salir. Cerrar la ventana a veces solo lo esconde."
                    .to_string(),
            ],
        });
    }

    let recorders: Vec<&App> = apps.in_category(Category::Recorder).collect();
    if recorders.iter().any(|a| a.name == catalog::D3DGEAR) {
        // https://obsproject.com/kb/known-conflicts: closing it is not enough.
        review.add(Advice {
            id: "apps.capture.d3dgear",
            level: Level::Recommended,
            area: Area::Programs,
            title: "Desinstala D3DGear".to_string(),
            why: "Deja ganchos en todos los juegos aunque esté cerrado, y eso choca con la captura de OBS.".to_string(),
            steps: vec![
                "Abre Configuración > Aplicaciones > Aplicaciones instaladas.".to_string(),
                "Busca D3DGear, haz clic en los tres puntos y elige Desinstalar.".to_string(),
            ],
        });
    }
    let others = names(recorders.into_iter().filter(|a| a.name != catalog::D3DGEAR));
    if !others.is_empty() {
        // https://obsproject.com/kb/known-conflicts ("Other Capture / Recording Software")
        review.add(Advice {
            id: "apps.capture.other",
            level: Level::Recommended,
            area: Area::Programs,
            title: format!("Cierra {} y deja solo OBS capturando", list(&others)),
            why: "Dos programas que graban o capturan el juego a la vez se pisan. La captura de OBS puede quedar en negro o el juego puede ir más lento."
                .to_string(),
            steps: vec![
                "Busca su icono junto al reloj de Windows, haz clic derecho y elige la opción para salir.".to_string(),
                "Si lo usas para guardar clips, apaga esa función o ciérralo solo durante el directo.".to_string(),
            ],
        });
    }

    let shapers: Vec<&App> = apps.in_category(Category::NetShaper).collect();
    if !shapers.is_empty() {
        // https://obsproject.com/kb/stream-connection-troubleshooting
        let mut steps: Vec<String> = Vec::new();
        for app in &shapers {
            let step = match app.name.as_str() {
                "Killer Network" => {
                    "Killer: abre Configuración > Aplicaciones > Aplicaciones instaladas y desinstala el paquete de Killer. Después instala solo el controlador de red desde la página del fabricante de tu PC o placa madre."
                }
                "Lenovo Vantage" => "Lenovo Vantage: abre el programa y apaga la opción Network Boost.",
                _ => continue,
            };
            steps.push(step.to_string());
        }
        let rest: Vec<String> = shapers
            .iter()
            .filter(|a| a.name != "Killer Network" && a.name != "Lenovo Vantage")
            .map(|a| a.name.clone())
            .collect();
        if !rest.is_empty() {
            steps.push(format!(
                "{}: desactívalo o desinstálalo desde Configuración > Aplicaciones > Aplicaciones instaladas.",
                list(&rest)
            ));
        }
        steps.push("Haz un directo de prueba y fíjate si siguen los cortes.".to_string());
        review.add(Advice {
            id: "apps.network.shaper",
            level: Level::Recommended,
            area: Area::Programs,
            title: format!("Desactiva el reparto de internet de {}", list(&names(shapers.iter().copied()))),
            why: "Estos programas reparten tu internet entre aplicaciones y pueden darle menos prioridad a OBS. Eso se nota como cortes en el directo."
                .to_string(),
            steps,
        });
    }

    let capture_ok = !review.has("apps.capture.other") && !review.has("apps.capture.d3dgear");
    if downloads.is_empty() && capture_ok {
        review.ok("No hay descargas ni otros programas de captura abiertos compitiendo con OBS.");
    }
}

/// Friendly names, without repeats, in report order.
fn names<'a>(apps: impl Iterator<Item = &'a App>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for app in apps {
        if !out.contains(&app.name) {
            out.push(app.name.clone());
        }
    }
    out
}

/// "A", "A y B", "A, B y C".
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} y {}", init.join(", "), last),
    }
}

/// Groups processes by program and keeps only the ones worth mentioning.
pub fn classify(samples: impl Iterator<Item = Sample>) -> Apps {
    let mut groups: HashMap<String, App> = HashMap::new();
    let mut obs_running = false;
    let mut seen = 0;

    for s in samples {
        seen += 1;
        let key = normalize(&s.exe);
        if OBS.contains(&key.as_str()) {
            obs_running = true;
            continue;
        }
        // A program we know beats the folder it lives in (Wallpaper Engine
        // lives in the Steam library but is not a game).
        let own = catalog::find(&key);
        // Unknown helpers inherit their parent's program, so a store's or a
        // browser's sub-processes add up on one line.
        let adopted = match (own, s.in_game_library) {
            (None, false) => s
                .parent_exe
                .as_deref()
                .map(normalize)
                .and_then(|parent| catalog::find(&parent).map(|found| (parent, found)))
                .filter(|(_, (_, cat))| *cat != Category::Protected),
            _ => None,
        };
        let (group_key, name, category, note) = match (own, adopted) {
            (Some((_, Category::Protected)), _) => continue,
            (Some((friendly, cat)), _) => (friendly.to_string(), friendly.to_string(), Some(cat), catalog::note(&key)),
            (None, Some((parent, (friendly, cat)))) => {
                (friendly.to_string(), friendly.to_string(), Some(cat), catalog::note(&parent))
            }
            (None, None) => (key.clone(), s.exe.clone(), None, None),
        };
        let is_game = category == Some(Category::Game) || (category.is_none() && s.in_game_library);
        let app = groups.entry(group_key).or_insert(App {
            name,
            category,
            is_game,
            note,
            process_count: 0,
            mem_bytes: 0,
            cpu_pct: 0.0,
        });
        app.process_count += 1;
        app.mem_bytes += s.mem_bytes;
        app.cpu_pct += s.cpu_pct;
        app.is_game |= is_game;
        if app.note.is_none() {
            app.note = note;
        }
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
            None if app.is_game => {
                if app.mem_bytes >= GAME_MIN_MEM || app.cpu_pct >= GAME_MIN_CPU {
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
    // Ties broken by name so the report does not reshuffle between runs.
    let order = |a: &App, b: &App| b.mem_bytes.cmp(&a.mem_bytes).then_with(|| a.name.cmp(&b.name));
    known.sort_by(order);
    unknown.sort_by(order);
    unknown.truncate(MAX_UNKNOWN);

    Apps { known, unknown, obs_running, seen }
}

fn normalize(exe: &str) -> String {
    let lower = exe.trim().to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(exe: &str, mem_mb: u64, cpu: f32) -> Sample {
        Sample { exe: exe.to_string(), parent_exe: None, in_game_library: false, mem_bytes: mem_mb * MIB, cpu_pct: cpu }
    }

    fn child(exe: &str, parent: &str, mem_mb: u64) -> Sample {
        Sample { parent_exe: Some(parent.to_string()), ..sample(exe, mem_mb, 0.0) }
    }

    fn in_library(exe: &str, mem_mb: u64, cpu: f32) -> Sample {
        Sample { in_game_library: true, ..sample(exe, mem_mb, cpu) }
    }

    fn apps(samples: Vec<Sample>) -> Apps {
        classify(samples.into_iter())
    }

    fn facts(samples: Vec<Sample>) -> Facts {
        Facts { apps: apps(samples), ..Facts::default() }
    }

    fn review(samples: Vec<Sample>) -> Review {
        let mut review = Review::default();
        advise(&facts(samples), &mut review);
        review
    }

    /// One line per process: `exe;parent;in_game_library(0/1);mem_mb;cpu`.
    /// Malformed lines are skipped, like `read` skips unreadable processes.
    fn parse_fixture(text: &str) -> Vec<Sample> {
        text.lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|line| {
                let mut f = line.split(';');
                let exe = f.next()?.trim().to_string();
                let parent = f.next()?.trim();
                let lib = f.next()?.trim() == "1";
                let mem: u64 = f.next()?.trim().parse().ok()?;
                let cpu: f32 = f.next()?.trim().parse().ok()?;
                let parent_exe = (!parent.is_empty()).then(|| parent.to_string());
                Some(Sample { exe, parent_exe, in_game_library: lib, mem_bytes: mem * MIB, cpu_pct: cpu })
            })
            .collect()
    }

    #[test]
    fn groups_all_processes_of_one_program() {
        let apps =
            apps(vec![sample("chrome.exe", 300, 0.5), sample("chrome.exe", 200, 0.5), sample("Chrome.exe", 100, 0.0)]);
        assert_eq!(apps.known.len(), 1);
        let chrome = &apps.known[0];
        assert_eq!(chrome.name, "Google Chrome");
        assert_eq!(chrome.process_count, 3);
        assert_eq!(chrome.mem_bytes, 600 * MIB);
        assert_eq!(chrome.kind(), ProgramKind::Review);
    }

    #[test]
    fn unknown_helpers_join_their_parent_program() {
        let apps = apps(vec![sample("Steam.exe", 100, 0.2), child("steamhelperx.exe", "steam.exe", 200)]);
        assert_eq!(apps.known.len(), 1);
        assert_eq!(apps.known[0].name, "Steam");
        assert_eq!(apps.known[0].process_count, 2);
        assert!(apps.unknown.is_empty());
    }

    #[test]
    fn children_of_windows_are_not_adopted_by_windows() {
        let apps = apps(vec![Sample { cpu_pct: 20.0, ..child("pesado.exe", "explorer.exe", 900) }]);
        assert_eq!(apps.unknown.len(), 1);
        assert_eq!(apps.unknown[0].kind(), ProgramKind::Unknown);
    }

    #[test]
    fn never_points_at_windows_antivirus_or_anticheat() {
        let apps = apps(vec![
            sample("svchost.exe", 2000, 30.0),
            sample("MsMpEng.exe", 900, 20.0),
            sample("dwm.exe", 800, 10.0),
            sample("EasyAntiCheat.exe", 900, 10.0),
            in_library("EasyAntiCheat_EOS.exe", 900, 10.0),
        ]);
        assert!(apps.known.is_empty(), "{:?}", apps.known);
        assert!(apps.unknown.is_empty());
        assert_eq!(apps.seen, 5);
    }

    #[test]
    fn torrents_are_reported_even_when_idle() {
        let apps = apps(vec![sample("qbittorrent.exe", 40, 0.0)]);
        assert_eq!(apps.known.len(), 1);
        assert_eq!(apps.known[0].category, Some(Category::Download));
        assert_eq!(apps.known[0].kind(), ProgramKind::Close);
    }

    #[test]
    fn light_known_programs_are_left_alone() {
        assert!(apps(vec![sample("Spotify.exe", 80, 0.1)]).known.is_empty());
    }

    #[test]
    fn unknown_programs_only_show_up_when_heavy() {
        let apps = apps(vec![sample("pesado.exe", 4000, 35.0), sample("cosita.exe", 50, 0.2)]);
        assert_eq!(apps.unknown.len(), 1);
        assert_eq!(apps.unknown[0].name, "pesado.exe");
    }

    #[test]
    fn a_busy_program_in_a_game_library_is_the_game() {
        let apps = apps(vec![in_library("MiJuego.exe", 2500, 20.0), in_library("updater.exe", 20, 0.0)]);
        assert_eq!(apps.known.len(), 1);
        assert_eq!(apps.known[0].kind(), ProgramKind::Game);
        assert!(apps.unknown.is_empty());
    }

    #[test]
    fn a_known_program_in_a_game_library_keeps_its_category() {
        let apps = apps(vec![in_library("wallpaper64.exe", 400, 4.0)]);
        assert_eq!(apps.known[0].category, Some(Category::Desktop));
        assert_eq!(apps.known[0].kind(), ProgramKind::Close);
    }

    #[test]
    fn popular_games_are_recognised_anywhere() {
        let apps = apps(vec![sample("cs2.exe", 100, 0.0)]);
        assert_eq!(apps.known[0].kind(), ProgramKind::Game);
        assert!(tip(&apps.known[0]).is_empty());
    }

    #[test]
    fn stream_tools_are_kept() {
        let apps = apps(vec![sample("Streamer.bot.exe", 60, 0.1)]);
        assert_eq!(apps.known[0].kind(), ProgramKind::Keep);
    }

    #[test]
    fn detects_obs_running_without_listing_it() {
        let apps = apps(vec![sample("obs64.exe", 900, 12.0), sample("obs-browser-page.exe", 300, 2.0)]);
        assert!(apps.obs_running);
        assert!(apps.known.is_empty() && apps.unknown.is_empty());
    }

    #[test]
    fn knowledge_base_fixes_replace_the_generic_tip() {
        let apps = apps(vec![sample("MSIAfterburner.exe", 30, 0.0)]);
        assert_eq!(tip(&apps.known[0]), catalog::note("msiafterburner").unwrap_or_default());
    }

    #[test]
    fn game_library_paths() {
        assert!(is_game_library(r"D:\SteamLibrary\steamapps\common\Juego\juego.exe"));
        assert!(is_game_library("C:/Program Files/Epic Games/Juego/juego.exe"));
        assert!(is_game_library(r"C:\XboxGames\Juego\Content\juego.exe"));
        assert!(!is_game_library(r"C:\Videos\My Epic Games Clips\x.exe"));
        assert!(!is_game_library(r"C:\Program Files\Programa\programa.exe"));
        assert!(!is_game_library(""));
        assert!(!is_game_library("\u{0}\u{fffd}basura::"));
    }

    #[test]
    fn tally_averages_processor_and_keeps_peak_memory() {
        let mut tally = Tally::default();
        let pid = Pid::from_u32(10);
        tally.add(pid, sample("x.exe", 100, 9.0));
        tally.add(pid, sample("x.exe", 300, 3.0));
        // Pid reused by another program: the newest one replaces it.
        let other = Pid::from_u32(11);
        tally.add(other, sample("a.exe", 10, 6.0));
        tally.add(other, sample("b.exe", 20, 6.0));
        let mut out = tally.finish(3);
        out.sort_by(|a, b| a.exe.cmp(&b.exe));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].exe, "b.exe");
        assert!((out[0].cpu_pct - 2.0).abs() < 1e-4);
        assert_eq!(out[1].mem_bytes, 300 * MIB);
        assert!((out[1].cpu_pct - 4.0).abs() < 1e-4);
    }

    #[test]
    fn tally_survives_zero_rounds() {
        let mut tally = Tally::default();
        tally.add(Pid::from_u32(1), sample("x.exe", 1, 5.0));
        assert_eq!(tally.finish(0)[0].cpu_pct, 5.0);
    }

    #[test]
    fn list_joins_in_spanish() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(list(&s(&[])), "");
        assert_eq!(list(&s(&["A"])), "A");
        assert_eq!(list(&s(&["A", "B", "C"])), "A, B y C");
    }

    #[test]
    fn nothing_seen_is_a_note_not_advice() {
        let mut review = Review::default();
        advise(&Facts::default(), &mut review);
        assert!(review.advice.is_empty());
        assert_eq!(review.notes.len(), 1);
        assert!(review.good.is_empty());
    }

    #[test]
    fn a_clean_pc_gets_a_good_word() {
        let review = review(vec![sample("chrome.exe", 300, 1.0)]);
        assert!(review.advice.is_empty());
        assert_eq!(review.good.len(), 1);
    }

    #[test]
    fn downloads_capture_and_shapers_each_get_one_advice() {
        let review = review(vec![
            sample("qbittorrent.exe", 40, 0.0),
            sample("utorrent.exe", 40, 0.0),
            sample("medal.exe", 200, 1.0),
            sample("bandicam.exe", 100, 0.0),
            sample("KillerNetworkService.exe", 20, 0.0),
            sample("cFosSpeed.exe", 20, 0.0),
        ]);
        assert_eq!(review.ids(), vec!["apps.download.open", "apps.capture.other", "apps.network.shaper"]);
        assert!(review.advice.iter().all(|a| a.level == Level::Recommended && a.area == Area::Programs));
        assert!(review.good.is_empty());
        let shaper = review.get("apps.network.shaper").map(|a| a.steps.len());
        // Killer, the rest together, and the test stream.
        assert_eq!(shaper, Some(3));
    }

    #[test]
    fn d3dgear_gets_its_own_uninstall_advice() {
        let review = review(vec![sample("D3DGear.exe", 50, 0.0)]);
        assert_eq!(review.ids(), vec!["apps.capture.d3dgear"]);
    }

    #[test]
    fn overlays_and_vpns_stay_on_their_cards() {
        let review = review(vec![sample("RTSS.exe", 20, 0.0), sample("NordVPN.exe", 80, 0.0)]);
        assert!(review.advice.is_empty());
        let facts = facts(vec![sample("RTSS.exe", 20, 0.0), sample("NordVPN.exe", 80, 0.0)]);
        let kinds: Vec<ProgramKind> = cards(&facts).iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&ProgramKind::Close) && kinds.contains(&ProgramKind::Review));
    }

    #[test]
    fn fixture_of_a_streamer_pc() {
        let samples = parse_fixture(include_str!("../../tests/fixtures/processes/streamer_pc.csv"));
        let facts = facts(samples);
        let cards = cards(&facts);
        let kind_of = |name: &str| cards.iter().find(|c| c.name == name).map(|c| c.kind);
        assert_eq!(kind_of("Google Chrome"), Some(ProgramKind::Review));
        assert_eq!(kind_of("Discord"), Some(ProgramKind::Review));
        assert_eq!(kind_of("qBittorrent"), Some(ProgramKind::Close));
        assert_eq!(kind_of("Streamer.bot"), Some(ProgramKind::Keep));
        assert_eq!(kind_of("Elgato Wave Link"), Some(ProgramKind::Keep));
        assert_eq!(kind_of("MiJuegoIndie.exe"), Some(ProgramKind::Game));
        assert_eq!(kind_of("editor-raro.exe"), Some(ProgramKind::Unknown));
        assert!(facts.apps.obs_running);
        // Windows, the antivirus and the anticheat never show up.
        assert!(cards.iter().all(|c| c.name != "Windows" && c.name != "Antivirus" && c.name != "Anticheat"));
        // Steam's unknown helper was folded into Steam.
        let steam = facts.apps.known.iter().find(|a| a.name == "Steam").map(|a| a.process_count);
        assert_eq!(steam, Some(3));
        let mut review = Review::default();
        advise(&facts, &mut review);
        assert_eq!(review.ids(), vec!["apps.download.open"]);
    }

    #[test]
    fn garbage_fixture_lines_are_skipped() {
        let samples = parse_fixture(include_str!("../../tests/fixtures/processes/garbage.csv"));
        assert_eq!(samples.len(), 1);
        let apps = classify(samples.into_iter());
        assert_eq!(apps.seen, 1);
        assert!(classify(parse_fixture("").into_iter()).seen == 0);
    }
}
