//! What each streaming platform accepts and recommends.
//!
//! CONTRACT: other areas call `bitrate_for` and `max_video_kbps`. Keep their
//! signatures and meaning; add more freely.

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
pub fn max_video_kbps(service: Option<&str>) -> Option<u32> {
    let service = service?.to_lowercase();
    service.contains("twitch").then_some(6000)
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
}
