//! How capable the processor and the graphics card are, judged from their names.
//!
//! Everything here is a heuristic over marketing names: Windows reports no
//! "laptop" flag for a processor and no "has an encoder" flag for a card. The
//! tables below are tested against a long list of real names, and the OBS
//! logs (`facts.logs.available_encoders`) always win over what we guess here.

use super::{Gpu, GpuVendor};

// ---------------------------------------------------------------------------
// Processor
// ---------------------------------------------------------------------------

/// Product line of a processor, as far as streaming cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CpuFamily {
    /// Core i3, i5, i7, i9.
    IntelCore,
    /// Core Ultra 5, 7, 9 (2023 onwards).
    IntelCoreUltra,
    /// Core 3, 5, 7 without "Ultra" and without the "i" (2024 onwards).
    IntelCoreSeries,
    /// Core m3, m5, m7: fanless tablet chips.
    IntelCoreM,
    /// Celeron, Pentium, Atom, the "Intel N100" line and Core 2.
    IntelEntry,
    Xeon,
    Ryzen,
    /// Ryzen AI laptops (2024 onwards).
    RyzenAi,
    Threadripper,
    /// Athlon, Sempron and the A and E series.
    AmdEntry,
    AmdFx,
    #[default]
    Unknown,
}

/// What kind of machine the processor was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CpuClass {
    /// Thin laptops, mini PCs and tablets (Intel U, Y, P, V, N; AMD U).
    /// They slow down when they get hot, which a long stream does.
    LowPowerLaptop,
    /// Gaming laptops (Intel H, HX; AMD H, HS, HX).
    PerformanceLaptop,
    Desktop,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuInfo {
    pub family: CpuFamily,
    /// 3, 5, 7 or 9 in "Core i7", "Core Ultra 7" or "Ryzen 7".
    pub tier: Option<u8>,
    /// Intel Core: generation 1 to 14. Core Ultra and Core 3/5/7: series 1
    /// or 2. Ryzen: the thousands digit of the model (3 in "Ryzen 5 3600").
    pub generation: Option<u32>,
    pub class: CpuClass,
}

impl CpuInfo {
    /// Chips made for price or battery, not for gaming while streaming.
    pub fn is_entry_level(&self) -> bool {
        match self.family {
            CpuFamily::IntelEntry | CpuFamily::IntelCoreM | CpuFamily::AmdEntry => true,
            // Ivy Bridge and older (2012): no AVX2, which x264 leans on.
            CpuFamily::IntelCore => self.generation.is_some_and(|g| g <= 3),
            _ => false,
        }
    }

    /// How the report describes the machine, when we can tell.
    pub fn class_label(&self) -> Option<&'static str> {
        match self.class {
            CpuClass::LowPowerLaptop => Some("de notebook de bajo consumo"),
            CpuClass::PerformanceLaptop => Some("de notebook para juegos"),
            CpuClass::Desktop => Some("de escritorio"),
            CpuClass::Unknown => None,
        }
    }
}

/// Classifies a processor by the name Windows reports, e.g.
/// "13th Gen Intel(R) Core(TM) i7-1355U".
pub fn classify_cpu(name: &str) -> CpuInfo {
    let clean = normalize(name);
    let words: Vec<&str> =
        clean.split(|c: char| c.is_whitespace() || c == '@' || c == ',').filter(|w| !w.is_empty()).collect();

    if clean.contains("intel") || words.iter().any(|w| w.starts_with("i3-") || w.starts_with("i5-")) {
        classify_intel(&clean, &words)
    } else if clean.contains("amd") || clean.contains("ryzen") || clean.contains("athlon") {
        classify_amd(&clean, &words)
    } else {
        CpuInfo::default()
    }
}

fn normalize(name: &str) -> String {
    name.to_lowercase().replace("(r)", "").replace("(tm)", " ").replace(['®', '™'], " ")
}

fn classify_intel(clean: &str, words: &[&str]) -> CpuInfo {
    if clean.contains("xeon") {
        return CpuInfo { family: CpuFamily::Xeon, class: CpuClass::Desktop, ..CpuInfo::default() };
    }
    if clean.contains("core2") || clean.contains("core 2 ") {
        return CpuInfo { family: CpuFamily::IntelEntry, ..CpuInfo::default() };
    }
    if clean.contains("celeron") || clean.contains("pentium") || clean.contains("atom") {
        let class = if clean.contains("atom") {
            CpuClass::LowPowerLaptop
        } else {
            words
                .iter()
                .filter_map(|w| model(w))
                .map(|m| entry_class(&m))
                .find(|c| *c != CpuClass::Unknown)
                .unwrap_or_default()
        };
        return CpuInfo { family: CpuFamily::IntelEntry, class, ..CpuInfo::default() };
    }

    // "Core i7-1355U", "Core i3-N305", "Core m3-8100Y".
    for word in words {
        let Some((kind, rest)) = word.split_once('-') else { continue };
        let Some(m) = model(rest) else { continue };
        match kind {
            "i3" | "i5" | "i7" | "i9" => {
                let tier = kind[1..].parse().ok();
                if m.prefix == "n" {
                    // Alder Lake-N ("i3-N305"): an entry chip with a Core name.
                    return CpuInfo {
                        family: CpuFamily::IntelEntry,
                        tier,
                        class: CpuClass::LowPowerLaptop,
                        ..CpuInfo::default()
                    };
                }
                return CpuInfo {
                    family: CpuFamily::IntelCore,
                    tier,
                    generation: intel_core_generation(m.digits),
                    class: intel_suffix_class(m.suffix),
                };
            }
            "m3" | "m5" | "m7" => {
                return CpuInfo {
                    family: CpuFamily::IntelCoreM,
                    tier: kind[1..].parse().ok(),
                    class: CpuClass::LowPowerLaptop,
                    ..CpuInfo::default()
                };
            }
            _ => {}
        }
    }

    // "Core Ultra 7 155H", "Core 5 120U": tier digit, then a 3-digit model.
    let core_at = words.iter().position(|w| *w == "core");
    if let Some(at) = core_at {
        let ultra = words.get(at + 1) == Some(&"ultra");
        let tier_at = if ultra { at + 2 } else { at + 1 };
        let tier: Option<u8> = words.get(tier_at).and_then(|w| w.parse().ok()).filter(|t| [3, 5, 7, 9].contains(t));
        let m = words.get(tier_at + 1).and_then(|w| model(w));
        if let (Some(tier), Some(m)) = (tier, m) {
            return CpuInfo {
                family: if ultra { CpuFamily::IntelCoreUltra } else { CpuFamily::IntelCoreSeries },
                tier: Some(tier),
                generation: m.digits.chars().next().and_then(|c| c.to_digit(10)),
                class: intel_suffix_class(m.suffix),
            };
        }
    }

    // "Intel(R) N100", "Intel(R) Processor N200".
    if words.iter().filter_map(|w| model(w)).any(|m| m.prefix == "n" && m.digits.len() == 3) {
        return CpuInfo { family: CpuFamily::IntelEntry, class: CpuClass::LowPowerLaptop, ..CpuInfo::default() };
    }
    CpuInfo::default()
}

/// Generation of a Core i model number: "1355" is 13th, "12400" is 12th,
/// "8750" is 8th, "920" is 1st (first generation models had three digits).
fn intel_core_generation(digits: &str) -> Option<u32> {
    let first_two = || digits.get(..2).and_then(|d| d.parse().ok());
    match digits.len() {
        5 => first_two(),
        4 if digits.starts_with('1') => first_two(),
        4 => digits.get(..1).and_then(|d| d.parse().ok()),
        3 => Some(1),
        _ => None,
    }
}

/// Intel's letter after the model number says what the chip was made for.
/// https://www.intel.com/content/www/us/en/processors/processor-numbers.html
fn intel_suffix_class(suffix: &str) -> CpuClass {
    match suffix {
        // U, Y: 15 W and below. G1-G7: Ice Lake / Tiger Lake U parts.
        // P: 28 W thin and light. V: Lunar Lake. M: 2nd-4th gen dual-core laptops.
        "u" | "y" | "p" | "v" | "m" | "um" | "lm" => CpuClass::LowPowerLaptop,
        s if s.starts_with('g') && s.len() == 2 && s[1..].chars().all(|c| c.is_ascii_digit()) => {
            CpuClass::LowPowerLaptop
        }
        s if s.starts_with('h') || matches!(s, "mq" | "qm" | "mx" | "xm") => CpuClass::PerformanceLaptop,
        "" | "k" | "f" | "kf" | "ks" | "t" | "s" | "te" | "x" | "xe" | "r" | "c" | "e" | "fs" => CpuClass::Desktop,
        _ => CpuClass::Unknown,
    }
}

/// Celeron and Pentium: "N4020", "J4125", "G4560", "4415U", "Silver N5030".
fn entry_class(m: &Model) -> CpuClass {
    match (m.prefix, m.suffix) {
        ("n" | "j", _) | (_, "u" | "y") => CpuClass::LowPowerLaptop,
        ("g", _) => CpuClass::Desktop,
        _ => CpuClass::Unknown,
    }
}

fn classify_amd(clean: &str, words: &[&str]) -> CpuInfo {
    if clean.contains("threadripper") {
        return CpuInfo { family: CpuFamily::Threadripper, class: CpuClass::Desktop, ..CpuInfo::default() };
    }
    if let Some(at) = words.iter().position(|w| *w == "ryzen") {
        let after = &words[at + 1..];
        let tier: Option<u8> = after.iter().take(3).find_map(|w| w.parse().ok()).filter(|t| [3, 5, 7, 9].contains(t));
        if after.first() == Some(&"ai") {
            // Ryzen AI 300: 28 W chips unless "HX" or the big "Max" ones.
            let strong = after.iter().any(|w| *w == "hx" || w.starts_with("max"));
            let generation = after
                .iter()
                .filter_map(|w| model(w))
                .find(|m| m.digits.len() == 3)
                .and_then(|m| m.digits.chars().next().and_then(|c| c.to_digit(10)));
            return CpuInfo {
                family: CpuFamily::RyzenAi,
                tier,
                generation,
                class: if strong { CpuClass::PerformanceLaptop } else { CpuClass::LowPowerLaptop },
            };
        }
        if after.first().is_some_and(|w| w.starts_with('z')) {
            // Ryzen Z1: handheld consoles.
            return CpuInfo { family: CpuFamily::Ryzen, class: CpuClass::LowPowerLaptop, ..CpuInfo::default() };
        }
        let m = after.iter().filter_map(|w| model(w)).find(|m| m.prefix.is_empty() && m.digits.len() == 4);
        return CpuInfo {
            family: CpuFamily::Ryzen,
            tier,
            generation: m.as_ref().and_then(|m| m.digits.chars().next()).and_then(|c| c.to_digit(10)),
            class: m.map(|m| amd_suffix_class(m.suffix)).unwrap_or_default(),
        };
    }
    if words.iter().any(|w| w.starts_with("fx-") || *w == "fx") {
        return CpuInfo { family: CpuFamily::AmdFx, class: CpuClass::Desktop, ..CpuInfo::default() };
    }
    let entry = clean.contains("athlon")
        || clean.contains("sempron")
        || words.iter().any(|w| {
            w.split_once('-').is_some_and(|(k, _)| matches!(k, "a4" | "a6" | "a8" | "a9" | "a10" | "a12" | "e1" | "e2"))
        });
    if entry {
        let class = words
            .iter()
            .filter_map(|w| model(w.rsplit('-').next().unwrap_or(w)))
            .map(|m| match m.suffix {
                "u" => CpuClass::LowPowerLaptop,
                "g" | "ge" => CpuClass::Desktop,
                _ => CpuClass::Unknown,
            })
            .find(|c| *c != CpuClass::Unknown)
            .unwrap_or_default();
        return CpuInfo { family: CpuFamily::AmdEntry, class, ..CpuInfo::default() };
    }
    CpuInfo::default()
}

/// AMD's letter after the model number.
/// https://www.amd.com/en/products/processors/laptop/ryzen-for-business.html
/// (U: 15-28 W, HS: 35 W, H and HX: 45 W and up; desktop parts have none or X, G).
fn amd_suffix_class(suffix: &str) -> CpuClass {
    match suffix {
        "u" | "c" => CpuClass::LowPowerLaptop,
        s if s.starts_with('h') => CpuClass::PerformanceLaptop,
        "" | "x" | "xt" | "x3d" | "g" | "ge" | "f" | "e" | "gt" => CpuClass::Desktop,
        _ => CpuClass::Unknown,
    }
}

/// A model number split in three: "n4020" is ("n", "4020", ""), "1355u" is
/// ("", "1355", "u"), "1135g7" is ("", "1135", "g7").
struct Model<'a> {
    prefix: &'a str,
    digits: &'a str,
    suffix: &'a str,
}

fn model(word: &str) -> Option<Model<'_>> {
    let start = word.find(|c: char| c.is_ascii_digit())?;
    let prefix = &word[..start];
    if prefix.len() > 1 || !prefix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = &word[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let digits = &rest[..end];
    let suffix = &rest[end..];
    if digits.len() < 3 || !suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(Model { prefix, digits, suffix })
}

// ---------------------------------------------------------------------------
// Graphics card
// ---------------------------------------------------------------------------

/// The part of a graphics card that prepares the video for the stream, as
/// far as its name tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoEncoder {
    /// The card has none.
    Missing,
    /// It has one, but current drivers or OBS no longer use it.
    TooOld,
    /// It works, but at the same bitrate it looks worse than a modern one:
    /// better kept at 720p.
    Older,
    /// Good enough for 1080p at 60 frames per second on a dedicated card.
    Modern,
    /// A name we do not recognise.
    Unknown,
}

impl VideoEncoder {
    /// Worth suggesting when the OBS logs do not say either way.
    pub fn likely_usable(self) -> bool {
        matches!(self, VideoEncoder::Older | VideoEncoder::Modern | VideoEncoder::Unknown)
    }
}

/// What the card's encoder is like, judged from its name.
pub fn classify_gpu(gpu: &Gpu) -> VideoEncoder {
    let name = gpu.name.to_lowercase().replace("(r)", "").replace("(tm)", "");
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match gpu.vendor {
        GpuVendor::Nvidia => nvidia(&name),
        GpuVendor::Amd => amd(&name),
        GpuVendor::Intel => intel(&name),
        GpuVendor::Other => VideoEncoder::Missing,
    }
}

/// NVIDIA's encoder table:
/// https://developer.nvidia.com/video-encode-and-decode-gpu-support-matrix-new
/// - No encoder ("Total # of NVENC" 0): GT 1030, MX110/MX130, MX150 to MX350,
///   MX450 and MX570 A. MX550 and MX570 have one.
/// - Kepler (GT/GTX 600 and 700 except 745/750) is no longer in the table:
///   its last drivers are older than what OBS 31+ asks for its encoder.
/// - Maxwell and Pascal (GTX 750, 900, 1000), and the GTX 1650 GDDR5 and
///   1630 (Turing chip with the previous "6th Gen" encoder): older encoder.
/// - Turing "7th Gen" (GTX 1650 Super, 1660, RTX 20), Ampere (RTX 30),
///   Ada (RTX 40) and Blackwell (RTX 50): modern encoder. NVIDIA presents the
///   Turing encoder as matching x264 "medium":
///   https://www.nvidia.com/en-us/geforce/guides/broadcasting-guide/
fn nvidia(name: &str) -> VideoEncoder {
    if name.contains("mx570 a") {
        return VideoEncoder::Missing;
    }
    if let Some(n) = number_after(name, "mx") {
        return match n {
            550 => VideoEncoder::Older,
            570 => VideoEncoder::Modern,
            _ => VideoEncoder::Missing,
        };
    }
    if let Some(n) = number_after(name, "gt ") {
        return match n {
            1030 | 610..=799 => VideoEncoder::Missing,
            _ => VideoEncoder::Unknown,
        };
    }
    if name.contains("rtx") {
        return VideoEncoder::Modern;
    }
    if let Some(n) = number_after(name, "gtx ") {
        return match n {
            1650 if name.contains("super") => VideoEncoder::Modern,
            1660..=1699 => VideoEncoder::Modern,
            745 | 750 | 900..=1099 | 1630 | 1650 => VideoEncoder::Older,
            600..=799 => VideoEncoder::TooOld,
            _ => VideoEncoder::Unknown,
        };
    }
    // Workstation cards: Quadro K is Kepler, M and P are Maxwell and Pascal,
    // T is Turing (T400 to T1000 have the 7th Gen encoder).
    if name.contains("quadro") {
        return match name.split_whitespace().skip_while(|w| *w != "quadro").nth(1).and_then(|w| w.chars().next()) {
            Some('k') => VideoEncoder::TooOld,
            Some('m' | 'p') => VideoEncoder::Older,
            Some('t' | 'r') => VideoEncoder::Modern,
            _ => VideoEncoder::Unknown,
        };
    }
    if name.contains("titan") {
        return if name.contains("rtx") { VideoEncoder::Modern } else { VideoEncoder::Older };
    }
    VideoEncoder::Unknown
}

/// AMD's feature table:
/// https://github.com/GPUOpen-LibrariesAndSDKs/AMF/wiki/GPU-and-APU-HW-Features-and-Support
/// - RX 6300, 6400 and 6500 (Navi 24) are "Decode only": no encoder.
/// - RX 7000 and RX 9000 (VCN 4.0 and 5.0, the generation that added AV1) and
///   the Radeon 740M to 890M inside Ryzen 7040 and newer: modern encoder.
/// - Everything older has an encoder whose H.264 trails NVIDIA's and Intel's
///   at the same bitrate, so it is treated as an older one.
fn amd(name: &str) -> VideoEncoder {
    if let Some(n) = number_after(name, "rx ") {
        return match n {
            6300..=6599 => VideoEncoder::Missing,
            7000..=7999 | 9000..=9999 => VideoEncoder::Modern,
            _ => VideoEncoder::Older,
        };
    }
    if let Some(n) = number_after(name, "radeon ") {
        // Integrated "Radeon 780M": the hundreds say the generation.
        if (740..=899).contains(&n) && name.contains(&format!("{n}m")) {
            return VideoEncoder::Modern;
        }
    }
    VideoEncoder::Older
}

/// Intel's media capabilities:
/// https://www.intel.com/content/www/us/en/docs/onevpl/developer-reference-media-intel-hardware/1-1/overview.html
/// - 6th gen Core onwards (HD 5xx/6xx, UHD 6xx) has a fixed-function H.264
///   encoder; 5th gen and older only a shader-based one, which OBS's current
///   Quick Sync encoder does not rely on.
/// - Iris Xe, UHD 7xx and Arc: the newest generation (Arc adds AV1).
fn intel(name: &str) -> VideoEncoder {
    if name.contains("arc") || name.contains("iris xe") || name.contains("uhd graphics 7") {
        return VideoEncoder::Modern;
    }
    if name.contains("iris pro") {
        return VideoEncoder::TooOld;
    }
    if let Some(n) = number_after(name, "hd graphics ") {
        return match n {
            500..=699 => VideoEncoder::Older,
            1000..=6299 => VideoEncoder::TooOld,
            _ => VideoEncoder::Unknown,
        };
    }
    if name.contains("uhd graphics") || name.contains("iris plus") || name.contains("iris graphics") {
        return VideoEncoder::Older;
    }
    VideoEncoder::Unknown
}

/// The number right after `marker`: `number_after("geforce gtx 1060 6gb", "gtx ")`
/// is 1060. Accepts letters glued to the end ("mx150", "rx 6500 xt").
fn number_after(name: &str, marker: &str) -> Option<u32> {
    let at = name.find(marker)? + marker.len();
    let rest = name.get(at..)?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::test_support::gpu;

    use CpuClass::{Desktop as D, LowPowerLaptop as L, PerformanceLaptop as P, Unknown as U};
    use CpuFamily as F;

    /// Name, then the expected family, tier, generation and class.
    type CpuCase = (&'static str, CpuFamily, Option<u8>, Option<u32>, CpuClass);

    /// Names as Windows reports them on real machines.
    #[test]
    fn processors_are_classified_by_name() {
        let cases: &[CpuCase] = &[
            ("13th Gen Intel(R) Core(TM) i7-1355U", F::IntelCore, Some(7), Some(13), L),
            ("12th Gen Intel(R) Core(TM) i5-12400F", F::IntelCore, Some(5), Some(12), D),
            ("12th Gen Intel(R) Core(TM) i5-1240P", F::IntelCore, Some(5), Some(12), L),
            ("12th Gen Intel(R) Core(TM) i5-12450H", F::IntelCore, Some(5), Some(12), P),
            ("11th Gen Intel(R) Core(TM) i5-1135G7 @ 2.40GHz", F::IntelCore, Some(5), Some(11), L),
            ("Intel(R) Core(TM) i5-1035G1 CPU @ 1.00GHz", F::IntelCore, Some(5), Some(10), L),
            ("Intel(R) Core(TM) i3-10100F CPU @ 3.60GHz", F::IntelCore, Some(3), Some(10), D),
            ("Intel(R) Core(TM) i9-14900K", F::IntelCore, Some(9), Some(14), D),
            ("Intel(R) Core(TM) i7-14650HX", F::IntelCore, Some(7), Some(14), P),
            ("Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz", F::IntelCore, Some(7), Some(8), P),
            ("Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz", F::IntelCore, Some(5), Some(8), L),
            ("Intel(R) Core(TM) i7-7700HQ CPU @ 2.80GHz", F::IntelCore, Some(7), Some(7), P),
            ("Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz", F::IntelCore, Some(7), Some(4), D),
            ("Intel(R) Core(TM) i7-4710MQ CPU @ 2.50GHz", F::IntelCore, Some(7), Some(4), P),
            ("Intel(R) Core(TM) i5-3470 CPU @ 3.20GHz", F::IntelCore, Some(5), Some(3), D),
            ("Intel(R) Core(TM) i5-3210M CPU @ 2.50GHz", F::IntelCore, Some(5), Some(3), L),
            ("Intel(R) Core(TM) i7 CPU 920 @ 2.67GHz", F::Unknown, None, None, U),
            ("Intel(R) Core(TM) i5-750 CPU", F::IntelCore, Some(5), Some(1), D),
            ("Intel(R) Core(TM) Ultra 7 155H", F::IntelCoreUltra, Some(7), Some(1), P),
            ("Intel(R) Core(TM) Ultra 7 258V", F::IntelCoreUltra, Some(7), Some(2), L),
            ("Intel(R) Core(TM) Ultra 9 285K", F::IntelCoreUltra, Some(9), Some(2), D),
            ("Intel(R) Core(TM) Ultra 5 125U", F::IntelCoreUltra, Some(5), Some(1), L),
            ("Intel(R) Core(TM) 5 120U", F::IntelCoreSeries, Some(5), Some(1), L),
            ("Intel(R) Core(TM) i3-N305", F::IntelEntry, Some(3), None, L),
            ("Intel(R) N100", F::IntelEntry, None, None, L),
            ("Intel(R) Celeron(R) N4020 CPU @ 1.10GHz", F::IntelEntry, None, None, L),
            ("Intel(R) Celeron(R) J4125 CPU @ 2.00GHz", F::IntelEntry, None, None, L),
            ("Intel(R) Pentium(R) Silver N5030 CPU @ 1.10GHz", F::IntelEntry, None, None, L),
            ("Intel(R) Pentium(R) CPU G4560 @ 3.50GHz", F::IntelEntry, None, None, D),
            ("Intel(R) Core(TM) m3-8100Y CPU @ 1.10GHz", F::IntelCoreM, Some(3), None, L),
            ("Intel(R) Core(TM)2 Duo CPU     E8400  @ 3.00GHz", F::IntelEntry, None, None, U),
            ("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz", F::Xeon, None, None, D),
            ("AMD Ryzen 5 3600 6-Core Processor", F::Ryzen, Some(5), Some(3), D),
            ("AMD Ryzen 7 5800H with Radeon Graphics", F::Ryzen, Some(7), Some(5), P),
            ("AMD Ryzen 7 7840HS w/ Radeon 780M Graphics", F::Ryzen, Some(7), Some(7), P),
            ("AMD Ryzen 5 7520U with Radeon Graphics", F::Ryzen, Some(5), Some(7), L),
            ("AMD Ryzen 5 PRO 4650U with Radeon Graphics", F::Ryzen, Some(5), Some(4), L),
            ("AMD Ryzen 5 5600G with Radeon Graphics", F::Ryzen, Some(5), Some(5), D),
            ("AMD Ryzen 9 7950X3D 16-Core Processor", F::Ryzen, Some(9), Some(7), D),
            ("AMD Ryzen 7 5700X 8-Core Processor", F::Ryzen, Some(7), Some(5), D),
            ("AMD Ryzen 9 6900HX with Radeon Graphics", F::Ryzen, Some(9), Some(6), P),
            ("AMD Ryzen Z1 Extreme", F::Ryzen, None, None, L),
            ("AMD Ryzen AI 9 HX 370 w/ Radeon 890M", F::RyzenAi, Some(9), Some(3), P),
            ("AMD Ryzen AI 7 350 w/ Radeon 860M", F::RyzenAi, Some(7), Some(3), L),
            ("AMD Ryzen AI Max+ 395 w/ Radeon 8060S", F::RyzenAi, None, Some(3), P),
            ("AMD Ryzen Threadripper 3960X 24-Core Processor", F::Threadripper, None, None, D),
            ("AMD Athlon Silver 3050U with Radeon Graphics", F::AmdEntry, None, None, L),
            ("AMD Athlon 3000G with Radeon Vega Graphics", F::AmdEntry, None, None, D),
            ("AMD A9-9425 RADEON R5, 5 COMPUTE CORES 2C+3G", F::AmdEntry, None, None, U),
            ("AMD FX(tm)-8350 Eight-Core Processor", F::AmdFx, None, None, D),
            ("Procesador desconocido", F::Unknown, None, None, U),
            ("", F::Unknown, None, None, U),
            ("Apple M2", F::Unknown, None, None, U),
        ];
        for (name, family, tier, generation, class) in cases {
            let got = classify_cpu(name);
            assert_eq!(
                (got.family, got.tier, got.generation, got.class),
                (*family, *tier, *generation, *class),
                "{name}"
            );
        }
    }

    #[test]
    fn entry_level_processors() {
        assert!(classify_cpu("Intel(R) Celeron(R) N4020 CPU @ 1.10GHz").is_entry_level());
        assert!(classify_cpu("Intel(R) Core(TM) i3-N305").is_entry_level());
        assert!(classify_cpu("AMD Athlon Silver 3050U with Radeon Graphics").is_entry_level());
        assert!(classify_cpu("Intel(R) Core(TM) i5-3470 CPU @ 3.20GHz").is_entry_level());
        assert!(!classify_cpu("Intel(R) Core(TM) i7-4790K CPU @ 4.00GHz").is_entry_level());
        assert!(!classify_cpu("13th Gen Intel(R) Core(TM) i7-1355U").is_entry_level());
        assert!(!classify_cpu("AMD Ryzen 5 3600 6-Core Processor").is_entry_level());
        assert!(!classify_cpu("garbage ### 12345").is_entry_level());
    }

    #[test]
    fn graphics_cards_are_classified_by_name() {
        use GpuVendor::{Amd, Intel, Nvidia, Other};
        use VideoEncoder::{Missing as No, Modern as New, Older as Old, TooOld as Gone, Unknown as Unk};
        let cases: &[(&str, GpuVendor, VideoEncoder)] = &[
            ("NVIDIA GeForce RTX 4090", Nvidia, New),
            ("NVIDIA GeForce RTX 3060 Laptop GPU", Nvidia, New),
            ("NVIDIA GeForce RTX 2060", Nvidia, New),
            ("NVIDIA GeForce RTX 5070", Nvidia, New),
            ("NVIDIA GeForce GTX 1660 SUPER", Nvidia, New),
            ("NVIDIA GeForce GTX 1660 Ti", Nvidia, New),
            ("NVIDIA GeForce GTX 1650 SUPER", Nvidia, New),
            ("NVIDIA GeForce GTX 1650", Nvidia, Old),
            ("NVIDIA GeForce GTX 1630", Nvidia, Old),
            ("NVIDIA GeForce GTX 1060 6GB", Nvidia, Old),
            ("NVIDIA GeForce GTX 1050 Ti", Nvidia, Old),
            ("NVIDIA GeForce GTX 970", Nvidia, Old),
            ("NVIDIA GeForce GTX 750 Ti", Nvidia, Old),
            ("NVIDIA GeForce GTX 760", Nvidia, Gone),
            ("NVIDIA GeForce GTX 660", Nvidia, Gone),
            ("NVIDIA GeForce GT 1030", Nvidia, No),
            ("NVIDIA GeForce GT 730", Nvidia, No),
            ("NVIDIA GeForce GT 710", Nvidia, No),
            ("NVIDIA GeForce MX150", Nvidia, No),
            ("NVIDIA GeForce MX250", Nvidia, No),
            ("NVIDIA GeForce MX 330", Nvidia, No),
            ("NVIDIA GeForce MX450", Nvidia, No),
            ("NVIDIA GeForce MX550", Nvidia, Old),
            ("NVIDIA GeForce MX570", Nvidia, New),
            ("NVIDIA GeForce MX570 A", Nvidia, No),
            ("NVIDIA RTX A2000", Nvidia, New),
            ("NVIDIA Quadro P2000", Nvidia, Old),
            ("NVIDIA Quadro K620", Nvidia, Gone),
            ("NVIDIA T600", Nvidia, Unk),
            ("AMD Radeon RX 7800 XT", Amd, New),
            ("AMD Radeon RX 9070 XT", Amd, New),
            ("AMD Radeon RX 6700 XT", Amd, Old),
            ("AMD Radeon RX 6600M", Amd, Old),
            ("AMD Radeon RX 6500 XT", Amd, No),
            ("AMD Radeon RX 6400", Amd, No),
            ("Radeon RX 580 Series", Amd, Old),
            ("AMD Radeon RX Vega 56", Amd, Old),
            ("AMD Radeon 780M Graphics", Amd, New),
            ("AMD Radeon 890M", Amd, New),
            ("AMD Radeon 610M", Amd, Old),
            ("AMD Radeon(TM) Graphics", Amd, Old),
            ("AMD Radeon(TM) Vega 8 Graphics", Amd, Old),
            ("Intel(R) Iris(R) Xe Graphics", Intel, New),
            ("Intel(R) UHD Graphics 770", Intel, New),
            ("Intel(R) Arc(TM) A750 Graphics", Intel, New),
            ("Intel(R) Arc(TM) Graphics", Intel, New),
            ("Intel(R) Arc(TM) 140V GPU (16GB)", Intel, New),
            ("Intel(R) UHD Graphics 630", Intel, Old),
            ("Intel(R) UHD Graphics 600", Intel, Old),
            ("Intel(R) UHD Graphics", Intel, Old),
            ("Intel(R) HD Graphics 520", Intel, Old),
            ("Intel(R) Iris(R) Plus Graphics", Intel, Old),
            ("Intel(R) HD Graphics 4600", Intel, Gone),
            ("Intel(R) HD Graphics 2500", Intel, Gone),
            ("Intel(R) HD Graphics", Intel, Unk),
            ("Parsec Virtual Display Adapter", Other, No),
            ("", Nvidia, Unk),
        ];
        for (name, vendor, expected) in cases {
            assert_eq!(classify_gpu(&gpu(name, *vendor, 0)), *expected, "{name}");
        }
    }

    #[test]
    fn model_numbers_split_safely() {
        assert!(model("").is_none());
        assert!(model("abc").is_none());
        assert!(model("12").is_none());
        assert!(model("ñ1234").is_none());
        let m = model("1135g7").expect("model");
        assert_eq!((m.prefix, m.digits, m.suffix), ("", "1135", "g7"));
        let m = model("n4020").expect("model");
        assert_eq!((m.prefix, m.digits, m.suffix), ("n", "4020", ""));
        assert_eq!(intel_core_generation("99"), None);
    }
}
