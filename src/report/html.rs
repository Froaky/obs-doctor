//! Draws the report as a single self-contained HTML page.
//!
//! The page must work offline: no external fonts, scripts, images or requests.
//! Styles, the script and the icons live in `assets/` and are inlined as they
//! are. Every piece of text that comes from the PC (program names, profile
//! names, source names) is untrusted and must go through [`escape`]. Dynamic
//! data only ever lands in element content or in plain quoted attributes
//! (`class`, `id`, `data-*`): never inside `<script>`, `<style>`, URLs or
//! event handlers.

use crate::model::{Advice, Level, ProgramCard, ProgramKind, ReportData, Row, Section, Tone, Verdict};
use crate::report::text;

const CSS: &str = include_str!("assets/report.css");
const JS: &str = include_str!("assets/report.js");
const ICONS: &str = include_str!("assets/icons.svg");

/// Segments in a meter, like the level meters in the OBS audio mixer.
const METER_SEGMENTS: f32 = 25.0;

/// Program groups in the order the streamer should look at them.
const PROGRAM_GROUPS: [(ProgramKind, &str); 5] = [
    (ProgramKind::Close, "Conviene cerrarlos antes del directo"),
    (ProgramKind::Review, "Ciérralos si no los usas en el directo"),
    (ProgramKind::Unknown, "Usan mucho y no sé qué son"),
    (ProgramKind::Game, "Parece tu juego"),
    (ProgramKind::Keep, "Herramientas de directo: déjalas abiertas"),
];

pub fn render(data: &ReportData) -> String {
    let mut out = String::with_capacity(CSS.len() + JS.len() + 32 * 1024);
    out.push_str("<!doctype html>\n<html lang=\"es\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta name=\"color-scheme\" content=\"light dark\">\n");
    out.push_str(&format!("<title>OBS Doctor: revisión del {}</title>\n", escape(&data.generated)));
    out.push_str("<style>\n");
    out.push_str(CSS);
    out.push_str("</style>\n</head>\n");
    out.push_str(&format!("<body data-revision=\"{}\">\n", revision(data)));
    out.push_str(ICONS);
    out.push_str("<a class=\"skip\" href=\"#main\">Saltar al reporte</a>\n");

    hero(&mut out, data);
    out.push_str("<main class=\"wrap\" id=\"main\">\n");
    advice_band(&mut out, &data.advice);
    programs_band(&mut out, &data.programs);
    good_band(&mut out, &data.good);
    facts_band(&mut out, &data.sections);
    suggested_band(&mut out, &data.suggested);
    notes_band(&mut out, &data.notes);
    out.push_str("</main>\n");
    footer(&mut out, data);

    out.push_str("<script>\n");
    out.push_str(JS);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

/// Makes any text safe to place inside HTML element content or a quoted attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Ticks saved by the page belong to one check-up. A new run gets a new
/// revision, so a change that is still listed does not show up already ticked.
/// Hex digits only, so it is safe in an attribute by construction.
fn revision(data: &ReportData) -> String {
    // FNV-1a: tiny, stable across builds, and good enough to tell runs apart.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let parts = [data.version, data.generated.as_str()].into_iter().chain(data.advice.iter().map(|a| a.id));
    for part in parts {
        for byte in part.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

fn icon(name: &str) -> String {
    format!("<svg class=\"ic\" aria-hidden=\"true\" focusable=\"false\"><use href=\"#i-{name}\"/></svg>")
}

fn level_class(level: Level) -> &'static str {
    match level {
        Level::Important => "lvl-important",
        Level::Recommended => "lvl-recommended",
        Level::Tip => "lvl-tip",
    }
}

fn level_label(level: Level) -> &'static str {
    match level {
        Level::Important => "Importante",
        Level::Recommended => "Recomendado",
        Level::Tip => "Para probar",
    }
}

/// "1 importante, 2 recomendados y 1 para probar".
fn tally(advice: &[Advice]) -> String {
    let count = |level| advice.iter().filter(|a| a.level == level).count();
    let parts: Vec<String> = [
        (count(Level::Important), "importante", "importantes"),
        (count(Level::Recommended), "recomendado", "recomendados"),
        (count(Level::Tip), "para probar", "para probar"),
    ]
    .into_iter()
    .filter(|(n, _, _)| *n > 0)
    .map(|(n, one, many)| text::plural(n, one, many))
    .collect();
    match parts.split_last() {
        None => String::from("Nada que cambiar"),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} y {last}", rest.join(", ")),
    }
}

fn hero(out: &mut String, data: &ReportData) {
    let (class, lamp) = match data.verdict {
        Verdict::Ready => ("v-ready", "ok"),
        Verdict::AlmostReady => ("v-almost", "adjust"),
        Verdict::NeedsWork => ("v-needs", "bang"),
    };
    let calm = match data.verdict {
        Verdict::Ready => "No encontré nada que cambiar. Más abajo está lo que ya revisé.",
        Verdict::AlmostReady => "Nada grave. Son mejoras que puedes hacer con calma, de arriba hacia abajo.",
        Verdict::NeedsWork => "Empieza por los importantes: son los que más se van a notar en el directo.",
    };
    out.push_str("<header class=\"hero\">\n<div class=\"wrap\">\n<div class=\"mast\">");
    out.push_str("<span class=\"brand\">OBS Doctor</span>");
    out.push_str(&format!("<span>Revisión del {}</span>", escape(&data.generated)));
    out.push_str("</div>\n");
    out.push_str(&format!(
        "<div class=\"verdict {class}\">\n<div class=\"lamp\" aria-hidden=\"true\">{}</div>\n",
        icon(lamp)
    ));
    out.push_str(&format!("<div>\n<h1>{}</h1>\n", escape(data.verdict.headline())));
    if !data.advice.is_empty() {
        out.push_str(&format!("<p class=\"tally\">{}</p>\n", escape(&tally(&data.advice))));
    }
    out.push_str(&format!("<p class=\"calm\">{calm}</p>\n</div>\n</div>\n"));

    // Only the parts this report actually has.
    let mut links: Vec<(&str, &str, Option<usize>)> = Vec::new();
    links.push(("b-advice", "Qué cambiar", Some(data.advice.len())));
    if !data.programs.is_empty() {
        links.push(("b-programs", "Programas", Some(data.programs.len())));
    }
    if !data.good.is_empty() {
        links.push(("b-good", "Lo que está bien", Some(data.good.len())));
    }
    if !data.sections.is_empty() {
        links.push(("b-facts", "Lo que encontré", None));
    }
    if !data.suggested.is_empty() {
        links.push(("b-suggested", "Con qué empezar", None));
    }
    if !data.notes.is_empty() {
        links.push(("b-notes", "Sin revisar", Some(data.notes.len())));
    }
    out.push_str("<nav class=\"toc cols\" aria-label=\"Partes del reporte\">\n<ul>\n");
    for (id, label, count) in links {
        let count = count.map(|n| format!("<span class=\"n\">{n}</span>")).unwrap_or_default();
        out.push_str(&format!("<li><a href=\"#{id}\">{label}{count}</a></li>\n"));
    }
    out.push_str("</ul>\n</nav>\n</div>\n</header>\n");
}

fn band_open(out: &mut String, id: &str, extra_class: &str, title: &str, lede: Option<&str>) {
    out.push_str(&format!(
        "<section class=\"band cols{extra_class}\" id=\"{id}\" aria-labelledby=\"{id}-h\">\n<div class=\"band-head\">\n<h2 id=\"{id}-h\">{}</h2>\n",
        escape(title)
    ));
    if let Some(lede) = lede {
        out.push_str(&format!("<p class=\"lede\">{}</p>\n", escape(lede)));
    }
    out.push_str("</div>\n");
}

fn advice_band(out: &mut String, advice: &[Advice]) {
    if advice.is_empty() {
        band_open(out, "b-advice", "", "Qué cambiar", None);
        out.push_str("<div class=\"band-body\">\n");
        out.push_str(&format!("<p class=\"nothing\">{}<span>No hay nada que cambiar.</span></p>\n", icon("ok")));
        out.push_str("</div>\n</section>\n");
        return;
    }
    band_open(
        out,
        "b-advice",
        " band-advice",
        "Qué cambiar",
        Some("Primero lo que más se nota. Cada cambio lo haces tú, a mano: el doctor solo mira."),
    );

    let total = advice.len();
    let hechos = if total == 1 { "hecho" } else { "hechos" };
    out.push_str("<div class=\"progress\">\n");
    out.push_str(&format!("<p class=\"count\" data-count aria-live=\"polite\">0 de {total} {hechos}</p>\n"));
    out.push_str("<nav class=\"chips\" aria-label=\"Ir a un cambio\">\n");
    for (i, item) in advice.iter().enumerate() {
        let n = i + 1;
        out.push_str(&format!(
            "<a class=\"chip {}\" href=\"#a-{n}\"><span class=\"num\">{n}</span>{}<span class=\"sr\">{}: {}</span></a>\n",
            level_class(item.level),
            icon("ok"),
            level_label(item.level),
            escape(&item.title)
        ));
    }
    out.push_str("</nav>\n</div>\n");

    out.push_str("<div class=\"band-body\">\n");
    for (i, item) in advice.iter().enumerate() {
        advice_card(out, i + 1, item);
    }
    out.push_str(
        "<p class=\"alldone\" data-alldone hidden>Marcaste todo. Vuelve a correr OBS Doctor para confirmar que quedó bien.</p>\n",
    );
    out.push_str(
        "<p class=\"fine\">Marcar «Listo» es una nota para ti y se guarda solo en este navegador. \
         La confirmación real es volver a correr OBS Doctor: si el cambio quedó hecho, ya no aparece.</p>\n",
    );
    out.push_str(
        "<p class=\"fine\" data-nostore hidden>Este navegador no deja guardar las marcas: se borran al cerrar la página.</p>\n",
    );
    out.push_str("</div>\n</section>\n");
}

fn advice_card(out: &mut String, n: usize, item: &Advice) {
    let title = escape(&item.title);
    out.push_str(&format!(
        "<article class=\"advice {}\" id=\"a-{n}\" aria-labelledby=\"a-{n}-t\">\n",
        level_class(item.level)
    ));
    out.push_str(&format!("<span class=\"advice-num\" aria-hidden=\"true\">{n}</span>\n"));
    out.push_str(&format!(
        "<div class=\"advice-meta\"><span class=\"level\">{}</span><span class=\"tag\">{}</span></div>\n",
        level_label(item.level),
        escape(item.area.label())
    ));
    out.push_str(&format!("<h3 class=\"advice-title\" id=\"a-{n}-t\">{title}</h3>\n"));
    out.push_str("<div class=\"advice-body\"><div class=\"advice-inner\">\n");
    if !item.why.is_empty() {
        out.push_str(&format!("<p class=\"why\">{}</p>\n", escape(&item.why)));
    }
    if !item.steps.is_empty() {
        out.push_str("<p class=\"how\">Cómo hacerlo</p>\n<ol class=\"steps\">\n");
        for step in &item.steps {
            out.push_str(&format!("<li><span>{}</span></li>\n", escape(step)));
        }
        out.push_str("</ol>\n");
    }
    out.push_str("</div></div>\n");
    out.push_str(&format!(
        "<label class=\"done\"><input type=\"checkbox\" data-advice=\"{}\"> Listo<span class=\"sr\">: {title}</span></label>\n",
        escape(item.id)
    ));
    out.push_str("</article>\n");
}

fn programs_band(out: &mut String, programs: &[ProgramCard]) {
    if programs.is_empty() {
        return;
    }
    band_open(
        out,
        "b-programs",
        "",
        "Programas abiertos",
        Some("Los que estaban usando tu PC durante la revisión, agrupados por qué conviene hacer con cada uno."),
    );
    out.push_str("<div class=\"band-body\">\n");
    for (kind, heading) in PROGRAM_GROUPS {
        let group: Vec<&ProgramCard> = programs.iter().filter(|p| p.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("<div class=\"group\">\n<h3>{heading}</h3>\n<ul class=\"progs\">\n"));
        for p in group {
            out.push_str("<li><div class=\"prog-line\">");
            out.push_str(&format!("<bdi class=\"prog-name\">{}</bdi>", escape(&p.name)));
            if !p.usage.is_empty() {
                out.push_str(&format!("<span class=\"prog-usage\">{}</span>", escape(&p.usage)));
            }
            out.push_str("</div>");
            if !p.tip.is_empty() {
                out.push_str(&format!("<p class=\"prog-tip\">{}</p>", escape(&p.tip)));
            }
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n</div>\n");
    }
    out.push_str("</div>\n</section>\n");
}

fn good_band(out: &mut String, good: &[String]) {
    if good.is_empty() {
        return;
    }
    band_open(out, "b-good", "", "Lo que ya está bien", None);
    out.push_str("<div class=\"band-body\">\n<ul class=\"good\">\n");
    for sentence in good {
        out.push_str(&format!("<li>{}<span>{}</span></li>\n", icon("ok"), escape(sentence)));
    }
    out.push_str("</ul>\n</div>\n</section>\n");
}

fn facts_band(out: &mut String, sections: &[Section]) {
    if sections.is_empty() {
        return;
    }
    band_open(
        out,
        "b-facts",
        "",
        "Lo que encontré",
        Some("Los datos de tu PC, tu OBS y tu internet en los que se basa este reporte."),
    );
    out.push_str("<div class=\"band-body\">\n");
    for section in sections {
        out.push_str(&format!(
            "<section class=\"facts\" id=\"s-{}\">\n<h3>{}</h3>\n",
            escape(section.id),
            escape(&section.title)
        ));
        section_body(out, section);
        out.push_str("</section>\n");
    }
    out.push_str("</div>\n</section>\n");
}

fn section_body(out: &mut String, section: &Section) {
    if let Some(intro) = &section.intro {
        out.push_str(&format!("<p class=\"intro\">{}</p>\n", escape(intro)));
    }
    if section.rows.is_empty() {
        return;
    }
    out.push_str("<dl>\n");
    for row in &section.rows {
        row_html(out, row);
    }
    out.push_str("</dl>\n");
}

fn row_html(out: &mut String, row: &Row) {
    // The tone is told by an icon and hidden words as well as by colour.
    let (class, mark) = match row.tone {
        Tone::Neutral => ("", String::new()),
        Tone::Good => (" tone-good", format!("{}<span class=\"sr\">Bien: </span>", icon("ok"))),
        Tone::Warn => (" tone-warn", format!("{}<span class=\"sr\">Atención: </span>", icon("warn"))),
        Tone::Bad => (" tone-bad", format!("{}<span class=\"sr\">Problema: </span>", icon("bad"))),
    };
    out.push_str(&format!("<div class=\"row{class}\"><dt>{}</dt><dd>", escape(&row.label)));
    out.push_str(&format!("<span class=\"val\">{mark}{}</span>", escape(&row.value)));
    if let Some(fraction) = row.meter {
        out.push_str(&meter(fraction));
    }
    if let Some(note) = &row.note {
        out.push_str(&format!("<span class=\"note\">{}</span>", escape(note)));
    }
    out.push_str("</dd></div>\n");
}

/// A segmented bar. The fill always ends on a segment edge, like a real meter.
fn meter(fraction: f32) -> String {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let lit = (fraction * METER_SEGMENTS).round();
    let end = lit * (100.0 / METER_SEGMENTS);
    let mut svg = String::from(
        "<svg class=\"meter\" viewBox=\"0 0 100 4\" preserveAspectRatio=\"none\" aria-hidden=\"true\" focusable=\"false\">\
         <line class=\"meter-track\" x1=\"0\" y1=\"2\" x2=\"100\" y2=\"2\"/>",
    );
    if lit > 0.0 {
        svg.push_str(&format!("<line class=\"meter-fill\" x1=\"0\" y1=\"2\" x2=\"{end:.0}\" y2=\"2\"/>"));
    }
    svg.push_str("</svg>");
    svg
}

fn suggested_band(out: &mut String, suggested: &Section) {
    if suggested.is_empty() {
        return;
    }
    let title = if suggested.title.is_empty() { "Con qué empezaría" } else { suggested.title.as_str() };
    band_open(
        out,
        "b-suggested",
        "",
        title,
        Some("Un punto de partida para esta PC. Pruébalo y ajusta según cómo se vea."),
    );
    out.push_str("<div class=\"band-body\">\n<div class=\"ficha\">\n");
    section_body(out, suggested);
    out.push_str("</div>\n</div>\n</section>\n");
}

fn notes_band(out: &mut String, notes: &[String]) {
    if notes.is_empty() {
        return;
    }
    band_open(
        out,
        "b-notes",
        "",
        "Lo que no pude revisar",
        Some("Estas partes quedaron fuera. El reporte no dice nada sobre ellas, ni bueno ni malo."),
    );
    out.push_str("<div class=\"band-body\">\n<ul class=\"notes\">\n");
    for note in notes {
        out.push_str(&format!("<li><span>{}</span></li>\n", escape(note)));
    }
    out.push_str("</ul>\n</div>\n</section>\n");
}

fn footer(out: &mut String, data: &ReportData) {
    out.push_str("<footer class=\"foot\">\n<div class=\"wrap cols\">\n");
    out.push_str("<p class=\"brand foot-brand\">OBS Doctor</p>\n<div class=\"foot-text\">\n");
    out.push_str("<p class=\"promise\">Solo leí. No cambié nada en tu PC, en Windows ni en OBS.</p>\n");
    out.push_str(
        "<p>Este reporte se armó en tu PC y no se envió a ningún lado. \
         No incluye tu clave de transmisión, nombres de red, direcciones ni tu nombre de usuario.</p>\n",
    );
    out.push_str(&format!(
        "<p>Versión {}. Revisión del {}. Cuando hagas los cambios, vuelve a correr OBS Doctor para ver cómo quedó.</p>\n",
        escape(data.version),
        escape(&data.generated)
    ));
    out.push_str("</div>\n</div>\n</footer>\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Area, Row};

    const HOSTILE: &str = "<script>alert(1)</script><img src=x onerror=alert(2)>\"' onclick=\"x\" & </style>";

    fn advice(id: &'static str, level: Level, title: &str) -> Advice {
        Advice {
            id,
            level,
            area: Area::Obs,
            title: title.to_string(),
            why: String::from("Porque sí."),
            steps: vec![String::from("Abre OBS."), String::from("Haz clic en Ajustes.")],
        }
    }

    fn empty() -> ReportData {
        ReportData {
            version: "0.2.0",
            generated: String::from("18/11/2026 10:42"),
            verdict: Verdict::Ready,
            advice: Vec::new(),
            good: Vec::new(),
            notes: Vec::new(),
            sections: Vec::new(),
            programs: Vec::new(),
            suggested: Section::default(),
        }
    }

    /// Made-up data covering every part of the page. Nothing in it comes from this PC.
    fn sample() -> ReportData {
        let mut data = empty();
        data.verdict = Verdict::NeedsWork;
        data.advice = vec![
            Advice {
                id: "stream.dropped.network",
                level: Level::Important,
                area: Area::LastStream,
                title: String::from("Baja la calidad de envío a 4500 kbps"),
                why: String::from(
                    "En tu último directo se perdió el 6 % de los cuadros porque la subida de tu internet no alcanzó. Tu público lo vio a los saltos.",
                ),
                steps: vec![
                    String::from("Abre OBS y haz clic en Ajustes."),
                    String::from("Entra en Salida."),
                    String::from("En «Tasa de bits de vídeo» escribe 4500 y haz clic en Aceptar."),
                ],
            },
            Advice {
                id: "obs.encoder.use-hardware",
                level: Level::Recommended,
                area: Area::Obs,
                title: String::from("Deja que la placa de video prepare el video"),
                why: String::from("Hoy lo hace el procesador y le quita fuerza al juego."),
                steps: vec![
                    String::from("En Ajustes, Salida, busca «Codificador de vídeo»."),
                    String::from("Elige NVIDIA NVENC H.264."),
                ],
            },
            Advice {
                id: "apps.close.launcher",
                level: Level::Recommended,
                area: Area::Programs,
                title: String::from("Cierra el lanzador de juegos mientras transmites"),
                why: String::from("Descarga actualizaciones en segundo plano y usa la subida de tu internet."),
                steps: vec![String::from("Ciérralo desde el icono junto al reloj.")],
            },
            Advice {
                id: "win.game-mode",
                level: Level::Tip,
                area: Area::Windows,
                title: String::from("Prueba con el Modo de juego activado"),
                why: String::from("En algunas PC ayuda a que el juego no se trabe mientras OBS graba."),
                steps: vec![String::from("Abre Configuración, Juegos, Modo de juego y actívalo.")],
            },
        ];
        data.programs = vec![
            ProgramCard {
                name: String::from("Lanzador de juegos"),
                kind: ProgramKind::Close,
                usage: String::from("1,2 GB de memoria, 8 % del procesador"),
                tip: String::from("Ciérralo antes del directo: descarga en segundo plano."),
            },
            ProgramCard {
                name: String::from("Navegador"),
                kind: ProgramKind::Review,
                usage: String::from("2,1 GB de memoria"),
                tip: String::from("Si no lees el chat ahí, ciérralo."),
            },
            ProgramCard {
                name: String::from("juego.exe"),
                kind: ProgramKind::Game,
                usage: String::from("3,4 GB de memoria, 35 % del procesador"),
                tip: String::new(),
            },
            ProgramCard {
                name: String::from("Chat de voz"),
                kind: ProgramKind::Keep,
                usage: String::from("300 MB de memoria"),
                tip: String::new(),
            },
        ];
        data.good = vec![
            String::from("Tu OBS graba a 60 cuadros por segundo y tu PC lo sostiene."),
            String::from("Tu internet tiene subida de sobra para 1080p."),
        ];
        data.sections = vec![
            Section::new("pc", "Tu PC")
                .row(Row::new("Procesador", text::product_name("Intel(R) Core(TM) i5-10400F CPU (6 núcleos)")))
                .row(Row::new("Memoria", "16 GB en total, usando 12 GB (75 %)").tone(Tone::Warn).meter(0.75))
                .row(
                    Row::new("Placa de video", text::product_name("NVIDIA GeForce RTX(TM) 3060 (dedicada, 12 GB)"))
                        .note("Puede preparar el video sola."),
                ),
            Section::new("net", "Tu internet")
                .row(Row::new("Subida", "12 Mbps").tone(Tone::Good).meter(0.4))
                .row(Row::new("Conexión", "Wifi").tone(Tone::Bad).note("Con cable es más estable.")),
            Section::new("obs", "Tu OBS").intro("No encontré un perfil de OBS para revisar."),
        ];
        data.suggested = Section::new("suggested", "Con qué empezaría en esta PC")
            .row(
                Row::new("Quién prepara el video", "NVIDIA NVENC H.264")
                    .note("Tu placa lo hace sin quitarle fuerza al juego."),
            )
            .row(Row::new("Imagen", "1080p a 60 cuadros"))
            .row(Row::new("Calidad de envío", "6000 kbps"));
        data.notes = vec![String::from("No pude medir tu internet porque usaste --sin-internet.")];
        data
    }

    fn hostile() -> ReportData {
        let h = || HOSTILE.to_string();
        let mut data = empty();
        data.verdict = Verdict::AlmostReady;
        data.generated = h();
        data.advice = vec![Advice {
            id: "x\" onclick=\"alert(3)",
            level: Level::Tip,
            area: Area::Programs,
            title: h(),
            why: h(),
            steps: vec![h()],
        }];
        data.programs = vec![ProgramCard { name: h(), kind: ProgramKind::Unknown, usage: h(), tip: h() }];
        data.good = vec![h()];
        data.notes = vec![h()];
        data.sections = vec![Section::new("x\"><script>", h()).intro(h()).row(Row::new(h(), h()).note(h()))];
        data.suggested = Section::new("s", h()).row(Row::new(h(), h()));
        data
    }

    /// The page without the static assets, which are ours and trusted.
    fn dynamic_part(page: &str) -> String {
        page.replace(CSS, "").replace(JS, "")
    }

    #[test]
    fn escapes_everything_html_cares_about() {
        assert_eq!(
            escape(r#"<script>alert("x")</script> & 'y'"#),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;"
        );
    }

    #[test]
    fn hostile_text_never_becomes_markup() {
        let page = render(&hostile());
        let rest = dynamic_part(&page);
        assert_eq!(rest.matches("<script").count(), 1, "only our own script tag");
        assert_eq!(rest.matches("</style").count(), 1, "only our own style end");
        assert!(!rest.contains("<img"));
        // The words may survive as text, but never as a live attribute.
        assert!(!rest.contains("onerror=alert(2)>"));
        assert!(!rest.contains("onclick=\""));
        assert!(!rest.contains("\"><script>"));
        assert!(rest.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn hostile_text_escaped_in_every_field() {
        let page = render(&hostile());
        let escaped = escape(HOSTILE);
        // generated (title, mast, footer), advice (chip, title, label, why,
        // step), program (name, usage, tip), good, note, section (title,
        // intro, label, value, note), suggested (title, label, value).
        assert!(page.matches(&escaped).count() >= 20, "{}", page.matches(&escaped).count());
    }

    #[test]
    fn the_page_never_reaches_outside() {
        let page = render(&sample());
        for needle in ["http://", "https://", "url(", "@import", "<link", "src=", "//cdn"] {
            assert!(!page.contains(needle), "found {needle}");
        }
    }

    #[test]
    fn parts_come_in_the_planned_order() {
        let page = render(&sample());
        let order = [
            "class=\"hero\"",
            "id=\"b-advice\"",
            "id=\"b-programs\"",
            "id=\"b-good\"",
            "id=\"b-facts\"",
            "id=\"b-suggested\"",
            "id=\"b-notes\"",
            "class=\"foot\"",
        ];
        let positions: Vec<usize> =
            order.iter().map(|n| page.find(n).unwrap_or_else(|| panic!("missing {n}"))).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    }

    #[test]
    fn programs_are_grouped_close_first_keep_last() {
        let page = render(&sample());
        let close = page.find("Lanzador de juegos</bdi>").unwrap_or(0);
        let review = page.find("Navegador</bdi>").unwrap_or(0);
        let game = page.find("juego.exe</bdi>").unwrap_or(0);
        let keep = page.find("Chat de voz</bdi>").unwrap_or(0);
        assert!(close > 0 && close < review && review < game && game < keep);
    }

    #[test]
    fn every_advice_has_a_tick_box_a_chip_and_a_counter() {
        let data = sample();
        let page = render(&data);
        let n = data.advice.len();
        assert_eq!(page.matches("type=\"checkbox\" data-advice=").count(), n);
        assert_eq!(page.matches("<a class=\"chip ").count(), n);
        for item in &data.advice {
            assert!(page.contains(&format!("data-advice=\"{}\"", item.id)));
        }
        assert!(page.contains(&format!("0 de {n} hechos")));
        assert_eq!(page.matches("class=\"advice lvl-important\"").count(), 1);
        assert_eq!(page.matches("class=\"advice lvl-recommended\"").count(), 2);
        assert_eq!(page.matches("class=\"advice lvl-tip\"").count(), 1);
    }

    #[test]
    fn an_empty_report_still_renders_and_says_so() {
        let page = render(&empty());
        assert!(page.contains("class=\"verdict v-ready\""));
        assert!(page.contains("class=\"nothing\""));
        assert!(!dynamic_part(&page).contains("data-count"));
        for band in ["b-programs", "b-good", "b-facts", "b-suggested", "b-notes"] {
            assert!(!page.contains(&format!("id=\"{band}\"")), "{band}");
        }
    }

    #[test]
    fn tones_and_meters() {
        let page = render(&sample());
        assert!(page.contains("row tone-warn"));
        assert!(page.contains("row tone-bad"));
        assert!(page.contains("row tone-good"));
        // 75 % of 25 segments rounds to 19, which ends at 76.
        assert!(meter(0.75).contains("x2=\"76\""));
        assert!(!meter(0.0).contains("meter-fill"));
        assert!(!meter(f32::NAN).contains("meter-fill"));
        assert!(meter(7.0).contains("x2=\"100\" y2=\"2\"/></svg>"));
    }

    #[test]
    fn tally_reads_like_a_sentence() {
        let a = |level| advice("a", level, "t");
        assert_eq!(tally(&[]), "Nada que cambiar");
        assert_eq!(tally(&[a(Level::Tip)]), "1 para probar");
        assert_eq!(
            tally(&[a(Level::Important), a(Level::Recommended), a(Level::Recommended)]),
            "1 importante y 2 recomendados"
        );
        assert_eq!(
            tally(&[a(Level::Important), a(Level::Recommended), a(Level::Tip)]),
            "1 importante, 1 recomendado y 1 para probar"
        );
    }

    #[test]
    fn revision_changes_between_runs_and_is_attribute_safe() {
        let a = sample();
        let mut b = sample();
        b.generated = String::from("19/11/2026 09:00");
        assert_ne!(revision(&a), revision(&b));
        assert_eq!(revision(&a), revision(&sample()));
        assert!(revision(&a).chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// Writes target/sample-report.html from made-up data, to look at the
    /// design in a browser without running the doctor on a real PC.
    #[test]
    fn writes_the_sample_report() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("sample-report.html");
        // Not being able to write the sample is not a failure of the page.
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&path, render(&sample()));
    }
}
