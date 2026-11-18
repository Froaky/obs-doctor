//! `stream.*`: advice that follows from how the last stream actually went.
//! One item per cause, each quoting what the log showed.
//!
//! Thresholds: [`NOTICEABLE_LOSS_PCT`] and [`SERIOUS_LOSS_PCT`] (sources
//! next to their definition). Fixes follow the OBS knowledge base:
//! - dropped frames: <https://obsproject.com/kb/dropped-frames-and-general-connection-issues>
//! - encoder overload: <https://obsproject.com/kb/encoding-performance-troubleshooting>
//! - rendering lag: <https://obsproject.com/kb/gpu-overload-issues>

use super::{Link, MIN_STREAM_SECS};
use super::{LogFacts, NOTICEABLE_LOSS_PCT, SERIOUS_LOSS_PCT, StreamEnd, StreamStats};
use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review};
use crate::obs::config::{EncoderKind, ObsStatus};
use crate::obs::menu;
use crate::report::text;

pub fn advise(facts: &Facts, review: &mut Review) {
    let logs = &facts.logs;
    if logs.logs_read == 0 {
        // Without OBS there are no logs to miss; the OBS area already says so.
        if !matches!(facts.obs, ObsStatus::NotFound { .. }) {
            review.note("No encontré registros de OBS, así que no pude revisar cómo salió tu último directo.");
        }
        return;
    }
    let Some(stream) = &logs.last_stream else {
        return;
    };
    advise_stream(stream, logs, review);
}

fn advise_stream(stream: &StreamStats, logs: &LogFacts, review: &mut Review) {
    let when = evidence(stream);
    match stream.end {
        StreamEnd::Unfinished => {
            review.note(
                "Tu último directo todavía no tiene cierre en el registro de OBS. Si sigues en directo, vuelve a correr OBS Doctor cuando termines.",
            );
            return;
        }
        StreamEnd::Abrupt => review.add(crash(&when)),
        StreamEnd::EncoderError => review.add(encoder_error(&when)),
        StreamEnd::Disconnected => review.add(connection_lost(&when)),
        StreamEnd::Stopped => {}
    }
    if stream.too_short() {
        review.note(format!(
            "Tu último directo duró menos de {MIN_STREAM_SECS} segundos: es muy poco para sacar conclusiones sobre cómo salió."
        ));
        return;
    }

    let before = review.advice.len();
    if let Some(pct) = stream.dropped_network_pct.filter(|p| *p > NOTICEABLE_LOSS_PCT) {
        review.add(dropped_frames(stream, pct, &when));
    } else if stream.dynamic_bitrate && stream.bitrate_reductions > 0 {
        review.add(bitrate_lowered(stream, &when));
    }
    if stream.end != StreamEnd::Disconnected && stream.disconnects > 0 {
        review.add(reconnects(stream.disconnects, &when));
    }
    if let Some(pct) = stream.skipped_encoding_pct.filter(|p| *p > NOTICEABLE_LOSS_PCT) {
        review.add(skipped_frames(stream, logs, pct, &when));
    }
    if let Some(pct) = stream.lagged_render_pct.filter(|p| *p > NOTICEABLE_LOSS_PCT) {
        review.add(lagged_frames(logs, pct, &when));
    }
    if stream.graphics_resets > 0 {
        review.add(graphics_reset(stream.graphics_resets, logs, &when));
    }

    let clean = review.advice.len() == before && stream.end == StreamEnd::Stopped;
    if clean && stream.dropped_network_pct.is_some() && stream.lagged_render_pct.is_some() {
        let date = stream.date.as_deref().map(|d| format!(", el {d},")).unwrap_or_default();
        review.ok(format!(
            "Tu último directo{date} salió limpio: se perdió menos del {} de los cuadros.",
            text::pct(NOTICEABLE_LOSS_PCT)
        ));
    }
}

/// "En tu último directo, el 17/11/2026," ready to start a sentence.
fn evidence(stream: &StreamStats) -> String {
    match &stream.date {
        Some(date) => format!("En tu último directo, el {date},"),
        None => "En tu último directo".to_string(),
    }
}

fn loss_level(pct: f32) -> Level {
    if pct > SERIOUS_LOSS_PCT { Level::Important } else { Level::Recommended }
}

fn times(count: u32) -> String {
    if count == 1 { "una vez".to_string() } else { format!("{count} veces") }
}

const TRY_AGAIN: &str = "Haz un directo corto de prueba y vuelve a correr OBS Doctor para ver el cambio.";

fn dropped_frames(stream: &StreamStats, pct: f32, when: &str) -> Advice {
    let mut why = format!(
        "{when} se perdió el {} de los cuadros porque tu internet no alcanzó a enviarlos a tiempo. Tu público lo vio como saltos o imagen congelada.",
        text::pct(pct)
    );
    if stream.link == Some(Link::Wifi) {
        why.push_str(" Ese directo salió por wifi.");
    }
    let mut steps = vec![menu::open(menu::OUTPUT)];
    match stream.bitrate_kbps {
        Some(kbps) => steps.push(format!(
            "En \"{}\" (la calidad de envío) baja de {kbps} a {} kbps.",
            menu::BITRATE,
            lower_bitrate(kbps)
        )),
        None => steps.push(format!("En \"{}\" (la calidad de envío) baja el número una quinta parte.", menu::BITRATE)),
    }
    if stream.link == Some(Link::Wifi) {
        steps.push(
            "Si puedes, conecta la PC al router con un cable: el wifi pierde cuadros aunque la señal parezca buena."
                .to_string(),
        );
    }
    steps.push(
        "Mientras estés en directo, pausa descargas, actualizaciones y videos en otros aparatos de tu casa."
            .to_string(),
    );
    steps.push(TRY_AGAIN.to_string());
    Advice {
        id: "stream.network.dropped-frames",
        level: loss_level(pct),
        area: Area::LastStream,
        title: match stream.bitrate_kbps {
            Some(kbps) => format!("Baja la calidad de envío de {kbps} kbps o mejora tu conexión"),
            None => "Baja la calidad de envío o mejora tu conexión".to_string(),
        },
        why,
        steps,
    }
}

/// A fifth less, rounded down to hundreds: a step big enough to notice,
/// small enough not to ruin the picture. Something to try, not a rule.
fn lower_bitrate(kbps: u32) -> u32 {
    (kbps / 5 * 4 / 100 * 100).max(500)
}

fn bitrate_lowered(stream: &StreamStats, when: &str) -> Advice {
    let mut steps = vec![menu::open(menu::OUTPUT)];
    if let Some(kbps) = stream.bitrate_kbps {
        steps.push(format!("En \"{}\" (la calidad de envío) prueba con {} kbps.", menu::BITRATE, lower_bitrate(kbps)));
    } else {
        steps.push(format!("En \"{}\" (la calidad de envío) prueba un número un poco más bajo.", menu::BITRATE));
    }
    steps.push(TRY_AGAIN.to_string());
    Advice {
        id: "stream.network.bitrate-lowered",
        level: Level::Tip,
        area: Area::LastStream,
        title: "Prueba una calidad de envío un poco más baja".to_string(),
        why: format!(
            "{when} tu internet no siempre alcanzó y OBS bajó la calidad sola {} para no perder cuadros. En esos ratos tu público vio la imagen más borrosa.",
            times(stream.bitrate_reductions)
        ),
        steps,
    }
}

fn reconnects(count: u32, when: &str) -> Advice {
    Advice {
        id: "stream.network.reconnects",
        level: Level::Recommended,
        area: Area::LastStream,
        title: "Revisa tu conexión antes del próximo directo".to_string(),
        why: format!(
            "{when} OBS perdió la conexión con la plataforma {} y tuvo que volver a conectarse. Mientras tanto tu público vio el directo cortado.",
            times(count)
        ),
        steps: vec![
            "Antes de salir en directo, reinicia el router y espera a que vuelva internet.".to_string(),
            "Si puedes, conecta la PC al router con un cable.".to_string(),
            "Mientras estés en directo, pausa descargas y actualizaciones en tu casa.".to_string(),
        ],
    }
}

fn connection_lost(when: &str) -> Advice {
    Advice {
        id: "stream.network.connection-lost",
        level: Level::Recommended,
        area: Area::LastStream,
        title: "Deja que OBS se reconecte solo si se corta internet".to_string(),
        why: format!(
            "{when} se cortó la conexión con la plataforma y OBS no volvió a conectarse, así que el directo terminó antes de tiempo."
        ),
        steps: vec![
            format!("En OBS abre {} > {}.", menu::SETTINGS, menu::ADVANCED),
            format!("Marca \"{}\".", menu::RECONNECT),
            "Antes del próximo directo, reinicia el router y, si puedes, conecta la PC con un cable.".to_string(),
        ],
    }
}

fn skipped_frames(stream: &StreamStats, logs: &LogFacts, pct: f32, when: &str) -> Advice {
    let kind = stream.encoder_kind();
    let who = match kind {
        Some(EncoderKind::Software) => "el procesador",
        Some(EncoderKind::Nvidia) => "tu placa NVIDIA",
        Some(EncoderKind::Amd) => "tu placa AMD",
        Some(EncoderKind::Intel) => "tu gráfica Intel",
        _ => "tu PC",
    };
    let mut why = format!(
        "{when} se saltó el {} de los cuadros porque {who}, que es quien prepara el video, no alcanzó a hacerlo a tiempo. Tu público lo vio entrecortado.",
        text::pct(pct)
    );
    if stream.shared_encoding {
        why.push_str(" Mientras tanto también estabas grabando, y eso le sumó trabajo.");
    }

    let mut steps = Vec::new();
    // OBS itself listed a hardware encoder as working on this PC.
    let hardware = [(EncoderKind::Nvidia, "NVIDIA"), (EncoderKind::Intel, "QuickSync"), (EncoderKind::Amd, "AMD")]
        .into_iter()
        .find(|(k, _)| logs.can_encode_with(*k) == Some(true));
    if kind == Some(EncoderKind::Software)
        && let Some((_, brand)) = hardware
    {
        steps.push(menu::open(menu::OUTPUT));
        steps.push(format!(
            "En \"{}\" (quién prepara el video) elige la opción que dice {brand} en lugar de x264.",
            menu::ENCODER
        ));
    } else {
        steps.push(menu::open(menu::VIDEO));
        let big = stream.picture.is_some_and(|(_, h)| h > 720);
        if big || stream.picture.is_none() {
            steps.push(format!("En \"{}\" elige 1280x720.", menu::OUTPUT_RES));
        }
        if stream.fps.is_some_and(|fps| fps > 30.5) {
            steps.push(format!("En \"{}\" elige 30.", menu::FPS));
        }
    }
    if stream.shared_encoding {
        steps.push("Si no necesitas el archivo, no grabes mientras transmites.".to_string());
    }
    steps.push(TRY_AGAIN.to_string());

    Advice {
        id: "stream.encoder.skipped-frames",
        level: loss_level(pct),
        area: Area::LastStream,
        title: "Dale menos trabajo a quien prepara el video".to_string(),
        why,
        steps,
    }
}

fn lagged_frames(logs: &LogFacts, pct: f32, when: &str) -> Advice {
    let mut steps = vec![
        "En las opciones de video del juego, limita los cuadros por segundo (por ejemplo a 60) o activa la sincronización vertical.".to_string(),
    ];
    if logs.startup.as_ref().and_then(|s| s.running_as_admin) == Some(false) {
        steps.push(
            "Cierra OBS, haz clic derecho en su ícono y elige \"Ejecutar como administrador\": así Windows le da prioridad a OBS en la placa de video."
                .to_string(),
        );
    }
    steps.push(
        "Quita de tus escenas las fuentes que no uses: cada una le pide trabajo a la placa de video.".to_string(),
    );
    steps.push(TRY_AGAIN.to_string());
    Advice {
        id: "stream.render.lagged-frames",
        level: loss_level(pct),
        area: Area::LastStream,
        title: "Deja aire a tu placa de video mientras transmites".to_string(),
        why: format!(
            "{when} OBS no alcanzó a dibujar a tiempo el {} de los cuadros porque la placa de video estaba ocupada al máximo, casi siempre por el juego. Tu público lo vio a tirones.",
            text::pct(pct)
        ),
        steps,
    }
}

fn graphics_reset(count: u32, logs: &LogFacts, when: &str) -> Advice {
    let card = logs.startup.as_ref().and_then(|s| s.render_card()).map(|a| a.name.to_lowercase());
    let site = match card.as_deref() {
        Some(name) if name.contains("nvidia") => "nvidia.com",
        Some(name) if name.contains("amd") || name.contains("radeon") => "amd.com",
        Some(name) if name.contains("intel") => "intel.com",
        _ => "la página del fabricante de tu placa",
    };
    Advice {
        id: "stream.gpu.reset",
        level: Level::Recommended,
        area: Area::LastStream,
        title: "Actualiza el controlador de tu placa de video".to_string(),
        why: format!(
            "{when} Windows reinició la placa de video {} en pleno directo. En ese momento la imagen se congela o se pone negra.",
            times(count)
        ),
        steps: vec![
            format!("Descarga el controlador más nuevo desde {site}."),
            "Instálalo y reinicia la PC.".to_string(),
            TRY_AGAIN.to_string(),
        ],
    }
}

fn crash(when: &str) -> Advice {
    Advice {
        id: "stream.ended.crash",
        level: Level::Recommended,
        area: Area::LastStream,
        title: "Actualiza OBS y el controlador de tu placa de video".to_string(),
        why: format!(
            "{when} el registro de OBS se corta en pleno directo: OBS o la PC se cerraron de golpe. Tener OBS y el controlador de video al día es lo primero que conviene probar."
        ),
        steps: vec![
            "Descarga la última versión de OBS desde obsproject.com e instálala encima.".to_string(),
            "Descarga el controlador más nuevo de tu placa de video desde la página del fabricante.".to_string(),
            TRY_AGAIN.to_string(),
        ],
    }
}

fn encoder_error(when: &str) -> Advice {
    Advice {
        id: "stream.ended.encoder-error",
        level: Level::Recommended,
        area: Area::LastStream,
        title: "Prueba otro codificador de video".to_string(),
        why: format!(
            "{when} el directo se cortó porque falló el codificador, que es quien prepara el video. Suele pasar con un controlador de video viejo."
        ),
        steps: vec![
            "Descarga el controlador más nuevo de tu placa de video desde la página del fabricante.".to_string(),
            format!("Si vuelve a pasar: {}", menu::open(menu::OUTPUT)),
            format!("En \"{}\" (quién prepara el video) elige otra opción y prueba.", menu::ENCODER),
            TRY_AGAIN.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::logs::Startup;

    fn facts_with(stream: StreamStats) -> Facts {
        Facts { logs: LogFacts { last_stream: Some(stream), logs_read: 1, ..LogFacts::default() }, ..Facts::default() }
    }

    fn clean() -> StreamStats {
        StreamStats {
            date: Some("17/11/2026".to_string()),
            duration_secs: Some(3600),
            dropped_network_pct: Some(0.0),
            skipped_encoding_pct: Some(0.0),
            lagged_render_pct: Some(0.0),
            encoder_id: Some("obs_x264".to_string()),
            ..StreamStats::default()
        }
    }

    fn review_of(facts: &Facts) -> Review {
        let mut review = Review::default();
        advise(facts, &mut review);
        review
    }

    #[test]
    fn a_clean_stream_is_praised() {
        let review = review_of(&facts_with(clean()));
        assert!(review.advice.is_empty(), "{:?}", review.ids());
        assert_eq!(review.good.len(), 1);
    }

    #[test]
    fn no_logs_is_a_note_unless_obs_is_missing() {
        // OBS is there (no profile yet) but left no logs.
        let facts = Facts { obs: ObsStatus::NoProfile { dir: Default::default() }, ..Facts::default() };
        let review = review_of(&facts);
        assert!(review.advice.is_empty());
        assert_eq!(review.notes.len(), 1);
        // The default is "OBS not found": nothing to add.
        assert!(review_of(&Facts::default()).notes.is_empty());
    }

    #[test]
    fn no_stream_yet_says_nothing() {
        let facts = Facts { logs: LogFacts { logs_read: 2, ..LogFacts::default() }, ..Facts::default() };
        let review = review_of(&facts);
        assert!(review.advice.is_empty() && review.good.is_empty() && review.notes.is_empty());
    }

    #[test]
    fn network_losses_by_size() {
        let small = review_of(&facts_with(StreamStats { dropped_network_pct: Some(0.9), ..clean() }));
        assert!(small.advice.is_empty());
        let some = review_of(&facts_with(StreamStats { dropped_network_pct: Some(3.0), ..clean() }));
        assert_eq!(some.get("stream.network.dropped-frames").map(|a| a.level), Some(Level::Recommended));
        assert!(some.good.is_empty());
        let lots = review_of(&facts_with(StreamStats {
            dropped_network_pct: Some(8.2),
            bitrate_kbps: Some(6000),
            link: Some(Link::Wifi),
            ..clean()
        }));
        let advice = lots.get("stream.network.dropped-frames").expect("advice");
        assert_eq!(advice.level, Level::Important);
        assert!(advice.steps.iter().any(|s| s.contains("4800")), "{:?}", advice.steps);
    }

    #[test]
    fn dynamic_bitrate_and_reconnects() {
        let review = review_of(&facts_with(StreamStats {
            dynamic_bitrate: true,
            bitrate_reductions: 3,
            disconnects: 2,
            ..clean()
        }));
        assert_eq!(review.ids(), vec!["stream.network.bitrate-lowered", "stream.network.reconnects"]);
        assert_eq!(review.get("stream.network.bitrate-lowered").map(|a| a.level), Some(Level::Tip));
    }

    #[test]
    fn a_lost_connection_is_one_item_not_two() {
        let review = review_of(&facts_with(StreamStats { end: StreamEnd::Disconnected, disconnects: 1, ..clean() }));
        assert_eq!(review.ids(), vec!["stream.network.connection-lost"]);
        assert!(review.good.is_empty());
    }

    #[test]
    fn encoder_overload_points_to_a_working_hardware_encoder() {
        let mut facts = facts_with(StreamStats { skipped_encoding_pct: Some(12.4), ..clean() });
        facts.logs.available_encoders = Some(vec!["obs_x264".to_string(), "obs_qsv11_v2".to_string()]);
        let review = review_of(&facts);
        let advice = review.get("stream.encoder.skipped-frames").expect("advice");
        assert_eq!(advice.level, Level::Important);
        assert!(advice.steps.iter().any(|s| s.contains("QuickSync")), "{:?}", advice.steps);

        // Already on hardware: lighten the picture instead.
        let mut facts = facts_with(StreamStats {
            skipped_encoding_pct: Some(2.0),
            encoder_id: Some("obs_nvenc_h264_tex".to_string()),
            picture: Some((1920, 1080)),
            fps: Some(60.0),
            ..clean()
        });
        facts.logs.available_encoders = Some(vec!["obs_nvenc_h264_tex".to_string()]);
        let review = review_of(&facts);
        let advice = review.get("stream.encoder.skipped-frames").expect("advice");
        assert_eq!(advice.level, Level::Recommended);
        assert!(advice.steps.iter().any(|s| s.contains("1280x720")));
        assert!(advice.steps.iter().any(|s| s.contains(menu::FPS)));
    }

    #[test]
    fn render_lag_mentions_admin_only_when_it_applies() {
        let mut facts = facts_with(StreamStats { lagged_render_pct: Some(6.7), ..clean() });
        facts.logs.startup = Some(Startup { running_as_admin: Some(false), ..Startup::default() });
        let advice = review_of(&facts).get("stream.render.lagged-frames").cloned().expect("advice");
        assert_eq!(advice.level, Level::Important);
        let with_admin = advice.steps.len();
        facts.logs.startup = Some(Startup { running_as_admin: Some(true), ..Startup::default() });
        let advice = review_of(&facts).get("stream.render.lagged-frames").cloned().expect("advice");
        assert_eq!(advice.steps.len(), with_admin - 1);
    }

    #[test]
    fn how_it_ended() {
        let crash = review_of(&facts_with(StreamStats {
            end: StreamEnd::Abrupt,
            dropped_network_pct: None,
            skipped_encoding_pct: None,
            lagged_render_pct: None,
            ..clean()
        }));
        assert_eq!(crash.ids(), vec!["stream.ended.crash"]);
        assert!(crash.good.is_empty());

        let encoder = review_of(&facts_with(StreamStats { end: StreamEnd::EncoderError, ..clean() }));
        assert_eq!(encoder.ids(), vec!["stream.ended.encoder-error"]);

        let live = review_of(&facts_with(StreamStats { end: StreamEnd::Unfinished, ..clean() }));
        assert!(live.advice.is_empty() && live.good.is_empty());
        assert_eq!(live.notes.len(), 1);
    }

    #[test]
    fn a_very_short_stream_is_not_judged() {
        let review =
            review_of(&facts_with(StreamStats { duration_secs: Some(10), dropped_network_pct: Some(40.0), ..clean() }));
        assert!(review.advice.is_empty() && review.good.is_empty());
        assert_eq!(review.notes.len(), 1);
    }

    #[test]
    fn graphics_resets() {
        let review = review_of(&facts_with(StreamStats { graphics_resets: 2, ..clean() }));
        assert_eq!(review.ids(), vec!["stream.gpu.reset"]);
    }

    #[test]
    fn bitrate_steps_down_a_fifth() {
        assert_eq!(lower_bitrate(6000), 4800);
        assert_eq!(lower_bitrate(2500), 2000);
        assert_eq!(lower_bitrate(100), 500);
    }
}
