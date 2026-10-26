//! Ares promotion sound10046AF0 through the existing Rules/audio and AI owners.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use std::sync::Arc;

const SOUNDS: &str = "[SoundList]\n0=GlobalVeteran\n1=GlobalElite\n2=HeroVeteran\n3=HeroElite\n";

// This extension fixture has the ordinary retail/MO General section.
// Missing-General global cues have independent stock-reader coverage.
fn scene_rules(sounds: &str) -> (NativeRulesProcessOwner, RuleSet) {
    let root = crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nVeteranCombat=1.0\n[Countries]\n0=Americans\n1=Russians\n\
         [AudioVisual]\nUpgradeVeteranSound=GlobalVeteran\nUpgradeEliteSound=GlobalElite\n\
         [InfantryTypes]\n0=HERO\n1=SILENT\n2=DEFAULT\n\
         [HERO]\nStrength=100\nSpeed=4\nPromote.VeteranSound=HeroVeteran\nPromote.EliteSound=HeroElite\n\
         [SILENT]\nStrength=100\nSpeed=4\nPromote.VeteranSound=HeroVeteran\n\
         [DEFAULT]\nStrength=100\nSpeed=4\n",
    );
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&IniFile::from_str(sounds))),
        Arc::new(EvaRegistry::from_ini(&IniFile::empty())),
    )
    .unwrap();
    let map = IniFile::from_str(
        "[HERO]\nPromote.VeteranSound=Unknown\nPromote.EliteSound=\n\
         [SILENT]\nPromote.VeteranSound=<none>\n",
    );
    let (rules, ..) = owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map)
        .unwrap()
        .into_parts();
    (owner, rules)
}

#[cfg(feature = "ares")]
#[test]
fn promotion_sound_overrides_retain_layers_silence_save_and_bound_catalog_identity() {
    let (owner, rules) = scene_rules(SOUNDS);
    let hero = rules.object("HERO").unwrap();
    assert_eq!(
        hero.promotion_sound_override(false),
        Some(Some("HeroVeteran"))
    );
    assert_eq!(hero.promotion_sound_override(true), Some(Some("HeroElite")));
    assert_eq!(
        rules
            .object("SILENT")
            .unwrap()
            .promotion_sound_override(false),
        Some(None)
    );
    assert_eq!(
        rules
            .object("DEFAULT")
            .unwrap()
            .promotion_sound_override(false),
        None
    );
    let (_, other) = scene_rules(&SOUNDS.replace("3=HeroElite", "3=OtherCue"));
    assert_ne!(
        rules.simulation_config_hash(),
        other.simulation_config_hash(),
        "fixed catalog binding changes the effective override despite identical Rules text"
    );
    let saved = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let saved = crate::util::save_codec::deserialize(&saved).unwrap();
    let restored = owner.project_saved_scenario_rules(&saved).unwrap();
    for id in ["HERO", "SILENT", "DEFAULT"] {
        for elite in [false, true] {
            assert_eq!(
                rules.object(id).unwrap().promotion_sound_override(elite),
                restored.object(id).unwrap().promotion_sound_override(elite)
            );
        }
    }
}

#[test]
fn promotion_sound_ai_selects_overrides_silence_and_stock_fallback() {
    let (_, rules) = scene_rules(SOUNDS);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    let ids: Vec<_> = [("HERO", 10), ("SILENT", 12), ("DEFAULT", 14)]
        .into_iter()
        .map(|(name, x)| {
            sim.spawn_object(name, "Americans", x, 10, 0, &rules)
                .unwrap()
        })
        .collect();
    sim.object_ai_stage(Some(&rules));
    assert!(
        !sim.take_sound_events_for_test()
            .iter()
            .any(|event| matches!(event, SimSoundEvent::UnitPromoted { .. })),
        "first sampling caches silently"
    );
    for elite in [false, true] {
        for id in &ids {
            let actor = sim.substrate.entities.get_mut(*id).unwrap();
            if elite {
                actor.veterancy.set_elite();
            } else {
                actor.veterancy.set_veteran();
            }
        }
        sim.object_ai_stage(Some(&rules));
        let cues: Vec<_> = sim
            .take_sound_events_for_test()
            .into_iter()
            .filter_map(|event| {
                if let SimSoundEvent::UnitPromoted { sound_id, .. } = event {
                    Some(sound_id.map(|id| sim.interner.resolve(id).to_owned()))
                } else {
                    None
                }
            })
            .collect();
        let global = if elite {
            "GlobalElite"
        } else {
            "GlobalVeteran"
        };
        let expected = if cfg!(feature = "ares") {
            vec![
                Some(if elite { "HeroElite" } else { "HeroVeteran" }.to_owned()),
                if elite { Some(global.to_owned()) } else { None },
                Some(global.to_owned()),
            ]
        } else {
            vec![Some(global.to_owned()); 3]
        };
        assert_eq!(cues, expected);
        sim.object_ai_stage(Some(&rules));
        assert!(
            !sim.take_sound_events_for_test()
                .iter()
                .any(|event| matches!(event, SimSoundEvent::UnitPromoted { .. })),
            "same rank never repeats its cue"
        );
    }
}
