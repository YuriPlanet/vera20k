//! Original Rules669CE2..669D68 binds promotion cues independently of General.
use super::RuleSet;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::{EvaRegistry, SoundRegistry};
use crate::sim::world::{SimSoundEvent, Simulation};
use std::sync::Arc;

const SOUNDS: &str =
    "[SoundList]\n0=VeteranCue\n1=EliteCue\n2=LanguageCue\n[Unregistered]\nSounds=unused\n";
fn scene_rules(layered: bool, sounds: &str) -> (NativeRulesProcessOwner, RuleSet) {
    let root = crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=Americans\n1=Russians\n[InfantryTypes]\n0=ACTOR\n\
         [ACTOR]\nStrength=100\nSpeed=4\n\
         [AudioVisual]\nUpgradeVeteranSound=VeteranCue\nUpgradeEliteSound=EliteCue\n",
    );
    assert!(root.section("General").is_none());
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        layered.then(|| {
            IniFile::from_str(
                "[AudioVisual]\nUpgradeVeteranSound=LanguageCue\nUpgradeEliteSound=Unknown\n",
            )
        }),
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&IniFile::from_str(sounds))),
        Arc::new(EvaRegistry::from_ini(&IniFile::empty())),
    )
    .unwrap();
    let map = if layered {
        IniFile::from_str("[AudioVisual]\nUpgradeVeteranSound=\nUpgradeEliteSound=<none>\n")
    } else {
        IniFile::empty()
    };
    let (rules, ..) = owner
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map)
        .unwrap()
        .into_parts();
    (owner, rules)
}

#[test]
fn global_promotion_cues_retain_fixed_catalog_layers_and_restore_without_general() {
    let (owner, rules) = scene_rules(true, SOUNDS);
    assert_eq!(
        rules.general.upgrade_veteran_sound.as_deref(),
        Some("LanguageCue")
    );
    assert_eq!(
        rules.general.upgrade_elite_sound.as_deref(),
        Some("EliteCue")
    );
    let (_, other) = scene_rules(true, &SOUNDS.replace("1=EliteCue", "1=OtherCue"));
    assert_eq!(other.general.upgrade_elite_sound, None);
    assert_ne!(
        rules.simulation_config_hash(),
        other.simulation_config_hash()
    );
    let saved = crate::util::save_codec::serialize(owner.saved_scenario_rules().unwrap()).unwrap();
    let saved = crate::util::save_codec::deserialize(&saved).unwrap();
    let restored = owner.project_saved_scenario_rules(&saved).unwrap();
    assert_eq!(
        rules.general.upgrade_veteran_sound,
        restored.general.upgrade_veteran_sound
    );
    assert_eq!(
        rules.general.upgrade_elite_sound,
        restored.general.upgrade_elite_sound
    );
}

#[test]
fn global_promotion_cues_reach_normal_ai_without_general() {
    let (_, rules) = scene_rules(false, SOUNDS);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 37);
    let id = sim
        .spawn_object("ACTOR", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.object_ai_stage(Some(&rules));
    assert!(
        !sim.take_sound_events_for_test()
            .iter()
            .any(|event| matches!(event, SimSoundEvent::UnitPromoted { .. }))
    );
    for (elite, expected) in [(false, "VeteranCue"), (true, "EliteCue")] {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        if elite {
            actor.veterancy.set_elite();
        } else {
            actor.veterancy.set_veteran();
        }
        sim.object_ai_stage(Some(&rules));
        let cues: Vec<_> = sim
            .take_sound_events_for_test()
            .into_iter()
            .filter_map(|event| {
                if let SimSoundEvent::UnitPromoted { sound_id, .. } = event {
                    Some(sound_id.map(|id| sim.interner().resolve(id).to_owned()))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(cues, vec![Some(expected.to_owned())]);
    }
}
