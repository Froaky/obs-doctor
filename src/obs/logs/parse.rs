//! Turns the text of one OBS log into facts. Pure: no files, no clock.
//!
//! Every pattern below is copied from the OBS 33.0 source, named next to it.
//! A line that matches nothing is ignored, so logs from other versions or
//! with extra plugin noise never make the parse fail; they just say less.
//!
//! Lines are only matched, never kept: the log also holds the server URL
//! (`Connecting to RTMP URL ...` in plugins/obs-outputs/rtmp-stream.c, which
//! with a custom server can include the stream key), IP addresses, file paths
//! and device names.

use super::{Adapter, GameMode, Link, Monitor, NvencProblem, RecordingStats, Startup, StreamEnd, StreamStats};

const MS_PER_DAY: u64 = 24 * 60 * 60 * 1000;
/// OBS stamps lines with the local time of day only. A jump back in time
/// bigger than this means the session went past midnight (a clock change of
/// an hour for daylight saving does not count).
const MIDNIGHT_JUMP_MS: u64 = 2 * 60 * 60 * 1000;

/// A calendar day, taken from the log file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// Reads the "2026-11-18" at the start of a log file name.
    pub fn parse(text: &str) -> Option<Date> {
        let year = text.get(0..4)?.parse().ok()?;
        let month = text.get(5..7)?.parse().ok()?;
        let day = text.get(8..10)?.parse().ok()?;
        let sep_ok = text.get(4..5) == Some("-") && text.get(7..8) == Some("-");
        let valid = sep_ok && (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month);
        valid.then_some(Date { year, month, day })
    }

    /// "18/11/2026".
    pub fn display(self) -> String {
        format!("{:02}/{:02}/{}", self.day, self.month, self.year)
    }

    fn add_days(self, days: u64) -> Date {
        let mut date = self;
        for _ in 0..days.min(366) {
            date = if date.day < days_in_month(date.year, date.month) {
                Date { day: date.day + 1, ..date }
            } else if date.month < 12 {
                Date { month: date.month + 1, day: 1, ..date }
            } else {
                Date { year: date.year.saturating_add(1), month: 1, day: 1 }
            };
        }
        date
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Everything one log says.
#[derive(Debug, Default)]
pub(super) struct Session {
    /// `None` when the log has no startup block (cut, or not an OBS log).
    pub startup: Option<Startup>,
    pub available_encoders: Option<Vec<String>>,
    /// Oldest first.
    pub streams: Vec<StreamStats>,
    /// Oldest first.
    pub recordings: Vec<RecordingStats>,
    /// OBS reached its own "Shutting down" step, so nothing was left live.
    pub clean_shutdown: bool,
}

pub(super) fn parse(text: &str, date: Option<Date>) -> Session {
    let mut parser = Parser { date, ..Parser::default() };
    for line in text.lines() {
        parser.line(line);
    }
    parser.finish()
}

/// What the stream encoder printed when it started.
#[derive(Debug, Clone, Default)]
struct EncoderInfo {
    /// Text inside the brackets: "x264 encoder", "obs-nvenc", "texture-amf-h264"...
    family: String,
    codec: Option<String>,
    bitrate_kbps: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    fps_num: Option<u32>,
    fps_den: Option<u32>,
}

impl EncoderInfo {
    /// The encoder id OBS 33 registers for this family and codec (ids from
    /// plugins/obs-x264, obs-nvenc/nvenc.c, obs-qsv11/obs-qsv11.c and
    /// obs-ffmpeg/texture-amf.cpp). `None` for families we cannot pin down.
    fn id(&self) -> Option<String> {
        let codec = self.codec.as_deref().unwrap_or("").to_ascii_lowercase();
        let codec = if codec.contains("hevc") || codec.contains("265") {
            "hevc"
        } else if codec.contains("av1") {
            "av1"
        } else {
            "h264"
        };
        let family = self.family.to_ascii_lowercase();
        let id = match family.as_str() {
            "x264 encoder" => "obs_x264".to_string(),
            "obs-nvenc" => format!("obs_nvenc_{codec}_tex"),
            "qsv encoder" => match codec {
                "hevc" => "obs_qsv11_hevc".to_string(),
                "av1" => "obs_qsv11_av1".to_string(),
                _ => "obs_qsv11_v2".to_string(),
            },
            amf if amf.starts_with("texture-amf-") || amf.starts_with("fallback-amf-") => {
                let (kind, codec) = amf.split_once("-amf-")?;
                format!("{codec}_{kind}_amf")
            }
            _ => return None,
        };
        Some(id)
    }

    fn fps(&self) -> Option<f32> {
        let (num, den) = (self.fps_num?, self.fps_den?);
        (num > 0 && den > 0).then(|| num as f32 / den as f32)
    }
}

/// A stream in progress, before it is turned into [`StreamStats`].
#[derive(Debug, Default)]
struct OpenStream {
    stats: StreamStats,
    start_ms: u64,
    encoder_error: bool,
    /// The connection was lost and not (yet) made again.
    connection_lost: bool,
}

#[derive(Debug, Default)]
struct OpenRecording {
    stats: RecordingStats,
    start_ms: u64,
}

#[derive(Debug, Default, PartialEq)]
enum Block {
    #[default]
    None,
    Adapters,
    EncoderList,
    EncoderListVideo,
    EncoderSettings,
}

#[derive(Debug, Default)]
struct Parser {
    date: Option<Date>,
    /// Milliseconds since the midnight of the day the log started.
    now_ms: u64,
    last_clock_ms: Option<u64>,
    days_passed: u64,

    startup: Startup,
    saw_startup: bool,
    render_adapter_seen: bool,
    encoders: Option<Vec<String>>,
    block: Block,

    /// Settings the stream encoder printed last time it started.
    stream_encoder: Option<EncoderInfo>,
    settings: Option<EncoderInfo>,
    /// Said while connecting, before the stream counts as started.
    next_link: Option<Link>,
    next_dynamic_bitrate: bool,

    stream: Option<OpenStream>,
    /// The stream that just stopped. OBS may print its final numbers right
    /// after the "Streaming Stop" banner, so it stays reachable until
    /// another output starts.
    closing_stream: Option<usize>,
    streams: Vec<StreamStats>,

    recording: Option<OpenRecording>,
    closing_recording: Option<usize>,
    recordings: Vec<RecordingStats>,
    replay_active: bool,
    virtualcam_active: bool,

    clean_shutdown: bool,
}

impl Parser {
    fn line(&mut self, line: &str) {
        let Some((clock_ms, msg)) = split_line(line) else {
            return;
        };
        self.tick(clock_ms);

        // Blocks: lines that only make sense right after a header line.
        if self.in_block(msg) {
            return;
        }

        if let Some(rest) = msg.strip_prefix("==== ") {
            self.banner(rest);
        } else if let Some(rest) = msg.strip_prefix("Output '") {
            self.output_line(rest);
        } else if let Some(rest) = msg.strip_prefix("[rtmp stream: '") {
            self.rtmp_line(rest);
        } else if let Some(rest) = msg.strip_prefix("Video stopped, number of skipped frames due to encoding lag: ") {
            self.skipped_line(rest);
        } else if msg.starts_with('[') && msg.ends_with("] settings:") {
            self.encoder_settings_start(msg);
        } else if msg == "Max audio buffering reached!" {
            if let Some(stream) = &mut self.stream {
                stream.stats.audio_buffering_maxed = true;
            }
        } else if msg.starts_with("Device Remove/Reset!") {
            if let Some(stream) = &mut self.stream {
                stream.stats.graphics_resets += 1;
            }
        } else {
            self.startup_line(msg);
        }
    }

    fn tick(&mut self, clock_ms: u64) {
        if let Some(last) = self.last_clock_ms
            && clock_ms + MIDNIGHT_JUMP_MS < last
        {
            self.days_passed += 1;
        }
        self.last_clock_ms = Some(clock_ms);
        self.now_ms = self.days_passed * MS_PER_DAY + clock_ms;
    }

    fn today(&self) -> Option<String> {
        self.date.map(|d| d.add_days(self.days_passed).display())
    }

    /// Handles a line that belongs to the block started by an earlier
    /// header. Returns false when the block is over (or there is none).
    fn in_block(&mut self, msg: &str) -> bool {
        let indented = msg.starts_with('\t') || msg.starts_with(' ');
        match self.block {
            Block::None => false,
            _ if !indented && self.block != Block::EncoderSettings => {
                self.block = Block::None;
                false
            }
            Block::Adapters => {
                self.adapter_line(msg.trim());
                true
            }
            Block::EncoderList | Block::EncoderListVideo => {
                let trimmed = msg.trim();
                if trimmed == "Video Encoders:" {
                    self.block = Block::EncoderListVideo;
                } else if trimmed.ends_with("Encoders:") {
                    self.block = Block::EncoderList;
                } else if self.block == Block::EncoderListVideo
                    && let Some(entry) = trimmed.strip_prefix("- ")
                    && let Some(id) = entry.split_whitespace().next()
                {
                    self.encoders.get_or_insert_with(Vec::new).push(id.to_string());
                }
                true
            }
            // plugins/obs-x264/obs-x264.c, obs-nvenc/nvenc.c and
            // obs-qsv11/obs-qsv11.c print "\tkey: value" lines after "settings:".
            Block::EncoderSettings => {
                if msg.starts_with('\t') {
                    self.encoder_setting(msg.trim());
                    true
                } else {
                    self.encoder_settings_end();
                    false
                }
            }
        }
    }

    // frontend/widgets/OBSBasic_Streaming.cpp, _Recording.cpp,
    // _ReplayBuffer.cpp and _VirtualCam.cpp; "Shutting down" in OBSApp.cpp.
    fn banner(&mut self, rest: &str) {
        let name = rest.trim_end_matches('=').trim();
        match name {
            "Streaming Start" => {
                self.close_stream(StreamEnd::Unfinished);
                self.closing_stream = None;
                self.closing_recording = None;
                let mut stats = StreamStats {
                    date: self.today(),
                    started_at: self.clock_text(),
                    shared_encoding: self.recording.is_some() || self.replay_active || self.virtualcam_active,
                    ..StreamStats::default()
                };
                if let Some(encoder) = &self.stream_encoder {
                    apply_encoder(&mut stats, encoder);
                }
                stats.link = self.next_link.take();
                stats.dynamic_bitrate = std::mem::take(&mut self.next_dynamic_bitrate);
                self.stream = Some(OpenStream { stats, start_ms: self.now_ms, ..OpenStream::default() });
            }
            "Streaming Stop" => {
                let end = match &self.stream {
                    Some(s) if s.encoder_error => StreamEnd::EncoderError,
                    Some(s) if s.connection_lost => StreamEnd::Disconnected,
                    _ => StreamEnd::Stopped,
                };
                self.close_stream(end);
            }
            "Recording Start" => {
                self.closing_stream = None;
                self.closing_recording = None;
                self.mark_shared();
                self.recording = Some(OpenRecording {
                    stats: RecordingStats { date: self.today(), ..RecordingStats::default() },
                    start_ms: self.now_ms,
                });
            }
            "Recording Stop" => self.close_recording(),
            "Replay Buffer Start" => {
                self.closing_stream = None;
                self.replay_active = true;
                self.mark_shared();
            }
            "Replay Buffer Stop" => self.replay_active = false,
            "Virtual Camera Start" => {
                self.closing_stream = None;
                self.virtualcam_active = true;
                self.mark_shared();
            }
            "Virtual Camera Stop" => self.virtualcam_active = false,
            "Shutting down" => self.clean_shutdown = true,
            _ => {}
        }
    }

    fn mark_shared(&mut self) {
        if let Some(stream) = &mut self.stream {
            stream.stats.shared_encoding = true;
        }
    }

    fn close_stream(&mut self, end: StreamEnd) {
        let Some(open) = self.stream.take() else {
            return;
        };
        let mut stats = open.stats;
        stats.end = end;
        stats.duration_secs = Some(self.now_ms.saturating_sub(open.start_ms) / 1000);
        // OBS only prints the skipped-frames line when some were skipped,
        // and only once nothing else is using the encoder. Alone, silence
        // means none; shared, the line may still come later.
        if end != StreamEnd::Unfinished && !stats.shared_encoding && stats.skipped_encoding_pct.is_none() {
            stats.skipped_encoding_pct = Some(0.0);
        }
        self.streams.push(stats);
        self.closing_stream = Some(self.streams.len() - 1);
    }

    fn close_recording(&mut self) {
        let Some(open) = self.recording.take() else {
            return;
        };
        let mut stats = open.stats;
        stats.duration_secs = Some(self.now_ms.saturating_sub(open.start_ms) / 1000);
        self.recordings.push(stats);
        self.closing_recording = Some(self.recordings.len() - 1);
    }

    fn target_stream(&mut self) -> Option<&mut StreamStats> {
        match &mut self.stream {
            Some(open) => Some(&mut open.stats),
            None => self.streams.get_mut(self.closing_stream?),
        }
    }

    fn target_recording(&mut self) -> Option<&mut RecordingStats> {
        match &mut self.recording {
            Some(open) => Some(&mut open.stats),
            None => self.recordings.get_mut(self.closing_recording?),
        }
    }

    // libobs/obs-output.c, log_frame_info().
    fn output_line(&mut self, rest: &str) {
        let Some((name, msg)) = rest.split_once("': ") else {
            return;
        };
        let is_stream = name.ends_with("_stream");
        let is_recording = name.ends_with("_file_output") || name.ends_with("_ffmpeg_output");
        if !is_stream && !is_recording {
            return;
        }

        let mut dropped = None;
        let mut lagged = None;
        if let Some(count) = msg.strip_prefix("Total frames output: ") {
            // Only printed with the "(N attempted)" part when frames were dropped.
            if leading_number(count).is_some_and(|n| n > 0) {
                dropped = Some(0.0);
            }
        } else if let Some(count) = msg.strip_prefix("Total drawn frames: ") {
            if leading_number(count).is_some_and(|n| n > 0) {
                lagged = Some(0.0);
            }
        } else if msg.starts_with("Number of lagged frames due to rendering lag/stalls: ") {
            lagged = percent_in_parens(msg);
        } else if msg.starts_with("Number of dropped frames due to insufficient bandwidth/connection stalls: ") {
            dropped = percent_in_parens(msg);
        } else {
            return;
        }

        if is_stream && let Some(stats) = self.target_stream() {
            // A zero from the totals line never overwrites a real figure.
            if let Some(value) = dropped
                && (value > 0.0 || stats.dropped_network_pct.is_none())
            {
                stats.dropped_network_pct = Some(value);
            }
            if let Some(value) = lagged
                && (value > 0.0 || stats.lagged_render_pct.is_none())
            {
                stats.lagged_render_pct = Some(value);
            }
        }
        if is_recording
            && let Some(stats) = self.target_recording()
            && let Some(value) = lagged
            && (value > 0.0 || stats.lagged_render_pct.is_none())
        {
            stats.lagged_render_pct = Some(value);
        }
    }

    // plugins/obs-outputs/rtmp-stream.c and rtmp-windows.c.
    fn rtmp_line(&mut self, rest: &str) {
        let Some((_, msg)) = rest.split_once("'] ") else {
            return;
        };
        // The first connection is made before the "Streaming Start" banner,
        // so what it says is kept for the stream that is about to start.
        if msg.starts_with("Dynamic bitrate enabled") {
            match &mut self.stream {
                Some(stream) => stream.stats.dynamic_bitrate = true,
                None => self.next_dynamic_bitrate = true,
            }
            return;
        }
        if let Some(interface) = msg.strip_prefix("Interface: ") {
            // "Interface: <adapter name> (<type>, <down>/<up> mbps)". The
            // adapter name is personal; only the type is read.
            let kind = interface.rfind(" (").and_then(|i| interface.get(i + 2..)).and_then(|t| t.split(',').next());
            let link = kind.map(|kind| match kind {
                "ethernet" => Link::Cable,
                "802.11" => Link::Wifi,
                _ => Link::Other,
            });
            match &mut self.stream {
                Some(stream) => stream.stats.link = link,
                None => self.next_link = link,
            }
            return;
        }
        let Some(stream) = &mut self.stream else {
            return;
        };
        if msg.starts_with("Disconnected from ") {
            stream.stats.disconnects += 1;
            stream.connection_lost = true;
        } else if msg.starts_with("Connection to ") && msg.ends_with(" successful") {
            stream.connection_lost = false;
        } else if msg.starts_with("Encoder error, disconnecting") {
            stream.encoder_error = true;
        } else if msg.starts_with("bitrate decreased to: ") {
            stream.stats.bitrate_reductions += 1;
        }
    }

    // libobs/media-io/video-io.c, log_skipped(): "%ld/%ld (%0.1f%%)".
    fn skipped_line(&mut self, rest: &str) {
        let Some(pct) = percent_in_parens(rest) else {
            return;
        };
        if let Some(stats) = self.target_stream() {
            stats.skipped_encoding_pct = Some(pct);
        }
        if let Some(stats) = self.target_recording() {
            stats.skipped_encoding_pct = Some(pct);
        }
    }

    /// "[x264 encoder: 'simple_video_stream'] settings:".
    fn encoder_settings_start(&mut self, msg: &str) {
        let inner = msg.trim_start_matches('[').trim_end_matches("] settings:");
        let Some((family, name)) = inner.split_once(": '") else {
            return;
        };
        let name = name.trim_end_matches('\'');
        // Encoder names given by frontend/utility/SimpleOutput.cpp and
        // AdvancedOutput.cpp ("simple_video_stream", "advanced_video_stream").
        if name.contains("stream") && !name.contains("record") {
            self.settings = Some(EncoderInfo { family: family.to_string(), ..EncoderInfo::default() });
            self.block = Block::EncoderSettings;
        }
    }

    fn encoder_setting(&mut self, entry: &str) {
        let Some(info) = &mut self.settings else {
            return;
        };
        let Some((key, value)) = entry.split_once(':') else {
            return;
        };
        let value = value.trim();
        let number = || leading_number(value).and_then(|n| u32::try_from(n).ok());
        match key.trim() {
            "codec" => info.codec = Some(value.to_string()),
            "bitrate" => info.bitrate_kbps = number().filter(|n| *n > 0),
            "width" => info.width = number(),
            "height" => info.height = number(),
            "fps_num" => info.fps_num = number(),
            "fps_den" => info.fps_den = number(),
            _ => {}
        }
    }

    fn encoder_settings_end(&mut self) {
        self.block = Block::None;
        let Some(info) = self.settings.take() else {
            return;
        };
        if let Some(stream) = &mut self.stream {
            apply_encoder(&mut stream.stats, &info);
        }
        self.stream_encoder = Some(info);
    }

    /// Lines from the startup block: libobs/obs-windows.c, libobs/obs.c,
    /// libobs-d3d11/d3d11-subsystem.cpp, frontend/OBSApp.cpp and obs-nvenc.
    fn startup_line(&mut self, msg: &str) {
        let trimmed = msg.trim();
        let s = &mut self.startup;
        if let Some(rest) = trimmed.strip_prefix("OBS ")
            && rest.starts_with(|c: char| c.is_ascii_digit())
            && s.obs_version.is_none()
        {
            s.obs_version = rest.split_whitespace().next().map(str::to_string);
            self.saw_startup = true;
        } else if let Some(value) = trimmed.strip_prefix("Running as administrator: ") {
            s.running_as_admin = parse_bool(value, "true", "false");
            self.saw_startup = true;
        } else if let Some(value) = trimmed.strip_prefix("Game Bar: ") {
            s.game_bar = parse_bool(value, "On", "Off");
        } else if let Some(value) = trimmed.strip_prefix("Game DVR: ") {
            s.game_dvr = parse_bool(value, "On", "Off");
        } else if let Some(value) = trimmed.strip_prefix("Game DVR Background Recording: ") {
            s.game_dvr_background_recording = parse_bool(value, "On", "Off");
        } else if let Some(value) = trimmed.strip_prefix("Game Mode: ") {
            s.game_mode = match value {
                "On" => Some(GameMode::On),
                "Off" => Some(GameMode::Off),
                v if v.starts_with("Probably On") => Some(GameMode::ProbablyOn),
                _ => None,
            };
        } else if trimmed == "Available Video Adapters:" && s.adapters.is_empty() {
            self.block = Block::Adapters;
        } else if let Some(rest) = trimmed.strip_prefix("Loading up D3D11 on adapter ") {
            if !self.render_adapter_seen {
                self.render_adapter_seen = true;
                // "<name> (<index>)": the name may itself contain brackets.
                s.render_adapter = rest
                    .rfind('(')
                    .and_then(|i| rest.get(i + 1..))
                    .and_then(|t| t.strip_suffix(')'))
                    .and_then(|n| n.parse().ok());
            }
        } else if trimmed == "Available Encoders:" && self.encoders.is_none() {
            self.block = Block::EncoderList;
        } else if trimmed == "Crash or unclean shutdown detected" {
            s.previous_exit_unclean = true;
        } else if trimmed.starts_with("[obs-nvenc] Current driver version does not support") {
            s.nvenc_problem = Some(NvencProblem::OutdatedDriver);
        } else if trimmed == "NVENC not supported" && s.nvenc_problem.is_none() {
            s.nvenc_problem = Some(NvencProblem::NotSupported);
        }
    }

    // libobs-d3d11/d3d11-subsystem.cpp, LogAdapterMonitors() and LogD3DAdapters().
    fn adapter_line(&mut self, line: &str) {
        let adapters = &mut self.startup.adapters;
        if let Some(rest) = line.strip_prefix("Adapter ")
            && let Some((_, name)) = rest.split_once(": ")
        {
            adapters.push(Adapter { name: name.trim().to_string(), ..Adapter::default() });
            return;
        }
        let Some(adapter) = adapters.last_mut() else {
            return;
        };
        if let Some(value) = line.strip_prefix("Dedicated VRAM:") {
            adapter.dedicated_vram_mb = leading_number(value.trim()).map(|bytes| bytes / (1024 * 1024));
        } else if let Some(value) = line.strip_prefix("HAGS Status:") {
            let value = value.trim();
            adapter.hags_enabled = if value.starts_with("Enabled") {
                Some(true)
            } else if value.starts_with("Disabled") {
                Some(false)
            } else {
                None
            };
        } else if let Some(value) = line.strip_prefix("Driver Version:") {
            let value = value.trim();
            adapter.driver_version = (value.starts_with(|c: char| c.is_ascii_digit())).then(|| value.to_string());
        } else if line.starts_with("output ") && line.ends_with(':') {
            adapter.monitors.push(Monitor::default());
        } else if let Some(monitor) = adapter.monitors.last_mut() {
            if let Some(size) = line.strip_prefix("size={").and_then(|s| s.strip_suffix('}')) {
                let mut parts = size.split(',').map(|p| p.trim().parse().unwrap_or(0));
                monitor.width = parts.next().unwrap_or(0);
                monitor.height = parts.next().unwrap_or(0);
            } else if let Some(rate) = line.strip_prefix("refresh=") {
                monitor.refresh_hz = rate.trim().parse().unwrap_or(0);
            }
        }
    }

    fn clock_text(&self) -> Option<String> {
        let clock = self.last_clock_ms?;
        Some(format!("{:02}:{:02}", clock / 3_600_000, clock / 60_000 % 60))
    }

    fn finish(mut self) -> Session {
        if self.block == Block::EncoderSettings {
            self.encoder_settings_end();
        }
        // Still live when the text ends: OBS is open, or it went down.
        self.close_stream(StreamEnd::Unfinished);
        self.close_recording();
        Session {
            startup: self.saw_startup.then_some(self.startup),
            available_encoders: self.encoders,
            streams: self.streams,
            recordings: self.recordings,
            clean_shutdown: self.clean_shutdown,
        }
    }
}

fn apply_encoder(stats: &mut StreamStats, encoder: &EncoderInfo) {
    stats.encoder_id = encoder.id();
    stats.bitrate_kbps = encoder.bitrate_kbps;
    stats.picture = encoder.width.zip(encoder.height).filter(|(w, h)| *w > 0 && *h > 0);
    stats.fps = encoder.fps();
}

/// Splits "10:22:42.532: message" into the time of day in milliseconds and
/// the message. `None` for lines without that stamp.
fn split_line(line: &str) -> Option<(u64, &str)> {
    let stamp = line.get(0..12)?;
    let after = line.get(12..)?;
    // An empty message is written as "HH:MM:SS.mmm: " or, trimmed, "HH:MM:SS.mmm:".
    let msg = after.strip_prefix(": ").or_else(|| after.strip_prefix(':'))?;
    let bytes = stamp.as_bytes();
    if bytes.get(2) != Some(&b':') || bytes.get(5) != Some(&b':') || bytes.get(8) != Some(&b'.') {
        return None;
    }
    let field = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<u64>().ok();
    let (h, m, s, ms) = (field(0..2)?, field(3..5)?, field(6..8)?, field(9..12)?);
    (h < 24 && m < 60 && s < 61).then_some((((h * 60 + m) * 60 + s) * 1000 + ms, msg))
}

/// The digits at the start of `text`: "1234 (5 attempted)" is 1234.
fn leading_number(text: &str) -> Option<u64> {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    text.get(..end)?.parse().ok()
}

/// The "12.3" in "... (12.3%)", clamped to 0-100.
fn percent_in_parens(text: &str) -> Option<f32> {
    let open = text.rfind('(')?;
    let inner = text.get(open + 1..)?.strip_suffix("%)")?;
    let value: f32 = inner.trim().parse().ok()?;
    value.is_finite().then(|| value.clamp(0.0, 100.0))
}

fn parse_bool(value: &str, yes: &str, no: &str) -> Option<bool> {
    if value == yes {
        Some(true)
    } else if value == no {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &str = include_str!("../../../tests/fixtures/logs/clean_stream.txt");
    const NETWORK: &str = include_str!("../../../tests/fixtures/logs/network_drops.txt");
    const ENCODER: &str = include_str!("../../../tests/fixtures/logs/encoder_overloaded.txt");
    const RENDER: &str = include_str!("../../../tests/fixtures/logs/render_lag.txt");
    const CRASH: &str = include_str!("../../../tests/fixtures/logs/crash_no_stats.txt");
    const RECORDING_ONLY: &str = include_str!("../../../tests/fixtures/logs/recording_only.txt");
    const TWO_STREAMS: &str = include_str!("../../../tests/fixtures/logs/two_streams.txt");
    const NO_OUTPUT: &str = include_str!("../../../tests/fixtures/logs/no_output.txt");
    const OLD_FORMAT: &str = include_str!("../../../tests/fixtures/logs/old_format.txt");

    fn only_stream(text: &str) -> StreamStats {
        let mut session = parse(text, Date::parse("2026-11-15 20-58-10.txt"));
        assert_eq!(session.streams.len(), 1, "{:?}", session.streams);
        session.streams.remove(0)
    }

    #[test]
    fn dates_from_file_names() {
        assert_eq!(Date::parse("2026-11-18 10-22-42.txt").map(Date::display).as_deref(), Some("18/11/2026"));
        assert_eq!(Date::parse("2026-02-30 10-22-42.txt"), None);
        assert_eq!(Date::parse("2026-13-01"), None);
        assert_eq!(Date::parse("hola"), None);
        assert_eq!(Date::parse(""), None);
        let last = Date::parse("2024-12-31").expect("valid");
        assert_eq!(last.add_days(1).display(), "01/01/2025");
        assert_eq!(Date::parse("2024-02-28").map(|d| d.add_days(1).display()).as_deref(), Some("29/02/2024"));
    }

    #[test]
    fn line_stamps() {
        assert_eq!(split_line("10:22:42.532: hola"), Some((37_362_532, "hola")));
        assert_eq!(split_line("10:22:42.532: "), Some((37_362_532, "")));
        assert_eq!(split_line("sin hora"), None);
        assert_eq!(split_line("99:22:42.532: x"), None);
        assert_eq!(split_line("1é:22:42.532: x"), None);
        assert_eq!(split_line(""), None);
    }

    #[test]
    fn startup_block() {
        let session = parse(CLEAN, None);
        let startup = session.startup.expect("startup block");
        assert_eq!(startup.obs_version.as_deref(), Some("33.0.1"));
        assert_eq!(startup.running_as_admin, Some(false));
        assert_eq!(startup.game_dvr, Some(true));
        assert_eq!(startup.game_dvr_background_recording, Some(false));
        assert_eq!(startup.game_bar, Some(true));
        assert_eq!(startup.game_mode, Some(GameMode::ProbablyOn));
        assert_eq!(startup.adapters.len(), 2);
        let card = startup.render_card().expect("render card");
        assert_eq!(card.name, "NVIDIA GeForce GTX 1660 SUPER");
        assert_eq!(card.driver_version.as_deref(), Some("32.0.15.6094"));
        assert_eq!(card.hags_enabled, Some(true));
        assert_eq!(card.dedicated_vram_mb, Some(6144));
        assert_eq!(startup.adapters.get(1).and_then(|a| a.hags_enabled), Some(false));
        assert_eq!(startup.adapters.get(1).and_then(|a| a.driver_version.clone()), None);
        assert_eq!(startup.common_refresh_hz(), Some(144));
        assert_eq!(startup.nvenc_problem, None);
        assert!(startup.previous_exit_unclean);
        assert_eq!(
            session.available_encoders,
            Some(vec!["obs_nvenc_h264_tex".to_string(), "obs_nvenc_hevc_tex".to_string(), "obs_x264".to_string()])
        );
    }

    #[test]
    fn clean_stream() {
        let stream = only_stream(CLEAN);
        assert_eq!(stream.date.as_deref(), Some("15/11/2026"));
        assert_eq!(stream.started_at.as_deref(), Some("21:00"));
        assert_eq!(stream.duration_secs, Some(2 * 3600 + 5));
        assert_eq!(stream.end, StreamEnd::Stopped);
        assert_eq!(stream.dropped_network_pct, Some(0.0));
        assert_eq!(stream.skipped_encoding_pct, Some(0.0));
        assert_eq!(stream.lagged_render_pct, Some(0.0));
        assert_eq!(stream.encoder_id.as_deref(), Some("obs_nvenc_h264_tex"));
        assert_eq!(stream.bitrate_kbps, Some(6000));
        assert_eq!(stream.picture, Some((1920, 1080)));
        assert_eq!(stream.fps, None);
        assert_eq!(stream.link, Some(Link::Cable));
        assert_eq!(stream.disconnects, 0);
        assert!(!stream.shared_encoding && !stream.dynamic_bitrate);
    }

    #[test]
    fn network_problems() {
        let stream = only_stream(NETWORK);
        assert_eq!(stream.dropped_network_pct, Some(8.2));
        assert_eq!(stream.disconnects, 2);
        assert_eq!(stream.end, StreamEnd::Stopped, "it reconnected the second time");
        assert_eq!(stream.link, Some(Link::Wifi));
        assert!(stream.dynamic_bitrate);
        assert_eq!(stream.bitrate_reductions, 3);
        assert_eq!(stream.encoder_id.as_deref(), Some("obs_x264"));
        assert_eq!(stream.fps, Some(30.0));
        assert_eq!(stream.picture, Some((1280, 720)));
        assert_eq!(stream.bitrate_kbps, Some(4500));
    }

    #[test]
    fn encoder_overloaded() {
        let stream = only_stream(ENCODER);
        assert_eq!(stream.skipped_encoding_pct, Some(12.4));
        assert_eq!(stream.dropped_network_pct, Some(0.0));
        assert_eq!(stream.encoder_id.as_deref(), Some("obs_x264"));
        assert_eq!(stream.fps, Some(60.0));
        assert!(stream.shared_encoding, "a recording ran at the same time");
    }

    #[test]
    fn render_lag() {
        let stream = only_stream(RENDER);
        assert_eq!(stream.lagged_render_pct, Some(6.7));
        assert_eq!(stream.encoder_id.as_deref(), Some("obs_qsv11_v2"));
        assert_eq!(stream.graphics_resets, 1);
        assert!(stream.audio_buffering_maxed);
        assert_eq!(stream.link, Some(Link::Other));
    }

    #[test]
    fn crash_leaves_an_unfinished_stream_without_stats() {
        let session = parse(CRASH, None);
        assert!(!session.clean_shutdown);
        let stream = session.streams.first().expect("one stream");
        assert_eq!(stream.end, StreamEnd::Unfinished);
        assert_eq!(stream.dropped_network_pct, None);
        assert_eq!(stream.skipped_encoding_pct, None);
        assert_eq!(stream.lagged_render_pct, None);
        assert_eq!(stream.duration_secs, Some(1800));
        assert_eq!(stream.date, None, "no file name, no date");
    }

    #[test]
    fn recording_only() {
        let session = parse(RECORDING_ONLY, Date::parse("2026-11-16 18-00-00.txt"));
        assert!(session.streams.is_empty());
        let recording = session.recordings.first().expect("one recording");
        assert_eq!(recording.date.as_deref(), Some("16/11/2026"));
        assert_eq!(recording.duration_secs, Some(600));
        assert_eq!(recording.skipped_encoding_pct, Some(2.5));
        assert_eq!(recording.lagged_render_pct, Some(0.0));
        assert!(session.clean_shutdown);
    }

    #[test]
    fn two_streams_in_one_log_and_midnight() {
        let session = parse(TWO_STREAMS, Date::parse("2026-11-17 22-00-00.txt"));
        assert_eq!(session.streams.len(), 2);
        let first = session.streams.first().expect("first");
        let last = session.streams.last().expect("last");
        assert_eq!(first.dropped_network_pct, Some(3.1));
        assert_eq!(first.date.as_deref(), Some("17/11/2026"));
        assert_eq!(last.dropped_network_pct, Some(0.0));
        assert_eq!(last.date.as_deref(), Some("18/11/2026"), "the second one began after midnight");
        assert_eq!(last.started_at.as_deref(), Some("00:10"));
        assert_eq!(last.duration_secs, Some(1200));
        assert_eq!(last.end, StreamEnd::Disconnected, "lost and never reconnected");
        assert_eq!(last.encoder_id.as_deref(), Some("h264_texture_amf"));
    }

    #[test]
    fn a_session_without_outputs() {
        let session = parse(NO_OUTPUT, None);
        assert!(session.streams.is_empty() && session.recordings.is_empty());
        assert!(session.clean_shutdown);
        assert_eq!(session.startup.and_then(|s| s.nvenc_problem), Some(NvencProblem::NotSupported));
    }

    #[test]
    fn an_older_obs_still_gives_what_it_can() {
        let session = parse(OLD_FORMAT, None);
        let stream = session.streams.first().expect("stream");
        // Older versions had no gaming-features block and other encoder names.
        assert_eq!(stream.dropped_network_pct, Some(1.5));
        assert_eq!(stream.encoder_id.as_deref(), Some("obs_x264"));
        let startup = session.startup.expect("version line");
        assert_eq!(startup.obs_version.as_deref(), Some("27.2.4"));
        assert_eq!(startup.game_mode, None);
        assert_eq!(startup.nvenc_problem, Some(NvencProblem::OutdatedDriver));
    }

    #[test]
    fn garbage_and_empty_text_give_nothing() {
        for text in ["", "\n\n", "esto no es un log\n\u{0}\u{fffd}", "10:00:00.000: ==== Streaming Stop ===="] {
            let session = parse(text, None);
            assert!(session.startup.is_none() && session.streams.is_empty() && session.available_encoders.is_none());
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(percent_in_parens("12/100 (12.0%)"), Some(12.0));
        assert_eq!(percent_in_parens("12/100 (NaN%)"), None);
        assert_eq!(percent_in_parens("12/100 (500.0%)"), Some(100.0));
        assert_eq!(percent_in_parens("sin nada"), None);
        assert_eq!(leading_number("1234 (5 attempted)"), Some(1234));
        assert_eq!(leading_number("x"), None);
        let amf = EncoderInfo { family: "fallback-amf-h265".to_string(), ..EncoderInfo::default() };
        assert_eq!(amf.id().as_deref(), Some("h265_fallback_amf"));
        let qsv =
            EncoderInfo { family: "qsv encoder".to_string(), codec: Some("HEVC".into()), ..EncoderInfo::default() };
        assert_eq!(qsv.id().as_deref(), Some("obs_qsv11_hevc"));
        let other = EncoderInfo { family: "AV1 encoder".to_string(), ..EncoderInfo::default() };
        assert_eq!(other.id(), None);
    }
}
