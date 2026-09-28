//! Resolve the neutral tech buildings the generator may place.
//!
//! `[AI] NeutralTechBuildings` in `rules(md).ini` names the types; each type's
//! footprint comes from `Foundation=` in `art(md).ini`, resolved through the
//! fixed foundation-name table. Nothing here is hardcoded: an absent list
//! yields an empty catalog, and the placement phase then places nothing.

use crate::map::rmg::phases::tech_buildings::TechType;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::ini_parser::IniFile;

/// Section and key naming the neutral types the generator draws from.
const AI_SECTION: &str = "AI";
const NEUTRAL_TECH_BUILDINGS: &str = "NeutralTechBuildings";
/// Art key owning every building footprint.
const FOUNDATION: &str = "Foundation";

/// Build the catalog from a merged rules INI and a merged art INI.
///
/// Both are the active standalone YR sources (`rulesmd` and `artmd`):
/// the stock YR list has six entries where base RA2 has four.
pub fn resolve(rules: &IniFile, art: &IniFile) -> Vec<TechType> {
    let Some(names) = rules
        .section(AI_SECTION)
        .and_then(|section| section.get_list(NEUTRAL_TECH_BUILDINGS))
    else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| TechType {
            name: name.to_string(),
            footprint: footprint_for(art, name),
        })
        .collect()
}

/// The NW-anchor-relative foundation cells of `name`.
///
/// Row-major order: the placement gate requires *every* cell to pass and
/// consumes no RNG while walking them, so the walk order is not observable.
/// An absent section, absent key, or unrecognised value resolves through the
/// foundation table's default entry.
fn footprint_for(art: &IniFile, name: &str) -> Vec<(i16, i16)> {
    let value = art
        .section(name)
        .and_then(|section| section.get(FOUNDATION))
        .unwrap_or_default();
    let (width, height) = foundation_dimensions(value);
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for dy in 0..height {
        for dx in 0..width {
            cells.push((dx as i16, dy as i16));
        }
    }
    cells
}

#[cfg(test)]
pub(crate) fn stock_contract_catalog() -> Vec<TechType> {
    let rules = IniFile::from_str(include_str!(
        "../../../tests/fixtures/ini/rmg_neutral_tech_rules_contract.ini"
    ));
    let art = IniFile::from_str(include_str!(
        "../../../tests/fixtures/ini/rmg_neutral_tech_art_contract.ini"
    ));
    resolve(&rules, &art)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_with(list: &str) -> IniFile {
        IniFile::from_str(&format!("[AI]\nNeutralTechBuildings={list}\n"))
    }

    /// The verified stock YR list and footprints through a narrow INI fixture.
    #[test]
    fn stock_yr_list_resolves_every_type_with_its_art_footprint() {
        let catalog = stock_contract_catalog();

        let names: Vec<&str> = catalog.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["CAAIRP", "CATHOSP", "CAOILD", "CAOUTP", "CAMACH", "CAPOWR"]
        );
        // Foundation= for each, as artmd.ini declares it.
        let sizes: Vec<usize> = catalog.iter().map(|t| t.footprint.len()).collect();
        assert_eq!(sizes, [9, 24, 4, 12, 9, 4]);
        // CAOILD is 2x2: the four NW-relative cells, row-major.
        let oil = catalog
            .iter()
            .find(|t| t.name == "CAOILD")
            .expect("CAOILD in the stock list");
        assert_eq!(oil.footprint, [(0, 0), (1, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn absent_section_or_key_yields_an_empty_catalog() {
        let art = IniFile::from_str("[CAOILD]\nFoundation=2x2\n");
        assert!(resolve(&IniFile::from_str(""), &art).is_empty());
        assert!(resolve(&IniFile::from_str("[AI]\nFixtureOnly=1\n"), &art).is_empty());
    }

    #[test]
    fn empty_list_value_yields_an_empty_catalog() {
        let art = IniFile::from_str("[CAOILD]\nFoundation=2x2\n");
        assert!(resolve(&rules_with(""), &art).is_empty());
    }

    /// A named type with no art entry, no `Foundation=`, or an unrecognised
    /// value still enters the catalog — at the foundation table's default.
    #[test]
    fn unresolved_foundations_fall_back_to_the_table_default() {
        let art = IniFile::from_str("[CANOKEY]\nHeight=5\n[CABAD]\nFoundation=7x7\n");
        let catalog = resolve(&rules_with("CAMISSING,CANOKEY,CABAD"), &art);
        assert_eq!(catalog.len(), 3);
        for entry in &catalog {
            assert_eq!(entry.footprint, [(0, 0)], "{} footprint", entry.name);
        }
    }

    #[test]
    fn list_entries_are_trimmed_and_blanks_dropped() {
        let art = IniFile::from_str("[CAOILD]\nFoundation=2x2\n[CAPOWR]\nFoundation=2x2\n");
        let catalog = resolve(&rules_with(" CAOILD , ,CAPOWR "), &art);
        let names: Vec<&str> = catalog.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["CAOILD", "CAPOWR"]);
    }

    /// A non-square foundation keeps its width/height orientation.
    #[test]
    fn rectangular_foundation_is_row_major_width_by_height() {
        let art = IniFile::from_str("[CAOUTP]\nFoundation=4x3\n");
        let catalog = resolve(&rules_with("CAOUTP"), &art);
        assert_eq!(catalog[0].footprint.len(), 12);
        assert_eq!(catalog[0].footprint[0], (0, 0));
        assert_eq!(catalog[0].footprint[3], (3, 0));
        assert_eq!(catalog[0].footprint[4], (0, 1));
        assert_eq!(catalog[0].footprint[11], (3, 2));
    }
    #[test]
    fn stock_contract_catalog_matches_selected_retail_startup() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
        let startup = owner
            .startup_compatibility_projection()
            .expect("RMG shell projection");
        let actual = resolve(startup.ini(), owner.fixed_art());
        let expected = stock_contract_catalog();
        assert_eq!(
            actual
                .iter()
                .map(|entry| (&entry.name, &entry.footprint))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|entry| (&entry.name, &entry.footprint))
                .collect::<Vec<_>>(),
            "selected retail neutral-tech list and every footprint cell"
        );
    }
}
