//! A starting configuration for this PC: which encoder, picture size, frame
//! rate and bitrate we would begin with. Useful even before OBS is set up.

use crate::facts::Facts;
use crate::hardware::{GpuVendor, Hardware, RAM_COMFORTABLE_GB, RAM_MINIMUM_GB};
use crate::model::{Row, Section};
use crate::net;
use crate::obs::config::{EncoderKind, ObsStatus};
use crate::platforms;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggested {
    pub encoder: EncoderKind,
    /// How the option reads in OBS's encoder list, plus a hint in parentheses.
    pub encoder_label: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl Default for Suggested {
    /// The most conservative setup: what any PC that can run OBS can do.
    fn default() -> Self {
        Self {
            encoder: EncoderKind::Software,
            encoder_label: "x264 (lo hace el procesador)".to_string(),
            width: 854,
            height: 480,
            fps: 30,
            bitrate_kbps: platforms::bitrate_for(480, 30.0),
        }
    }
}

impl Suggested {
    /// Pixels per second: a rough measure of how much work the picture is.
    pub fn load(&self) -> u64 {
        self.width as u64 * self.height as u64 * self.fps as u64
    }
}

pub fn suggest(facts: &Facts) -> Suggested {
    from_hardware(&facts.hw)
}

fn from_hardware(hw: &Hardware) -> Suggested {
    let cores = hw.physical_cores;
    let ram = hw.ram_total_gb();
    let roomy = cores >= 6 && ram >= RAM_COMFORTABLE_GB;

    let hardware_encoder = hw.best_gpu().and_then(|gpu| {
        let (kind, label) = match gpu.vendor {
            GpuVendor::Nvidia if !gpu.nvenc_unlikely() => (EncoderKind::Nvidia, "NVENC, H.264 (tu placa NVIDIA)"),
            GpuVendor::Amd => (EncoderKind::Amd, "AMD, H.264 (tu placa Radeon)"),
            GpuVendor::Intel => (EncoderKind::Intel, "QSV, H.264 (tu gráfica Intel)"),
            _ => return None,
        };
        Some((kind, label, gpu.is_integrated()))
    });

    let (encoder, encoder_label, (width, height, fps)) = match hardware_encoder {
        // A dedicated card encodes almost for free.
        Some((kind, label, false)) => {
            let size = if roomy {
                (1920, 1080, 60)
            } else if cores >= 4 && ram >= RAM_MINIMUM_GB {
                (1280, 720, 60)
            } else {
                (1280, 720, 30)
            };
            (kind, label, size)
        }
        // Integrated graphics also have to draw the game.
        Some((kind, label, true)) => (kind, label, if roomy { (1280, 720, 60) } else { (1280, 720, 30) }),
        // No hardware encoder: the processor does everything.
        None => {
            let size = if cores >= 8 && ram >= RAM_COMFORTABLE_GB {
                (1280, 720, 60)
            } else if cores >= 4 {
                (1280, 720, 30)
            } else {
                (854, 480, 30)
            };
            (EncoderKind::Software, "x264 (lo hace el procesador)", size)
        }
    };

    Suggested {
        encoder,
        encoder_label: encoder_label.to_string(),
        width,
        height,
        fps,
        bitrate_kbps: platforms::bitrate_for(height, fps as f32),
    }
}

/// The "Con qué empezaría en esta PC" block of the report.
pub fn section(facts: &Facts) -> Section {
    let s = &facts.suggested;
    let closing = match facts.obs {
        ObsStatus::Found(_) => "Es un punto de partida, no una regla. Haz un directo de prueba y ajusta desde ahí.",
        _ => {
            "Es un punto de partida para cuando abras OBS por primera vez. Haz un directo de prueba y ajusta desde ahí."
        }
    };
    Section::new("suggested", "Con qué empezaría en esta PC")
        .intro(closing)
        .row(Row::new("Quién prepara el video", s.encoder_label.clone()))
        .row(Row::new("Imagen", format!("{}x{} a {} fps", s.width, s.height, s.fps)))
        .row(Row::new("Calidad de envío", format!("{} kbps", s.bitrate_kbps)))
        .row(Row::new(
            "Internet que necesitas",
            format!("al menos {} Mbps de subida", net::upload_needed_mbps(s.bitrate_kbps)),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::{gpu, pc};

    fn suggestion(hw: Hardware) -> Suggested {
        suggest(&Facts { hw, ..Facts::default() })
    }

    #[test]
    fn gaming_pc_gets_1080p60_on_the_card() {
        let s = suggestion(pc(6, 16, vec![gpu("NVIDIA GeForce GTX 1660", GpuVendor::Nvidia, 6)]));
        assert_eq!((s.encoder, s.width, s.height, s.fps, s.bitrate_kbps), (EncoderKind::Nvidia, 1920, 1080, 60, 6000));
    }

    #[test]
    fn laptop_with_integrated_graphics_gets_720p30() {
        let s = suggestion(pc(4, 8, vec![gpu("Intel(R) UHD Graphics 620", GpuVendor::Intel, 0)]));
        assert_eq!((s.encoder, s.height, s.fps, s.bitrate_kbps), (EncoderKind::Intel, 720, 30, 3000));
    }

    #[test]
    fn pc_without_any_card_falls_back_to_the_processor() {
        let s = suggestion(pc(2, 4, vec![]));
        assert_eq!((s.encoder, s.height, s.fps), (EncoderKind::Software, 480, 30));
    }

    #[test]
    fn nvidia_card_without_encoder_is_not_suggested() {
        let s = suggestion(pc(4, 8, vec![gpu("NVIDIA GeForce GT 1030", GpuVendor::Nvidia, 2)]));
        assert_eq!(s.encoder, EncoderKind::Software);
    }

    #[test]
    fn the_section_always_has_the_four_rows() {
        let facts = Facts::default();
        let section = section(&facts);
        assert_eq!(section.rows.len(), 4);
        assert!(section.intro.is_some());
    }
}
