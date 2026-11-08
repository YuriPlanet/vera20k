//! Ares `[MouseCursors]` custom cursor rows.
//!
//! Ares extends the compiled 86-row cursor table with named rows read from
//! RULES: `ModeI=858,10,5,-1,1,Center,Middle`. The seven fields are
//! `(start frame, frame count, rate, mini start, mini count, hotspot X,
//! hotspot Y)`; mods then point a TechnoType at one of these names through
//! `Cursor.Deploy=`, `Cursor.Spy=` and friends. MO 3.3.6 uses `ModeI` /
//! `ModeII` for its two-mode vehicles (Athena Cannon, Siren, ...).
//!
//! VERA's fixed cursor table is in `render::cursor_atlas`; this module owns
//! only the INI projection, so the renderer can build one extra animation
//! sequence per named row without hard-coding a mod's frame numbers.

use crate::rules::ini_parser::IniFile;

/// One named custom cursor row. `rate` is in native 16 ms timer units; zero
/// means a static cursor. `mini_*` are retained for completeness but VERA's
/// software cursor draws the main set only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MouseCursorRow {
    pub start_frame: u32,
    pub frame_count: u32,
    pub rate: u32,
    pub mini_start: i32,
    pub mini_count: i32,
    pub hotspot_x: String,
    pub hotspot_y: String,
}

/// Ordered named rows. The position in this vector is the `CursorId::Custom`
/// index, so a `Cursor.Deploy=` name and the built software-cursor sequence
/// stay in step across a match.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MouseCursorCatalog {
    rows: Vec<(String, MouseCursorRow)>,
}

impl MouseCursorCatalog {
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut rows = Vec::new();
        for (name, raw) in ini.section_or_empty("MouseCursors").raw_entries() {
            let fields: Vec<&str> = raw.split(',').map(str::trim).collect();
            if fields.len() < 7 {
                continue;
            }
            let number = |text: &str| text.parse::<i64>().ok();
            let (Some(start), Some(count), Some(rate)) =
                (number(fields[0]), number(fields[1]), number(fields[2]))
            else {
                continue;
            };
            if start < 0 || count <= 0 || rate < 0 {
                continue;
            }
            rows.push((
                name.to_owned(),
                MouseCursorRow {
                    start_frame: start as u32,
                    frame_count: count as u32,
                    rate: rate as u32,
                    mini_start: number(fields[3]).unwrap_or(-1) as i32,
                    mini_count: number(fields[4]).unwrap_or(1) as i32,
                    hotspot_x: fields[5].to_owned(),
                    hotspot_y: fields[6].to_owned(),
                },
            ));
        }
        Self { rows }
    }

    pub fn rows(&self) -> &[(String, MouseCursorRow)] {
        &self.rows
    }

    pub fn index_of(&self, name: &str) -> Option<u16> {
        self.rows
            .iter()
            .position(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .and_then(|index| u16::try_from(index).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mo_mode_rows_parse_in_source_order_and_resolve_case_insensitively() {
        let ini = IniFile::from_str(
            "[MouseCursors]\nModeI=858,10,5,-1,1,Center,Middle\n\
             ModeII=868,10,5,-1,1,Center,Middle\nStatic=1,1,0,-1,1,Left,Top\n",
        );
        let catalog = MouseCursorCatalog::from_ini(&ini);
        assert_eq!(catalog.rows().len(), 3);
        assert_eq!(catalog.index_of("ModeI"), Some(0));
        assert_eq!(catalog.index_of("modeii"), Some(1));
        assert_eq!(catalog.index_of("missing"), None);
        let mode_ii = &catalog.rows()[1].1;
        assert_eq!((mode_ii.start_frame, mode_ii.frame_count, mode_ii.rate), (868, 10, 5));
        assert_eq!(mode_ii.hotspot_x, "Center");
        assert_eq!(catalog.rows()[2].1.rate, 0);
    }
}
