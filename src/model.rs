//! The shared vocabulary. Every area of the doctor (PC, OBS, internet, Windows,
//! programs) reports what it found using these types, and both the console
//! and the HTML report are drawn from them. Nothing here knows about OBS.

/// How much a piece of advice matters. Sorted most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// The stream will visibly suffer until this is fixed.
    Important,
    /// A clear improvement, not an emergency.
    Recommended,
    /// Nice to know.
    Tip,
}

/// Which part of the setup an item belongs to. Used to group and label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    LastStream,
    Obs,
    Scenes,
    Internet,
    Pc,
    Windows,
    Programs,
}

impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Area::LastStream => "Tu último directo",
            Area::Obs => "OBS",
            Area::Scenes => "Escenas",
            Area::Internet => "Internet",
            Area::Pc => "Tu PC",
            Area::Windows => "Windows",
            Area::Programs => "Programas",
        }
    }
}

/// One thing the streamer should change, written for someone who has never
/// read a streaming guide: what to do, why it helps, and the exact clicks.
#[derive(Debug, Clone)]
pub struct Advice {
    /// Stable identifier, e.g. `"obs.encoder.use-hardware"`. Never shown.
    /// Tests assert on it, the HTML report remembers ticked items by it, and
    /// cross-area rules use it to replace a generic tip with a sharper one.
    pub id: &'static str,
    pub level: Level,
    pub area: Area,
    /// Imperative and concrete: "Baja la imagen a 720p a 30 fps".
    pub title: String,
    /// One or two plain sentences. No jargon, no acronyms without explanation.
    pub why: String,
    /// The clicks, in order, using the names the Spanish interface shows.
    pub steps: Vec<String>,
}

/// Colour hint for a fact shown in the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Good,
    Warn,
    Bad,
}

/// One "label: value" line of a section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub tone: Tone,
    /// Optional bar from 0.0 (empty) to 1.0 (full), e.g. memory in use.
    pub meter: Option<f32>,
    /// Optional small print under the value.
    pub note: Option<String>,
}

impl Row {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into(), ..Self::default() }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn meter(mut self, fraction: f32) -> Self {
        self.meter = Some(fraction.clamp(0.0, 1.0));
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// A block of facts: "Tu PC", "Tu OBS", "Tu internet"...
#[derive(Debug, Clone, Default)]
pub struct Section {
    /// Stable identifier, e.g. `"pc"`. Used for anchors and tests.
    pub id: &'static str,
    pub title: String,
    /// Optional sentence shown before the rows (or instead of them when there
    /// is nothing to list, e.g. "No encontré la configuración de OBS").
    pub intro: Option<String>,
    pub rows: Vec<Row>,
}

impl Section {
    pub fn new(id: &'static str, title: impl Into<String>) -> Self {
        Self { id, title: title.into(), intro: None, rows: Vec::new() }
    }

    pub fn intro(mut self, text: impl Into<String>) -> Self {
        self.intro = Some(text.into());
        self
    }

    pub fn row(mut self, row: Row) -> Self {
        self.rows.push(row);
        self
    }

    pub fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.intro.is_none() && self.rows.is_empty()
    }
}

/// Everything the rules concluded.
#[derive(Debug, Default)]
pub struct Review {
    pub advice: Vec<Advice>,
    /// Things that are already fine, as full sentences.
    pub good: Vec<String>,
    /// Things we could not check, so the streamer knows the report has a gap.
    pub notes: Vec<String>,
}

impl Review {
    /// Adds a piece of advice. A second item with the same id is ignored, so
    /// two areas can safely reach the same conclusion.
    pub fn add(&mut self, advice: Advice) {
        if !self.has(advice.id) {
            self.advice.push(advice);
        }
    }

    pub fn ok(&mut self, sentence: impl Into<String>) {
        self.good.push(sentence.into());
    }

    pub fn note(&mut self, sentence: impl Into<String>) {
        self.notes.push(sentence.into());
    }

    pub fn has(&self, id: &str) -> bool {
        self.advice.iter().any(|a| a.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&Advice> {
        self.advice.iter().find(|a| a.id == id)
    }

    /// Removes an item, e.g. when a sharper diagnosis replaces a generic tip.
    pub fn remove(&mut self, id: &str) -> Option<Advice> {
        let index = self.advice.iter().position(|a| a.id == id)?;
        Some(self.advice.remove(index))
    }

    /// Most important first; ties keep the order in which they were found.
    pub fn sort(&mut self) {
        self.advice.sort_by_key(|a| a.level);
    }

    pub fn count(&self, level: Level) -> usize {
        self.advice.iter().filter(|a| a.level == level).count()
    }

    /// Ids of all advice, handy in test failure messages.
    #[cfg(test)]
    pub fn ids(&self) -> Vec<&'static str> {
        self.advice.iter().map(|a| a.id).collect()
    }
}

/// What to do about a program that is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramKind {
    /// Closing (or pausing) it helps the stream.
    Close,
    /// Depends on whether the streamer needs it live.
    Review,
    /// A tool streamers use while live: recognised, leave it alone.
    Keep,
    /// Looks like the game being streamed.
    Game,
    /// Heavy, but we do not know what it is.
    Unknown,
}

/// A running program as shown in the report.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgramCard {
    pub name: String,
    pub kind: ProgramKind,
    /// Plain description of what it is using: "2,1 GB de memoria, 12 % del procesador".
    pub usage: String,
    /// What to do and why. Empty when there is nothing to say.
    pub tip: String,
}

/// The headline of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to change.
    Ready,
    /// Only recommendations and tips left.
    AlmostReady,
    /// At least one important item.
    NeedsWork,
}

impl Verdict {
    pub fn of(review: &Review) -> Self {
        if review.count(Level::Important) > 0 {
            Verdict::NeedsWork
        } else if review.advice.is_empty() {
            Verdict::Ready
        } else {
            Verdict::AlmostReady
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Verdict::Ready => "Estás listo para salir en directo",
            Verdict::AlmostReady => "Casi listo: quedan algunos ajustes",
            Verdict::NeedsWork => "Hay cosas importantes que arreglar antes de salir en directo",
        }
    }
}

/// Everything a report shows, already in display form. The console and the
/// HTML page are two drawings of this same value.
#[derive(Debug)]
pub struct ReportData {
    pub version: &'static str,
    /// Local date and time of the check, e.g. "18/11/2026 10:42".
    pub generated: String,
    pub verdict: Verdict,
    pub advice: Vec<Advice>,
    pub good: Vec<String>,
    pub notes: Vec<String>,
    pub sections: Vec<Section>,
    pub programs: Vec<ProgramCard>,
    /// "Con qué empezaría en esta PC".
    pub suggested: Section,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advice(id: &'static str, level: Level) -> Advice {
        Advice { id, level, area: Area::Obs, title: id.to_string(), why: String::new(), steps: vec![] }
    }

    #[test]
    fn duplicate_ids_are_ignored() {
        let mut review = Review::default();
        review.add(advice("a", Level::Tip));
        review.add(advice("a", Level::Important));
        assert_eq!(review.advice.len(), 1);
        assert_eq!(review.advice[0].level, Level::Tip);
    }

    #[test]
    fn sort_puts_important_first_and_keeps_order_within_a_level() {
        let mut review = Review::default();
        review.add(advice("tip", Level::Tip));
        review.add(advice("rec-1", Level::Recommended));
        review.add(advice("imp", Level::Important));
        review.add(advice("rec-2", Level::Recommended));
        review.sort();
        assert_eq!(review.ids(), vec!["imp", "rec-1", "rec-2", "tip"]);
    }

    #[test]
    fn remove_takes_the_item_out() {
        let mut review = Review::default();
        review.add(advice("a", Level::Tip));
        assert!(review.remove("a").is_some());
        assert!(review.remove("a").is_none());
        assert!(!review.has("a"));
    }

    #[test]
    fn verdict_follows_the_worst_item() {
        let mut review = Review::default();
        assert_eq!(Verdict::of(&review), Verdict::Ready);
        review.add(advice("t", Level::Tip));
        assert_eq!(Verdict::of(&review), Verdict::AlmostReady);
        review.add(advice("i", Level::Important));
        assert_eq!(Verdict::of(&review), Verdict::NeedsWork);
    }

    #[test]
    fn meter_is_clamped() {
        assert_eq!(Row::new("a", "b").meter(1.7).meter, Some(1.0));
        assert_eq!(Row::new("a", "b").meter(-0.2).meter, Some(0.0));
    }
}
