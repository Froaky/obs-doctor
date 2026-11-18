//! Everything the doctor found on this PC, before any judgement.

use crate::hardware::Hardware;
use crate::net::NetFacts;
use crate::obs::config::{ObsConfig, ObsStatus};
use crate::obs::logs::LogFacts;
use crate::obs::scenes::SceneFacts;
use crate::processes::Apps;
use crate::suggest::Suggested;
use crate::system::SystemFacts;

/// One value per area. Each area owns its own type; rules read across areas
/// through this struct. `Default` is "we found nothing", which every rule
/// must tolerate.
#[derive(Debug, Default)]
pub struct Facts {
    pub hw: Hardware,
    pub apps: Apps,
    pub obs: ObsStatus,
    pub logs: LogFacts,
    pub scenes: SceneFacts,
    pub system: SystemFacts,
    pub net: NetFacts,
    /// Filled last, from everything above.
    pub suggested: Suggested,
}

impl Facts {
    /// The active OBS profile, when OBS is installed and has one.
    pub fn config(&self) -> Option<&ObsConfig> {
        match &self.obs {
            ObsStatus::Found(config) => Some(config),
            _ => None,
        }
    }
}
