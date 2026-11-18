//! The internet connection: how it is connected and how fast and steady it is.

use crate::facts::Facts;
use crate::model::{Review, Section};

/// How the PC reaches the internet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Connection {
    #[default]
    Unknown,
    Cable,
    Wifi,
    /// Phone tethering, mobile broadband, anything else.
    Other,
}

/// What we know about the connection. `Default` means "not measured".
///
/// CONTRACT: other areas read the fields below. Keep their names, types and
/// meaning; add more fields freely.
#[derive(Debug, Clone, Default)]
pub struct NetFacts {
    /// True only when the speed test actually ran and produced numbers.
    pub measured: bool,
    pub connection: Connection,
    /// Sustained upload speed in megabits per second. The number that
    /// decides how much bitrate a stream can carry.
    pub upload_mbps: Option<f64>,
    pub download_mbps: Option<f64>,
    /// Typical round trip in milliseconds.
    pub latency_ms: Option<f64>,
    /// How much the round trip varies, in milliseconds.
    pub jitter_ms: Option<f64>,
}

/// Measures the connection. `progress` receives short status lines to show
/// while it works ("Midiendo la subida..."). Must never panic and must give
/// up politely when there is no internet.
pub fn read(_progress: &mut dyn FnMut(&str)) -> NetFacts {
    NetFacts::default()
}

/// What we can know without sending or downloading test data (the streamer
/// passed `--sin-internet`): connection type yes, speeds no.
pub fn skipped() -> NetFacts {
    NetFacts::default()
}

pub fn sections(_facts: &Facts) -> Vec<Section> {
    Vec::new()
}

pub fn advise(_facts: &Facts, _review: &mut Review) {}

/// Upload speed (Mbps) a stream of this bitrate needs, leaving headroom:
/// the stream should use about two thirds of the upload at most.
pub fn upload_needed_mbps(bitrate_kbps: u32) -> u32 {
    ((bitrate_kbps as f64 * 1.5) / 1000.0).ceil() as u32
}

/// The highest video bitrate (kbps) this upload can carry with the same
/// headroom, or `None` when the upload was not measured.
pub fn max_bitrate_for_upload(upload_mbps: Option<f64>) -> Option<u32> {
    upload_mbps.filter(|u| *u > 0.0).map(|u| (u * 1000.0 / 1.5).floor() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_needed_leaves_headroom() {
        assert_eq!(upload_needed_mbps(3000), 5);
        assert_eq!(upload_needed_mbps(6000), 9);
    }

    #[test]
    fn bitrate_cap_follows_the_upload() {
        assert_eq!(max_bitrate_for_upload(Some(9.0)), Some(6000));
        assert_eq!(max_bitrate_for_upload(Some(3.0)), Some(2000));
        assert_eq!(max_bitrate_for_upload(Some(0.0)), None);
        assert_eq!(max_bitrate_for_upload(None), None);
    }
}
