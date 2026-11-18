//! A starting configuration for this PC: which encoder, picture size, frame
//! rate and bitrate we would begin with. Useful even before OBS is set up.
//!
//! It weighs, in this order: what OBS itself said it can use (the logs),
//! the graphics cards and how good their encoders are, the processor's class
//! and cores, the memory, the monitors, the platform's limit and, when it was
//! measured, the upload. Every step that changes the result leaves a reason
//! the report shows.

use crate::facts::Facts;
use crate::hardware::tiers::{CpuClass, CpuInfo, VideoEncoder};
use crate::hardware::{Gpu, Hardware, RAM_COMFORTABLE_GB, RAM_MINIMUM_GB};
use crate::model::{Row, Section};
use crate::net;
use crate::obs::config::{EncoderKind, ObsStatus};
use crate::obs::logs::LogFacts;
use crate::platforms;
use crate::report::text;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggested {
    pub encoder: EncoderKind,
    /// How the option reads in OBS's encoder list, plus a hint in parentheses.
    pub encoder_label: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    // ---- added in v0.2 ----
    /// The card that prepares the video, when it is one.
    pub gpu_name: Option<String>,
    /// True when the PC itself only manages the smallest picture (not the
    /// internet or the platform).
    pub low_end: bool,
    /// Plain-language reasons, shown under each row of the suggestion.
    pub why_encoder: Option<String>,
    pub why_picture: Option<String>,
    pub why_bitrate: Option<String>,
}

impl Default for Suggested {
    /// The most conservative setup: what any PC that can run OBS can do.
    fn default() -> Self {
        Self {
            encoder: EncoderKind::Software,
            encoder_label: SOFTWARE_LABEL.to_string(),
            width: 854,
            height: 480,
            fps: 30,
            bitrate_kbps: platforms::bitrate_for(480, 30.0),
            gpu_name: None,
            low_end: false,
            why_encoder: None,
            why_picture: None,
            why_bitrate: None,
        }
    }
}

impl Suggested {
    /// Pixels per second: a rough measure of how much work the picture is.
    pub fn load(&self) -> u64 {
        self.width as u64 * self.height as u64 * self.fps as u64
    }
}

const SOFTWARE_LABEL: &str = "x264 (lo hace el procesador)";

/// Picture sizes we suggest, biggest first. Stepping down this ladder is how
/// the internet and the monitor lower a suggestion.
const LADDER: [(u32, u32, u32); 4] = [(1920, 1080, 60), (1280, 720, 60), (1280, 720, 30), (854, 480, 30)];
const P1080_60: usize = 0;
const P720_60: usize = 1;
const P720_30: usize = 2;
const P480_30: usize = 3;

pub fn suggest(facts: &Facts) -> Suggested {
    let hw = &facts.hw;
    let cpu = hw.cpu();
    let (pick, skipped) = pick_encoder(hw, &facts.logs);

    let (mut rung, why_picture, low_end) = picture_for(hw, &cpu, pick.as_ref());
    let mut why_picture = why_picture;

    // Capturing a 50 Hz screen at 60 frames per second only repeats frames.
    let refresh = facts.logs.startup.as_ref().and_then(|s| s.common_refresh_hz());
    if let Some(hz) = refresh.filter(|hz| *hz < 60 && LADDER[rung].2 == 60) {
        rung = P720_30;
        why_picture = Some(format!(
            "Tu pantalla muestra {hz} cuadros por segundo: transmitir a 60 no se vería más fluido, así que empiezo con 30."
        ));
    }

    // The upload decides how much can leave the PC. Only when it was measured.
    let upload_max = if facts.net.measured { net::max_bitrate_for_upload(facts.net.upload_mbps) } else { None };
    let mut why_bitrate = None;
    if let Some(max) = upload_max {
        let before = rung;
        while rung < P480_30 && bitrate_of(rung) > max {
            rung += 1;
        }
        if rung != before {
            why_picture = Some(format!(
                "La subida de tu internet ({}) no alcanza para una imagen más grande sin que se corte.",
                text::mbps(facts.net.upload_mbps.unwrap_or_default())
            ));
        }
    }

    let (width, height, fps) = LADDER[rung];
    let mut bitrate_kbps = bitrate_of(rung);
    if let Some(max) = upload_max.filter(|max| *max < bitrate_kbps) {
        bitrate_kbps = max;
        why_bitrate = Some(format!(
            "Es lo más que deja tu subida ({}) con margen para que no se corte.",
            text::mbps(facts.net.upload_mbps.unwrap_or_default())
        ));
    }
    let platform = facts.config().and_then(|c| c.platform());
    let platform_limit = platform.and_then(|p| p.max_video_kbps_for(Some((width, height)), fps as f32).map(|l| (p, l)));
    if let Some((p, limit)) = platform_limit.filter(|(_, l)| *l < bitrate_kbps) {
        bitrate_kbps = limit;
        why_bitrate = Some(format!("{} acepta hasta {limit} kbps para esta imagen.", p.name));
    }

    let (encoder, encoder_label, gpu_name, why_encoder) = match &pick {
        Some(p) => {
            (p.kind, label_for(p.kind).to_string(), Some(text::product_name(&p.gpu.name)), Some(encoder_reason(p)))
        }
        None => (EncoderKind::Software, SOFTWARE_LABEL.to_string(), None, Some(software_reason(hw, &skipped))),
    };

    Suggested {
        encoder,
        encoder_label,
        width,
        height,
        fps,
        bitrate_kbps,
        gpu_name,
        low_end,
        why_encoder,
        why_picture,
        why_bitrate,
    }
}

fn bitrate_of(rung: usize) -> u32 {
    let (_, height, fps) = LADDER[rung];
    platforms::bitrate_for(height, fps as f32)
}

/// The card chosen to prepare the video.
struct Pick<'a> {
    gpu: &'a Gpu,
    kind: EncoderKind,
    quality: VideoEncoder,
    integrated: bool,
}

/// Why a card was passed over.
enum Skip<'a> {
    /// OBS did not list its encoder the last time it started.
    NotListed(&'a Gpu),
    /// Its name says it has no usable encoder.
    NoEncoder(&'a Gpu),
}

/// The best card that can prepare the video. The OBS logs are ground truth:
/// a family OBS did not list is never chosen, and one it listed is trusted
/// even if the name says otherwise.
fn pick_encoder<'a>(hw: &'a Hardware, logs: &LogFacts) -> (Option<Pick<'a>>, Vec<Skip<'a>>) {
    let mut skipped = Vec::new();
    for gpu in hw.gpus_ranked() {
        let Some(kind) = gpu.encoder_kind() else { continue };
        let quality = gpu.encoder();
        let usable = match logs.can_encode_with(kind) {
            Some(false) => {
                skipped.push(Skip::NotListed(gpu));
                continue;
            }
            Some(true) => true,
            None => quality.likely_usable(),
        };
        if !usable {
            skipped.push(Skip::NoEncoder(gpu));
            continue;
        }
        // Listed by OBS but unknown or "missing" by name: be careful with it.
        let quality = if quality.likely_usable() { quality } else { VideoEncoder::Unknown };
        return (Some(Pick { gpu, kind, quality, integrated: gpu.is_integrated() }), skipped);
    }
    (None, skipped)
}

/// Picture size for this PC, a reason when it is held back, and whether the
/// PC only manages the smallest picture.
fn picture_for(hw: &Hardware, cpu: &CpuInfo, pick: Option<&Pick>) -> (usize, Option<String>, bool) {
    let cores = hw.physical_cores;
    let ram = hw.ram_total_gb();
    let roomy = cores >= 6 && ram >= RAM_COMFORTABLE_GB;
    let enough = cores >= 4 && ram >= RAM_MINIMUM_GB;
    // Processors made for price or battery, too few cores or too little
    // memory: the game alone takes most of it.
    let tight = cpu.is_entry_level() || cores < 4 || ram < RAM_MINIMUM_GB;
    let low_power = cpu.class == CpuClass::LowPowerLaptop;
    let bottom = Some(
        "Esta PC está justa para jugar y transmitir a la vez: con 480p a 30 cuadros el directo sale fluido."
            .to_string(),
    );

    match pick {
        // A dedicated card encodes almost for free.
        Some(p) if !p.integrated => {
            let modern = p.quality == VideoEncoder::Modern;
            if roomy && modern {
                (P1080_60, None, false)
            } else if roomy || enough {
                let why = (!modern).then(|| {
                    "Su forma de preparar el video es de una generación anterior: con la misma calidad de envío, 720p se ve más limpio que 1080p."
                        .to_string()
                });
                (P720_60, why, false)
            } else {
                (
                    P720_30,
                    Some("Con pocos núcleos o poca memoria, 30 cuadros dejan aire para el juego.".to_string()),
                    false,
                )
            }
        }
        // Integrated graphics also have to draw the game and OBS's scenes.
        Some(p) => {
            if tight {
                (P480_30, bottom, true)
            } else if roomy && p.quality == VideoEncoder::Modern && !low_power {
                (P720_60, None, false)
            } else {
                let why = if low_power {
                    "La gráfica integrada también dibuja el juego, y los procesadores de notebook de bajo consumo bajan la velocidad cuando se calientan. Si tu juego es liviano, puedes probar 60 cuadros."
                } else {
                    "La gráfica integrada también dibuja el juego: 30 cuadros le dejan aire. Si tu juego es liviano, puedes probar 60."
                };
                (P720_30, Some(why.to_string()), false)
            }
        }
        // No card can do it: the processor encodes and runs the game.
        None => {
            if tight || low_power {
                (P480_30, bottom, true)
            } else if cores >= 8 && ram >= RAM_COMFORTABLE_GB && cpu.class == CpuClass::Desktop {
                (P720_60, None, false)
            } else {
                (
                    P720_30,
                    Some(
                        "Preparar el video con el procesador mientras juegas es pesado: 30 cuadros le dejan aire."
                            .to_string(),
                    ),
                    false,
                )
            }
        }
    }
}

/// The option as it reads in OBS's simple mode list ("Hardware (NVENC,
/// H.264)" in es-ES.ini), with a hint of which part of the PC it is.
fn label_for(kind: EncoderKind) -> &'static str {
    match kind {
        EncoderKind::Nvidia => "NVENC, H.264 (tu placa NVIDIA)",
        EncoderKind::Amd => "AMD, H.264 (tu placa Radeon)",
        EncoderKind::Intel => "QSV, H.264 (tu gráfica Intel)",
        _ => SOFTWARE_LABEL,
    }
}

fn encoder_reason(p: &Pick) -> String {
    let name = text::product_name(&p.gpu.name);
    match (p.integrated, p.quality) {
        (false, VideoEncoder::Modern) => {
            format!("Tu {name} prepara el video sola y con buena calidad, sin quitarle fuerza al procesador.")
        }
        (false, _) => format!("Tu {name} puede preparar el video sola y así el procesador queda libre para el juego."),
        (true, _) => {
            format!("Tu gráfica {name} puede preparar el video y así el procesador queda libre para el juego.")
        }
    }
}

fn software_reason(hw: &Hardware, skipped: &[Skip]) -> String {
    let first = skipped.first().map(|skip| match skip {
        Skip::NotListed(gpu) => {
            format!("OBS no ofreció tu {} para preparar el video la última vez que se abrió, así que lo hace el procesador.", text::product_name(&gpu.name))
        }
        Skip::NoEncoder(gpu) => {
            format!("Tu {} no puede preparar el video para OBS, así que lo hace el procesador.", text::product_name(&gpu.name))
        }
    });
    first.unwrap_or_else(|| {
        if hw.gpus.is_empty() {
            "No encontré una placa de video que pueda preparar el video, así que lo hace el procesador.".to_string()
        } else {
            "Ninguna placa de esta PC puede preparar el video para OBS, así que lo hace el procesador.".to_string()
        }
    })
}

/// The "Con qué empezaría en esta PC" block of the report.
pub fn section(facts: &Facts) -> Section {
    let s = &facts.suggested;
    let closing = match facts.obs {
        ObsStatus::Found(_) => "Es un punto de partida, no una regla. Haz un directo de prueba y ajusta desde ahí.",
        _ => {
            "Es un punto de partida para cuando abras OBS por primera vez. Haz un directo de prueba y ajusta desde ahí."
        }
    };
    let with_note = |row: Row, note: &Option<String>| match note {
        Some(note) => row.note(note.clone()),
        None => row,
    };
    let internet_note = if facts.net.measured {
        facts.net.upload_mbps.map(|u| format!("Tu subida medida es de {}.", text::mbps(u)))
    } else {
        None
    };
    Section::new("suggested", "Con qué empezaría en esta PC")
        .intro(closing)
        .row(with_note(Row::new("Quién prepara el video", s.encoder_label.clone()), &s.why_encoder))
        .row(with_note(Row::new("Imagen", format!("{}x{} a {} fps", s.width, s.height, s.fps)), &s.why_picture))
        .row(with_note(Row::new("Calidad de envío", format!("{} kbps", s.bitrate_kbps)), &s.why_bitrate))
        .row(with_note(
            Row::new(
                "Internet que necesitas",
                format!("al menos {} Mbps de subida", net::upload_needed_mbps(s.bitrate_kbps)),
            ),
            &internet_note,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::GpuVendor::{Amd, Intel, Nvidia};
    use crate::hardware::test_support::{gpu, pc};
    use crate::net::NetFacts;
    use crate::obs::logs::{Adapter, Monitor, Startup};

    fn suggestion(hw: Hardware) -> Suggested {
        suggest(&Facts { hw, ..Facts::default() })
    }

    fn named(cpu: &str, cores: usize, ram_gb: u64, gpus: Vec<Gpu>) -> Hardware {
        Hardware { cpu_name: cpu.to_string(), ..pc(cores, ram_gb, gpus) }
    }

    /// Real PCs and what we would start with on each, before looking at the
    /// internet or the platform.
    #[test]
    fn thirty_real_pcs() {
        use EncoderKind::{Amd as A, Intel as I, Nvidia as N, Software as S};
        let cases: Vec<(Hardware, (EncoderKind, u32, u32, bool))> = vec![
            // Mateo's laptop: thin and light, integrated Iris Xe.
            (
                named(
                    "13th Gen Intel(R) Core(TM) i7-1355U",
                    10,
                    32,
                    vec![gpu("Intel(R) Iris(R) Xe Graphics", Intel, 0)],
                ),
                (I, 720, 30, false),
            ),
            (
                named("12th Gen Intel(R) Core(TM) i5-12400F", 6, 16, vec![gpu("NVIDIA GeForce RTX 3060", Nvidia, 12)]),
                (N, 1080, 60, false),
            ),
            (
                named(
                    "AMD Ryzen 5 3600 6-Core Processor",
                    6,
                    16,
                    vec![gpu("NVIDIA GeForce GTX 1660 SUPER", Nvidia, 6)],
                ),
                (N, 1080, 60, false),
            ),
            (
                named("AMD Ryzen 5 3600 6-Core Processor", 6, 16, vec![gpu("NVIDIA GeForce GTX 1060 6GB", Nvidia, 6)]),
                (N, 720, 60, false),
            ),
            (
                named("Intel(R) Core(TM) i5-4590 CPU @ 3.30GHz", 4, 8, vec![gpu("NVIDIA GeForce GTX 970", Nvidia, 4)]),
                (N, 720, 60, false),
            ),
            (
                named(
                    "Intel(R) Core(TM) i3-10100F CPU @ 3.60GHz",
                    4,
                    8,
                    vec![gpu("NVIDIA GeForce GT 1030", Nvidia, 2)],
                ),
                (S, 720, 30, false),
            ),
            (
                named("AMD Ryzen 5 5600G with Radeon Graphics", 6, 16, vec![gpu("AMD Radeon(TM) Graphics", Amd, 0)]),
                (A, 720, 30, false),
            ),
            (
                named(
                    "Intel(R) Celeron(R) N4020 CPU @ 1.10GHz",
                    2,
                    4,
                    vec![gpu("Intel(R) UHD Graphics 600", Intel, 0)],
                ),
                (I, 480, 30, true),
            ),
            (
                named(
                    "Intel(R) Pentium(R) Silver N5030 CPU @ 1.10GHz",
                    4,
                    8,
                    vec![gpu("Intel(R) UHD Graphics 605", Intel, 0)],
                ),
                (I, 480, 30, true),
            ),
            (
                named(
                    "Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz",
                    4,
                    8,
                    vec![gpu("Intel(R) UHD Graphics 620", Intel, 0)],
                ),
                (I, 720, 30, false),
            ),
            (
                named(
                    "Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz",
                    6,
                    16,
                    vec![gpu("Intel(R) UHD Graphics 630", Intel, 0), gpu("NVIDIA GeForce GTX 1050 Ti", Nvidia, 4)],
                ),
                (N, 720, 60, false),
            ),
            (
                named(
                    "AMD Ryzen 7 5800H with Radeon Graphics",
                    8,
                    16,
                    vec![gpu("AMD Radeon(TM) Graphics", Amd, 0), gpu("NVIDIA GeForce RTX 3060 Laptop GPU", Nvidia, 6)],
                ),
                (N, 1080, 60, false),
            ),
            (
                named(
                    "11th Gen Intel(R) Core(TM) i5-1135G7 @ 2.40GHz",
                    4,
                    8,
                    vec![gpu("Intel(R) Iris(R) Xe Graphics", Intel, 0)],
                ),
                (I, 720, 30, false),
            ),
            (
                named("AMD Ryzen 5 7520U with Radeon Graphics", 4, 8, vec![gpu("AMD Radeon 610M", Amd, 0)]),
                (A, 720, 30, false),
            ),
            (
                named(
                    "AMD Ryzen 7 7840HS w/ Radeon 780M Graphics",
                    8,
                    16,
                    vec![gpu("AMD Radeon 780M Graphics", Amd, 0)],
                ),
                (A, 720, 60, false),
            ),
            (named("AMD Ryzen 5 5500", 6, 16, vec![gpu("AMD Radeon RX 6500 XT", Amd, 4)]), (S, 720, 30, false)),
            (
                named(
                    "Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz",
                    4,
                    16,
                    vec![gpu("NVIDIA GeForce GTX 1070", Nvidia, 8)],
                ),
                (N, 720, 60, false),
            ),
            (
                named("AMD Ryzen 7 5700X 8-Core Processor", 8, 32, vec![gpu("AMD Radeon RX 6700 XT", Amd, 12)]),
                (A, 720, 60, false),
            ),
            (
                named("AMD Ryzen 7 7800X3D 8-Core Processor", 8, 32, vec![gpu("AMD Radeon RX 7800 XT", Amd, 16)]),
                (A, 1080, 60, false),
            ),
            (
                named(
                    "13th Gen Intel(R) Core(TM) i9-13900K",
                    24,
                    32,
                    vec![gpu("Intel(R) UHD Graphics 770", Intel, 0), gpu("NVIDIA GeForce RTX 4090", Nvidia, 24)],
                ),
                (N, 1080, 60, false),
            ),
            (
                named(
                    "12th Gen Intel(R) Core(TM) i5-12450H",
                    8,
                    16,
                    vec![gpu("Intel(R) UHD Graphics", Intel, 0), gpu("NVIDIA GeForce RTX 3050 Laptop GPU", Nvidia, 4)],
                ),
                (N, 1080, 60, false),
            ),
            (
                named("AMD Ryzen 5 2600 Six-Core Processor", 6, 16, vec![gpu("Radeon RX 580 Series", Amd, 8)]),
                (A, 720, 60, false),
            ),
            (
                named(
                    "Intel(R) Core(TM) i5-3470 CPU @ 3.20GHz",
                    4,
                    8,
                    vec![gpu("Intel(R) HD Graphics 2500", Intel, 0), gpu("NVIDIA GeForce GT 730", Nvidia, 2)],
                ),
                (S, 480, 30, true),
            ),
            (
                named("AMD FX(tm)-8350 Eight-Core Processor", 8, 8, vec![gpu("NVIDIA GeForce GTX 750 Ti", Nvidia, 2)]),
                (N, 720, 60, false),
            ),
            (
                named(
                    "12th Gen Intel(R) Core(TM) i7-1260P",
                    12,
                    16,
                    vec![gpu("Intel(R) Iris(R) Xe Graphics", Intel, 0)],
                ),
                (I, 720, 30, false),
            ),
            (
                named("Intel(R) Core(TM) Ultra 7 155H", 16, 32, vec![gpu("Intel(R) Arc(TM) Graphics", Intel, 0)]),
                (I, 720, 60, false),
            ),
            (
                named(
                    "AMD Ryzen AI 9 HX 370 w/ Radeon 890M",
                    12,
                    32,
                    vec![gpu("AMD Radeon 890M", Amd, 0), gpu("NVIDIA GeForce RTX 4070 Laptop GPU", Nvidia, 8)],
                ),
                (N, 1080, 60, false),
            ),
            (
                named(
                    "AMD Athlon Silver 3050U with Radeon Graphics",
                    2,
                    4,
                    vec![gpu("AMD Radeon(TM) Graphics", Amd, 0)],
                ),
                (A, 480, 30, true),
            ),
            (
                named(
                    "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz",
                    6,
                    16,
                    vec![gpu("Intel(R) UHD Graphics 630", Intel, 0)],
                ),
                (I, 720, 30, false),
            ),
            (named("AMD Ryzen 9 5900X 12-Core Processor", 12, 32, vec![]), (S, 720, 60, false)),
            (
                named("Intel(R) Core(TM) i3-N305", 8, 8, vec![gpu("Intel(R) UHD Graphics", Intel, 0)]),
                (I, 480, 30, true),
            ),
            (
                named("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz", 14, 32, vec![gpu("Radeon RX 580 Series", Amd, 8)]),
                (A, 720, 60, false),
            ),
            (
                named(
                    "Intel(R) Core(TM) i7-1065G7 CPU @ 1.30GHz",
                    4,
                    16,
                    vec![gpu("Intel(R) Iris(R) Plus Graphics", Intel, 0), gpu("NVIDIA GeForce MX250", Nvidia, 2)],
                ),
                (I, 720, 30, false),
            ),
        ];
        assert!(cases.len() >= 30);
        for (hw, expected) in cases {
            let name = hw.cpu_name.clone();
            let s = suggestion(hw);
            assert_eq!((s.encoder, s.height, s.fps, s.low_end), expected, "{name}");
            assert_eq!(s.bitrate_kbps, platforms::bitrate_for(s.height, s.fps as f32), "{name}");
            assert!(s.why_encoder.is_some(), "{name}");
        }
    }

    #[test]
    fn obs_logs_overrule_the_card_name() {
        let hw = named(
            "12th Gen Intel(R) Core(TM) i5-12400",
            6,
            16,
            vec![gpu("Intel(R) UHD Graphics 730", Intel, 0), gpu("NVIDIA GeForce RTX 3060", Nvidia, 12)],
        );
        // NVENC missing (old driver, say): fall back to the Intel graphics.
        let mut facts = Facts { hw: hw.clone(), ..Facts::default() };
        facts.logs.available_encoders = Some(vec!["obs_x264".into(), "obs_qsv11_v2".into()]);
        let s = suggest(&facts);
        assert_eq!((s.encoder, s.height), (EncoderKind::Intel, 720));

        // Nothing but x264 listed: never a hardware encoder.
        facts.logs.available_encoders = Some(vec!["obs_x264".into()]);
        assert_eq!(suggest(&facts).encoder, EncoderKind::Software);

        // A card the name says has none, but OBS lists it: trusted, carefully.
        let odd = named("AMD Ryzen 5 3600", 6, 16, vec![gpu("NVIDIA GeForce MX450", Nvidia, 2)]);
        let mut facts = Facts { hw: odd, ..Facts::default() };
        facts.logs.available_encoders = Some(vec!["obs_x264".into(), "obs_nvenc_h264_tex".into()]);
        let s = suggest(&facts);
        assert_eq!((s.encoder, s.height, s.fps), (EncoderKind::Nvidia, 720, 60));
    }

    #[test]
    fn a_50_hz_screen_gets_30_frames() {
        let hw = named("AMD Ryzen 5 3600", 6, 16, vec![gpu("NVIDIA GeForce RTX 3060", Nvidia, 12)]);
        let mut facts = Facts { hw, ..Facts::default() };
        facts.logs.startup = Some(Startup {
            adapters: vec![Adapter {
                monitors: vec![Monitor { width: 1920, height: 1080, refresh_hz: 50 }],
                ..Adapter::default()
            }],
            ..Startup::default()
        });
        let s = suggest(&facts);
        assert_eq!((s.height, s.fps, s.bitrate_kbps), (720, 30, 3000));
        // 144 Hz changes nothing.
        if let Some(startup) = facts.logs.startup.as_mut() {
            startup.adapters[0].monitors[0].refresh_hz = 144;
        }
        assert_eq!(suggest(&facts).fps, 60);
    }

    #[test]
    fn a_measured_upload_lowers_the_picture_and_the_bitrate() {
        let hw = named("AMD Ryzen 5 3600", 6, 16, vec![gpu("NVIDIA GeForce RTX 3060", Nvidia, 12)]);
        let net = |mbps: f64| NetFacts { measured: true, upload_mbps: Some(mbps), ..NetFacts::default() };

        let facts = Facts { hw: hw.clone(), net: net(20.0), ..Facts::default() };
        assert_eq!((suggest(&facts).height, suggest(&facts).bitrate_kbps), (1080, 6000));

        // 5 Mbps carries 3333 kbps: 720p30 at 3000.
        let facts = Facts { hw: hw.clone(), net: net(5.0), ..Facts::default() };
        let s = suggest(&facts);
        assert_eq!((s.height, s.fps, s.bitrate_kbps, s.low_end), (720, 30, 3000, false));
        assert!(s.why_picture.is_some());

        // 1.5 Mbps carries 1000 kbps: 480p30, bitrate capped.
        let facts = Facts { hw: hw.clone(), net: net(1.5), ..Facts::default() };
        let s = suggest(&facts);
        assert_eq!((s.height, s.bitrate_kbps), (480, 1000));
        assert!(s.why_bitrate.is_some());

        // Not measured: the upload is never assumed.
        let facts = Facts { hw, net: NetFacts { upload_mbps: Some(1.0), ..NetFacts::default() }, ..Facts::default() };
        assert_eq!(suggest(&facts).height, 1080);
    }

    #[test]
    fn the_section_always_has_the_four_rows() {
        let facts = Facts::default();
        let section = section(&facts);
        assert_eq!(section.rows.len(), 4);
        assert!(section.intro.is_some());
    }

    #[test]
    fn an_unknown_pc_gets_the_safest_start() {
        let s = suggest(&Facts::default());
        assert_eq!((s.encoder, s.height, s.fps, s.low_end), (EncoderKind::Software, 480, 30, true));
    }
}
