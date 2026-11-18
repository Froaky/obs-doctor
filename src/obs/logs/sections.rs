//! The "Tu último directo" block of the report.

use super::{Link, NOTICEABLE_LOSS_PCT, SERIOUS_LOSS_PCT, StreamEnd, StreamStats};
use crate::facts::Facts;
use crate::model::{Area, Row, Section, Tone};
use crate::obs::config::EncoderKind;
use crate::report::text;

pub fn sections(facts: &Facts) -> Vec<Section> {
    let logs = &facts.logs;
    let mut section = Section::new("last-stream", Area::LastStream.label());
    if logs.logs_read == 0 {
        return vec![section.intro("No encontré registros de OBS en esta PC.")];
    }
    let Some(stream) = &logs.last_stream else {
        section = section.intro(format!(
            "Todavía no hay ningún directo en los registros de OBS que revisé ({}).",
            text::plural(logs.logs_read, "sesión", "sesiones")
        ));
        if let Some(recording) = &logs.last_recording {
            let when = recording.date.clone().unwrap_or_else(|| "sin fecha".to_string());
            section.push(Row::new("Última grabación", when));
        }
        return vec![section];
    };
    stream_rows(&mut section, stream);
    vec![section]
}

fn stream_rows(section: &mut Section, stream: &StreamStats) {
    let when = match (&stream.date, &stream.started_at) {
        (Some(date), Some(time)) => Some(format!("{date}, {time}")),
        (Some(date), None) => Some(date.clone()),
        (None, Some(time)) => Some(time.clone()),
        (None, None) => None,
    };
    if let Some(when) = when {
        section.push(Row::new("Cuándo", when));
    }
    if let Some(secs) = stream.duration_secs {
        let prefix =
            if stream.end == StreamEnd::Abrupt || stream.end == StreamEnd::Unfinished { "al menos " } else { "" };
        section.push(Row::new("Duración", format!("{prefix}{}", duration(secs))));
    }
    if let Some(kind) = stream.encoder_kind() {
        let who = match kind {
            EncoderKind::Software => "el procesador",
            EncoderKind::Nvidia => "tu placa NVIDIA",
            EncoderKind::Amd => "tu placa AMD",
            EncoderKind::Intel => "tu gráfica Intel",
            EncoderKind::Other => "otro codificador",
        };
        section.push(Row::new("Quién preparó el video", who));
    }
    let mut picture = Vec::new();
    if let Some((w, h)) = stream.picture {
        picture.push(format!("{w}x{h}"));
    }
    if let Some(fps) = stream.fps {
        picture.push(format!("{} cuadros por segundo", text::fps(fps)));
    }
    if let Some(kbps) = stream.bitrate_kbps {
        picture.push(format!("{kbps} kbps"));
    }
    if !picture.is_empty() {
        section.push(Row::new("Imagen enviada", picture.join(", ")));
    }
    if let Some(link) = stream.link {
        let value = match link {
            Link::Cable => "por cable",
            Link::Wifi => "por wifi",
            Link::Other => "otra conexión",
        };
        section.push(Row::new("Salió", value));
    }

    loss_row(section, "Perdidos por internet", stream.dropped_network_pct);
    let skipped_note = stream.shared_encoding.then_some("Grababas a la vez: este número cuenta las dos cosas.");
    if let Some(row) = loss(stream.skipped_encoding_pct, "Saltados al preparar") {
        section.push(match skipped_note {
            Some(note) => row.note(note),
            None => row,
        });
    }
    loss_row(section, "Atrasados al dibujar", stream.lagged_render_pct);

    if stream.disconnects > 0 {
        section.push(Row::new("Cortes de conexión", stream.disconnects.to_string()).tone(if stream.disconnects > 1 {
            Tone::Bad
        } else {
            Tone::Warn
        }));
    }
    if stream.dynamic_bitrate && stream.bitrate_reductions > 0 {
        section.push(Row::new("Bajadas de calidad", stream.bitrate_reductions.to_string()).tone(Tone::Warn));
    }
    let (end, tone) = match stream.end {
        StreamEnd::Stopped => ("lo cerraste tú", Tone::Good),
        StreamEnd::Disconnected => ("se cortó la conexión", Tone::Bad),
        StreamEnd::EncoderError => ("falló quien prepara el video", Tone::Bad),
        StreamEnd::Abrupt => ("OBS se cerró de golpe", Tone::Bad),
        StreamEnd::Unfinished => ("sin cierre (¿sigue en directo?)", Tone::Neutral),
    };
    section.push(Row::new("Cómo terminó", end).tone(tone));
}

fn loss_row(section: &mut Section, label: &str, pct: Option<f32>) {
    if let Some(row) = loss(pct, label) {
        section.push(row);
    }
}

/// Same colours as OBS's own statistics panel.
fn loss(pct: Option<f32>, label: &str) -> Option<Row> {
    let pct = pct?;
    let tone = if pct > SERIOUS_LOSS_PCT {
        Tone::Bad
    } else if pct > NOTICEABLE_LOSS_PCT {
        Tone::Warn
    } else {
        Tone::Good
    };
    Some(Row::new(label, text::pct(pct)).tone(tone))
}

/// "2 h 05 min", "45 min", "20 s".
fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs / 60 % 60);
    match (h, m) {
        (0, 0) => format!("{secs} s"),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::logs::{LogFacts, RecordingStats};

    fn facts(logs: LogFacts) -> Facts {
        Facts { logs, ..Facts::default() }
    }

    #[test]
    fn no_logs_and_no_stream_are_one_line() {
        let none = sections(&facts(LogFacts::default()));
        assert_eq!(none.len(), 1);
        assert!(none[0].intro.is_some() && none[0].rows.is_empty());

        let recording = RecordingStats { date: Some("16/11/2026".to_string()), ..RecordingStats::default() };
        let no_stream =
            sections(&facts(LogFacts { logs_read: 3, last_recording: Some(recording), ..LogFacts::default() }));
        assert!(no_stream[0].intro.is_some());
        assert_eq!(no_stream[0].rows.len(), 1);
    }

    #[test]
    fn a_stream_shows_its_numbers_with_obs_colours() {
        let stream = StreamStats {
            date: Some("17/11/2026".to_string()),
            started_at: Some("21:00".to_string()),
            duration_secs: Some(7205),
            dropped_network_pct: Some(8.2),
            skipped_encoding_pct: Some(2.0),
            lagged_render_pct: Some(0.0),
            disconnects: 1,
            shared_encoding: true,
            ..StreamStats::default()
        };
        let section =
            sections(&facts(LogFacts { logs_read: 1, last_stream: Some(stream), ..LogFacts::default() })).remove(0);
        assert_eq!(section.id, "last-stream");
        let tones: Vec<Tone> = section.rows.iter().map(|r| r.tone).collect();
        // Cuándo, Duración, three losses, cortes, cómo terminó.
        assert_eq!(
            tones,
            vec![Tone::Neutral, Tone::Neutral, Tone::Bad, Tone::Warn, Tone::Good, Tone::Warn, Tone::Good]
        );
        assert!(section.rows[3].note.is_some());
    }

    #[test]
    fn durations() {
        assert_eq!(duration(7205), "2 h 00 min");
        assert_eq!(duration(2700), "45 min");
        assert_eq!(duration(20), "20 s");
    }
}
