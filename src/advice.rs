//! Turns what we found into advice a streamer can follow without a glossary.
//!
//! Every rule is a pure function of the hardware and the OBS config, so each
//! one has a test with a concrete PC behind it.

use crate::hardware::{GpuVendor, Hardware};
use crate::obs::{EncoderKind, ObsConfig, ObsStatus};

/// Names as they appear in the Spanish OBS interface.
pub mod menu {
    pub const SETTINGS: &str = "Ajustes";
    pub const OUTPUT: &str = "Salida";
    pub const VIDEO: &str = "Vídeo";
    pub const ENCODER: &str = "Codificador de vídeo";
    pub const BITRATE: &str = "Bitrate de vídeo";
    pub const OUTPUT_RES: &str = "Resolución de salida (Escalada)";
    pub const FPS: &str = "Valores comunes de FPS";
    pub const RATE_CONTROL: &str = "Control de la frecuencia";
    /// Simple mode calls it one way, advanced x264 the other.
    pub const PRESET: &str = "Preajuste del codificador\" (o \"Perfil de uso de CPU";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Important,
    Recommended,
    Tip,
}

#[derive(Debug, Clone)]
pub struct Advice {
    pub level: Level,
    pub title: String,
    pub why: String,
    pub steps: Vec<String>,
}

/// A starting point for this PC, useful even before OBS is configured.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggested {
    pub encoder: EncoderKind,
    pub encoder_label: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl Suggested {
    fn load(&self) -> u64 {
        self.width as u64 * self.height as u64 * self.fps as u64
    }
}

#[derive(Debug, Default)]
pub struct Review {
    pub advice: Vec<Advice>,
    pub good: Vec<String>,
}

const RAM_COMFORTABLE_GB: f64 = 13.0; // a "16 GB" PC reports a bit less
const RAM_MINIMUM_GB: f64 = 5.5; // an "8 GB" PC with integrated graphics
const RAM_ALMOST_FULL_PCT: u32 = 85;
const TWITCH_MAX_KBPS: u32 = 6000;

pub fn suggest(hw: &Hardware) -> Suggested {
    let cores = hw.physical_cores;
    let ram = hw.ram_total_gb();
    let roomy = cores >= 6 && ram >= RAM_COMFORTABLE_GB;

    let hardware_encoder = hw.best_gpu().and_then(|gpu| {
        let (kind, label) = match gpu.vendor {
            GpuVendor::Nvidia if !gpu.nvenc_unlikely() => (EncoderKind::Nvidia, "NVENC, H.264 (tu placa NVIDIA)"),
            GpuVendor::Amd => (EncoderKind::Amd, "AMD, H.264 (tu placa Radeon)"),
            GpuVendor::Intel => (EncoderKind::Intel, "QSV, H.264 (tu gráfica Intel)"),
            _ => return None,
        };
        Some((kind, label, gpu.is_integrated()))
    });

    let (encoder, encoder_label, (width, height, fps)) = match hardware_encoder {
        // A dedicated card encodes almost for free.
        Some((kind, label, false)) => {
            let size = if roomy {
                (1920, 1080, 60)
            } else if cores >= 4 && ram >= RAM_MINIMUM_GB {
                (1280, 720, 60)
            } else {
                (1280, 720, 30)
            };
            (kind, label, size)
        }
        // Integrated graphics also have to draw the game.
        Some((kind, label, true)) => {
            (kind, label, if roomy { (1280, 720, 60) } else { (1280, 720, 30) })
        }
        // No hardware encoder: the processor does everything.
        None => {
            let size = if cores >= 8 && ram >= RAM_COMFORTABLE_GB {
                (1280, 720, 60)
            } else if cores >= 4 {
                (1280, 720, 30)
            } else {
                (854, 480, 30)
            };
            (EncoderKind::Software, "x264 (lo hace el procesador)", size)
        }
    };

    Suggested { encoder, encoder_label, width, height, fps, bitrate_kbps: bitrate_for(height, fps as f32) }
}

/// Bitrate the big platforms recommend for each picture size.
pub fn bitrate_for(height: u32, fps: f32) -> u32 {
    let smooth = fps > 40.0;
    match height {
        0..=480 => 1500,
        481..=720 => if smooth { 4500 } else { 3000 },
        _ => if smooth { 6000 } else { 4500 },
    }
}

pub fn review(hw: &Hardware, obs: &ObsStatus, suggested: &Suggested) -> Review {
    let mut review = Review::default();

    if let ObsStatus::Found(cfg) = obs {
        check_encoder(cfg, suggested, &mut review);
        check_picture_size(cfg, suggested, &mut review);
        check_bitrate(cfg, &mut review);
        check_rate_control(cfg, &mut review);
        check_preset(cfg, &mut review);
    }
    check_memory(hw, &mut review);

    review.advice.sort_by_key(|a| a.level);
    review
}

fn check_encoder(cfg: &ObsConfig, suggested: &Suggested, review: &mut Review) {
    if cfg.encoder == EncoderKind::Software && suggested.encoder.is_hardware() {
        review.advice.push(Advice {
            level: Level::Important,
            title: "Pasa el trabajo de transmitir a tu placa de video".to_string(),
            why: "Ahora mismo tu procesador hace dos cosas a la vez: mover el juego y preparar el video del directo. \
                  Tu placa de video puede encargarse del video casi sin esfuerzo y dejar el procesador libre para el juego."
                .to_string(),
            steps: vec![
                format!("En OBS abre {} > {}.", menu::SETTINGS, menu::OUTPUT),
                format!("En \"{}\" elige la opción de hardware: {}.", menu::ENCODER, suggested.encoder_label),
                "Si esa opción no aparece en la lista, tu placa no lo permite: deja el que tienes.".to_string(),
            ],
        });
    } else if cfg.encoder.is_hardware() {
        review.good.push("Tu placa de video se encarga del video del directo: el procesador queda libre para el juego.".to_string());
    }
}

fn check_picture_size(cfg: &ObsConfig, suggested: &Suggested, review: &mut Review) {
    let Some((width, height)) = cfg.output else {
        return;
    };
    let fps = cfg.fps.round().min(60.0) as u64;
    let load = width as u64 * height as u64 * fps;

    if height > 1080 {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: format!("Estás transmitiendo en {width}x{height}: es más de lo que hace falta"),
            why: "Casi nadie ve un directo por encima de 1080p, y enviar una imagen tan grande le exige mucho a tu PC y a tu internet."
                .to_string(),
            steps: picture_steps(suggested),
        });
    } else if load as f64 > suggested.load() as f64 * 1.3 {
        review.advice.push(Advice {
            level: if cfg.encoder == EncoderKind::Software { Level::Important } else { Level::Recommended },
            title: format!(
                "Baja la imagen a {}p a {} fps",
                suggested.height, suggested.fps
            ),
            why: format!(
                "Estás transmitiendo en {}p a {} fps, y eso es mucho para esta PC: el directo se va a trabar o el juego va a perder fluidez. \
                 Una imagen un poco más chica pero estable se ve mejor que una grande a los saltos.",
                height,
                cfg.fps.round()
            ),
            steps: picture_steps(suggested),
        });
    } else {
        review.good.push(format!(
            "El tamaño de imagen ({}p a {} fps) está bien para tu PC.",
            height,
            cfg.fps.round()
        ));
    }

    if cfg.fps > 61.0 {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: "Baja los fps a 60".to_string(),
            why: format!(
                "Tienes OBS a {} fps, pero las plataformas muestran como mucho 60. Todo lo que pase de ahí es trabajo que tu PC hace para nada.",
                cfg.fps.round()
            ),
            steps: vec![
                format!("En OBS abre {} > {}.", menu::SETTINGS, menu::VIDEO),
                format!("En \"{}\" elige 60.", menu::FPS),
            ],
        });
    }
}

fn picture_steps(suggested: &Suggested) -> Vec<String> {
    vec![
        format!("En OBS abre {} > {}.", menu::SETTINGS, menu::VIDEO),
        format!("En \"{}\" pon {}x{}.", menu::OUTPUT_RES, suggested.width, suggested.height),
        format!("En \"{}\" elige {}.", menu::FPS, suggested.fps),
    ]
}

fn check_bitrate(cfg: &ObsConfig, review: &mut Review) {
    let Some((_, height)) = cfg.output else {
        return;
    };
    let recommended = bitrate_for(height.min(1080), cfg.fps);
    let on_twitch = cfg.service.as_deref().is_some_and(|s| s.to_lowercase().contains("twitch"));

    if on_twitch && cfg.bitrate_kbps > TWITCH_MAX_KBPS {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: format!("Baja la calidad de envío a {TWITCH_MAX_KBPS}"),
            why: format!(
                "Tienes {} y Twitch recomienda no pasar de {TWITCH_MAX_KBPS}. Por encima de eso, a muchos espectadores se les corta el directo.",
                cfg.bitrate_kbps
            ),
            steps: bitrate_steps(TWITCH_MAX_KBPS),
        });
    } else if (cfg.bitrate_kbps as f64) < recommended as f64 * 0.6 {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: format!("Sube la calidad de envío a unos {recommended}"),
            why: format!(
                "Tienes {} para una imagen de {}p a {} fps. Con tan poco, la imagen se ve borrosa o \"a cuadraditos\" cuando hay movimiento.",
                cfg.bitrate_kbps,
                height,
                cfg.fps.round()
            ),
            steps: {
                let mut steps = bitrate_steps(recommended);
                steps.push(format!(
                    "Antes mide tu internet en speedtest.net: la subida debería ser de al menos {} Mbps. Si no llegas, mejor baja el tamaño de la imagen.",
                    upload_needed_mbps(recommended)
                ));
                steps
            },
        });
    } else {
        review.good.push(format!(
            "La calidad de envío ({} kbps) va bien con el tamaño de tu imagen.",
            cfg.bitrate_kbps
        ));
    }
}

fn bitrate_steps(target: u32) -> Vec<String> {
    vec![
        format!("En OBS abre {} > {}.", menu::SETTINGS, menu::OUTPUT),
        format!("En \"{}\" escribe {target}.", menu::BITRATE),
    ]
}

/// Leave headroom: the stream should use about two thirds of the upload at most.
pub fn upload_needed_mbps(bitrate_kbps: u32) -> u32 {
    ((bitrate_kbps as f64 * 1.5) / 1000.0).ceil() as u32
}

fn check_rate_control(cfg: &ObsConfig, review: &mut Review) {
    let Some(rc) = cfg.rate_control.as_deref() else {
        return;
    };
    if !rc.eq_ignore_ascii_case("CBR") {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: "Usa una calidad de envío constante (CBR)".to_string(),
            why: format!(
                "Tienes OBS en modo {rc}, que sube y baja la cantidad de datos todo el tiempo. \
                 Las plataformas de directos piden un envío parejo; si no, a los espectadores se les corta."
            ),
            steps: vec![
                format!("En OBS abre {} > {}.", menu::SETTINGS, menu::OUTPUT),
                format!("En \"{}\" elige CBR.", menu::RATE_CONTROL),
            ],
        });
    }
}

fn check_preset(cfg: &ObsConfig, review: &mut Review) {
    if cfg.encoder != EncoderKind::Software {
        return;
    }
    let Some(preset) = cfg.preset.as_deref() else {
        return;
    };
    const TOO_SLOW: &[&str] = &["faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"];
    if TOO_SLOW.contains(&preset.to_lowercase().as_str()) {
        review.advice.push(Advice {
            level: Level::Recommended,
            title: "Alivia el trabajo del procesador".to_string(),
            why: format!(
                "OBS está en el modo \"{preset}\", que le pide mucho esfuerzo al procesador por una mejora de imagen que casi no se nota en un directo."
            ),
            steps: vec![
                format!("En OBS abre {} > {}.", menu::SETTINGS, menu::OUTPUT),
                format!("En \"{}\" elige veryfast.", menu::PRESET),
            ],
        });
    }
}

fn check_memory(hw: &Hardware, review: &mut Review) {
    let used = hw.ram_used_pct();
    if used >= RAM_ALMOST_FULL_PCT {
        review.advice.push(Advice {
            level: Level::Important,
            title: "Libera memoria antes de salir en directo".to_string(),
            why: format!(
                "Tu memoria está al {used} %. Cuando se llena, Windows empieza a usar el disco como si fuera memoria y todo se vuelve lento: el juego, OBS y el directo."
            ),
            steps: vec!["Cierra los programas de la lista de más abajo, empezando por los que más memoria ocupan.".to_string()],
        });
    } else if hw.ram_total_gb() < RAM_MINIMUM_GB {
        review.advice.push(Advice {
            level: Level::Tip,
            title: "Tu PC tiene poca memoria: cuídala".to_string(),
            why: "Con esta cantidad de memoria, juego + OBS + navegador entran justos. Cuanto menos tengas abierto, mejor va a salir el directo."
                .to_string(),
            steps: vec!["Antes de transmitir, cierra todo lo que no uses en el directo.".to_string()],
        });
    } else {
        review.good.push(format!("Tienes memoria libre de sobra (usas el {used} %)."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::{gpu, pc};
    use crate::obs::parse_profile;

    fn gaming_pc() -> Hardware {
        pc(6, 16, vec![gpu("NVIDIA GeForce GTX 1660", GpuVendor::Nvidia, 6)])
    }

    fn office_laptop() -> Hardware {
        pc(4, 8, vec![gpu("Intel(R) UHD Graphics 620", GpuVendor::Intel, 0)])
    }

    fn old_pc() -> Hardware {
        pc(2, 4, vec![])
    }

    fn found(basic: &str, encoder: Option<&str>, service: Option<&str>) -> ObsStatus {
        ObsStatus::Found(parse_profile(basic, encoder, service, "p"))
    }

    fn titles(review: &Review) -> String {
        review.advice.iter().map(|a| a.title.as_str()).collect::<Vec<_>>().join(" | ")
    }

    #[test]
    fn gaming_pc_gets_1080p60_on_the_card() {
        let s = suggest(&gaming_pc());
        assert_eq!((s.encoder, s.width, s.height, s.fps, s.bitrate_kbps), (EncoderKind::Nvidia, 1920, 1080, 60, 6000));
    }

    #[test]
    fn laptop_with_integrated_graphics_gets_720p30() {
        let s = suggest(&office_laptop());
        assert_eq!((s.encoder, s.height, s.fps, s.bitrate_kbps), (EncoderKind::Intel, 720, 30, 3000));
    }

    #[test]
    fn pc_without_any_card_falls_back_to_the_processor() {
        let s = suggest(&old_pc());
        assert_eq!((s.encoder, s.height, s.fps), (EncoderKind::Software, 480, 30));
    }

    #[test]
    fn nvidia_card_without_encoder_is_not_suggested() {
        let hw = pc(4, 8, vec![gpu("NVIDIA GeForce GT 1030", GpuVendor::Nvidia, 2)]);
        assert_eq!(suggest(&hw).encoder, EncoderKind::Software);
    }

    #[test]
    fn default_obs_on_a_gaming_pc_should_move_to_the_card() {
        let hw = gaming_pc();
        let obs = found("[Video]\nOutputCX=1280\nOutputCY=720\n", None, None);
        let review = review(&hw, &obs, &suggest(&hw));
        assert_eq!(review.advice[0].level, Level::Important);
        assert!(review.advice[0].title.contains("placa de video"), "{}", titles(&review));
    }

    #[test]
    fn too_big_a_picture_on_a_weak_pc_is_important() {
        let hw = office_laptop();
        let obs = found(
            "[SimpleOutput]\nStreamEncoder=x264\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        );
        let review = review(&hw, &obs, &suggest(&hw));
        assert!(titles(&review).contains("Baja la imagen a 720p a 30 fps"), "{}", titles(&review));
    }

    #[test]
    fn low_bitrate_for_the_picture_size_is_flagged() {
        let hw = gaming_pc();
        let obs = found(
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=2500\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        );
        let review = review(&hw, &obs, &suggest(&hw));
        assert!(titles(&review).contains("Sube la calidad de envío a unos 6000"), "{}", titles(&review));
    }

    #[test]
    fn twitch_above_its_limit_is_flagged() {
        let hw = gaming_pc();
        let obs = found(
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=9000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(r#"{"settings":{"service":"Twitch"}}"#),
        );
        let review = review(&hw, &obs, &suggest(&hw));
        assert!(titles(&review).contains("Baja la calidad de envío a 6000"), "{}", titles(&review));
    }

    #[test]
    fn variable_bitrate_and_slow_preset_are_flagged() {
        let hw = old_pc();
        let obs = found(
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\n[Video]\nOutputCX=854\nOutputCY=480\n",
            Some(r#"{"bitrate":1500,"preset":"medium","rate_control":"VBR"}"#),
            None,
        );
        let review = review(&hw, &obs, &suggest(&hw));
        assert!(titles(&review).contains("CBR"), "{}", titles(&review));
        assert!(titles(&review).contains("Alivia el trabajo del procesador"), "{}", titles(&review));
    }

    #[test]
    fn a_well_configured_pc_gets_no_complaints() {
        let hw = gaming_pc();
        let obs = found(
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(r#"{"settings":{"service":"Twitch"}}"#),
        );
        let review = review(&hw, &obs, &suggest(&hw));
        assert!(review.advice.is_empty(), "{}", titles(&review));
        assert!(review.good.len() >= 3);
    }

    #[test]
    fn memory_almost_full_is_important_even_without_obs() {
        let mut hw = gaming_pc();
        hw.ram_used = hw.ram_total / 100 * 92;
        let obs = ObsStatus::NotFound { looked_in: "x".into() };
        let review = review(&hw, &obs, &suggest(&hw));
        assert_eq!(review.advice[0].level, Level::Important);
        assert!(review.advice[0].title.contains("memoria"));
    }
}
