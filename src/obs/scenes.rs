//! The scene collection: which sources and filters the streamer has set up.

use super::ObsPaths;
use crate::facts::Facts;
use crate::model::{Review, Section};

#[derive(Debug, Clone, Default)]
pub struct SceneFacts {}

pub fn read(_paths: &ObsPaths) -> SceneFacts {
    SceneFacts::default()
}

pub fn sections(_facts: &Facts) -> Vec<Section> {
    Vec::new()
}

pub fn advise(_facts: &Facts, _review: &mut Review) {}
