//! Advice that follows from how OBS is configured, compared with what this PC
//! and this platform can take.
//!
//! Every rule is a pure function of [`Facts`], so each one has a test with a
//! concrete PC behind it.

use super::config::{EncoderKind, ObsConfig};
use super::menu;
use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review};
use crate::net;
use crate::platforms;
use crate::suggest::Suggested;

const TWITCH_MAX_KBPS: u32 = 6000;

pub fn advise(facts: &Facts, review: &mut Review) {
    let Some(config) = facts.config() else {
        return;
    };
    let suggested = &facts.suggested;
    check_encoder(config, suggested, review);
    check_picture_size(config, suggested, review);
    check_bitrate(config, review);
    check_rate_control(config, review);
    check_preset(config, review);
}

fn check_encoder(config: &ObsConfig, suggested: &Suggested, review: &mut Review) {
    if config.encoder == EncoderKind::Software && suggested.encoder.is_hardware() {
        review.add(Advice {
            id: "obs.encoder.use-hardware",
            level: Level::Important,
            area: Area::Obs,
            title: "Pasa el trabajo de transmitir a tu placa de video".to_string(),
            why: "Ahora mismo tu procesador hace dos cosas a la vez: mover el juego y preparar el video del directo. \
                  Tu placa de video puede encargarse del video casi sin esfuerzo y dejar el procesador libre para el juego."
                .to_string(),
            steps: vec![
                menu::open(menu::OUTPUT),
                format!("En \"{}\" elige la opción de hardware: {}.", menu::ENCODER, suggested.encoder_label),
                "Si esa opción no aparece en la lista, tu placa no lo permite: deja el que tienes.".to_string(),
            ],
        });
    } else if config.encoder.is_hardware() {
        review.ok("Tu placa de video se encarga del video del directo: el procesador queda libre para el juego.");
    }
}

fn check_picture_size(config: &ObsConfig, suggested: &Suggested, review: &mut Review) {
    let Some((width, height)) = config.output else {
        return;
    };
    let fps = config.fps.round().min(60.0) as u64;
    let load = width as u64 * height as u64 * fps;

    if height > 1080 {
        review.add(Advice {
            id: "obs.picture.above-1080p",
            level: Level::Recommended,
            area: Area::Obs,
            title: format!("Estás transmitiendo en {width}x{height}: es más de lo que hace falta"),
            why: "Casi nadie ve un directo por encima de 1080p, y enviar una imagen tan grande le exige mucho a tu PC y a tu internet."
                .to_string(),
            steps: picture_steps(suggested),
        });
    } else if load as f64 > suggested.load() as f64 * 1.3 {
        review.add(Advice {
            id: "obs.picture.too-big",
            level: if config.encoder == EncoderKind::Software { Level::Important } else { Level::Recommended },
            area: Area::Obs,
            title: format!("Baja la imagen a {}p a {} fps", suggested.height, suggested.fps),
            why: format!(
                "Estás transmitiendo en {}p a {} fps, y eso es mucho para esta PC: el directo se va a trabar o el juego va a perder fluidez. \
                 Una imagen un poco más chica pero estable se ve mejor que una grande a los saltos.",
                height,
                config.fps.round()
            ),
            steps: picture_steps(suggested),
        });
    } else {
        review.ok(format!("El tamaño de imagen ({}p a {} fps) está bien para tu PC.", height, config.fps.round()));
    }

    if config.fps > 61.0 {
        review.add(Advice {
            id: "obs.picture.fps-above-60",
            level: Level::Recommended,
            area: Area::Obs,
            title: "Baja los fps a 60".to_string(),
            why: format!(
                "Tienes OBS a {} fps, pero las plataformas muestran como mucho 60. Todo lo que pase de ahí es trabajo que tu PC hace para nada.",
                config.fps.round()
            ),
            steps: vec![menu::open(menu::VIDEO), format!("En \"{}\" elige 60.", menu::FPS)],
        });
    }
}

fn picture_steps(suggested: &Suggested) -> Vec<String> {
    vec![
        menu::open(menu::VIDEO),
        format!("En \"{}\" pon {}x{}.", menu::OUTPUT_RES, suggested.width, suggested.height),
        format!("En \"{}\" elige {}.", menu::FPS, suggested.fps),
    ]
}

fn check_bitrate(config: &ObsConfig, review: &mut Review) {
    let Some((_, height)) = config.output else {
        return;
    };
    let recommended = platforms::bitrate_for(height.min(1080), config.fps);
    let on_twitch = config.service.as_deref().is_some_and(|s| s.to_lowercase().contains("twitch"));

    if on_twitch && config.bitrate_kbps > TWITCH_MAX_KBPS {
        review.add(Advice {
            id: "obs.bitrate.above-platform-max",
            level: Level::Recommended,
            area: Area::Obs,
            title: format!("Baja la calidad de envío a {TWITCH_MAX_KBPS}"),
            why: format!(
                "Tienes {} y Twitch recomienda no pasar de {TWITCH_MAX_KBPS}. Por encima de eso, a muchos espectadores se les corta el directo.",
                config.bitrate_kbps
            ),
            steps: bitrate_steps(TWITCH_MAX_KBPS),
        });
    } else if (config.bitrate_kbps as f64) < recommended as f64 * 0.6 {
        let mut steps = bitrate_steps(recommended);
        steps.push(format!(
            "Antes revisa tu internet: la subida debería ser de al menos {} Mbps. Si no llegas, mejor baja el tamaño de la imagen.",
            net::upload_needed_mbps(recommended)
        ));
        review.add(Advice {
            id: "obs.bitrate.too-low",
            level: Level::Recommended,
            area: Area::Obs,
            title: format!("Sube la calidad de envío a unos {recommended}"),
            why: format!(
                "Tienes {} para una imagen de {}p a {} fps. Con tan poco, la imagen se ve borrosa o \"a cuadraditos\" cuando hay movimiento.",
                config.bitrate_kbps,
                height,
                config.fps.round()
            ),
            steps,
        });
    } else {
        review.ok(format!("La calidad de envío ({} kbps) va bien con el tamaño de tu imagen.", config.bitrate_kbps));
    }
}

fn bitrate_steps(target: u32) -> Vec<String> {
    vec![menu::open(menu::OUTPUT), format!("En \"{}\" escribe {target}.", menu::BITRATE)]
}

fn check_rate_control(config: &ObsConfig, review: &mut Review) {
    let Some(rate_control) = config.rate_control.as_deref() else {
        return;
    };
    if !rate_control.eq_ignore_ascii_case("CBR") {
        review.add(Advice {
            id: "obs.rate-control.not-constant",
            level: Level::Recommended,
            area: Area::Obs,
            title: "Usa una calidad de envío constante (CBR)".to_string(),
            why: format!(
                "Tienes OBS en modo {rate_control}, que sube y baja la cantidad de datos todo el tiempo. \
                 Las plataformas de directos piden un envío parejo; si no, a los espectadores se les corta."
            ),
            steps: vec![menu::open(menu::OUTPUT), format!("En \"{}\" elige CBR.", menu::RATE_CONTROL)],
        });
    }
}

fn check_preset(config: &ObsConfig, review: &mut Review) {
    if config.encoder != EncoderKind::Software {
        return;
    }
    let Some(preset) = config.preset.as_deref() else {
        return;
    };
    const TOO_SLOW: &[&str] = &["faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"];
    if TOO_SLOW.contains(&preset.to_lowercase().as_str()) {
        review.add(Advice {
            id: "obs.preset.too-slow",
            level: Level::Recommended,
            area: Area::Obs,
            title: "Alivia el trabajo del procesador".to_string(),
            why: format!(
                "OBS está en el modo \"{preset}\", que le pide mucho esfuerzo al procesador por una mejora de imagen que casi no se nota en un directo."
            ),
            steps: vec![
                menu::open(menu::OUTPUT),
                format!("En \"{}\" (o \"{}\") elige veryfast.", menu::PRESET_SIMPLE, menu::PRESET_X264),
            ],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::{gpu, pc};
    use crate::hardware::{GpuVendor, Hardware};
    use crate::obs::config::{ObsStatus, parse_profile};
    use crate::suggest::suggest;

    fn gaming_pc() -> Hardware {
        pc(6, 16, vec![gpu("NVIDIA GeForce GTX 1660", GpuVendor::Nvidia, 6)])
    }

    fn office_laptop() -> Hardware {
        pc(4, 8, vec![gpu("Intel(R) UHD Graphics 620", GpuVendor::Intel, 0)])
    }

    fn old_pc() -> Hardware {
        pc(2, 4, vec![])
    }

    /// Facts for a PC with a given OBS profile, with the suggestion filled in.
    fn facts(hw: Hardware, basic: &str, encoder: Option<&str>, service: Option<&str>) -> Facts {
        let mut facts =
            Facts { hw, obs: ObsStatus::Found(parse_profile(basic, encoder, service, "p")), ..Facts::default() };
        facts.suggested = suggest(&facts);
        facts
    }

    fn review_of(facts: &Facts) -> Review {
        let mut review = Review::default();
        advise(facts, &mut review);
        review
    }

    #[test]
    fn default_obs_on_a_gaming_pc_should_move_to_the_card() {
        let review = review_of(&facts(gaming_pc(), "[Video]\nOutputCX=1280\nOutputCY=720\n", None, None));
        assert_eq!(
            review.get("obs.encoder.use-hardware").map(|a| a.level),
            Some(Level::Important),
            "{:?}",
            review.ids()
        );
    }

    #[test]
    fn too_big_a_picture_on_a_weak_pc_is_important() {
        let review = review_of(&facts(
            office_laptop(),
            "[SimpleOutput]\nStreamEncoder=x264\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        ));
        let advice = review.get("obs.picture.too-big").expect("should flag the picture size");
        assert_eq!(advice.level, Level::Important);
        assert!(advice.title.contains("720p a 30 fps"), "{}", advice.title);
    }

    #[test]
    fn low_bitrate_for_the_picture_size_is_flagged() {
        let review = review_of(&facts(
            gaming_pc(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=2500\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        ));
        assert!(review.get("obs.bitrate.too-low").is_some_and(|a| a.title.contains("6000")), "{:?}", review.ids());
    }

    #[test]
    fn twitch_above_its_limit_is_flagged() {
        let review = review_of(&facts(
            gaming_pc(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=9000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(r#"{"settings":{"service":"Twitch"}}"#),
        ));
        assert!(review.has("obs.bitrate.above-platform-max"), "{:?}", review.ids());
    }

    #[test]
    fn variable_bitrate_and_slow_preset_are_flagged() {
        let review = review_of(&facts(
            old_pc(),
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\n[Video]\nOutputCX=854\nOutputCY=480\n",
            Some(r#"{"bitrate":1500,"preset":"medium","rate_control":"VBR"}"#),
            None,
        ));
        assert!(review.has("obs.rate-control.not-constant"), "{:?}", review.ids());
        assert!(review.has("obs.preset.too-slow"), "{:?}", review.ids());
    }

    #[test]
    fn a_well_configured_pc_gets_no_complaints() {
        let review = review_of(&facts(
            gaming_pc(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(r#"{"settings":{"service":"Twitch"}}"#),
        ));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert!(review.good.len() >= 3);
    }

    #[test]
    fn without_obs_there_is_nothing_to_review() {
        let review = review_of(&Facts { hw: gaming_pc(), ..Facts::default() });
        assert!(review.advice.is_empty() && review.good.is_empty());
    }
}
