//! Minimal INI reader for OBS config files (`basic.ini`, `user.ini`, `global.ini`).

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Ini {
    sections: HashMap<String, HashMap<String, String>>,
}

impl Ini {
    pub fn parse(text: &str) -> Self {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current = String::new();

        // OBS writes these files with a UTF-8 BOM.
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                current = name.trim().to_string();
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                sections
                    .entry(current.clone())
                    .or_default()
                    .insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        Self { sections }
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section)?.get(key).map(String::as_str)
    }

    pub fn get_u32(&self, section: &str, key: &str) -> Option<u32> {
        self.get(section, key)?.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sections_and_keys() {
        let ini = Ini::parse("\u{feff}[Video]\nBaseCX=1920\nBaseCY = 1080\n\n[Output]\nMode=Simple\n");
        assert_eq!(ini.get_u32("Video", "BaseCX"), Some(1920));
        assert_eq!(ini.get_u32("Video", "BaseCY"), Some(1080));
        assert_eq!(ini.get("Output", "Mode"), Some("Simple"));
    }

    #[test]
    fn missing_values_are_none() {
        let ini = Ini::parse("[Video]\nBaseCX=abc\n");
        assert_eq!(ini.get_u32("Video", "BaseCX"), None);
        assert_eq!(ini.get("Video", "Nope"), None);
        assert_eq!(ini.get("Nope", "BaseCX"), None);
    }

    #[test]
    fn keeps_equals_signs_inside_values() {
        let ini = Ini::parse("[A]\nk=a=b\n");
        assert_eq!(ini.get("A", "k"), Some("a=b"));
    }
}
