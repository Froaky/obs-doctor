//! Prints the report. All the wording the streamer reads lives here and in `advice`.

use std::io::{self, Write};

use colored::Colorize;

use crate::advice::{self, Level, Review, Suggested};
use crate::hardware::{Hardware, GIB};
use crate::obs::{EncoderKind, ObsConfig, ObsStatus, OutputMode};
use crate::processes::{App, Apps};

const WIDTH: usize = 78;

pub fn enable_colors() {
    // Old Windows consoles need this switched on to understand colours.
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);
}

pub fn banner() {
    println!();
    println!("  {}  {}", "OBS DOCTOR".bold().cyan(), format!("v{}", env!("CARGO_PKG_VERSION")).dimmed());
    println!("  {}", "Reviso tu PC y tu OBS y te digo qué cambiar. No toco nada: los cambios los haces tú.".dimmed());
    println!();
    println!("  Mirando tu PC, dame un par de segundos...");
    let _ = io::stdout().flush();
}

pub fn print(hw: &Hardware, obs: &ObsStatus, suggested: &Suggested, review: &Review, apps: &Apps) {
    print_pc(hw);
    print_obs(obs);
    print_advice(review);
    print_apps(apps);
    print_good(review);
    print_suggested(suggested, obs);
    println!();
}

fn section(title: &str) {
    println!();
    println!("  {}", title.bold().cyan());
    println!("  {}", "-".repeat(title.chars().count()).cyan());
}

fn row(label: &str, value: &str) {
    println!("  {:<13}{}", label.dimmed(), value);
}

fn print_pc(hw: &Hardware) {
    section("TU PC");
    row("Procesador", &format!("{} ({} núcleos)", hw.cpu_name, hw.physical_cores));
    row(
        "Memoria",
        &format!(
            "{} GB en total, usando {} GB ({} %)",
            gb(hw.ram_total),
            gb(hw.ram_used),
            hw.ram_used_pct()
        ),
    );
    if hw.gpus.is_empty() {
        row("Placa", "No encontré ninguna placa de video");
    }
    for gpu in &hw.gpus {
        let kind = if gpu.is_integrated() {
            "integrada".to_string()
        } else {
            format!("dedicada, {} GB", gb(gpu.vram_bytes))
        };
        row("Placa", &format!("{} ({kind})", gpu.name));
    }
    row("Sistema", &hw.os);
}

fn print_obs(obs: &ObsStatus) {
    section("TU OBS");
    match obs {
        ObsStatus::NotFound { looked_in } => {
            wrap(
                "No encontré la configuración de OBS. Si ya lo instalaste, ábrelo una vez y vuelve a ejecutar el doctor. \
                 Mientras tanto, más abajo te dejo con qué configuración empezar.",
                2,
            );
            println!("  {}", format!("(busqué en {})", looked_in.display()).dimmed());
        }
        ObsStatus::NoProfile { dir } => {
            wrap("Encontré OBS, pero todavía no tiene ningún perfil guardado. Ábrelo una vez y vuelve a ejecutar el doctor.", 2);
            println!("  {}", format!("(busqué en {})", dir.display()).dimmed());
        }
        ObsStatus::Found(cfg) => print_config(cfg),
    }
}

fn print_config(cfg: &ObsConfig) {
    row("Perfil", &cfg.profile);
    row("Transmite a", cfg.service.as_deref().unwrap_or("Sin plataforma elegida (o servidor propio)"));
    let picture = match cfg.output {
        Some((w, h)) => format!("{w}x{h} a {} fps", fps_text(cfg.fps)),
        None => format!("tamaño sin definir, a {} fps", fps_text(cfg.fps)),
    };
    row("Imagen", &picture);
    if let (Some(base), Some(out)) = (cfg.base, cfg.output) {
        if base != out {
            row("", &format!("(tu pantalla en OBS es {}x{}; se achica antes de enviarla)", base.0, base.1));
        }
    }
    row("Envío", &format!("{} kbps", cfg.bitrate_kbps));
    row("Video", &encoder_text(cfg));
    row(
        "Modo",
        match cfg.mode {
            OutputMode::Simple => "Sencillo",
            OutputMode::Advanced => "Avanzado",
        },
    );
}

fn encoder_text(cfg: &ObsConfig) -> String {
    let who = match cfg.encoder {
        EncoderKind::Software => "lo prepara el procesador",
        EncoderKind::Nvidia => "lo prepara tu placa NVIDIA",
        EncoderKind::Amd => "lo prepara tu placa AMD",
        EncoderKind::Intel => "lo prepara tu gráfica Intel",
        EncoderKind::Other => "codificador que no reconozco",
    };
    format!("{} ({who})", cfg.encoder_id)
}

fn print_advice(review: &Review) {
    section("QUÉ CAMBIARÍA");
    if review.advice.is_empty() {
        println!("  {}", "Nada. Por lo que veo, estás bien configurado.".green());
        return;
    }
    for (i, item) in review.advice.iter().enumerate() {
        let tag = match item.level {
            Level::Important => "[IMPORTANTE]".red().bold(),
            Level::Recommended => "[RECOMENDADO]".yellow().bold(),
            Level::Tip => "[CONSEJO]".blue().bold(),
        };
        if i > 0 {
            println!();
        }
        println!("  {}. {} {}", i + 1, tag, item.title.bold());
        wrap(&format!("Por qué: {}", item.why), 5);
        println!("     {}", "Cómo:".dimmed());
        for step in &item.steps {
            wrap_bullet(step, 7);
        }
    }
}

fn print_apps(apps: &Apps) {
    section("PROGRAMAS ABIERTOS QUE TE CONVIENE REVISAR");
    if apps.known.is_empty() && apps.unknown.is_empty() {
        println!("  {}", "No veo nada abierto que le robe recursos al directo.".green());
        return;
    }
    wrap("Yo no cierro nada. Cierra tú lo que no necesites para el directo:", 2);
    for app in &apps.known {
        println!();
        println!("  {} {}", "-".dimmed(), usage_line(app));
        if let Some(category) = app.category {
            wrap(category.tip(), 4);
        }
    }
    if !apps.unknown.is_empty() {
        println!();
        wrap(
            "Estos no los conozco, pero están usando bastante. Si alguno es tu juego, ignóralo; si no sabes qué es, no lo cierres sin averiguar:",
            2,
        );
        for app in &apps.unknown {
            println!("  {} {}", "-".dimmed(), usage_line(app));
        }
    }
    if !apps.obs_running {
        println!();
        println!("  {}", "(OBS está cerrado ahora; con OBS y el juego abiertos vas a tener menos margen.)".dimmed());
    }
}

fn usage_line(app: &App) -> String {
    let mut parts = vec![format!("{} de memoria", mem_text(app.mem_bytes))];
    if app.cpu_pct >= 1.0 {
        parts.push(format!("{} % del procesador", app.cpu_pct.round()));
    }
    if app.process_count > 1 {
        parts.push(format!("{} procesos", app.process_count));
    }
    format!("{} {}", app.name.bold(), format!("({})", parts.join(", ")).dimmed())
}

fn print_good(review: &Review) {
    if review.good.is_empty() {
        return;
    }
    section("LO QUE YA ESTÁ BIEN");
    for line in &review.good {
        println!("  {} {}", "OK".green().bold(), line);
    }
}

fn print_suggested(s: &Suggested, obs: &ObsStatus) {
    section("CON QUÉ EMPEZARÍA EN ESTA PC");
    row("Video", s.encoder_label);
    row("Imagen", &format!("{}x{} a {} fps", s.width, s.height, s.fps));
    row("Envío", &format!("{} kbps", s.bitrate_kbps));
    row("Internet", &format!("al menos {} Mbps de subida (mídela en speedtest.net)", advice::upload_needed_mbps(s.bitrate_kbps)));
    println!();
    let note = match obs {
        ObsStatus::Found(_) => "Es un punto de partida, no una regla. Haz un directo de prueba y ajusta desde ahí.",
        _ => "Es un punto de partida para cuando abras OBS por primera vez. Haz un directo de prueba y ajusta desde ahí.",
    };
    wrap(note, 2);
}

/// When the program was opened with a double click, its window closes the
/// moment we finish. Wait for Enter so the report can actually be read.
pub fn pause_if_double_clicked() {
    if !owns_console() {
        return;
    }
    print!("  Presiona Enter para salir...");
    let _ = io::stdout().flush();
    let _ = io::stdin().read_line(&mut String::new());
}

#[cfg(windows)]
fn owns_console() -> bool {
    use windows::Win32::System::Console::GetConsoleProcessList;
    let mut pids = [0u32; 2];
    // SAFETY: the buffer is valid for the length we pass.
    let count = unsafe { GetConsoleProcessList(&mut pids) };
    count == 1 // we are alone: nobody launched us from a terminal
}

#[cfg(not(windows))]
fn owns_console() -> bool {
    false
}

fn gb(bytes: u64) -> String {
    let value = bytes as f64 / GIB;
    if value >= 10.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}").replace('.', ",")
    }
}

fn mem_text(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{} GB", format!("{:.1}", mb / 1024.0).replace('.', ","))
    } else {
        format!("{mb:.0} MB")
    }
}

fn fps_text(fps: f32) -> String {
    if (fps - fps.round()).abs() < 0.005 {
        format!("{fps:.0}")
    } else {
        format!("{fps:.2}").replace('.', ",")
    }
}

fn wrap(text: &str, indent: usize) {
    for line in wrap_lines(text, WIDTH - indent) {
        println!("{}{}", " ".repeat(indent), line);
    }
}

fn wrap_bullet(text: &str, indent: usize) {
    for (i, line) in wrap_lines(text, WIDTH - indent - 2).iter().enumerate() {
        let mark = if i == 0 { "- " } else { "  " };
        println!("{}{}{}", " ".repeat(indent), mark, line);
    }
}

fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed = current.chars().count() + word.chars().count() + usize::from(!current.is_empty());
        if needed > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_without_breaking_words() {
        let lines = wrap_lines("uno dos tres cuatro cinco", 9);
        assert_eq!(lines, vec!["uno dos", "tres", "cuatro", "cinco"]);
    }

    #[test]
    fn formats_sizes_the_spanish_way() {
        assert_eq!(gb(8 * GIB as u64), "8,0");
        assert_eq!(gb(16 * GIB as u64), "16");
        assert_eq!(mem_text(1536 * 1024 * 1024), "1,5 GB");
        assert_eq!(mem_text(300 * 1024 * 1024), "300 MB");
        assert_eq!(fps_text(60.0), "60");
        assert_eq!(fps_text(59.94), "59,94");
    }
}
