//! Draws the report in the terminal: a short summary by default, or the whole
//! thing with `--consola`.

use std::io::{self, Write};
use std::path::Path;

use colored::{ColoredString, Colorize};

use super::text::wrap;
use crate::model::{Advice, Level, ProgramCard, ProgramKind, ReportData, Row, Section, Tone, Verdict};

const WIDTH: usize = 78;

pub fn enable_colors() {
    // Old Windows consoles need this switched on to understand colours.
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);
}

pub fn banner() {
    println!();
    println!("  {}  {}", "OBS DOCTOR".bold().cyan(), format!("v{}", env!("CARGO_PKG_VERSION")).dimmed());
    println!("  {}", "Reviso tu PC, tu internet y tu OBS, y te digo qué cambiar.".dimmed());
    println!("  {}", "No toco nada: los cambios los haces tú.".dimmed());
    println!();
}

/// One progress line while the doctor works.
pub fn step(message: &str) {
    println!("  {} {}", ">".cyan(), message);
    let _ = io::stdout().flush();
}

/// Extra detail under the current step (e.g. while measuring the connection).
pub fn detail(message: &str) {
    println!("    {}", message.dimmed());
    let _ = io::stdout().flush();
}

pub fn problem(message: &str) {
    for line in wrap(message, WIDTH - 2) {
        eprintln!("  {}", line.red());
    }
}

/// The short version shown when the full report opens in the browser.
pub fn print_summary(data: &ReportData, saved_to: &Path, opened: bool) {
    println!();
    print_headline(data);
    for (i, item) in data.advice.iter().take(5).enumerate() {
        println!("  {}. {} {}", i + 1, level_tag(item.level), item.title);
    }
    if data.advice.len() > 5 {
        println!("  {}", format!("... y {} más en el reporte.", data.advice.len() - 5).dimmed());
    }
    println!();
    if opened {
        println!("  El reporte completo, con el paso a paso, se abrió en tu navegador.");
    } else {
        println!("  El reporte completo, con el paso a paso, quedó guardado. Ábrelo con doble clic:");
    }
    println!("  {}", saved_to.display().to_string().dimmed());
    println!();
}

/// The whole report as text.
pub fn print_full(data: &ReportData) {
    println!();
    print_headline(data);
    print_advice(&data.advice);
    print_programs(&data.programs);
    print_good(&data.good);
    for section in &data.sections {
        print_section(section);
    }
    print_section(&data.suggested);
    print_notes(&data.notes);
    println!();
    println!("  {}", format!("Revisión hecha el {}.", data.generated).dimmed());
    println!();
}

fn print_headline(data: &ReportData) {
    let headline = data.verdict.headline().bold();
    let headline = match data.verdict {
        Verdict::Ready => headline.green(),
        Verdict::AlmostReady => headline.yellow(),
        Verdict::NeedsWork => headline.red(),
    };
    println!("  {headline}");
    if !data.advice.is_empty() {
        // Same tally as the HTML report, one count per level.
        println!("  {}", super::html::tally(&data.advice).dimmed());
    }
    println!();
}

fn heading(title: &str) {
    println!();
    println!("  {}", title.to_uppercase().bold().cyan());
    println!("  {}", "-".repeat(title.chars().count()).cyan());
}

fn level_tag(level: Level) -> ColoredString {
    match level {
        Level::Important => "[IMPORTANTE]".red().bold(),
        Level::Recommended => "[RECOMENDADO]".yellow().bold(),
        Level::Tip => "[PARA PROBAR]".blue().bold(),
    }
}

fn print_advice(advice: &[Advice]) {
    heading("Qué cambiaría");
    if advice.is_empty() {
        println!("  {}", "Nada. Por lo que veo, estás bien configurado.".green());
        return;
    }
    for (i, item) in advice.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("  {}. {} {}", i + 1, level_tag(item.level), item.title.bold());
        println!("     {}", item.area.label().dimmed());
        paragraph(&format!("Por qué: {}", item.why), 5);
        if !item.steps.is_empty() {
            println!("     {}", "Cómo:".dimmed());
            for step in &item.steps {
                bullet(step, 7);
            }
        }
    }
}

fn print_programs(programs: &[ProgramCard]) {
    if programs.is_empty() {
        return;
    }
    heading("Programas abiertos");
    println!("  {}", "Yo no cierro nada. Decide tú qué necesitas para el directo.".dimmed());
    let groups = [
        (ProgramKind::Close, "Te conviene cerrarlos o pausarlos"),
        (ProgramKind::Review, "Revisa si los necesitas"),
        (ProgramKind::Unknown, "No los conozco, pero están usando bastante"),
        (ProgramKind::Game, "Parece ser tu juego"),
        (ProgramKind::Keep, "Herramientas del directo: déjalas"),
    ];
    for (kind, title) in groups {
        let cards: Vec<&ProgramCard> = programs.iter().filter(|p| p.kind == kind).collect();
        if cards.is_empty() {
            continue;
        }
        println!();
        println!("  {}", title.bold());
        for card in cards {
            let usage = if card.usage.is_empty() { String::new() } else { format!(" ({})", card.usage) };
            println!("  {} {}{}", "-".dimmed(), card.name.bold(), usage.dimmed());
            if !card.tip.is_empty() {
                paragraph(&card.tip, 4);
            }
        }
    }
}

fn print_good(good: &[String]) {
    if good.is_empty() {
        return;
    }
    heading("Lo que ya está bien");
    for line in good {
        let lines = wrap(line, WIDTH - 5);
        for (i, text) in lines.iter().enumerate() {
            if i == 0 {
                println!("  {} {}", "OK".green().bold(), text);
            } else {
                println!("     {text}");
            }
        }
    }
}

fn print_section(section: &Section) {
    if section.is_empty() {
        return;
    }
    heading(&section.title);
    if let Some(intro) = &section.intro {
        paragraph(intro, 2);
    }
    let label_width = section.rows.iter().map(|r| r.label.chars().count()).max().unwrap_or(0).min(22) + 2;
    for row in &section.rows {
        print_row(row, label_width);
    }
}

fn print_row(row: &Row, label_width: usize) {
    let value = match row.tone {
        Tone::Neutral => row.value.normal(),
        Tone::Good => row.value.green(),
        Tone::Warn => row.value.yellow(),
        Tone::Bad => row.value.red(),
    };
    let padding = " ".repeat(label_width.saturating_sub(row.label.chars().count()));
    println!("  {}{}{}", row.label.dimmed(), padding, value);
    if let Some(note) = &row.note {
        for line in wrap(note, WIDTH - 2 - label_width) {
            println!("  {}{}", " ".repeat(label_width), line.dimmed());
        }
    }
}

fn print_notes(notes: &[String]) {
    if notes.is_empty() {
        return;
    }
    heading("Lo que no pude revisar");
    for note in notes {
        bullet(note, 2);
    }
}

fn paragraph(text: &str, indent: usize) {
    for line in wrap(text, WIDTH - indent) {
        println!("{}{}", " ".repeat(indent), line);
    }
}

fn bullet(text: &str, indent: usize) {
    for (i, line) in wrap(text, WIDTH - indent - 2).iter().enumerate() {
        let mark = if i == 0 { "- " } else { "  " };
        println!("{}{}{}", " ".repeat(indent), mark, line);
    }
}

/// When the program was opened with a double click, its window closes the
/// moment we finish. Wait for Enter so the result can actually be read.
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
