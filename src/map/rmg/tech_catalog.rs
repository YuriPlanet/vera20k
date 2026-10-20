//! Resolve the neutral tech buildings the generator may place.
//!
//! `[AI] NeutralTechBuildings` in `rules(md).ini` names the types; each type's
//! footprint comes from `Foundation=` in `art(md).ini`, resolved through the
//! fixed foundation-name table. Nothing here is hardcoded: an absent list
//! yields an empty catalog, and the placement phase then places nothing.

use crate::map::rmg::phases::tech_buildings::TechType;
use crate::rules::foundation::{FOUNDATION_TABLE, read_foundation};
use crate::rules::ini_parser::IniFile;

/// Build the catalog from the BuildingType IDs of `[AI]
/// NeutralTechBuildings=` and a merged art INI.
///
/// The IDs are those the ordered Rules processor's BuildingType list read
/// (`0x0067B550`: ReadString 0x80, `strtok`) kept
/// (`ProcessedRulesLayers::neutral_tech_buildings`) over the active
/// standalone YR sources: the stock YR list has six entries where base RA2
/// has four.
pub fn resolve(names: &[String], art: &IniFile) -> Vec<TechType> {
    names
        .iter()
        .map(|name| TechType {
            name: name.clone(),
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
    // The art Foundation read (`0x00474DA0`, 0x20 bytes).
    let id = read_foundation(art.section_or_empty(name), 0);
    let foundation = FOUNDATION_TABLE[usize::from(id)];
    let (width, height) = (foundation.width, foundation.height);
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for dy in 0..height {
        for dx in 0..width {
            cells.push((dx as i16, dy as i16));
        }
    }
    cells
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn stock_contract_catalog() -> Vec<TechType> {
    let rules = IniFile::from_str(crate::test_fixture::text(
        "tests/fixtures/ini/rmg_neutral_tech_rules_contract.ini",
    ));
    let art = IniFile::from_str(crate::test_fixture::text(
        "tests/fixtures/ini/rmg_neutral_tech_art_contract.ini",
    ));
    let processed = crate::rules::native_processing::RulesLayerStack::new(rules)
        .process()
        .expect("neutral tech rules fixture");
    resolve(processed.neutral_tech_buildings(), &art)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog of `rules` through the production reader.
    fn catalog(rules: &str, art: &IniFile) -> Vec<TechType> {
        let processed =
            crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(rules))
                .process()
                .unwrap();
        resolve(processed.neutral_tech_buildings(), art)
    }

    fn rules_with(list: &str) -> String {
        format!("[AI]\nNeutralTechBuildings={list}\n")
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

    /// A named type with no art entry, no `Foundation=`, or an unrecognised
    /// value still enters the catalog — at the foundation table's default.
    #[test]
    fn unresolved_foundations_fall_back_to_the_table_default() {
        let art = IniFile::from_str("[CANOKEY]\nHeight=5\n[CABAD]\nFoundation=7x7\n");
        let catalog = catalog(&rules_with("CAMISSING,CANOKEY,CABAD"), &art);
        assert_eq!(catalog.len(), 3);
        for entry in &catalog {
            assert_eq!(entry.footprint, [(0, 0)], "{} footprint", entry.name);
        }
    }

    /// `strtok` skips empty fields but the type list allocates each token as
    /// written, so a spaced name misses its art section.
    #[test]
    fn list_entries_keep_their_spaces_and_empty_fields_drop() {
        let art = IniFile::from_str("[CAOILD]\nFoundation=2x2\n[CAPOWR]\nFoundation=2x2\n");
        let catalog = catalog(&rules_with("CAOILD , ,,CAPOWR"), &art);
        let names: Vec<&str> = catalog.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["CAOILD ", " ", "CAPOWR"]);
        assert_eq!(catalog[0].footprint, [(0, 0)]);
        assert_eq!(catalog[2].footprint.len(), 4);
    }

    /// Each entry is the BuildingType its factory found: `none` is null and
    /// a token names the registered Type whatever its case.
    #[test]
    fn list_entries_are_the_types_the_factory_found() {
        let art = IniFile::from_str("[CAOILD]\nFoundation=2x2\n");
        let catalog = catalog(
            "[BuildingTypes]\n0=CAOILD\n[CAOILD]\nFoundation=2x2\n\
             [AI]\nNeutralTechBuildings=caoild,none\n",
            &art,
        );
        let names: Vec<&str> = catalog.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["CAOILD"]);
        assert_eq!(catalog[0].footprint.len(), 4);
    }

    /// A non-square foundation keeps its width/height orientation.
    #[test]
    fn rectangular_foundation_is_row_major_width_by_height() {
        let art = IniFile::from_str("[CAOUTP]\nFoundation=4x3\n");
        let catalog = catalog(&rules_with("CAOUTP"), &art);
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
        let actual = resolve(startup.neutral_tech_buildings(), owner.fixed_art());
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
