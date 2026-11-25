//! The per-game Ares and Phobos switches (`rules::ares_switch`,
//! `rules::phobos_switch`) through the production Rules owner: retail data
//! plays as gamemd; a game's own Ares key or `ares.mix` turns Ares on, and the
//! routed paths with it, and a game's own Phobos key turns Phobos on.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};
use crate::rules::ruleset::RuleSet;

/// The retail install as it is. The retail fixtures ship `ares.mix` on a
/// thread read the Ares way (`retail_ini_fixture::retail_rules_owner`); the
/// retail archives do not.
fn installed(assets: &AssetManager) -> NativeRulesProcessOwner {
    assert!(!assets.ships_ares());
    retail_rules_owner(assets).with_ares_mix(false)
}

fn skirmish(owner: &mut NativeRulesProcessOwner, mode: Option<&IniFile>, map: &IniFile) -> RuleSet {
    owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(mode), map)
        .unwrap()
        .into_parts()
        .0
}

/// Every retail mode on a retail map, and a retail campaign map with its
/// rules overrides, hold no key that only an Ares or a Phobos reader reads.
#[test]
fn retail_modes_and_maps_keep_ares_and_phobos_off() {
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
        // A build with the `phobos` feature plays Phobos in every game.
        assert!(
            crate::rules::PHOBOS_ENABLED || !rules.phobos(),
            "{}",
            mode.override_file
        );
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
    assert!(crate::rules::PHOBOS_ENABLED || !rules.phobos());
}

/// A map's Phobos-only key, the building queue limit a harder-AI file sets,
/// turns Phobos on and leaves Ares off; `AllowParallelAIQueues=` alone, which
/// Ares reads too, turns neither on.
#[cfg(not(feature = "phobos"))]
#[test]
fn a_maps_phobos_key_turns_phobos_on() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let mut owner = installed(&assets);
    let shared = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[GlobalControls]\nAllowParallelAIQueues=yes\n"),
    );
    assert!(!shared.phobos() && !shared.ares());
    let phobos = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[GlobalControls]\nForbidParallelAIQueues.Building=yes\n"),
    );
    assert!(phobos.phobos());
    assert!(!phobos.ares());
    assert!(phobos.parallel_ai_queues_act());
    // The spare power switch alone turns it on too, and the house then keeps
    // retail's `[AI] PowerSurplus=50` spare.
    let spare = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[AI]\nEnablePowerSurplus=yes\n"),
    );
    assert!(spare.phobos() && !spare.ares());
    let power = crate::sim::power_system::PowerState::default();
    assert_eq!(power.power_surplus(&spare), 50);
    assert_eq!(power.power_surplus(&phobos), 0, "Phobos on, the switch off");
    assert_eq!(power.power_surplus(&shared), 0);
}

/// A map's `ImmuneToAbduction=` (Ares' TechnoTypeExt reader alone reads it,
/// in a game read either way) turns Ares on, and with it Ares' Cloakable
/// building (BuildingClass_Init_Cloakable, 1005C4F0, hook 0x442CE0): the
/// building's AI visit runs its cloak producer. The stock `Cloakable=` alone
/// leaves both off, as gamemd ignores it on a building.
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
        sim.object_ai_visit_one(
            id,
            Some(rules),
            &mut crate::sim::world::ObjectAiCtx::default(),
        );
        sim.entities().get(id).unwrap().cloak.is_some()
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
        &IniFile::from_str("[GAPOWR]\nCloakable=yes\n[HTNK]\nImmuneToAbduction=yes\n"),
    );
    assert!(ares.ares());
    assert!(cloaks(&ares));
}

/// VERA's Ares setting Off: the same map's Ares key turns nothing on, and the
/// processed Rules keep that answer for their projections. The rules say
/// that the setting is what keeps Ares off there, as a player set to Auto
/// plays another game from the same files; on a map without an Ares key the
/// setting keeps nothing off, and both play the same game.
#[test]
fn the_ares_setting_off_keeps_a_maps_ares_key_off() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let map = IniFile::from_str("[HTNK]\nImmuneToAbduction=yes\n");
    let mut owner = installed(&assets).with_ares_keys_off(true);
    let kept = skirmish(&mut owner, None, &map);
    assert!(!kept.ares() && kept.ares_kept_off());
    let plain = skirmish(&mut owner, None, &IniFile::empty());
    assert!(!plain.ares() && !plain.ares_kept_off());
    let auto = skirmish(&mut installed(&assets), None, &map);
    assert!(auto.ares() && !auto.ares_kept_off());
    // A key only a Rules pass reads, not the projection.
    let pass_key = IniFile::from_str("[ROBO]\nWaterImage=mtnk\n");
    assert!(skirmish(&mut installed(&assets), None, &pass_key).ares());
    let kept = skirmish(&mut owner, None, &pass_key);
    assert!(!kept.ares() && kept.ares_kept_off());
}

/// `ares.mix` mounted at startup (Load_Bootstrap_AresMIX, hook 0x0053029E)
/// turns Ares on for retail rules, and the switch is
/// part of the game's configuration, so a replay or a peer with the other
/// answer is refused.
#[test]
fn ares_mix_turns_ares_on() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let stock = skirmish(&mut installed(&assets), None, &IniFile::empty());
    let mut owner = retail_rules_owner(&assets).with_ares_mix(true);
    let ares = skirmish(&mut owner, None, &IniFile::empty());
    assert!(ares.ares());
    assert_ne!(
        ares.simulation_config_hash(),
        stock.simulation_config_hash()
    );
}

#[test]
fn water_image_key_enables_ares_and_admits_the_unspawned_model() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let mut owner = installed(&assets);
    let rules = skirmish(
        &mut owner,
        None,
        &IniFile::from_str("[ROBO]\nWaterImage=mtnk\n"),
    );
    assert!(rules.ares(), "WaterImage alone selects the Ares draw hooks");
    assert!(
        crate::rules::voxel_frame_catalog::unit_atlas_variants("ROBO", Some(&rules))
            .iter()
            .any(|id| id == "MTNK"),
        "an alternate model must be admitted before it has its own unit"
    );
}

#[test]
fn water_image_retains_find_only_bindings_through_layers_and_save() {
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let mut owner = installed(&assets);
    let mode = IniFile::from_str(
        "[ROBO]\nWaterImage=mtNk\n[CMIN]\nWaterImage=MTNK\n[CMON]\nWaterImage=MTNK\n",
    );
    let rules = skirmish(
        &mut owner,
        Some(&mode),
        &IniFile::from_str(
            "[ROBO]\nWaterImage=MISSING\n[CMIN]\nWaterImage=<none>\n[MTNK]\nwaterimage=ROBO\n",
        ),
    );
    assert_eq!(rules.object("ROBO").unwrap().water_image(), Some("MTNK"));
    assert_eq!(rules.object("CMIN").unwrap().water_image(), None);
    assert_eq!(rules.object("MTNK").unwrap().water_image(), None);
    assert!(
        crate::rules::voxel_frame_catalog::unit_atlas_variants("CMIN", Some(&rules))
            .iter()
            .any(|id| id == "MTNK"),
        "the dock-unloading type can itself select WaterImage"
    );
    assert!(rules.object("MISSING").is_none(), "Find never allocates");
    let saved = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let restored = crate::util::save_codec::deserialize(&saved).unwrap();
    let restored = owner.project_saved_scenario_rules(&restored).unwrap();
    assert_eq!(restored.object("ROBO").unwrap().water_image(), Some("MTNK"));
    assert_eq!(restored.object("CMIN").unwrap().water_image(), None);
}

/// Saved Rules restore only into a game read the same way: the Rules owner's
/// own `ares.mix` answer decides, not the launch or the calling thread.
#[test]
fn saved_rules_restore_only_into_a_game_read_the_same_way() {
    let owner = |ships_ares: bool| {
        let _ares_way = crate::rules::ares_switch::read_ares_way(ships_ares);
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::empty(),
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
            std::sync::Arc::default(),
        )
        .unwrap();
        owner
            .load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            )
            .unwrap();
        owner
    };
    let (stock, ares) = (owner(false), owner(true));
    for (from, into, restores) in [
        (&stock, &stock, true),
        (&ares, &ares, true),
        (&ares, &stock, false),
        (&stock, &ares, false),
    ] {
        assert_eq!(
            into.project_saved_scenario_rules(from.saved_scenario_rules().unwrap())
                .is_ok(),
            restores
        );
    }
}

/// The reader recorder and the reading mode live on the thread that opened
/// their scope (`ares_switch`'s module notes), so the Rules code those scopes
/// run must read on that thread: no Rules source (test files aside) spawns a
/// thread or uses a thread pool. The retail fixture, which opens no scope for
/// its threads, is the one known use, and finding it shows the search works.
#[test]
fn rules_code_reads_on_the_thread_that_opened_its_scope() {
    fn sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() {
                if name != "tests" {
                    sources(&path, found);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
                found.push(path);
            }
        }
    }
    let rules = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rules");
    let mut files = Vec::new();
    sources(&rules, &mut files);
    let threaded: Vec<String> = files
        .iter()
        .filter(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            [
                "rayon",
                "par_iter",
                "thread::spawn",
                "thread::scope",
                "thread::Builder",
            ]
            .iter()
            .any(|pattern| text.contains(pattern))
        })
        .map(|path| path.strip_prefix(&rules).unwrap().display().to_string())
        .collect();
    assert_eq!(threaded, ["retail_ini_fixture.rs"]);
}

/// A SuperWeaponType's `Type=` is one of Ares' reads of the ordered pass, so
/// a Rules process read the Ares way keeps the key among the Ares inputs it
/// saves with the rules.
#[test]
fn a_super_weapons_type_key_stays_an_ares_input() {
    use crate::rules::native_processing::RulesLayerStack;
    let _way = crate::rules::ares_switch::read_ares_way(true);
    let rules = IniFile::from_str("[SuperWeaponTypes]\n0=TESTSW\n[TESTSW]\nType=MultiMissile\n");
    let processed = RulesLayerStack::new(rules)
        .process_with_fixed_art(&IniFile::from_str(""))
        .unwrap();
    let inputs = format!("{:?}", processed.ares_inputs());
    assert!(inputs.contains(r#"("TESTSW", "Type")"#), "{inputs}");
}
