//! What each streaming platform accepts and recommends.
//!
//! CONTRACT: other areas call `bitrate_for` and `max_video_kbps`. Keep their
//! signatures and meaning; add more freely.
//!
//! The table starts from the list OBS 33 ships and enforces,
//! `plugins/rtmp-services/data/services.json` (format_version 5), because
//! that is what OBS itself applies when "Ignorar las recomendaciones..." is
//! off. Each row says where its numbers come from. Platforms that OBS 33 does
//! not list (Kick, TikTok) can only be reached through a custom server, so
//! for them the doctor cannot tell which platform it is; their rows are here
//! for when a newer OBS lists them by name.

/// One streaming platform, as far as the doctor needs to know it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    /// How the doctor names it to the streamer.
    pub name: &'static str,
    /// Lowercase fragments that identify it in the service name OBS stores
    /// in `service.json` ("Twitch", "YouTube - RTMPS", "Facebook Live"...).
    matches: &'static [&'static str],
    /// Highest video bitrate (kbps) the platform takes. `None` when no
    /// primary source states one.
    pub max_video_kbps: Option<u32>,
    /// Highest audio bitrate (kbps). `None` when unknown.
    pub max_audio_kbps: Option<u32>,
    /// Longest keyframe interval (seconds) the platform accepts.
    pub max_keyint_sec: Option<u32>,
    /// Per picture size and frame rate limits, in the order OBS checks them:
    /// `(width, height, fps, max kbps)`. Empty when the platform has one
    /// limit for everything.
    pub bitrate_matrix: &'static [(u32, u32, u32, u32)],
    /// True when OBS 33 lists it in Ajustes > Emisión, so OBS applies its
    /// limits by itself unless told to ignore them.
    pub listed_in_obs: bool,
}

/// Every platform the doctor knows.
pub const PLATFORMS: &[Platform] = &[
    // services.json: "Twitch", keyint 2, max video bitrate 6000, max audio
    // bitrate 320. Twitch's own guide (https://help.twitch.tv/s/article/broadcasting-guidelines)
    // could not be loaded by a plain HTTP client to cross-check, so the
    // numbers OBS enforces are the ones used.
    Platform {
        name: "Twitch",
        matches: &["twitch"],
        max_video_kbps: Some(6000),
        max_audio_kbps: Some(320),
        max_keyint_sec: Some(2),
        bitrate_matrix: &[],
        listed_in_obs: true,
    },
    // services.json: "YouTube - RTMPS" (alt names "YouTube / YouTube Gaming",
    // "YouTube - RTMP") and "YouTube - HLS": keyint 2, max video 51000, max
    // audio 160. https://support.google.com/youtube/answer/2853702 asks for a
    // keyframe every 2 seconds and "no more than 4", and 128 kbps stereo audio.
    Platform {
        name: "YouTube",
        matches: &["youtube"],
        max_video_kbps: Some(51000),
        max_audio_kbps: Some(160),
        max_keyint_sec: Some(4),
        bitrate_matrix: &[],
        listed_in_obs: true,
    },
    // services.json: "Facebook Live": keyint 2, max fps 60, max video 9000,
    // max audio 128, and this bitrate matrix. Meta's page
    // (https://www.facebook.com/business/help/162540111070395) needs a
    // browser, so the matrix OBS enforces is the source.
    Platform {
        name: "Facebook",
        matches: &["facebook"],
        max_video_kbps: Some(9000),
        max_audio_kbps: Some(128),
        max_keyint_sec: Some(2),
        bitrate_matrix: &[
            (640, 360, 30, 1000),
            (640, 360, 60, 1500),
            (852, 480, 30, 2000),
            (852, 480, 60, 3000),
            (1280, 720, 30, 4000),
            (1280, 720, 60, 6000),
            (1920, 1080, 30, 6000),
            (1920, 1080, 60, 9000),
        ],
        listed_in_obs: true,
    },
    // services.json: "Trovo" (alt name "Madcat"): keyint 2, max video 9000,
    // max audio 160.
    Platform {
        name: "Trovo",
        matches: &["trovo", "madcat"],
        max_video_kbps: Some(9000),
        max_audio_kbps: Some(160),
        max_keyint_sec: Some(2),
        bitrate_matrix: &[],
        listed_in_obs: true,
    },
    // Not in OBS 33's list. https://help.kick.com/en/articles/7066931-how-to-stream-on-kick-com:
    // CBR between 1000 and 8000 kbps, keyframe every 2 seconds, up to 1080p60.
    Platform {
        name: "Kick",
        matches: &["kick"],
        max_video_kbps: Some(8000),
        max_audio_kbps: None,
        max_keyint_sec: Some(2),
        bitrate_matrix: &[],
        listed_in_obs: false,
    },
    // Not in OBS 33's list, and TikTok publishes no limits that a plain HTTP
    // client can read (https://www.tiktok.com/live/creators/en-US/article/live-studio-stream-key),
    // so nothing is claimed about it.
    Platform {
        name: "TikTok",
        matches: &["tiktok"],
        max_video_kbps: None,
        max_audio_kbps: None,
        max_keyint_sec: None,
        bitrate_matrix: &[],
        listed_in_obs: false,
    },
];

/// The platform behind a service name as OBS stores it. `None` for custom
/// servers, unknown platforms and no service at all.
pub fn find(service: Option<&str>) -> Option<&'static Platform> {
    let service = service?.to_lowercase();
    PLATFORMS.iter().find(|p| p.matches.iter().any(|m| service.contains(m)))
}

impl Platform {
    /// The video bitrate limit for this picture, the way OBS works it out
    /// (rtmp-common.c, `get_bitrate_matrix_max`): the first matrix row with
    /// the same size and a frame rate at least as high, else the general
    /// maximum.
    pub fn max_video_kbps_for(&self, picture: Option<(u32, u32)>, fps: f32) -> Option<u32> {
        let from_matrix = picture.and_then(|(w, h)| {
            self.bitrate_matrix
                .iter()
                .find(|(mw, mh, mfps, _)| *mw == w && *mh == h && f64::from(fps) <= f64::from(*mfps) + 0.0000001)
                .map(|(_, _, _, kbps)| *kbps)
        });
        from_matrix.or(self.max_video_kbps)
    }
}

/// Bitrate (kbps) that suits a picture of this height and frame rate when we
/// know nothing about the platform.
pub fn bitrate_for(height: u32, fps: f32) -> u32 {
    let smooth = fps > 40.0;
    match height {
        0..=480 => 1500,
        481..=720 => {
            if smooth {
                4500
            } else {
                3000
            }
        }
        _ => {
            if smooth {
                6000
            } else {
                4500
            }
        }
    }
}

/// The highest video bitrate (kbps) the platform accepts or recommends, given
/// the service name OBS stores (e.g. "Twitch", "YouTube - RTMPS").
/// `None` for unknown platforms and custom servers.
// Part of the CONTRACT for other areas; this area itself goes through
// `ObsConfig::platform_max_kbps`, which also knows the picture size.
#[cfg_attr(not(test), allow(dead_code))]
pub fn max_video_kbps(service: Option<&str>) -> Option<u32> {
    find(service)?.max_video_kbps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_grows_with_the_picture() {
        assert_eq!(bitrate_for(480, 30.0), 1500);
        assert_eq!(bitrate_for(720, 30.0), 3000);
        assert_eq!(bitrate_for(720, 60.0), 4500);
        assert_eq!(bitrate_for(1080, 30.0), 4500);
        assert_eq!(bitrate_for(1080, 59.94), 6000);
    }

    #[test]
    fn only_known_platforms_have_a_maximum() {
        assert_eq!(max_video_kbps(Some("Twitch")), Some(6000));
        assert_eq!(max_video_kbps(Some("Mi servidor")), None);
        assert_eq!(max_video_kbps(None), None);
    }

    #[test]
    fn service_names_obs_stores_are_recognised() {
        assert_eq!(find(Some("YouTube - RTMPS")).map(|p| p.name), Some("YouTube"));
        assert_eq!(find(Some("YouTube / YouTube Gaming")).map(|p| p.name), Some("YouTube"));
        assert_eq!(find(Some("Facebook Live")).map(|p| p.name), Some("Facebook"));
        assert_eq!(find(Some("Madcat")).map(|p| p.name), Some("Trovo"));
        assert_eq!(max_video_kbps(Some("YouTube - HLS")), Some(51000));
        assert_eq!(max_video_kbps(Some("TikTok")), None);
    }

    #[test]
    fn facebook_limits_follow_the_picture_like_obs_does() {
        let fb = find(Some("Facebook Live")).expect("facebook");
        assert_eq!(fb.max_video_kbps_for(Some((1280, 720)), 30.0), Some(4000));
        assert_eq!(fb.max_video_kbps_for(Some((1280, 720)), 60.0), Some(6000));
        assert_eq!(fb.max_video_kbps_for(Some((1920, 1080)), 59.94), Some(9000));
        // A size that is not in the matrix falls back to the general limit.
        assert_eq!(fb.max_video_kbps_for(Some((1600, 900)), 30.0), Some(9000));
        assert_eq!(fb.max_video_kbps_for(None, 30.0), Some(9000));
    }

    #[test]
    fn the_table_has_no_duplicates_and_sane_values() {
        for (i, a) in PLATFORMS.iter().enumerate() {
            for b in &PLATFORMS[i + 1..] {
                assert_ne!(a.name, b.name);
                for m in a.matches {
                    assert!(!b.matches.contains(m), "{m} matches two platforms");
                }
            }
            if let Some(max) = a.max_video_kbps {
                assert!((1000..=60000).contains(&max), "{}", a.name);
            }
            assert!(a.bitrate_matrix.iter().all(|r| Some(r.3) <= a.max_video_kbps), "{}", a.name);
        }
    }
}
