//! Turns the facts into a report and gets it in front of the streamer.

pub mod console;
pub mod html;
pub mod text;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::facts::Facts;
use crate::model::{ReportData, Review, Section, Verdict};
use crate::{diagnosis, hardware, net, obs, processes, suggest, system};

/// Runs every rule over the facts and lays the result out for display.
pub fn build(facts: &Facts) -> ReportData {
    let mut review = Review::default();
    // Order matters only for ties within a level: what happened in the last
    // stream comes first because it is evidence, not a guess.
    obs::logs::advise(facts, &mut review);
    obs::rules::advise(facts, &mut review);
    net::advise(facts, &mut review);
    obs::scenes::advise(facts, &mut review);
    system::advise(facts, &mut review);
    hardware::advise(facts, &mut review);
    processes::advise(facts, &mut review);
    diagnosis::refine(facts, &mut review);
    review.sort();

    let mut sections: Vec<Section> = Vec::new();
    sections.extend(obs::logs::sections(facts));
    sections.extend(hardware::sections(facts));
    sections.extend(obs::config::sections(facts));
    sections.extend(net::sections(facts));
    sections.extend(obs::scenes::sections(facts));
    sections.extend(system::sections(facts));
    sections.retain(|s| !s.is_empty());

    ReportData {
        version: env!("CARGO_PKG_VERSION"),
        generated: local_time_text(),
        verdict: Verdict::of(&review),
        advice: review.advice,
        good: review.good,
        notes: review.notes,
        sections,
        programs: processes::cards(facts),
        suggested: suggest::section(facts),
    }
}

/// Where the page is saved when the streamer does not choose a place.
pub fn default_path() -> PathBuf {
    let base =
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
    base.join("obs-doctor").join("reporte.html")
}

pub fn write_html(data: &ReportData, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, html::render(data))
}

/// Opens the saved page with the default browser. Returns false if it could not.
#[cfg(windows)]
pub fn open_in_browser(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};

    let Ok(full) = fs::canonicalize(path) else {
        return false;
    };
    // canonicalize() returns a `\\?\` path, which the shell does not accept.
    let text = full.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    let wide: Vec<u16> = std::ffi::OsStr::new(&text).encode_wide().chain(std::iter::once(0)).collect();

    // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string that outlives the call.
    let result = unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
    };
    // By contract, values above 32 mean success.
    result.0 as usize > 32
}

#[cfg(not(windows))]
pub fn open_in_browser(path: &Path) -> bool {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(opener).arg(path).spawn().is_ok()
}

/// "18/11/2026 10:42" in the PC's own time zone.
#[cfg(windows)]
fn local_time_text() -> String {
    // SAFETY: GetLocalTime has no preconditions and returns the struct by value.
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}/{:02}/{} {:02}:{:02}", now.wDay, now.wMonth, now.wYear, now.wHour, now.wMinute)
}

#[cfg(not(windows))]
fn local_time_text() -> String {
    String::from("hoy")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pc_still_produces_a_report() {
        let data = build(&Facts::default());
        assert_eq!(data.version, env!("CARGO_PKG_VERSION"));
        assert!(!data.suggested.is_empty(), "there is always a starting suggestion");
        assert!(data.sections.iter().all(|s| !s.is_empty()));
    }

    #[test]
    fn the_default_path_is_an_html_file_in_our_own_folder() {
        let path = default_path();
        assert_eq!(path.file_name().unwrap(), "reporte.html");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "obs-doctor");
    }
}
