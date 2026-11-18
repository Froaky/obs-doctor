//! Finds the OBS logs and reads them without ever doing unbounded work.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::parse::Date;

/// A session log is a few hundred kilobytes; a noisy plugin can make it grow
/// to many megabytes. Past this size only the beginning and the end are read.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Enough for the startup block (about 10 KB) with a wide margin.
const HEAD_BYTES: u64 = 256 * 1024;
/// Upper limit for everything read in one run.
pub(super) const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// A logs folder never holds this many files; stop looking if it does.
const MAX_DIR_ENTRIES: usize = 5000;

pub(super) struct LogFile {
    pub path: PathBuf,
    /// Day the session started, taken from the file name.
    pub date: Option<Date>,
}

/// The `limit` most recent logs, newest first. Empty when the folder is
/// missing or unreadable.
pub(super) fn newest_logs(dir: &Path, limit: usize) -> Vec<LogFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .take(MAX_DIR_ENTRIES)
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_log_name(name))
        .collect();
    // The name is the moment the session started, written so that sorting
    // the text sorts by time.
    names.sort_unstable_by(|a, b| b.cmp(a));
    names.truncate(limit);
    names.into_iter().map(|name| LogFile { date: Date::parse(&name), path: dir.join(name) }).collect()
}

/// OBS names its logs "2026-11-18 10-22-42.txt" (`GenerateTimeDateFilename`
/// in frontend/OBSApp.cpp). Anything else in the folder is not ours to read.
fn is_log_name(name: &str) -> bool {
    const SHAPE: &[u8] = b"dddd-dd-dd dd-dd-dd.txt";
    let bytes = name.as_bytes();
    bytes.len() == SHAPE.len()
        && bytes.iter().zip(SHAPE).all(|(byte, shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            other => byte.eq_ignore_ascii_case(other),
        })
}

/// Reads a log as text, taking at most what is left of `budget`. Bytes that
/// are not valid UTF-8 are replaced, never rejected. `None` when the file
/// cannot be read or the budget is spent.
pub(super) fn read_bounded(path: &Path, budget: &mut u64) -> Option<String> {
    let allowed = MAX_FILE_BYTES.min(*budget);
    if allowed == 0 {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();

    let mut bytes = Vec::new();
    if len <= allowed {
        // `take` matters: OBS may be appending to this very file right now.
        file.take(allowed).read_to_end(&mut bytes).ok()?;
    } else {
        let head = HEAD_BYTES.min(allowed / 2);
        let tail = allowed - head;
        (&mut file).take(head).read_to_end(&mut bytes).ok()?;
        drop_partial_last_line(&mut bytes);
        let mut rest = Vec::new();
        file.seek(SeekFrom::Start(len - tail)).ok()?;
        file.take(tail).read_to_end(&mut rest).ok()?;
        bytes.extend_from_slice(after_first_line(&rest));
    }
    *budget = budget.saturating_sub(bytes.len() as u64);
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Cuts the buffer back to its last complete line.
fn drop_partial_last_line(bytes: &mut Vec<u8>) {
    let keep = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    bytes.truncate(keep);
}

/// Skips the first line, which starts somewhere in its middle.
fn after_first_line(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| *b == b'\n').map_or(bytes.len(), |i| i + 1);
    bytes.get(start..).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A folder of our own under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("obs-doctor-logs-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            TempDir(dir)
        }

        fn file(&self, name: &str, content: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            File::create(&path).and_then(|mut f| f.write_all(content)).expect("temp file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_files_named_like_obs_logs_count() {
        assert!(is_log_name("2026-11-18 10-22-42.txt"));
        assert!(is_log_name("2026-11-18 10-22-42.TXT"));
        assert!(!is_log_name("2026-11-18 10-22-42.txt.bak"));
        assert!(!is_log_name("Crash 2026-11-18 10-22-42.txt"));
        assert!(!is_log_name("notas.txt"));
        assert!(!is_log_name(""));
        assert!(!is_log_name("2026-11-18 10-22-4é.txt"));
    }

    #[test]
    fn newest_first_and_capped() {
        let dir = TempDir::new("order");
        for name in ["2026-11-16 09-00-00.txt", "2026-11-18 10-22-42.txt", "2026-11-17 23-59-59.txt", "leeme.txt"] {
            dir.file(name, b"x");
        }
        let logs = newest_logs(&dir.0, 2);
        let names: Vec<String> =
            logs.iter().filter_map(|l| l.path.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
        assert_eq!(names, ["2026-11-18 10-22-42.txt", "2026-11-17 23-59-59.txt"]);
        assert_eq!(logs.first().and_then(|l| l.date).map(Date::display).as_deref(), Some("18/11/2026"));
    }

    #[test]
    fn a_missing_folder_gives_no_logs() {
        assert!(newest_logs(Path::new("Z:/no/existe"), 10).is_empty());
    }

    #[test]
    fn invalid_utf8_is_replaced_not_rejected() {
        let dir = TempDir::new("utf8");
        let path = dir.file("2026-11-18 10-22-42.txt", b"10:00:00.000: caf\xe9\n10:00:01.000: ok\n");
        let mut budget = 1000;
        let text = read_bounded(&path, &mut budget).expect("readable");
        assert!(text.contains("caf\u{fffd}") && text.contains("ok"));
        assert_eq!(budget, 1000 - 35);
    }

    #[test]
    fn a_spent_budget_or_a_missing_file_reads_nothing() {
        let dir = TempDir::new("budget");
        let path = dir.file("2026-11-18 10-22-42.txt", b"hola\n");
        assert_eq!(read_bounded(&path, &mut 0), None);
        assert_eq!(read_bounded(&dir.0.join("no-existe.txt"), &mut 100), None);
        assert_eq!(read_bounded(&dir.file("2026-11-18 10-22-43.txt", b""), &mut 100).as_deref(), Some(""));
    }

    #[test]
    fn a_huge_file_keeps_its_first_and_last_whole_lines() {
        let dir = TempDir::new("huge");
        let mut content = Vec::new();
        for i in 0..2000 {
            content.extend_from_slice(format!("line {i:04}\n").as_bytes());
        }
        let path = dir.file("2026-11-18 10-22-42.txt", &content);
        // 205 bytes allowed: 102 for the head, 103 for the tail.
        let mut budget = 205;
        let text = read_bounded(&path, &mut budget).expect("readable");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.first(), Some(&"line 0000"));
        assert_eq!(lines.last(), Some(&"line 1999"));
        assert!(lines.iter().all(|l| l.len() == 9), "no cut lines: {lines:?}");
        assert!(lines.len() < 25);
        assert!(budget < 205);
    }

    #[test]
    fn cutting_helpers_handle_buffers_without_a_newline() {
        let mut bytes = b"sin salto".to_vec();
        drop_partial_last_line(&mut bytes);
        assert!(bytes.is_empty());
        assert!(after_first_line(b"sin salto").is_empty());
        assert_eq!(after_first_line(b"a\nb\n"), b"b\n");
    }
}
