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

/// VERA extension: the urban pass's lists for an opted-in Urban Presence on
/// a theater.
///
/// Structures are the theater's main civilian buildings: civilian types
/// (`CA`/`CT`/`CI`) that are not capturable and not neutral tech buildings,
/// of at least 2x2 and at most thirty cells, whose body art exists on this
/// theater (`has_file` answers for the names
/// [`crate::rules::art_data::building_body_shp_candidates`] tries) and whose
/// `Name=` reads as no wall, fence or street prop: authored maps set those
/// apart from the blocks they line. On a theater `urban_survey.ini` lists,
/// only the buildings authored maps of that theater put in their towns are
/// kept: the rest draw Westwood's placeholder cube or another theater's
/// style there. A survey of 2,576 authored towns found
/// each mostly of one style family (the name without its number), so the list
/// is a main family drawn from `seed` by family size, then a second one; the
/// main family leads, inside the eighteen entries the pass may reuse.
/// The foots are Ares' default `[Urban]` lists, kept where the Rules define
/// the type. The props are the theater's surveyed street furniture
/// (`[THEATER.PROPS]`) of one cell with body art; the trees are the kinds
/// authored towns of the theater use. The farms and landmarks of the
/// Countryside and Landmark Extras are the buildings `extras` names
/// (`RMGMD.INI [Extras] Farms=`, `Landmarks=`).
#[allow(clippy::too_many_arguments)]
pub fn resolve_vera_urban(
    rules: &crate::rules::ruleset::RuleSet,
    art: &IniFile,
    tech: &[TechType],
    extras: &crate::map::rmg::extras::ExtrasSettings,
    theater_ext: &str,
    theater_name: &str,
    seed: u16,
    has_file: impl Fn(&str) -> bool,
) -> crate::map::rmg::phases::urban_areas::UrbanCatalog {
    use crate::map::rmg::phases::urban_areas::{UrbanCatalog, UrbanFoot, UrbanStructure};
    const SET_APART: [&str; 18] = [
        "wall",
        "fence",
        "gate",
        "corner",
        "middle",
        "gum",
        "barricade",
        "hedge",
        "light",
        "lamp",
        "pole",
        "sign",
        "barrel",
        "dumpster",
        "mail",
        "booth",
        "flag",
        "bench",
    ];
    const INFANTRY: [&str; 6] = ["CIV1", "CIV2", "CIV3", "CIVA", "CIVB", "CIVC"];
    const VEHICLES: [&str; 8] = [
        "TRUCKA", "TRUCKB", "COP", "EUROC", "SUVW", "SUVB", "FTRK", "AMBU",
    ];

    let surveyed = surveyed_urban_names(theater_name);
    let surveyed_props = surveyed_urban_names(&format!("{theater_name}.PROPS")).unwrap_or_default();
    let mut props = Vec::new();
    let registry = rules.art();
    let mut families: Vec<(String, Vec<UrbanStructure>)> = Vec::new();
    for id in &rules.building_ids {
        let Some(object) = rules.object(id) else {
            continue;
        };
        let upper = id.to_ascii_uppercase();
        if surveyed_props.contains(&upper.as_str()) {
            // One-cell furniture; a letter after the number is a piece of a
            // composite landmark, as below.
            let footprint = footprint_for(art, id);
            let bytes = upper.as_bytes();
            let piece = bytes.len() >= 2
                && bytes[bytes.len() - 1].is_ascii_alphabetic()
                && bytes[bytes.len() - 2].is_ascii_digit();
            let image_id = registry.resolve_effective_image_id(id, &object.image);
            if footprint.len() == 1
                && !piece
                && crate::rules::art_data::building_body_shp_candidates(
                    Some(registry),
                    &object.image,
                    &image_id,
                    theater_ext,
                    theater_name,
                )
                .iter()
                .any(|file| has_file(file))
            {
                props.push(UrbanStructure {
                    name: id.clone(),
                    footprint: Some(footprint),
                });
                continue;
            }
        }
        if surveyed
            .as_ref()
            .is_some_and(|names| !names.contains(&upper.as_str()))
        {
            continue;
        }
        if !(upper.starts_with("CA") || upper.starts_with("CT") || upper.starts_with("CI"))
            || object.capturable
            || tech.iter().any(|tech| tech.name.eq_ignore_ascii_case(id))
        {
            continue;
        }
        let name = object
            .name
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if SET_APART.iter().any(|word| name.contains(word)) {
            continue;
        }
        let footprint = footprint_for(art, id);
        let (w, h) = footprint
            .iter()
            .fold((0, 0), |(w, h), &(dx, dy)| (w.max(dx + 1), h.max(dy + 1)));
        if w < 2 || h < 2 || footprint.len() > 30 {
            continue;
        }
        let image_id = registry.resolve_effective_image_id(id, &object.image);
        let drawn = crate::rules::art_data::building_body_shp_candidates(
            Some(registry),
            &object.image,
            &image_id,
            theater_ext,
            theater_name,
        )
        .iter()
        .any(|file| has_file(file));
        if !drawn {
            continue;
        }
        // A letter after the number marks one piece of a composite landmark
        // (`CASTL05A`..`H`), which only reads whole: leave the pieces out.
        let bytes = upper.as_bytes();
        let end = bytes.len();
        if end >= 2 && bytes[end - 1].is_ascii_alphabetic() && bytes[end - 2].is_ascii_digit() {
            continue;
        }
        // The family: the name without its number.
        let family = upper
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .to_string();
        let entry = UrbanStructure {
            name: id.clone(),
            footprint: Some(footprint),
        };
        match families.iter_mut().find(|(name, _)| *name == family) {
            Some((_, members)) => members.push(entry),
            None => families.push((family, vec![entry])),
        }
    }

    // A main family of at least three types, drawn by family size; then more
    // families, the same way, until the list holds a dozen types (authored
    // towns mix about three, the main one most of the time).
    const MAIN_FAMILY_TYPES: usize = 3;
    const LIST_TYPES: usize = 12;
    let mut rng = crate::map::rmg::RmgRng::new(seed ^ 0x5EED);
    let mut structures = Vec::new();
    let mut draw = |families: &mut Vec<(String, Vec<UrbanStructure>)>, min: usize| {
        let eligible: Vec<usize> = (0..families.len())
            .filter(|&index| families[index].1.len() >= min)
            .collect();
        let total: i32 = eligible
            .iter()
            .map(|&index| families[index].1.len() as i32)
            .sum();
        if total == 0 {
            return None;
        }
        let mut pick = rng.uniform(0, total - 1);
        for &index in &eligible {
            let size = families[index].1.len() as i32;
            if pick < size {
                return Some(families.remove(index).1);
            }
            pick -= size;
        }
        None
    };
    if let Some(main) = draw(&mut families, MAIN_FAMILY_TYPES).or_else(|| draw(&mut families, 1)) {
        structures.extend(main);
    }
    while structures.len() < LIST_TYPES {
        match draw(&mut families, 1) {
            Some(more) => structures.extend(more),
            None => break,
        }
    }
    let feet = |names: &[&str]| {
        names
            .iter()
            .filter(|name| rules.object(name).is_some())
            .map(|name| UrbanFoot {
                name: (*name).to_string(),
                known: true,
            })
            .collect()
    };
    // The farm buildings and monuments of the Countryside and Landmark
    // Extras: the named types the Rules define whose body art exists on this
    // theater, the test the town buildings above pass.
    let named = |names: &[String]| -> Vec<UrbanStructure> {
        names
            .iter()
            .filter_map(|name| {
                let id = rules
                    .building_ids
                    .iter()
                    .find(|id| id.eq_ignore_ascii_case(name))?;
                let object = rules.object(id)?;
                let image_id = registry.resolve_effective_image_id(id, &object.image);
                crate::rules::art_data::building_body_shp_candidates(
                    Some(registry),
                    &object.image,
                    &image_id,
                    theater_ext,
                    theater_name,
                )
                .iter()
                .any(|file| has_file(file))
                .then(|| UrbanStructure {
                    name: id.clone(),
                    footprint: Some(footprint_for(art, id)),
                })
            })
            .collect()
    };
    UrbanCatalog {
        structures,
        infantry: feet(&INFANTRY),
        vehicles: feet(&VEHICLES),
        props,
        trees: street_trees(theater_name),
        farms: named(&extras.farms),
        landmarks: named(&extras.landmarks),
    }
}

/// The `TREExx` terrain objects authored towns of a theater stand along
/// their streets (the same survey): temperate kinds, snow-laden ones on Snow
/// and palms and cacti on Desert.
fn street_trees(theater_name: &str) -> Vec<String> {
    let range = match theater_name.to_ascii_uppercase().as_str() {
        "SNOW" => 20..=28,
        "DESERT" => 31..=36,
        _ => 1..=12,
    };
    range.map(|index| format!("TREE{index:02}")).collect()
}

/// The `[THEATER]` row of `urban_survey.ini`: the civilian buildings
/// authored maps of that theater place in towns. `None` for a theater the
/// survey has no row for, which keeps every candidate.
fn surveyed_urban_names(theater_name: &str) -> Option<Vec<&'static str>> {
    const SURVEY: &str = include_str!("urban_survey.ini");
    let header = format!("[{}]", theater_name.to_ascii_uppercase());
    let mut lines = SURVEY.lines();
    lines.by_ref().find(|line| line.trim() == header)?;
    Some(
        lines
            .take_while(|line| !line.trim().is_empty() && !line.starts_with('['))
            .flat_map(str::split_whitespace)
            .collect(),
    )
}

/// Resolve the Ares `[Urban]` lists of `settings` against the processed Rules
/// type arrays and the art foundations: a structure that is no BuildingType
/// has no footprint, a foot that is no type is unknown.
#[cfg(feature = "ares")]
pub fn resolve_ares_urban(
    settings: &crate::map::rmg::RmgSettings,
    processed: &crate::rules::native_processing::ProcessedRulesLayers,
    art: &IniFile,
) -> crate::map::rmg::phases::urban_areas::UrbanCatalog {
    use crate::map::rmg::phases::urban_areas::{UrbanCatalog, UrbanFoot, UrbanStructure};
    use crate::rules::native_processing::UrbanTypeKind;
    let feet = |names: &[String], kind| {
        names
            .iter()
            .map(|name| UrbanFoot {
                name: name.clone(),
                known: processed.registers_urban_type(kind, name),
            })
            .collect()
    };
    UrbanCatalog {
        structures: settings
            .urban_structures
            .iter()
            .map(|name| UrbanStructure {
                name: name.clone(),
                footprint: processed
                    .registers_urban_type(UrbanTypeKind::Building, name)
                    .then(|| footprint_for(art, name)),
            })
            .collect(),
        infantry: feet(&settings.urban_infantry, UrbanTypeKind::Infantry),
        vehicles: feet(&settings.urban_vehicles, UrbanTypeKind::Vehicle),
        ..Default::default()
    }
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

    /// The list's comma-separated names are registered BuildingTypes, so
    /// Ares's Find-only read (Buf_AI 1005B830, hook 0x672B0E) resolves them.
    fn rules_with(list: &str) -> String {
        let types: String = list
            .split(',')
            .enumerate()
            .map(|(index, name)| format!("{index}={name}\n"))
            .collect();
        format!("[BuildingTypes]\n{types}[AI]\nNeutralTechBuildings={list}\n")
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
    /// written, so a spaced name misses its art section. Ares's Find-only
    /// read (Buf_AI 1005B830, hook 0x672B0E) allocates no spaced name.
    #[cfg(not(feature = "ares"))]
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
