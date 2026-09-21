//! Parser for the `[Briefing]` map section.
//!
//! Original RA2/YR maps often store mission text here as numbered lines. This
//! first-pass parser keeps the content in order so UI paths can display it
//! without committing to final campaign/CSF behavior yet.

use crate::rules::ini_parser::IniFile;

/// Ordered briefing text extracted from a map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BriefingSection {
    /// Ordered non-empty briefing lines.
    pub lines: Vec<String>,
}

impl BriefingSection {
    /// Returns the briefing as a single paragraph for simple UI display.
    #[cfg(test)]
    pub fn joined_text(&self) -> Option<String> {
        if self.lines.is_empty() {
            None
        } else {
            Some(self.lines.join(" "))
        }
    }
}

/// Parse the `[Briefing]` section from a map INI.
pub fn parse_briefing_section(ini: &IniFile) -> BriefingSection {
    let Some(section) = ini.section("Briefing") else {
        return BriefingSection::default();
    };

    let lines = section
        .keys()
        .filter_map(|key| section.get(key))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    BriefingSection { lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn parse_briefing_lines_in_order() {
        let ini = IniFile::from_str("[Briefing]\n1=Secure the base.\n2=Destroy the lab.\n");
        let briefing = parse_briefing_section(&ini);
        assert_eq!(
            briefing.lines,
            vec![
                "Secure the base.".to_string(),
                "Destroy the lab.".to_string()
            ]
        );
        assert_eq!(
            briefing.joined_text().as_deref(),
            Some("Secure the base. Destroy the lab.")
        );
    }

    #[test]
    fn missing_briefing_returns_default() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        let briefing = parse_briefing_section(&ini);
        assert!(briefing.lines.is_empty());
        assert_eq!(briefing.joined_text(), None);
    }
}
