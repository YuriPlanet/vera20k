//! Ignored retail check of the BuildingType construction controls: every
//! retail building's `Buildup=` SHP bound in each theater through the
//! production rules load, as `BuildingTypeClass::ReadINI` computes it
//! (`0x004615C5` -> `0x0045F230`).

use std::path::PathBuf;

use super::load_rules_with_merged_ini;
use crate::assets::asset_manager::AssetManager;
use crate::map::theater;
use crate::rules::buildup_asset_catalog::{BuildupAssetCatalog, NO_BUILDUP, buildup_shp_names};
use crate::rules::object_type::ObjectCategory;

const THEATERS: [&str; 6] = ["TEMPERATE", "SNOW", "URBAN", "DESERT", "NEWURBAN", "LUNAR"];

/// Controls from the retail Buildup SHP headers: the tactical capture
/// ledger's types (Fight.MAP is NEWURBAN) and a type whose theater SHP only
/// the original theaters carry (the rest fall back to the generic one).
const PINNED: [(&str, &str, [i32; 3]); 10] = [
    ("NEWURBAN", "NACNST", [0, 31, 1]),
    ("NEWURBAN", "NAPOWR", [0, 26, 2]),
    ("NEWURBAN", "NAREFN", [0, 26, 2]),
    ("NEWURBAN", "NARADR", [0, 30, 1]),
    ("NEWURBAN", "YACNST", [0, 27, 1]),
    ("NEWURBAN", "YAPOWR", [0, 23, 2]),
    ("NEWURBAN", "YAREFN", [0, 17, 3]),
    ("NEWURBAN", "NAPSIS", [0, 24, 2]),
    ("TEMPERATE", "NATBNK", [0, 10, 5]),
    ("DESERT", "NATBNK", [0, 11, 4]),
];

#[test]
#[ignore = "requires the sealed retail RA2/YR install"]
fn retail_buildup_controls_bind_in_every_theater() {
    let retail_dir = PathBuf::from(
        std::env::var("RA2_DIR").expect("RA2_DIR must name the sealed retail RA2/YR install"),
    );
    let mut pinned = 0;
    for theater_name in THEATERS {
        let mut assets = AssetManager::new(&retail_dir).expect("open retail MIX archives");
        theater::load_theater(&mut assets, theater_name).expect("load retail theater");
        let (mut rules, _rules_ini, art_ini, _receipt) =
            load_rules_with_merged_ini(&assets, None, None).expect("load production rules");
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.merge_art_data(&art);
        let catalog = BuildupAssetCatalog::bind(&rules, &assets, theater_name);

        let mut unbound = Vec::new();
        for object in rules
            .all_objects()
            .filter(|object| object.category == ObjectCategory::Building)
        {
            let Some(buildup) = rules
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image)
                .and_then(|art| art.buildup.clone())
            else {
                continue;
            };
            if catalog.control(&object.id) == NO_BUILDUP {
                // Native Retrieve misses too: neither name is in the MIXes.
                let candidates = buildup_shp_names(&buildup, theater_name);
                assert!(
                    candidates.iter().all(|name| assets.get_ref(name).is_none()),
                    "{theater_name} {}: {candidates:?} exists but did not bind",
                    object.id
                );
                unbound.push(object.id.clone());
            }
        }
        // YACOMDMK is in no retail archive.
        assert_eq!(unbound, ["YACOMD"], "{theater_name}");

        for (_, type_id, control) in PINNED
            .iter()
            .filter(|(theater, ..)| *theater == theater_name)
        {
            assert_eq!(
                catalog.control(type_id),
                *control,
                "{theater_name} {type_id}"
            );
            pinned += 1;
        }
    }
    assert_eq!(pinned, PINNED.len());
}
