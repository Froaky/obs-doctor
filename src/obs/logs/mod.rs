//! What OBS wrote down in its own log files: facts about this PC that OBS
//! already detected, and how the last stream actually went.
//!
//! - [`files`]: finds the newest logs and reads a bounded amount of them.
//! - [`parse`]: turns the text of one log into facts (pure).
//! - [`rules`]: advice that follows from the last stream.
//! - [`sections`]: the "Tu último directo" block of the report.
//!
//! Privacy: OBS logs contain the address of the streaming server (which, with
//! a custom server, can carry the stream key), IP addresses, file paths with
//! the Windows user name and device names. None of that is kept. A log line is
//! never stored: only numbers, encoder ids and hardware model names.

mod files;
mod parse;
mod rules;
mod sections;

use super::ObsPaths;
use super::config::EncoderKind;

pub use rules::advise;
pub use sections::sections;

/// OBS keeps 10 logs unless told otherwise (`MaxLogs` default in OBS 33,
/// frontend/OBSApp.cpp). Looking further back would describe a setup the
/// streamer probably no longer has.
const MAX_LOG_FILES: usize = 10;

/// A loss above this share of frames is something viewers start to notice.
///
/// OBS's own statistics panel paints each of the three loss figures yellow
/// above 1 % and red above 5 % (OBS 33, frontend/widgets/OBSBasicStats.cpp).
/// We use the same two steps so the report agrees with what the streamer
/// sees inside OBS. The official log analyzer is more lenient (warning at
/// 5 %, critical at 15 %; 3 % and 10 % for rendering lag):
/// <https://github.com/obsproject/loganalyzer> (checks/network.py,
/// checks/encoding.py, checks/graphics.py).
pub const NOTICEABLE_LOSS_PCT: f32 = 1.0;
/// A loss above this share of frames clearly hurts the stream. See
/// [`NOTICEABLE_LOSS_PCT`].
pub const SERIOUS_LOSS_PCT: f32 = 5.0;

/// A stream shorter than this says little about the setup: the official
/// analyzer asks for about 30 seconds of output before it draws conclusions
/// (<https://github.com/obsproject/loganalyzer>, checks/vars.py, `cleanLog`).
pub const MIN_STREAM_SECS: u64 = 30;

/// Facts taken from the OBS logs. `Default` means "no log could be read".
///
/// CONTRACT: other areas read the fields below. Keep their names, types and
/// meaning; add more fields freely.
#[derive(Debug, Clone, Default)]
pub struct LogFacts {
    /// Video encoder ids OBS listed as available on this PC when it last
    /// started, e.g. `["obs_x264", "obs_qsv11_v2"]`. This is ground truth
    /// about which hardware encoders really work here. `None` when no log was
    /// readable or the list was not in it.
    pub available_encoders: Option<Vec<String>>,
    /// The most recent stream found in the logs, with how it went.
    /// `None` when the streamer has not streamed (or no log says so).
    pub last_stream: Option<StreamStats>,
    /// What OBS detected about this PC the last time it started. `None` when
    /// the newest log has no startup block.
    pub startup: Option<Startup>,
    /// The most recent recording in the logs that were read. A recording is
    /// never reported as a stream; this is here so other areas can use it as
    /// evidence of what the PC can take.
    pub last_recording: Option<RecordingStats>,
    /// How many log files were read. Zero means there was nothing to read.
    pub logs_read: usize,
}

impl LogFacts {
    /// Whether OBS itself listed an encoder of this kind as usable on this PC.
    /// `None` when the logs do not say.
    pub fn can_encode_with(&self, kind: EncoderKind) -> Option<bool> {
        let ids = self.available_encoders.as_ref()?;
        Some(ids.iter().any(|id| EncoderKind::from_id(id) == kind))
    }
}

/// How one stream went, according to the statistics OBS prints when it ends.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamStats {
    /// When it happened, ready to show: "17/11/2026".
    pub date: Option<String>,
    pub duration_secs: Option<u64>,
    /// Share of frames (0-100) lost because the connection could not send
    /// them in time. `None` when the log does not say.
    pub dropped_network_pct: Option<f32>,
    /// Share of frames (0-100) skipped because the encoder could not keep up.
    pub skipped_encoding_pct: Option<f32>,
    /// Share of frames (0-100) missed because drawing the picture took too
    /// long (graphics card too busy).
    pub lagged_render_pct: Option<f32>,
    /// Id of the video encoder that stream used, e.g. `"obs_x264"`.
    ///
    /// The log names the encoder family and codec, not the id, so this is the
    /// id OBS 33 lists for that family and codec (an NVIDIA H.264 stream is
    /// always reported as `"obs_nvenc_h264_tex"`).
    pub encoder_id: Option<String>,

    // Everything below is not part of the original contract.
    /// Local time it started, "21:04".
    pub started_at: Option<String>,
    pub end: StreamEnd,
    /// Times the connection to the platform was lost while live.
    pub disconnects: u32,
    /// True when OBS was set to lower the quality by itself instead of losing
    /// frames ("dynamic bitrate"). With it on, a bad connection shows up in
    /// `bitrate_reductions` rather than in `dropped_network_pct`.
    pub dynamic_bitrate: bool,
    /// Times OBS lowered the quality by itself because the connection could
    /// not keep up.
    pub bitrate_reductions: u32,
    /// What the stream encoder was set to send, in kbps.
    pub bitrate_kbps: Option<u32>,
    /// Size of the picture that was sent.
    pub picture: Option<(u32, u32)>,
    pub fps: Option<f32>,
    /// How that stream left the PC. Only OBS on Windows over RTMP logs it.
    pub link: Option<Link>,
    /// True when a recording, the replay buffer or the virtual camera ran at
    /// the same time. OBS counts encoder skips for all of them together, so
    /// `skipped_encoding_pct` may not be the stream's fault alone.
    pub shared_encoding: bool,
    /// Times the graphics card was reset by Windows while live.
    pub graphics_resets: u32,
    /// True when OBS hit its limit of audio delay while live.
    pub audio_buffering_maxed: bool,
}

impl StreamStats {
    /// Kind of encoder that stream used, when the log says.
    pub fn encoder_kind(&self) -> Option<EncoderKind> {
        self.encoder_id.as_deref().map(EncoderKind::from_id)
    }

    /// True when it was too short to judge the setup by it.
    pub fn too_short(&self) -> bool {
        self.duration_secs.is_some_and(|secs| secs < MIN_STREAM_SECS)
    }
}

/// How a stream came to an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamEnd {
    /// The streamer stopped it (or nothing says otherwise).
    #[default]
    Stopped,
    /// The connection dropped and OBS gave up reconnecting.
    Disconnected,
    /// The video encoder failed and OBS had to stop.
    EncoderError,
    /// The log of that session just ends: OBS or the PC went down while live.
    Abrupt,
    /// No end in the newest log: either still live or OBS went down.
    Unfinished,
}

/// The kind of network link a stream went out through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Cable,
    Wifi,
    Other,
}

/// How a recording went. Kept apart from streams on purpose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingStats {
    /// "17/11/2026".
    pub date: Option<String>,
    pub duration_secs: Option<u64>,
    /// Share of frames (0-100) the encoder skipped. `None` when unknown.
    pub skipped_encoding_pct: Option<f32>,
    /// Share of frames (0-100) OBS could not draw in time.
    pub lagged_render_pct: Option<f32>,
}

/// What OBS printed about this PC when it started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Startup {
    /// "33.0.1".
    pub obs_version: Option<String>,
    pub running_as_admin: Option<bool>,
    /// Windows "Game DVR" (captures in the Xbox Game Bar).
    pub game_dvr: Option<bool>,
    pub game_dvr_background_recording: Option<bool>,
    pub game_bar: Option<bool>,
    pub game_mode: Option<GameMode>,
    /// Graphics cards in the order OBS lists them.
    pub adapters: Vec<Adapter>,
    /// Index into `adapters` of the card OBS draws with.
    pub render_adapter: Option<usize>,
    /// Why OBS could not offer the NVIDIA encoder, when it said so.
    pub nvenc_problem: Option<NvencProblem>,
    /// OBS found that its previous session had not been closed properly.
    pub previous_exit_unclean: bool,
}

impl Startup {
    /// The card OBS draws with, when known.
    pub fn render_card(&self) -> Option<&Adapter> {
        self.adapters.get(self.render_adapter?)
    }

    /// Refresh rate shared by every monitor, when they all agree.
    // Offered to the hardware and suggest areas, which do not use it yet.
    #[allow(dead_code)]
    pub fn common_refresh_hz(&self) -> Option<u32> {
        let mut rates = self.adapters.iter().flat_map(|a| &a.monitors).map(|m| m.refresh_hz).filter(|hz| *hz > 0);
        let first = rates.next()?;
        rates.all(|hz| hz == first).then_some(first)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    On,
    Off,
    /// No setting stored: Windows turns it on by default.
    ProbablyOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvencProblem {
    /// The installed NVIDIA driver is too old for this OBS.
    OutdatedDriver,
    /// OBS could not load NVENC at all (usually: no NVIDIA card or driver).
    NotSupported,
}

/// A graphics card as OBS saw it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Adapter {
    /// Model name, e.g. "NVIDIA GeForce GTX 1660".
    pub name: String,
    /// "32.0.101.7076".
    pub driver_version: Option<String>,
    /// Windows "hardware-accelerated GPU scheduling".
    pub hags_enabled: Option<bool>,
    pub dedicated_vram_mb: Option<u64>,
    pub monitors: Vec<Monitor>,
}

/// A screen plugged into a graphics card. Zero means "not stated".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

pub fn read(paths: &ObsPaths) -> LogFacts {
    let logs = files::newest_logs(&paths.logs_dir, MAX_LOG_FILES);
    let mut budget = files::MAX_TOTAL_BYTES;
    // Lazy on purpose: `gather` stops asking for older logs once it has what
    // it needs, so most runs read a single file.
    let sessions = logs.iter().filter_map(|log| {
        let text = files::read_bounded(&log.path, &mut budget)?;
        Some(parse::parse(&text, log.date))
    });
    gather(sessions)
}

/// Combines the sessions, newest first, into what the report needs.
fn gather(sessions: impl Iterator<Item = parse::Session>) -> LogFacts {
    let mut facts = LogFacts::default();
    for session in sessions {
        let newest = facts.logs_read == 0;
        facts.logs_read += 1;
        if newest {
            facts.startup = session.startup;
        }
        if facts.available_encoders.is_none() {
            facts.available_encoders = session.available_encoders;
        }
        if facts.last_recording.is_none() {
            facts.last_recording = session.recordings.into_iter().next_back();
        }
        if facts.last_stream.is_none()
            && let Some(mut stream) = session.streams.into_iter().next_back()
        {
            if stream.end == StreamEnd::Unfinished {
                stream.end = match (session.clean_shutdown, newest) {
                    // OBS closed in an orderly way, so the stream did end.
                    (true, _) => StreamEnd::Stopped,
                    // A later session exists, so this one is over, yet it never said goodbye.
                    (false, false) => StreamEnd::Abrupt,
                    (false, true) => StreamEnd::Unfinished,
                };
            }
            facts.last_stream = Some(stream);
        }
        if facts.last_stream.is_some() && facts.available_encoders.is_some() {
            break;
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const CLEAN: &str = include_str!("../../../tests/fixtures/logs/clean_stream.txt");
    const CRASH: &str = include_str!("../../../tests/fixtures/logs/crash_no_stats.txt");
    const RECORDING_ONLY: &str = include_str!("../../../tests/fixtures/logs/recording_only.txt");
    const NO_OUTPUT: &str = include_str!("../../../tests/fixtures/logs/no_output.txt");

    /// Sessions newest first, one day apart, the newest on 17/11/2026.
    fn sessions(texts: &[&str]) -> impl Iterator<Item = parse::Session> {
        let days = ["2026-11-17", "2026-11-16", "2026-11-15", "2026-11-14"];
        texts
            .iter()
            .zip(days.iter().chain(std::iter::repeat(&"2026-11-01")))
            .map(|(text, day)| parse::parse(text, parse::Date::parse(day)))
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn no_logs_means_no_facts() {
        let facts = gather(std::iter::empty());
        assert_eq!(facts.logs_read, 0);
        assert!(facts.available_encoders.is_none() && facts.last_stream.is_none() && facts.startup.is_none());
        assert_eq!(facts.can_encode_with(EncoderKind::Intel), None);
    }

    #[test]
    fn a_missing_folder_is_tolerated() {
        let paths = ObsPaths { logs_dir: PathBuf::from("Z:/no/existe/obs-studio/logs"), ..ObsPaths::default() };
        let facts = read(&paths);
        assert_eq!(facts.logs_read, 0);
        assert!(facts.last_stream.is_none());
    }

    #[test]
    fn the_stream_is_found_in_an_older_log() {
        let facts = gather(sessions(&[NO_OUTPUT, RECORDING_ONLY, CLEAN]));
        assert_eq!(facts.logs_read, 3);
        let stream = facts.last_stream.expect("the third log has a stream");
        assert_eq!(stream.date.as_deref(), Some("15/11/2026"));
        // Startup facts always describe the newest session.
        assert_eq!(facts.startup.and_then(|s| s.obs_version).as_deref(), Some("33.0.1"));
        // The recording is newer than the stream and is reported apart.
        assert_eq!(facts.last_recording.and_then(|r| r.date).as_deref(), Some("16/11/2026"));
    }

    #[test]
    fn older_logs_are_not_read_once_the_stream_is_found() {
        let facts = gather(sessions(&[CLEAN, CRASH, RECORDING_ONLY]));
        assert_eq!(facts.logs_read, 1);
    }

    #[test]
    fn a_recording_is_never_presented_as_a_stream() {
        let facts = gather(sessions(&[RECORDING_ONLY, NO_OUTPUT]));
        assert!(facts.last_stream.is_none());
        assert!(facts.last_recording.is_some());
        assert_eq!(facts.logs_read, 2);
    }

    #[test]
    fn a_stream_without_an_end_in_an_older_log_ended_abruptly() {
        let facts = gather(sessions(&[NO_OUTPUT, CRASH]));
        assert_eq!(facts.last_stream.map(|s| s.end), Some(StreamEnd::Abrupt));
    }

    #[test]
    fn a_stream_without_an_end_in_the_newest_log_may_still_be_live() {
        let facts = gather(sessions(&[CRASH]));
        assert_eq!(facts.last_stream.map(|s| s.end), Some(StreamEnd::Unfinished));
    }

    #[test]
    fn available_encoders_come_from_the_newest_log_that_lists_them() {
        let facts = gather(sessions(&["10:00:00.000: OBS 33.0.1 (64-bit, windows)\n", CLEAN]));
        let ids = facts.available_encoders.clone().expect("listed in the second log");
        assert!(ids.contains(&"obs_x264".to_string()), "{ids:?}");
        assert_eq!(facts.can_encode_with(EncoderKind::Software), Some(true));
        assert_eq!(facts.can_encode_with(EncoderKind::Amd), Some(false));
    }

    #[test]
    fn startup_helpers() {
        let startup = Startup {
            adapters: vec![Adapter {
                name: "A".to_string(),
                monitors: vec![
                    Monitor { width: 1920, height: 1080, refresh_hz: 60 },
                    Monitor { width: 0, height: 0, refresh_hz: 0 },
                    Monitor { width: 1366, height: 768, refresh_hz: 60 },
                ],
                ..Adapter::default()
            }],
            render_adapter: Some(0),
            ..Startup::default()
        };
        assert_eq!(startup.common_refresh_hz(), Some(60));
        assert_eq!(startup.render_card().map(|a| a.name.as_str()), Some("A"));
        assert_eq!(Startup::default().common_refresh_hz(), None);
        assert!(Startup::default().render_card().is_none());
    }
}
