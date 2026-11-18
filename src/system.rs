//! Windows settings and state that affect a stream (power, game features,
//! graphics drivers...). Read-only.

use crate::facts::Facts;
use crate::model::{Review, Section};

/// What we read from Windows. `Default` means "could not read anything".
///
/// CONTRACT: other areas read the fields below. Keep their names, types and
/// meaning; add more fields freely.
#[derive(Debug, Clone, Default)]
pub struct SystemFacts {
    /// A laptop running on its battery right now. `None` when unknown or
    /// when the PC has no battery.
    pub on_battery: Option<bool>,
}

pub fn read() -> SystemFacts {
    SystemFacts::default()
}

pub fn sections(_facts: &Facts) -> Vec<Section> {
    Vec::new()
}

pub fn advise(_facts: &Facts, _review: &mut Review) {}
