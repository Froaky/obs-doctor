//! What OBS wrote down in its own log files: facts about this PC that OBS
//! already detected, and how the last stream actually went.

use super::ObsPaths;
use crate::facts::Facts;
use crate::model::{Review, Section};

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
    pub encoder_id: Option<String>,
}

pub fn read(_paths: &ObsPaths) -> LogFacts {
    LogFacts::default()
}

pub fn sections(_facts: &Facts) -> Vec<Section> {
    Vec::new()
}

pub fn advise(_facts: &Facts, _review: &mut Review) {}
