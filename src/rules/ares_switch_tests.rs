//! The per-game Ares switch (`rules::ares_switch`) through the production
//! Rules owner: retail data plays as gamemd; a game's own Ares key or
//! `ares.mix` turns Ares on, and the routed paths with it.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};
use crate::rules::ruleset::RuleSet;

/// The retail install as it is. The retail fixtures ship `ares.mix` in a
/// build with Ares; the retail archives do not.
fn installed(assets: &AssetManager) -> NativeRulesProcessOwner {
    let owner = retail_rules_owner(assets);
    #[cfg(feature = "ares")]
    let owner = {
        assert!(!assets.ships_ares());
        owner.with_ares_mix(false)
    };
    owner
}

fn skirmish(owner: &mut NativeRulesProcessOwner, mode: Option<&IniFile>, map: &IniFile) -> RuleSet {
    owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(mode), map)
        .unwrap()
        .into_parts()
        .0
}

/// Every retail mode on a retail map, and a retail campaign map with its
/// rules overrides, hold no key that only an Ares reader reads.
#[test]
fn retail_modes_and_maps_keep_ares_off() {
    let Some((root, mut assets)) = retail_assets() else {
        return;
    };
    assets.register_neutral_archives().unwrap();
    crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
        &root,
        &mut assets,
        None,
    )
    .unwrap();
    let map =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, "XMP03T4.MAP", &assets)
            .unwrap();
    let roster = crate::rules::retail_sources::select_ini(&assets, "MPModesMD.ini").unwrap();
    let modes = crate::skirmish_modes::skirmish_modes_from_selected_ini(
        &assets,
        &roster.ini,
        &roster.source.source_archive,
    )
    .unwrap();
    let mut owner = installed(&assets);
    for mode in &modes {
        let mode_ini =
            crate::rules::retail_sources::select_mode_ini(&assets, Some(&mode.override_file))
                .unwrap();
        let rules = skirmish(&mut owner, mode_ini.as_ref(), &map.map.ini);
        assert!(!rules.ares(), "{}", mode.override_file);
    }
    let campaign = IniFile::from_bytes(assets.get_ref("ALL02UMD.MAP").unwrap()).unwrap();
    let early = assets
        .get_ref("ALL02UMD.INI")
        .map(IniFile::from_bytes)
        .transpose()
        .unwrap();
    let rules = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(early.as_ref()),
            &campaign,
        )
        .unwrap()
        .into_parts()
        .0;
    assert!(!rules.ares());
}

/// A map's `ImmuneToEMP=` (Ares' EMP reader alone reads it) turns Ares on,
/// and with it Ares' Cloakable building (BuildingClass_Init_Cloakable,
/// 1005C4F0, hook 0x442CE0): the building's AI visit runs its cloak producer.
/// The stock `Cloakable=` alone leaves both off, as gamemd ignores it on a
/// building.
#[test]
fn a_maps_ares_key_turns_ares_on() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let mut owner = installed(&assets);
    let cloaks = |rules: &RuleSet| {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0xC10A);
        let id = sim
            .spawn_object("GAPOWR", "Americans", 12, 12, 0, rules)
            .unwrap();
        sim.object_ai_visit_one(id, Some(rules), crate::sim::world::ObjectAiCtx::default());
        sim.substrate.entities.get(id).unwrap().cloak.is_some()
    };
    let stock = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[GAPOWR]\nCloakable=yes\n"),
    );
    assert!(!stock.ares());
    assert!(!cloaks(&stock));
    let ares = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[GAPOWR]\nCloakable=yes\n[HTNK]\nImmuneToEMP=yes\n"),
    );
    assert_eq!(ares.ares(), crate::rules::ARES_BUILD);
    assert_eq!(cloaks(&ares), crate::rules::ARES_BUILD);
}

/// `ares.mix` mounted at startup (Load_Bootstrap_AresMIX, hook 0x0053029E)
/// turns Ares on for retail rules.
#[cfg(feature = "ares")]
#[test]
fn ares_mix_turns_ares_on() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let mut owner = retail_rules_owner(&assets).with_ares_mix(true);
    assert!(skirmish(&mut owner, None, &IniFile::empty()).ares());
}
