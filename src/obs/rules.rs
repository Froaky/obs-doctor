//! Advice that follows from how OBS is configured, compared with what this PC,
//! this connection and this platform can take.
//!
//! Every rule is a pure function of [`Facts`], so each one has a test with a
//! concrete PC behind it. Rules that are only a hunch (a setting that is
//! usually too heavy for this kind of PC) are worded as something to try and
//! stay at Tip level; they move up only when the last stream shows the
//! problem really happened.

use super::config::{EncoderKind, ObsConfig, OutputMode, Recording, ScaleFilter};
use super::logs::{LogFacts, NOTICEABLE_LOSS_PCT, SERIOUS_LOSS_PCT, StreamStats};
use super::menu;
use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review};
use crate::net;
use crate::platforms;
use crate::report::text;
use crate::suggest::Suggested;

/// Below this the audio sounds thin. OBS's default is 160 kbps
/// (frontend/widgets/OBSBasic.cpp) and YouTube asks for 128 kbps stereo
/// (https://support.google.com/youtube/answer/2853702).
const MIN_AUDIO_KBPS: u32 = 96;

/// x264's own default keyframe interval, in frames, used when OBS's is 0
/// ("auto"): obs-x264.c leaves `keyint_max` untouched in that case.
const X264_AUTO_KEYINT_FRAMES: f32 = 250.0;

pub fn advise(facts: &Facts, review: &mut Review) {
    let Some(config) = facts.config() else {
        return;
    };
    let suggested = &facts.suggested;
    check_encoder(config, facts, review);
    check_picture_size(config, suggested, review);
    if !check_upload(config, facts, review) {
        check_bitrate(config, facts, review);
    }
    check_rate_control(config, review);
    check_keyframes(config, review);
    check_preset(config, facts, review);
    check_expensive_options(config, facts, review);
    check_recording(config, facts, review);
    check_scale_filter(config, facts, review);
    check_audio(config, review);
}

/// True when the suggestion for this PC is 720p at 30 fps or less: the
/// processor, the memory or the graphics leave little room.
fn weak_pc(suggested: &Suggested) -> bool {
    suggested.load() < 1280 * 720 * 60
}

/// The last stream, when it is long enough to judge the setup by it.
fn last_stream(logs: &LogFacts) -> Option<&StreamStats> {
    logs.last_stream.as_ref().filter(|s| !s.too_short())
}

/// Share of frames the encoder skipped in the last stream, when that stream
/// used this kind of encoder and the loss was noticeable.
fn encoder_struggled(logs: &LogFacts, kind: EncoderKind) -> Option<f32> {
    let stream = last_stream(logs)?;
    if stream.encoder_kind() != Some(kind) {
        return None;
    }
    stream.skipped_encoding_pct.filter(|pct| *pct >= NOTICEABLE_LOSS_PCT)
}

fn check_encoder(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    let suggested = &facts.suggested;
    let encoder = config.effective_encoder(&facts.logs);
    // OBS's own list of working encoders has the last word.
    let card_works = facts.logs.can_encode_with(suggested.encoder) != Some(false);
    if encoder == EncoderKind::Software && suggested.encoder.is_hardware() && card_works {
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
    } else if encoder.is_hardware() {
        review.ok("Tu placa de video se encarga del video del directo: el procesador queda libre para el juego.");
    }
}

fn check_picture_size(config: &ObsConfig, suggested: &Suggested, review: &mut Review) {
    let Some((width, height)) = config.stream_size() else {
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

/// Rounds a bitrate down to a value people type: a multiple of 500.
fn round_down(kbps: u32) -> u32 {
    (kbps / 500 * 500).max(500)
}

/// `obs.bitrate.above-upload`: the stream needs more than the measured
/// upload can carry. Returns true when it fired, so the generic bitrate
/// rule does not contradict it.
fn check_upload(config: &ObsConfig, facts: &Facts, review: &mut Review) -> bool {
    let net = &facts.net;
    if !net.measured {
        return false;
    }
    let (Some(upload), Some(cap)) = (net.upload_mbps, net::max_bitrate_for_upload(net.upload_mbps)) else {
        return false;
    };
    let sending = config.effective_bitrate_kbps();
    if sending <= cap {
        return false;
    }
    // Video plus audio over the whole upload: frames will be lost for sure.
    let over_the_line = f64::from(sending + config.audio_kbps) > upload * 1000.0;
    let target = round_down(cap);
    let mut steps = bitrate_steps(config, target);
    if let Some((_, height)) = config.stream_size()
        && target < platforms::bitrate_for(height.min(1080), config.fps)
    {
        steps.push(format!(
            "Con {target} kbps la imagen se ve mejor más chica: en {} > {} elige 1280x720 o menos.",
            menu::SETTINGS,
            menu::VIDEO
        ));
    }
    review.add(Advice {
        id: "obs.bitrate.above-upload",
        level: if over_the_line { Level::Important } else { Level::Recommended },
        area: Area::Obs,
        title: format!("Baja la calidad de envío a {target} kbps"),
        why: format!(
            "OBS envía {sending} kbps y la subida de tu internet es de {}. Hace falta dejar margen: si no, se pierden cuadros y tu público ve saltos.",
            text::mbps(upload)
        ),
        steps,
    });
    true
}

fn check_bitrate(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    let Some((_, height)) = config.stream_size() else {
        return;
    };
    let platform_max = config.platform_max_kbps();
    let limits_ignored = config.ignore_platform_limits || !config.obs_enforces_platform_limits();

    if let (Some(platform), Some(max)) = (config.platform(), platform_max)
        && limits_ignored
        && config.bitrate_kbps > max
    {
        let mut steps = bitrate_steps(config, max);
        if config.ignore_platform_limits {
            steps.push(format!(
                "En {} > {} desmarca \"{}\": así OBS no vuelve a pasarse.",
                menu::SETTINGS,
                menu::STREAM,
                menu::IGNORE_RECOMMENDED
            ));
        }
        review.add(Advice {
            id: "obs.bitrate.above-platform-max",
            level: Level::Recommended,
            area: Area::Obs,
            title: format!("Baja la calidad de envío a {max} kbps"),
            why: format!(
                "Tienes {} kbps y {} acepta hasta {max} kbps para tu imagen. Por encima de eso, a muchos espectadores se les corta el directo o la plataforma lo rechaza.",
                config.bitrate_kbps, platform.name
            ),
            steps,
        });
        return;
    }

    let mut recommended = platforms::bitrate_for(height.min(1080), config.fps);
    if let Some(max) = platform_max {
        recommended = recommended.min(max);
    }
    let sending = config.effective_bitrate_kbps();
    if (sending as f64) < recommended as f64 * 0.6 {
        let mut steps = bitrate_steps(config, recommended);
        if !facts.net.measured {
            steps.push(format!(
                "Antes revisa tu internet: la subida debería ser de al menos {} Mbps. Si no llegas, mejor baja el tamaño de la imagen.",
                net::upload_needed_mbps(recommended)
            ));
        }
        review.add(Advice {
            id: "obs.bitrate.too-low",
            level: Level::Recommended,
            area: Area::Obs,
            title: format!("Sube la calidad de envío a unos {recommended}"),
            why: format!(
                "Tienes {} para una imagen de {}p a {} fps. Con tan poco, la imagen se ve borrosa o \"a cuadraditos\" cuando hay movimiento.",
                sending,
                height,
                config.fps.round()
            ),
            steps,
        });
    } else {
        review.ok(format!("La calidad de envío ({sending} kbps) va bien con el tamaño de tu imagen."));
    }
}

fn bitrate_steps(config: &ObsConfig, target: u32) -> Vec<String> {
    let field = match config.mode {
        OutputMode::Simple => menu::BITRATE,
        // The advanced encoder settings call it "Tasa de bits".
        OutputMode::Advanced => "Tasa de bits",
    };
    vec![menu::open(menu::OUTPUT), format!("En \"{field}\" escribe {target}.")]
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

/// `obs.keyframe.platform-rejects`: only possible in advanced mode with the
/// platform's settings switched off, because otherwise OBS sets the
/// platform's interval by itself (AdvancedOutput.cpp, UpdateStreamSettings).
fn check_keyframes(config: &ObsConfig, review: &mut Review) {
    let (Some(keyint), Some(platform)) = (config.keyint_sec, config.platform()) else {
        return;
    };
    let Some(max) = platform.max_keyint_sec else {
        return;
    };
    if config.obs_enforces_platform_limits() {
        return;
    }
    let level = if keyint > max {
        Level::Recommended
    } else if keyint == 0 && config.encoder == EncoderKind::Software {
        // x264's automatic interval is 250 frames: over 4 s even at 60 fps.
        if X264_AUTO_KEYINT_FRAMES / config.fps.max(1.0) > max as f32 { Level::Recommended } else { return }
    } else if keyint == 0 {
        // Hardware encoders pick their own automatic interval; it may be fine.
        Level::Tip
    } else {
        return;
    };
    let target = max.min(2);
    review.add(Advice {
        id: "obs.keyframe.platform-rejects",
        level,
        area: Area::Obs,
        title: format!("Pon un fotograma clave cada {target} segundos"),
        why: format!(
            "{} pide un fotograma clave (una imagen completa) cada {max} segundos como mucho. Si tardan más, el directo tarda en arrancar para quien entra y se ve borroso después de cada corte.",
            platform.name
        ),
        steps: vec![
            menu::open(menu::OUTPUT),
            format!("En la pestaña {}, en \"{}\" escribe {target}.", menu::STREAM, menu::KEYFRAME),
        ],
    });
}

/// x264 presets slower than the default "veryfast". OBS's x264 label says
/// it plainly: "superior = menos CPU".
fn check_preset(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    if config.encoder != EncoderKind::Software || !config.encoder_id.contains("x264") {
        return;
    }
    let Some(preset) = config.preset.as_deref().map(str::to_lowercase) else {
        return;
    };
    const SLOW: &[&str] = &["medium", "slow", "slower", "veryslow", "placebo"];
    const SOMEWHAT_SLOW: &[&str] = &["faster", "fast"];
    let struggled = encoder_struggled(&facts.logs, EncoderKind::Software);
    let fires = SLOW.contains(&preset.as_str())
        || (SOMEWHAT_SLOW.contains(&preset.as_str()) && (weak_pc(&facts.suggested) || struggled.is_some()));
    if !fires {
        return;
    }
    let level =
        if struggled.is_some_and(|pct| pct >= SERIOUS_LOSS_PCT) { Level::Important } else { Level::Recommended };
    let mut why = format!(
        "OBS está en el modo \"{preset}\", que le pide mucho esfuerzo al procesador por una mejora de imagen que casi no se nota en un directo."
    );
    if let Some(pct) = struggled {
        why.push_str(&format!(" En tu último directo se saltó el {} de los cuadros por eso.", text::pct(pct)));
    }
    review.add(Advice {
        id: "obs.preset.too-slow",
        level,
        area: Area::Obs,
        title: "Alivia el trabajo del procesador".to_string(),
        why,
        steps: vec![
            menu::open(menu::OUTPUT),
            format!("En \"{}\" (o \"{}\") elige veryfast.", menu::PRESET_SIMPLE, menu::PRESET_X264),
        ],
    });
}

/// What makes a hardware encoder work harder, with the step that undoes it.
fn expensive_options(config: &ObsConfig) -> Vec<String> {
    let o = &config.options;
    let preset = o.preset.as_deref().map(str::to_lowercase);
    let preset_field = match config.mode {
        OutputMode::Simple => menu::PRESET_SIMPLE,
        OutputMode::Advanced => menu::ENC_PRESET,
    };
    let mut steps = Vec::new();
    match config.encoder {
        EncoderKind::Nvidia => {
            // p6/p7 and the pre-OBS 28 "slow" and "mq" (max quality).
            if preset.as_deref().is_some_and(|p| matches!(p, "p6" | "p7" | "slow" | "mq")) {
                steps.push(format!("En \"{preset_field}\" elige P5."));
            }
            if o.multipass.as_deref() == Some("fullres") {
                steps.push(format!("En \"{}\" elige \"{}\".", menu::MULTIPASS, menu::MULTIPASS_QRES));
            }
            if o.lookahead == Some(true) {
                steps.push(format!("Desmarca \"{}\".", menu::LOOKAHEAD));
            }
        }
        // obs-qsv11: TU1 "El más lento", TU2 "Más lento".
        EncoderKind::Intel if preset.as_deref().is_some_and(|p| matches!(p, "tu1" | "tu2")) => {
            steps.push(format!("En \"{}\" elige TU4.", menu::TARGET_USAGE));
        }
        // texture-amf.cpp: "quality" and "highQuality" are the heavy ones.
        EncoderKind::Amd if preset.as_deref().is_some_and(|p| matches!(p, "quality" | "highquality")) => {
            steps.push(format!("En \"{preset_field}\" elige \"Equilibrado\"."));
        }
        _ => {}
    }
    steps
}

/// `obs.encoder.expensive-options`: heavy hardware encoder settings. A
/// strong card handles them fine (NVIDIA itself recommends some of them),
/// so this only speaks up with evidence, or as a tip on a modest PC.
fn check_expensive_options(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    if !config.encoder.is_hardware() {
        return;
    }
    let changes = expensive_options(config);
    if changes.is_empty() {
        return;
    }
    let struggled = encoder_struggled(&facts.logs, config.encoder);
    // AMF's "quality" is its default: only worth touching with evidence.
    let amd_default = config.encoder == EncoderKind::Amd;
    let (level, why) = match struggled {
        Some(pct) => (
            Level::Recommended,
            format!(
                "En tu último directo la placa se saltó el {} de los cuadros, y tienes activadas opciones que le piden más trabajo por una mejora que casi no se nota en un directo.",
                text::pct(pct)
            ),
        ),
        None if weak_pc(&facts.suggested) && !amd_default => (
            Level::Tip,
            "Tienes activadas opciones que le piden más trabajo a la placa. En una PC como la tuya conviene probar sin ellas: la diferencia en la imagen casi no se nota."
                .to_string(),
        ),
        None => return,
    };
    let mut steps = vec![menu::open(menu::OUTPUT)];
    steps.extend(changes);
    steps.push("Haz un directo corto de prueba y compara.".to_string());
    review.add(Advice {
        id: "obs.encoder.expensive-options",
        level,
        area: Area::Obs,
        title: "Aligera los ajustes del codificador de tu placa".to_string(),
        why,
        steps,
    });
}

/// `obs.recording.second-software-encoder`: OBS itself warns about this
/// ("Basic.Settings.Output.Simple.Warn.Encoder": a software encoder for the
/// recording means extra processor work while streaming and recording).
fn check_recording(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    let heavy = match &config.recording {
        Recording::Separate { encoder: EncoderKind::Software, .. } => "un segundo codificador que usa el procesador",
        Recording::Lossless => "la calidad sin pérdida, que usa el procesador y llena el disco muy rápido",
        _ => return,
    };
    let recorded_while_live = last_stream(&facts.logs).is_some_and(|s| s.shared_encoding);
    let struggled =
        last_stream(&facts.logs).and_then(|s| s.skipped_encoding_pct).is_some_and(|pct| pct >= NOTICEABLE_LOSS_PCT);
    let level =
        if recorded_while_live && (struggled || weak_pc(&facts.suggested)) { Level::Recommended } else { Level::Tip };
    let steps = match config.mode {
        OutputMode::Simple => vec![
            menu::open(menu::OUTPUT),
            format!("En \"{}\" elige \"{}\".", menu::REC_QUALITY, menu::REC_SAME_AS_STREAM),
        ],
        OutputMode::Advanced => vec![
            menu::open(menu::OUTPUT),
            format!(
                "En la pestaña {}, en \"{}\" elige \"{}\".",
                menu::RECORDING,
                menu::ENCODER,
                menu::REC_USE_STREAM_ENCODER
            ),
        ],
    };
    review.add(Advice {
        id: "obs.recording.second-software-encoder",
        level,
        area: Area::Obs,
        title: "Graba con la misma preparación que el directo".to_string(),
        why: format!(
            "Tus grabaciones usan {heavy}. Si grabas mientras transmites, ese trabajo se suma al del directo y puede trabarlo."
        ),
        steps,
    });
}

/// `obs.scale-filter.expensive`: Lanczos takes 36 samples per pixel against
/// Bicubic's 16 (OBS's own labels), work for the graphics card.
fn check_scale_filter(config: &ObsConfig, facts: &Facts, review: &mut Review) {
    let (filter, in_encoder) = match config.rescale {
        Some((_, filter)) => (filter, true),
        None if config.base.is_some() && config.base != config.output => (config.scale_filter, false),
        None => return,
    };
    if filter != ScaleFilter::Lanczos {
        return;
    }
    let lagged = last_stream(&facts.logs).and_then(|s| s.lagged_render_pct).filter(|pct| *pct >= NOTICEABLE_LOSS_PCT);
    let modest_graphics = facts.hw.best_gpu().is_none_or(|gpu| gpu.is_integrated());
    let level = match (lagged, modest_graphics) {
        (Some(_), _) => Level::Recommended,
        (None, true) => Level::Tip,
        (None, false) => return,
    };
    let steps = if in_encoder {
        vec![
            menu::open(menu::OUTPUT),
            format!("Junto a \"{}\", cambia Lanczos por {}.", menu::RESCALE, menu::SCALE_BICUBIC),
        ]
    } else {
        vec![menu::open(menu::VIDEO), format!("En \"{}\" elige \"{}\".", menu::SCALE_FILTER, menu::SCALE_BICUBIC)]
    };
    review.add(Advice {
        id: "obs.scale-filter.expensive",
        level,
        area: Area::Obs,
        title: "Achica la imagen con el filtro Bicúbico".to_string(),
        why: "OBS achica tu imagen con el filtro Lanczos, el más pesado para la placa de video. Bicúbico se ve casi igual en un directo y cuesta menos de la mitad."
            .to_string(),
        steps,
    });
}

/// `obs.audio.bitrate-out-of-range`: too little sounds thin; more than the
/// platform takes only happens when its limits are ignored.
fn check_audio(config: &ObsConfig, review: &mut Review) {
    let platform_max = config.platform().and_then(|p| p.max_audio_kbps);
    let limits_ignored = config.ignore_platform_limits || !config.obs_enforces_platform_limits();
    let (target, why) = if config.audio_kbps < MIN_AUDIO_KBPS {
        (
            160,
            format!(
                "El audio sale a {} kbps: con tan poco, las voces y la música suenan apagadas o metálicas.",
                config.audio_kbps
            ),
        )
    } else if let Some(max) = platform_max.filter(|max| limits_ignored && config.audio_kbps > *max) {
        (
            max,
            format!(
                "El audio sale a {} kbps y la plataforma acepta hasta {max}. Lo que sobra no mejora el sonido y puede dar problemas al conectar.",
                config.audio_kbps
            ),
        )
    } else {
        return;
    };
    let where_ = match config.mode {
        OutputMode::Simple => format!("En \"{}\" elige {target}.", menu::AUDIO_BITRATE),
        OutputMode::Advanced => {
            format!(
                "En la pestaña Audio, en la pista que usas para el directo, pon \"{}\" en {target}.",
                menu::AUDIO_BITRATE
            )
        }
    };
    review.add(Advice {
        id: "obs.audio.bitrate-out-of-range",
        level: Level::Tip,
        area: Area::Obs,
        title: format!("Pon el audio en {target} kbps"),
        why,
        steps: vec![menu::open(menu::OUTPUT), where_],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::{gpu, pc};
    use crate::hardware::{GpuVendor, Hardware};
    use crate::net::NetFacts;
    use crate::obs::config::{ObsStatus, ProfileFiles, parse, parse_profile};

    const TWITCH: &str = r#"{"type":"rtmp_common","settings":{"service":"Twitch","key":"x"}}"#;
    const YOUTUBE: &str = r#"{"type":"rtmp_common","settings":{"service":"YouTube - RTMPS"}}"#;
    const FACEBOOK: &str = r#"{"type":"rtmp_common","settings":{"service":"Facebook Live"}}"#;

    fn gaming_pc() -> Hardware {
        pc(6, 16, vec![gpu("NVIDIA GeForce GTX 1660", GpuVendor::Nvidia, 6)])
    }

    fn office_laptop() -> Hardware {
        pc(4, 8, vec![gpu("Intel(R) UHD Graphics 620", GpuVendor::Intel, 0)])
    }

    fn strong() -> Suggested {
        Suggested {
            encoder: EncoderKind::Nvidia,
            encoder_label: "NVENC".into(),
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 6000,
        }
    }

    fn modest(encoder: EncoderKind) -> Suggested {
        Suggested { encoder, encoder_label: "x".into(), width: 1280, height: 720, fps: 30, bitrate_kbps: 3000 }
    }

    /// Facts for a PC with a given OBS profile and a hand-made suggestion.
    fn facts(hw: Hardware, suggested: Suggested, basic: &str, encoder: Option<&str>, service: Option<&str>) -> Facts {
        Facts { hw, obs: ObsStatus::Found(parse_profile(basic, encoder, service, "p")), suggested, ..Facts::default() }
    }

    fn review_of(facts: &Facts) -> Review {
        let mut review = Review::default();
        advise(facts, &mut review);
        review
    }

    fn stream(encoder_id: &str, skipped: f32, lagged: f32, shared: bool) -> StreamStats {
        StreamStats {
            duration_secs: Some(3600),
            encoder_id: Some(encoder_id.into()),
            skipped_encoding_pct: Some(skipped),
            lagged_render_pct: Some(lagged),
            shared_encoding: shared,
            ..StreamStats::default()
        }
    }

    fn measured(upload_mbps: f64) -> NetFacts {
        NetFacts { measured: true, upload_mbps: Some(upload_mbps), ..NetFacts::default() }
    }

    const NVENC_1080P60: &str =
        "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n";

    #[test]
    fn default_obs_on_a_gaming_pc_should_move_to_the_card() {
        let review = review_of(&facts(
            gaming_pc(),
            strong(),
            "[SimpleOutput]\nStreamEncoder=x264\n[Video]\nOutputCX=1280\nOutputCY=720\n",
            None,
            None,
        ));
        assert_eq!(
            review.get("obs.encoder.use-hardware").map(|a| a.level),
            Some(Level::Important),
            "{:?}",
            review.ids()
        );
    }

    #[test]
    fn simple_mode_default_encoder_is_nvenc_when_obs_lists_it() {
        let mut f = facts(gaming_pc(), strong(), "[Video]\nOutputCX=1280\nOutputCY=720\n", None, None);
        f.logs.available_encoders = Some(vec!["obs_x264".into(), "obs_nvenc_h264_tex".into()]);
        assert!(!review_of(&f).has("obs.encoder.use-hardware"));
    }

    #[test]
    fn no_hardware_suggestion_when_obs_says_the_card_cannot() {
        let mut f = facts(gaming_pc(), strong(), "[SimpleOutput]\nStreamEncoder=x264\n", None, None);
        f.logs.available_encoders = Some(vec!["obs_x264".into()]);
        assert!(!review_of(&f).has("obs.encoder.use-hardware"));
    }

    #[test]
    fn too_big_a_picture_on_a_weak_pc_is_important() {
        let review = review_of(&facts(
            office_laptop(),
            modest(EncoderKind::Intel),
            "[SimpleOutput]\nStreamEncoder=x264\nVBitrate=6000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        ));
        let advice = review.get("obs.picture.too-big").expect("should flag the picture size");
        assert_eq!(advice.level, Level::Important);
    }

    #[test]
    fn the_rescaled_size_is_what_counts() {
        let review = review_of(&facts(
            office_laptop(),
            modest(EncoderKind::Intel),
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_qsv11_v2\nRescaleFilter=2\nRescaleRes=1280x720\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=30\n",
            Some(r#"{"bitrate":3000}"#),
            None,
        ));
        assert!(!review.has("obs.picture.too-big"), "{:?}", review.ids());
    }

    #[test]
    fn low_bitrate_for_the_picture_size_is_flagged() {
        let review = review_of(&facts(
            gaming_pc(),
            strong(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=2500\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            None,
        ));
        assert!(review.has("obs.bitrate.too-low"), "{:?}", review.ids());
    }

    #[test]
    fn twitch_limit_obs_enforces_is_not_flagged() {
        let review = review_of(&facts(
            gaming_pc(),
            strong(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=9000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(TWITCH),
        ));
        assert!(!review.has("obs.bitrate.above-platform-max"), "{:?}", review.ids());
    }

    #[test]
    fn twitch_above_its_limit_is_flagged_when_limits_are_ignored() {
        let review = review_of(&facts(
            gaming_pc(),
            strong(),
            "[Stream1]\nIgnoreRecommended=true\n[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=9000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(TWITCH),
        ));
        let advice = review.get("obs.bitrate.above-platform-max").expect("flagged");
        assert_eq!(advice.level, Level::Recommended);
        assert!(advice.title.contains("6000"));
        assert_eq!(advice.steps.len(), 3);
    }

    #[test]
    fn facebook_limit_depends_on_the_picture() {
        // Advanced mode with the service settings off: 5000 kbps at 720p30
        // is over Facebook's 4000.
        let review = review_of(&facts(
            gaming_pc(),
            strong(),
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_nvenc_h264_tex\nApplyServiceSettings=false\n[Video]\nOutputCX=1280\nOutputCY=720\nFPSCommon=30\n",
            Some(r#"{"bitrate":5000,"keyint_sec":2}"#),
            Some(FACEBOOK),
        ));
        assert!(review.get("obs.bitrate.above-platform-max").is_some_and(|a| a.title.contains("4000")));
    }

    #[test]
    fn above_upload_needs_a_measurement() {
        let mut f = facts(gaming_pc(), strong(), NVENC_1080P60, None, None);
        assert!(!review_of(&f).has("obs.bitrate.above-upload"), "never without a measured upload");

        f.net = measured(5.0);
        let review = review_of(&f);
        let advice = review.get("obs.bitrate.above-upload").expect("6000 kbps over 5 Mbps");
        assert_eq!(advice.level, Level::Important);
        assert!(advice.title.contains("3000"), "{}", advice.title);
        assert!(!review.has("obs.bitrate.too-low"));

        f.net = measured(8.0);
        let advice = review_of(&f).get("obs.bitrate.above-upload").cloned().expect("6000 over a 5333 cap");
        assert_eq!(advice.level, Level::Recommended);

        f.net = measured(20.0);
        assert!(!review_of(&f).has("obs.bitrate.above-upload"));
    }

    #[test]
    fn above_upload_uses_what_obs_really_sends() {
        // 9000 configured, but OBS sends 6000 to Twitch; 10 Mbps carries it.
        let mut f = facts(
            gaming_pc(),
            strong(),
            "[SimpleOutput]\nStreamEncoder=nvenc\nVBitrate=9000\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n",
            None,
            Some(TWITCH),
        );
        f.net = measured(10.0);
        assert!(!review_of(&f).has("obs.bitrate.above-upload"));
    }

    #[test]
    fn keyframes_only_matter_when_obs_does_not_set_them() {
        let advanced = |keyint: u32, apply: bool| {
            facts(
                gaming_pc(),
                strong(),
                &format!(
                    "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\nApplyServiceSettings={apply}\n[Video]\nOutputCX=1280\nOutputCY=720\nFPSCommon=30\n"
                ),
                Some(&format!(r#"{{"bitrate":3000,"keyint_sec":{keyint}}}"#)),
                Some(TWITCH),
            )
        };
        assert!(!review_of(&advanced(10, true)).has("obs.keyframe.platform-rejects"));
        assert!(!review_of(&advanced(2, false)).has("obs.keyframe.platform-rejects"));
        assert_eq!(
            review_of(&advanced(10, false)).get("obs.keyframe.platform-rejects").map(|a| a.level),
            Some(Level::Recommended)
        );
        // x264 "auto" is 250 frames: about 8 seconds at 30 fps.
        assert_eq!(
            review_of(&advanced(0, false)).get("obs.keyframe.platform-rejects").map(|a| a.level),
            Some(Level::Recommended)
        );
        // YouTube takes up to 4 seconds.
        let youtube = facts(
            gaming_pc(),
            strong(),
            "[Output]\nMode=Advanced\n[AdvOut]\nApplyServiceSettings=false\n",
            Some(r#"{"keyint_sec":3}"#),
            Some(YOUTUBE),
        );
        assert!(!review_of(&youtube).has("obs.keyframe.platform-rejects"));
    }

    #[test]
    fn variable_bitrate_and_slow_preset_are_flagged() {
        let review = review_of(&facts(
            pc(2, 4, vec![]),
            Suggested::default(),
            "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_x264\n[Video]\nOutputCX=854\nOutputCY=480\n",
            Some(r#"{"bitrate":1500,"preset":"medium","rate_control":"VBR"}"#),
            None,
        ));
        assert!(review.has("obs.rate-control.not-constant"), "{:?}", review.ids());
        assert_eq!(review.get("obs.preset.too-slow").map(|a| a.level), Some(Level::Recommended));
    }

    #[test]
    fn x264_fast_is_fine_on_a_strong_pc_but_not_on_a_weak_one() {
        let basic =
            "[SimpleOutput]\nStreamEncoder=x264\nPreset=fast\nVBitrate=3000\n[Video]\nOutputCX=1280\nOutputCY=720\n";
        assert!(!review_of(&facts(gaming_pc(), strong(), basic, None, None)).has("obs.preset.too-slow"));
        assert!(
            review_of(&facts(office_laptop(), modest(EncoderKind::Software), basic, None, None))
                .has("obs.preset.too-slow")
        );

        let mut f = facts(gaming_pc(), strong(), basic, None, None);
        f.logs.last_stream = Some(stream("obs_x264", 8.0, 0.0, false));
        assert_eq!(review_of(&f).get("obs.preset.too-slow").map(|a| a.level), Some(Level::Important));
    }

    #[test]
    fn heavy_nvenc_options_need_evidence_or_a_modest_pc() {
        let basic = "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_nvenc_h264_tex\n[Video]\nOutputCX=1920\nOutputCY=1080\nFPSCommon=60\n";
        let json = r#"{"bitrate":6000,"preset":"p7","multipass":"fullres","lookahead":true}"#;

        assert!(
            !review_of(&facts(gaming_pc(), strong(), basic, Some(json), None)).has("obs.encoder.expensive-options")
        );

        let modest_pc = facts(gaming_pc(), modest(EncoderKind::Nvidia), basic, Some(json), None);
        let advice = review_of(&modest_pc).get("obs.encoder.expensive-options").cloned().expect("tip");
        assert_eq!(advice.level, Level::Tip);
        assert_eq!(advice.steps.len(), 5, "open, preset, multipass, lookahead, try");

        let mut evidence = facts(gaming_pc(), strong(), basic, Some(json), None);
        evidence.logs.last_stream = Some(stream("obs_nvenc_h264_tex", 3.0, 0.0, false));
        assert_eq!(
            review_of(&evidence).get("obs.encoder.expensive-options").map(|a| a.level),
            Some(Level::Recommended)
        );

        // Skips by another encoder are not evidence against this one.
        let mut other = facts(gaming_pc(), strong(), basic, Some(json), None);
        other.logs.last_stream = Some(stream("obs_x264", 3.0, 0.0, false));
        assert!(!review_of(&other).has("obs.encoder.expensive-options"));
    }

    #[test]
    fn default_nvenc_settings_are_never_called_heavy() {
        let mut f = facts(gaming_pc(), modest(EncoderKind::Nvidia), NVENC_1080P60, None, None);
        f.logs.last_stream = Some(stream("obs_nvenc_h264_tex", 9.0, 0.0, false));
        assert!(!review_of(&f).has("obs.encoder.expensive-options"));
    }

    #[test]
    fn amd_default_quality_only_with_evidence() {
        let basic = "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=h264_texture_amf\n";
        let f = facts(office_laptop(), modest(EncoderKind::Amd), basic, None, None);
        assert!(!review_of(&f).has("obs.encoder.expensive-options"));
        let mut f = f;
        f.logs.last_stream = Some(stream("h264_texture_amf", 2.0, 0.0, false));
        assert!(review_of(&f).has("obs.encoder.expensive-options"));
    }

    #[test]
    fn recording_with_a_second_software_encoder() {
        let files = ProfileFiles {
            basic_ini: "[Output]\nMode=Advanced\n[AdvOut]\nEncoder=obs_nvenc_h264_tex\nRecEncoder=obs_x264\n",
            ..ProfileFiles::default()
        };
        let mut f = Facts {
            hw: gaming_pc(),
            obs: ObsStatus::Found(parse(&files, "p")),
            suggested: strong(),
            ..Facts::default()
        };
        assert_eq!(review_of(&f).get("obs.recording.second-software-encoder").map(|a| a.level), Some(Level::Tip));

        f.logs.last_stream = Some(stream("obs_nvenc_h264_tex", 2.0, 0.0, true));
        assert_eq!(
            review_of(&f).get("obs.recording.second-software-encoder").map(|a| a.level),
            Some(Level::Recommended)
        );

        let same = facts(gaming_pc(), strong(), "[SimpleOutput]\nRecQuality=Stream\n", None, None);
        assert!(!review_of(&same).has("obs.recording.second-software-encoder"));
        let hardware = facts(gaming_pc(), strong(), "[SimpleOutput]\nStreamEncoder=nvenc\nRecQuality=HQ\n", None, None);
        assert!(!review_of(&hardware).has("obs.recording.second-software-encoder"));
        let lossless = facts(gaming_pc(), strong(), "[SimpleOutput]\nRecQuality=Lossless\n", None, None);
        assert!(review_of(&lossless).has("obs.recording.second-software-encoder"));
    }

    #[test]
    fn lanczos_only_matters_when_it_is_used_on_modest_graphics() {
        let lanczos = "[Video]\nBaseCX=1920\nBaseCY=1080\nOutputCX=1280\nOutputCY=720\nScaleType=lanczos\n";
        assert_eq!(
            review_of(&facts(office_laptop(), modest(EncoderKind::Intel), lanczos, None, None))
                .get("obs.scale-filter.expensive")
                .map(|a| a.level),
            Some(Level::Tip)
        );
        assert!(!review_of(&facts(gaming_pc(), strong(), lanczos, None, None)).has("obs.scale-filter.expensive"));

        let mut lag = facts(gaming_pc(), strong(), lanczos, None, None);
        lag.logs.last_stream = Some(stream("obs_nvenc_h264_tex", 0.0, 4.0, false));
        assert_eq!(review_of(&lag).get("obs.scale-filter.expensive").map(|a| a.level), Some(Level::Recommended));

        // Same size in and out: the filter is never used.
        let unused = "[Video]\nBaseCX=1280\nBaseCY=720\nOutputCX=1280\nOutputCY=720\nScaleType=lanczos\n";
        assert!(
            !review_of(&facts(office_laptop(), modest(EncoderKind::Intel), unused, None, None))
                .has("obs.scale-filter.expensive")
        );
    }

    #[test]
    fn audio_out_of_range() {
        let low = facts(gaming_pc(), strong(), "[SimpleOutput]\nABitrate=64\n", None, None);
        assert_eq!(review_of(&low).get("obs.audio.bitrate-out-of-range").map(|a| a.level), Some(Level::Tip));

        // 320 to YouTube is lowered by OBS to 160 unless the limits are ignored.
        let enforced = facts(gaming_pc(), strong(), "[SimpleOutput]\nABitrate=320\n", None, Some(YOUTUBE));
        assert!(!review_of(&enforced).has("obs.audio.bitrate-out-of-range"));
        let ignored = facts(
            gaming_pc(),
            strong(),
            "[Stream1]\nIgnoreRecommended=true\n[SimpleOutput]\nABitrate=320\n",
            None,
            Some(YOUTUBE),
        );
        assert!(review_of(&ignored).get("obs.audio.bitrate-out-of-range").is_some_and(|a| a.title.contains("160")));
    }

    #[test]
    fn a_well_configured_pc_gets_no_complaints() {
        let mut f = facts(gaming_pc(), strong(), NVENC_1080P60, None, Some(TWITCH));
        f.net = measured(30.0);
        f.logs.last_stream = Some(stream("obs_nvenc_h264_tex", 0.0, 0.0, false));
        let review = review_of(&f);
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert!(review.good.len() >= 3);
    }

    #[test]
    fn without_obs_there_is_nothing_to_review() {
        let review = review_of(&Facts { hw: gaming_pc(), ..Facts::default() });
        assert!(review.advice.is_empty() && review.good.is_empty());
    }
}
