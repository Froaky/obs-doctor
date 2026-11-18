//! What the machine has: processor, memory, graphics cards, system.

pub mod tiers;

use sysinfo::System;

use crate::facts::Facts;
use crate::model::{Advice, Area, Level, Review, Row, Section, Tone};
use crate::report::text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct Gpu {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_bytes: u64,
}

impl Gpu {
    /// Integrated graphics share memory and power with the processor.
    /// Heuristic: there is no reliable flag for this, so we go by brand and name.
    pub fn is_integrated(&self) -> bool {
        let name = self.name.to_lowercase();
        match self.vendor {
            GpuVendor::Intel => !name.contains("arc"),
            GpuVendor::Amd => {
                name.ends_with("radeon graphics") || name.contains("radeon(tm) graphics") || name.contains("vega")
            }
            _ => false,
        }
    }

    /// Entry-level NVIDIA cards that ship without the hardware video encoder.
    pub fn nvenc_unlikely(&self) -> bool {
        let name = self.name.to_lowercase();
        self.vendor == GpuVendor::Nvidia
            && ["gt 1030", "gt 710", "gt 720", "gt 730", "gt 610", "gt 620", "gt 630", " mx"]
                .iter()
                .any(|m| name.contains(m))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Hardware {
    pub cpu_name: String,
    pub physical_cores: usize,
    pub ram_total: u64,
    pub ram_used: u64,
    pub gpus: Vec<Gpu>,
    pub os: String,
}

impl Hardware {
    pub fn ram_total_gb(&self) -> f64 {
        self.ram_total as f64 / GIB
    }

    pub fn ram_used_pct(&self) -> u32 {
        if self.ram_total == 0 {
            return 0;
        }
        (self.ram_used as f64 / self.ram_total as f64 * 100.0).round() as u32
    }

    /// The card OBS should lean on: a dedicated one if there is any.
    pub fn best_gpu(&self) -> Option<&Gpu> {
        self.gpus.iter().max_by_key(|g| {
            let rank = match (g.is_integrated(), g.vendor) {
                (false, GpuVendor::Nvidia) => 5,
                (false, GpuVendor::Amd) => 4,
                (false, GpuVendor::Intel) => 3,
                (true, GpuVendor::Intel) => 2,
                (true, _) => 1,
                (false, GpuVendor::Other) => 0,
            };
            (rank, g.vram_bytes)
        })
    }
}

pub const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub const RAM_COMFORTABLE_GB: f64 = 13.0; // a "16 GB" PC reports a bit less
pub const RAM_MINIMUM_GB: f64 = 5.5; // an "8 GB" PC with integrated graphics
const RAM_ALMOST_FULL_PCT: u32 = 85;

pub fn read(sys: &System) -> Hardware {
    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Procesador desconocido".to_string());
    let logical_cores = sys.cpus().len();

    Hardware {
        cpu_name,
        physical_cores: System::physical_core_count().unwrap_or(logical_cores),
        ram_total: sys.total_memory(),
        ram_used: sys.used_memory(),
        gpus: gpus(),
        os: System::long_os_version().unwrap_or_else(|_| "Sistema desconocido".to_string()),
    }
}

/// The "Tu PC" block of the report.
pub fn sections(facts: &Facts) -> Vec<Section> {
    let hw = &facts.hw;
    if hw.cpu_name.is_empty() && hw.ram_total == 0 {
        return Vec::new();
    }
    let mut section = Section::new("pc", "Tu PC");
    section.push(Row::new("Procesador", format!("{} ({} núcleos)", hw.cpu_name, hw.physical_cores)));

    let used = hw.ram_used_pct();
    let memory_tone = if used >= RAM_ALMOST_FULL_PCT {
        Tone::Bad
    } else if used >= 70 {
        Tone::Warn
    } else {
        Tone::Good
    };
    section.push(
        Row::new(
            "Memoria",
            format!("{} GB en total, usando {} GB ({} %)", text::gb(hw.ram_total), text::gb(hw.ram_used), used),
        )
        .tone(memory_tone)
        .meter(used as f32 / 100.0),
    );

    if hw.gpus.is_empty() {
        section.push(Row::new("Placa de video", "No encontré ninguna"));
    }
    for gpu in &hw.gpus {
        let kind = if gpu.is_integrated() {
            "integrada".to_string()
        } else {
            format!("dedicada, {} GB", text::gb(gpu.vram_bytes))
        };
        section.push(Row::new("Placa de video", format!("{} ({kind})", gpu.name)));
    }
    section.push(Row::new("Sistema", hw.os.clone()));
    vec![section]
}

pub fn advise(facts: &Facts, review: &mut Review) {
    let hw = &facts.hw;
    if hw.ram_total == 0 {
        return;
    }
    let used = hw.ram_used_pct();
    if used >= RAM_ALMOST_FULL_PCT {
        review.add(Advice {
            id: "pc.memory.almost-full",
            level: Level::Important,
            area: Area::Pc,
            title: "Libera memoria antes de salir en directo".to_string(),
            why: format!(
                "Tu memoria está al {used} %. Cuando se llena, Windows empieza a usar el disco como si fuera memoria y todo se vuelve lento: el juego, OBS y el directo."
            ),
            steps: vec![
                "Cierra los programas de la lista de programas abiertos, empezando por los que más memoria ocupan.".to_string(),
            ],
        });
    } else if hw.ram_total_gb() < RAM_MINIMUM_GB {
        review.add(Advice {
            id: "pc.memory.small",
            level: Level::Tip,
            area: Area::Pc,
            title: "Tu PC tiene poca memoria: cuídala".to_string(),
            why: "Con esta cantidad de memoria, juego + OBS + navegador entran justos. Cuanto menos tengas abierto, mejor va a salir el directo."
                .to_string(),
            steps: vec!["Antes de transmitir, cierra todo lo que no uses en el directo.".to_string()],
        });
    } else {
        review.ok(format!("Tienes memoria libre de sobra (usas el {used} %)."));
    }
}

#[cfg(windows)]
fn gpus() -> Vec<Gpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};

    let mut found: Vec<Gpu> = Vec::new();
    // SAFETY: plain DXGI enumeration; every call is checked and the COM
    // objects are released when the wrappers drop.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return found;
        };
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue; // Microsoft Basic Render Driver
            }
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]).trim().to_string();
            if found.iter().any(|g| g.name == name) {
                continue;
            }
            found.push(Gpu {
                name,
                vendor: vendor_from_id(desc.VendorId),
                vram_bytes: desc.DedicatedVideoMemory as u64,
            });
        }
    }
    found
}

#[cfg(not(windows))]
fn gpus() -> Vec<Gpu> {
    Vec::new() // TODO: Linux/macOS
}

#[cfg_attr(not(windows), allow(dead_code))]
fn vendor_from_id(id: u32) -> GpuVendor {
    match id {
        0x10DE => GpuVendor::Nvidia,
        0x1002 | 0x1022 => GpuVendor::Amd,
        0x8086 => GpuVendor::Intel,
        _ => GpuVendor::Other,
    }
}

/// Builders for tests in any module that needs a PC to reason about.
#[cfg(test)]
pub mod test_support {
    use super::*;

    pub fn gpu(name: &str, vendor: GpuVendor, vram_gb: u64) -> Gpu {
        Gpu { name: name.to_string(), vendor, vram_bytes: vram_gb * GIB as u64 }
    }

    pub fn pc(cores: usize, ram_gb: u64, gpus: Vec<Gpu>) -> Hardware {
        Hardware {
            cpu_name: "CPU de prueba".to_string(),
            physical_cores: cores,
            ram_total: ram_gb * GIB as u64,
            ram_used: ram_gb * GIB as u64 / 2,
            gpus,
            os: "Windows de prueba".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn integrated_cards_are_detected_by_name() {
        assert!(gpu("Intel(R) UHD Graphics 630", GpuVendor::Intel, 0).is_integrated());
        assert!(gpu("AMD Radeon(TM) Graphics", GpuVendor::Amd, 0).is_integrated());
        assert!(!gpu("Intel(R) Arc(TM) A750 Graphics", GpuVendor::Intel, 8).is_integrated());
        assert!(!gpu("AMD Radeon RX 6600", GpuVendor::Amd, 8).is_integrated());
        assert!(!gpu("NVIDIA GeForce GTX 1650", GpuVendor::Nvidia, 4).is_integrated());
    }

    #[test]
    fn dedicated_card_wins_over_integrated() {
        let hw = pc(
            6,
            16,
            vec![
                gpu("Intel(R) UHD Graphics 630", GpuVendor::Intel, 0),
                gpu("NVIDIA GeForce GTX 1650", GpuVendor::Nvidia, 4),
            ],
        );
        assert_eq!(hw.best_gpu().unwrap().vendor, GpuVendor::Nvidia);
    }

    #[test]
    fn entry_level_nvidia_has_no_encoder() {
        assert!(gpu("NVIDIA GeForce GT 1030", GpuVendor::Nvidia, 2).nvenc_unlikely());
        assert!(gpu("NVIDIA GeForce MX150", GpuVendor::Nvidia, 2).nvenc_unlikely());
        assert!(gpu("NVIDIA GeForce MX 250", GpuVendor::Nvidia, 2).nvenc_unlikely());
        assert!(!gpu("NVIDIA GeForce GTX 1050 Ti", GpuVendor::Nvidia, 4).nvenc_unlikely());
    }

    #[test]
    fn memory_almost_full_is_important() {
        let mut hw = pc(6, 16, vec![]);
        hw.ram_used = hw.ram_total / 100 * 92;
        let facts = Facts { hw, ..Facts::default() };
        let mut review = Review::default();
        advise(&facts, &mut review);
        assert_eq!(review.get("pc.memory.almost-full").map(|a| a.level), Some(Level::Important));
    }

    #[test]
    fn the_pc_section_lists_processor_memory_and_cards() {
        let facts =
            Facts { hw: pc(6, 16, vec![gpu("NVIDIA GeForce GTX 1650", GpuVendor::Nvidia, 4)]), ..Facts::default() };
        let sections = sections(&facts);
        assert_eq!(sections.len(), 1);
        let labels: Vec<&str> = sections[0].rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["Procesador", "Memoria", "Placa de video", "Sistema"]);
        assert_eq!(sections[0].rows[1].meter, Some(0.5));
    }

    #[test]
    fn an_unknown_pc_shows_no_section() {
        assert!(sections(&Facts::default()).is_empty());
    }
}
