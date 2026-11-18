//! Cross-area conclusions: when two facts from different areas explain each
//! other (frames lost to the connection AND the PC is on Wi-Fi), replace the
//! generic tips with one sharper piece of advice.
//!
//! Runs after every area has spoken, over the advice they produced. It only
//! merges or sharpens: it never adds a conclusion the areas' own evidence
//! does not already support.
//!
//! Left out on purpose:
//! - render lag + heavy scenes or an integrated card: `scenes.*` already ties
//!   its advice to `render_lag` and `integrated` (see `Pressure` in scenes).
//! - render lag + HAGS: `win.hags.try-off` already requires that evidence.

use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review};
use crate::net::{self, Band, Connection, Signal};
use crate::obs::config::EncoderKind;
use crate::obs::logs::{Link, StreamStats, lower_bitrate};
use crate::obs::menu;
use crate::report::text;

/// The one item that replaces "you lost frames", "you reconnected" and
/// "use a cable" when the last stream went out over Wi-Fi.
pub const WIFI_STREAM_LOSSES: &str = "diagnosis.wifi.stream-losses";

const DROPPED: &str = "stream.network.dropped-frames";
const RECONNECTS: &str = "stream.network.reconnects";
const BITRATE_LOWERED: &str = "stream.network.bitrate-lowered";
const USE_CABLE: &str = "net.wifi.use-cable";
const SKIPPED: &str = "stream.encoder.skipped-frames";

pub fn refine(facts: &Facts, review: &mut Review) {
    let Some(stream) = facts.logs.last_stream.as_ref() else {
        return;
    };
    wifi_stream_losses(facts, stream, review);
    encoder_cause(stream, review);
}

/// Did the last stream go out over Wi-Fi, as far as we can tell? The log's
/// own record wins. Without it, the connection right now stands in, which is
/// the usual case: a PC rarely switches between cable and Wi-Fi. If the log
/// says Wi-Fi but the PC is on a cable now, the streamer already fixed it.
fn streamed_over_wifi(facts: &Facts, stream: &StreamStats) -> bool {
    match stream.link {
        Some(Link::Wifi) => facts.net.connection != Connection::Cable,
        Some(_) => false,
        None => facts.net.connection == Connection::Wifi,
    }
}

/// Frames lost to the connection or reconnections, over Wi-Fi: one item
/// that names the cause, instead of two generic ones plus the Wi-Fi tip.
fn wifi_stream_losses(facts: &Facts, stream: &StreamStats, review: &mut Review) {
    let dropped = review.has(DROPPED);
    let reconnected = review.has(RECONNECTS);
    if !(dropped || reconnected) || !streamed_over_wifi(facts, stream) {
        return;
    }

    let replaced: Vec<Advice> =
        [DROPPED, RECONNECTS, BITRATE_LOWERED, USE_CABLE].into_iter().filter_map(|id| review.remove(id)).collect();
    // Evidence from a real stream: at least Recommended, Important when the
    // loss itself already was.
    let level = replaced.iter().map(|a| a.level).min().unwrap_or(Level::Recommended).min(Level::Recommended);

    // Signal and band describe the connection now, so they only count when
    // the PC is on Wi-Fi now.
    let wifi_now = facts.net.connection == Connection::Wifi;
    let signal = facts.net.wifi_signal.filter(|_| wifi_now).map(net::signal_class);
    let on_2_4 = wifi_now && facts.net.wifi_band == Some(Band::Ghz2_4);
    let weak = matches!(signal, Some(Signal::Fair | Signal::Weak));

    let when = match &stream.date {
        Some(date) => format!("En tu último directo, el {date},"),
        None => "En tu último directo".to_string(),
    };
    let pct = stream.dropped_network_pct.filter(|_| dropped);
    let what = match (pct, reconnected) {
        (Some(pct), true) => format!(
            "{when} se perdió el {} de los cuadros y OBS tuvo que volver a conectarse {}.",
            text::pct(pct),
            times(stream.disconnects)
        ),
        (Some(pct), false) => format!("{when} se perdió el {} de los cuadros.", text::pct(pct)),
        _ => format!("{when} OBS perdió la conexión {} y tuvo que volver a conectarse.", times(stream.disconnects)),
    };
    let link =
        if stream.link == Some(Link::Wifi) { "Ese directo salió por wifi" } else { "Tu PC está conectada por wifi" };
    let detail = match signal {
        Some(Signal::Weak) => ", con señal débil",
        Some(Signal::Fair) => ", con señal regular",
        _ if on_2_4 => ", en la banda de 2,4 GHz",
        _ => "",
    };
    let why = format!(
        "{what} {link}{detail}. El wifi tiene bajones de un segundo que no notas al navegar, pero en directo se ven como saltos o cortes. Con cable eso no pasa."
    );

    let mut steps = vec![
        "Conecta la PC al router con un cable de red. Si el router está lejos, un adaptador de red por el enchufe eléctrico suele funcionar mejor que el wifi.".to_string(),
    ];
    if on_2_4 {
        steps.push(
            "Si por ahora no puedes usar cable, conéctate a la red de 5 GHz de tu router. Suele tener el mismo nombre con \"5G\" al final.".to_string(),
        );
    }
    if weak {
        steps.push(
            "Si sigues en wifi, acerca la PC al router o quita lo que haya entre ellos (paredes, muebles de metal)."
                .to_string(),
        );
    }
    if dropped && let Some(kbps) = stream.bitrate_kbps {
        steps.push(format!(
            "Mientras sigas en wifi, {} En \"{}\" (la calidad de envío) baja de {kbps} a {} kbps.",
            lowercase_first(&menu::open(menu::OUTPUT)),
            menu::BITRATE,
            lower_bitrate(kbps)
        ));
    }
    steps.push(
        "Mientras estés en directo, pausa descargas, actualizaciones y videos en otros aparatos de tu casa."
            .to_string(),
    );
    steps.push("Haz un directo corto de prueba y vuelve a correr OBS Doctor para ver el cambio.".to_string());

    review.add(Advice {
        id: WIFI_STREAM_LOSSES,
        level,
        area: Area::LastStream,
        title: "Conecta la PC al router con un cable antes del próximo directo".to_string(),
        why,
        steps,
    });
}

/// Frames skipped by the encoder, plus a setting of that same encoder that
/// explains it: the setting is the fix, so it keeps the evidence and the
/// level, and the generic "give it less work" goes away.
fn encoder_cause(stream: &StreamStats, review: &mut Review) {
    let Some(skipped) = review.get(SKIPPED).map(|a| a.level) else {
        return;
    };
    let Some(kind) = stream.encoder_kind() else {
        return;
    };
    // Each setting only explains frames skipped by its own kind of encoder.
    let cause = if kind == EncoderKind::Software {
        ["obs.encoder.use-hardware", "obs.preset.too-slow"].into_iter().find(|id| review.has(id))
    } else if kind.is_hardware() {
        Some("obs.encoder.expensive-options").filter(|id| review.has(id))
    } else {
        None
    };
    let Some(cause) = cause else {
        return;
    };
    let pct = stream.skipped_encoding_pct;
    review.remove(SKIPPED);
    let Some(advice) = review.advice.iter_mut().find(|a| a.id == cause) else {
        return;
    };
    advice.level = advice.level.min(skipped);
    // The other two already quote the last stream when they have it.
    if cause == "obs.encoder.use-hardware"
        && let Some(pct) = pct
    {
        advice.why.push_str(&format!(" En tu último directo se saltó el {} de los cuadros por eso.", text::pct(pct)));
    }
    if stream.shared_encoding {
        let at = advice.steps.len().saturating_sub(1);
        advice.steps.insert(at, "Si no necesitas el archivo, no grabes mientras transmites.".to_string());
    }
}

fn times(count: u32) -> String {
    if count == 1 { "una vez".to_string() } else { format!("{count} veces") }
}

fn lowercase_first(sentence: &str) -> String {
    let mut chars = sentence.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Area;
    use crate::net::NetFacts;
    use crate::obs::logs::LogFacts;

    fn advice(id: &'static str, level: Level) -> Advice {
        Advice {
            id,
            level,
            area: Area::LastStream,
            title: String::new(),
            why: String::new(),
            steps: vec!["a".into(), "b".into()],
        }
    }

    fn stream() -> StreamStats {
        StreamStats {
            date: Some("17/11/2026".to_string()),
            duration_secs: Some(3600),
            dropped_network_pct: Some(0.0),
            skipped_encoding_pct: Some(0.0),
            lagged_render_pct: Some(0.0),
            encoder_id: Some("obs_x264".to_string()),
            bitrate_kbps: Some(6000),
            ..StreamStats::default()
        }
    }

    fn facts(stream: StreamStats, net: NetFacts) -> Facts {
        Facts {
            logs: LogFacts { last_stream: Some(stream), logs_read: 1, ..LogFacts::default() },
            net,
            ..Facts::default()
        }
    }

    fn wifi(signal: Option<u32>, band: Option<Band>) -> NetFacts {
        NetFacts { connection: Connection::Wifi, wifi_signal: signal, wifi_band: band, ..NetFacts::default() }
    }

    fn cable() -> NetFacts {
        NetFacts { connection: Connection::Cable, link_mbps: Some(1000.0), ..NetFacts::default() }
    }

    /// The real pipeline for the areas involved, then the diagnosis.
    fn reviewed(facts: &Facts) -> Review {
        let mut review = Review::default();
        crate::obs::logs::advise(facts, &mut review);
        crate::net::advise(facts, &mut review);
        refine(facts, &mut review);
        review
    }

    #[test]
    fn dropped_frames_over_wifi_become_one_item() {
        let f = facts(StreamStats { dropped_network_pct: Some(3.0), ..stream() }, wifi(Some(90), Some(Band::Ghz5)));
        let review = reviewed(&f);
        assert_eq!(review.ids(), vec![WIFI_STREAM_LOSSES]);
        assert_eq!(review.get(WIFI_STREAM_LOSSES).map(|a| a.level), Some(Level::Recommended));
    }

    #[test]
    fn serious_loss_keeps_its_level() {
        let f = facts(StreamStats { dropped_network_pct: Some(8.2), ..stream() }, wifi(Some(90), Some(Band::Ghz5)));
        assert_eq!(reviewed(&f).get(WIFI_STREAM_LOSSES).map(|a| a.level), Some(Level::Important));
    }

    #[test]
    fn reconnects_over_wifi_replace_the_generic_items() {
        let f = facts(
            StreamStats { disconnects: 2, dynamic_bitrate: true, bitrate_reductions: 3, ..stream() },
            wifi(Some(30), Some(Band::Ghz2_4)),
        );
        let review = reviewed(&f);
        assert_eq!(review.ids(), vec![WIFI_STREAM_LOSSES]);
        let merged = review.get(WIFI_STREAM_LOSSES).expect("merged");
        assert_eq!(merged.level, Level::Recommended);
        // Cable, 5 GHz, move closer, downloads, try again. No bitrate step:
        // no frames were lost.
        assert_eq!(merged.steps.len(), 5);
    }

    #[test]
    fn the_log_link_counts_when_the_connection_is_unknown() {
        let f = facts(
            StreamStats { dropped_network_pct: Some(3.0), link: Some(Link::Wifi), ..stream() },
            NetFacts::default(),
        );
        assert!(reviewed(&f).has(WIFI_STREAM_LOSSES));
    }

    #[test]
    fn a_stream_over_cable_keeps_the_generic_items() {
        let f = facts(
            StreamStats { dropped_network_pct: Some(3.0), link: Some(Link::Cable), ..stream() },
            wifi(Some(90), Some(Band::Ghz5)),
        );
        let review = reviewed(&f);
        assert!(!review.has(WIFI_STREAM_LOSSES));
        assert!(review.has(DROPPED));
        assert!(review.has(USE_CABLE));
    }

    #[test]
    fn already_on_cable_now_is_left_alone() {
        let f = facts(StreamStats { dropped_network_pct: Some(3.0), link: Some(Link::Wifi), ..stream() }, cable());
        let review = reviewed(&f);
        assert!(!review.has(WIFI_STREAM_LOSSES));
        assert!(review.has(DROPPED));
    }

    #[test]
    fn wifi_without_losses_keeps_the_plain_tip() {
        let f = facts(stream(), wifi(Some(90), Some(Band::Ghz5)));
        let review = reviewed(&f);
        assert_eq!(review.ids(), vec![USE_CABLE]);
        assert_eq!(review.get(USE_CABLE).map(|a| a.level), Some(Level::Tip));
    }

    #[test]
    fn a_lowered_bitrate_alone_is_not_enough() {
        let f = facts(
            StreamStats { dynamic_bitrate: true, bitrate_reductions: 2, ..stream() },
            wifi(Some(90), Some(Band::Ghz5)),
        );
        let review = reviewed(&f);
        assert!(!review.has(WIFI_STREAM_LOSSES));
        assert!(review.has(BITRATE_LOWERED));
    }

    #[test]
    fn no_stream_changes_nothing() {
        let f = Facts { net: wifi(Some(90), None), ..Facts::default() };
        let mut review = Review::default();
        review.add(advice(USE_CABLE, Level::Tip));
        refine(&f, &mut review);
        assert_eq!(review.ids(), vec![USE_CABLE]);
    }

    #[test]
    fn hardware_switch_absorbs_skipped_frames_from_x264() {
        let f = facts(StreamStats { skipped_encoding_pct: Some(9.0), shared_encoding: true, ..stream() }, cable());
        let mut review = Review::default();
        review.add(advice(SKIPPED, Level::Important));
        review.add(advice("obs.encoder.use-hardware", Level::Recommended));
        refine(&f, &mut review);
        assert_eq!(review.ids(), vec!["obs.encoder.use-hardware"]);
        let merged = review.get("obs.encoder.use-hardware").expect("kept");
        assert_eq!(merged.level, Level::Important);
        assert_eq!(merged.steps.len(), 3);
    }

    #[test]
    fn slow_preset_absorbs_skipped_frames_but_keeps_the_worse_level() {
        let f = facts(StreamStats { skipped_encoding_pct: Some(2.0), ..stream() }, cable());
        let mut review = Review::default();
        review.add(advice(SKIPPED, Level::Recommended));
        review.add(advice("obs.preset.too-slow", Level::Important));
        refine(&f, &mut review);
        assert_eq!(review.ids(), vec!["obs.preset.too-slow"]);
        assert_eq!(review.get("obs.preset.too-slow").map(|a| a.level), Some(Level::Important));
    }

    #[test]
    fn expensive_options_only_explain_a_hardware_encoder() {
        let mut review = Review::default();
        review.add(advice(SKIPPED, Level::Recommended));
        review.add(advice("obs.encoder.expensive-options", Level::Tip));
        let x264 = facts(StreamStats { skipped_encoding_pct: Some(2.0), ..stream() }, cable());
        refine(&x264, &mut review);
        assert!(review.has(SKIPPED));

        let nvenc = facts(
            StreamStats {
                skipped_encoding_pct: Some(2.0),
                encoder_id: Some("obs_nvenc_h264_tex".to_string()),
                ..stream()
            },
            cable(),
        );
        refine(&nvenc, &mut review);
        assert_eq!(review.ids(), vec!["obs.encoder.expensive-options"]);
        assert_eq!(review.get("obs.encoder.expensive-options").map(|a| a.level), Some(Level::Recommended));
    }

    #[test]
    fn skipped_frames_without_a_known_cause_stay() {
        let f = facts(StreamStats { skipped_encoding_pct: Some(2.0), ..stream() }, cable());
        let mut review = Review::default();
        review.add(advice(SKIPPED, Level::Recommended));
        review.add(advice("obs.scale-filter.expensive", Level::Tip));
        refine(&f, &mut review);
        assert_eq!(review.ids(), vec![SKIPPED, "obs.scale-filter.expensive"]);
    }
}
