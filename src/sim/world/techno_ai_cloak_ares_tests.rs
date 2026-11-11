//! Ares cloak hooks through the stock cloak producer: per-type stages and
//! sounds (7036EB/703A79/70380A/70375B) and CloakDisallowed10043A40's
//! Cloakable.Deployed and CloakHeight arms.

use super::*;
use crate::map::playfield::PlayfieldBounds;
use crate::rules::ini_parser::IniFile;
use crate::sim::world::SimSoundEvent;

fn rules(extra_general: &str) -> RuleSet {
    RuleSet::from_ini_with_fixed_art_and_sounds_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[General]\nCloakingStages=9\nCloakDelay=0\n{extra_general}\
             [AudioVisual]\nCloakSound=NavalUnitEmerge\nDecloakSound=GlobalDecloak\n\
             [VehicleTypes]\n0=SUB\n1=PLAIN\n[InfantryTypes]\n0=GHOST\n\
             [SUB]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
             Cloakable.Stages=3\nCloakSound=SubCloak\nDecloakSound=none\n\
             [PLAIN]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
             [GHOST]\nStrength=100\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
             Deployer=yes\nCloakable.Deployed=yes\n",
        )),
        &IniFile::empty(),
        &["NavalUnitEmerge", "GlobalDecloak", "SubCloak"],
    )
    .expect("Ares cloak rules")
}

fn scene(rules: &RuleSet, kind: &str) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(0xC10A_A9E5);
    sim.fog.set_width_for_test(64);
    sim.fog.set_height_for_test(64);
    sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(
        64, 2, 2, 56, 52,
    ));
    let id = sim
        .spawn_object_at_height(kind, "Soviet", 20, 20, 0, 0, rules)
        .unwrap();
    // Unlimbo leaves a cloakable unit fully cloaked; start uncloaked.
    sim.substrate.entities.get_mut(id).unwrap().cloak =
        Some(Box::new(crate::sim::cloak_disguise::CloakRuntime::new(0)));
    (sim, id)
}

fn visit(sim: &mut Simulation, id: u64, rules: &RuleSet, frame: u32) -> i32 {
    sim.session.set_binary_frame_for_test(frame);
    tick_stock_cloak_producer(sim, id, rules, None, FrameEffects::default());
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .cloak
        .as_deref()
        .unwrap()
        .state()
}

fn sounds(sim: &Simulation) -> Vec<&str> {
    sim.sound_events
        .as_slice()
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::CloakSound { sound_id, .. } => Some(sound_id.as_str()),
            _ => None,
        })
        .collect()
}

/// Returns the frame the cloak completed on and the uncloak's first depth.
fn cloak_then_deactivate(rules: &RuleSet, kind: &str) -> (Simulation, u32, u32) {
    let (mut sim, id) = scene(rules, kind);
    assert_eq!(visit(&mut sim, id, rules, 0), 1);
    let completed = (1..=12)
        .find(|&frame| visit(&mut sim, id, rules, frame) == 2)
        .expect("the cloak completes");
    // Ares CloakDisallowed10043A40 refuses a Deactivated self-cloaker.
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .deactivated
        .set_for_test(true);
    assert_eq!(visit(&mut sim, id, rules, completed + 1), 3);
    let depth = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .cloak
        .as_deref()
        .unwrap()
        .depth();
    (sim, completed, depth)
}

#[test]
fn ares_cloak_stages_and_sounds_follow_the_type_before_rules() {
    let rules = rules("");
    // Hook703A79 divides by Cloakable.Stages=3: progress 2 is already
    // VisualCharacter 3, against progress 5 for the general 9 stages.
    let (sub, sub_completed, sub_depth) = cloak_then_deactivate(&rules, "SUB");
    let (plain, plain_completed, plain_depth) = cloak_then_deactivate(&rules, "PLAIN");
    assert_eq!((sub_completed, plain_completed), (2, 5));
    // Hook7036EB starts the uncloak at stages - 1.
    assert_eq!((sub_depth, plain_depth), (2, 8));
    // Hook70380A: the type's CloakSound; hook70375B: the type's explicit
    // none silences the decloak, while an unset type uses DecloakSound.
    assert_eq!(sounds(&sub), ["SubCloak"]);
    assert_eq!(sounds(&plain), ["NavalUnitEmerge", "GlobalDecloak"]);
}

#[test]
fn ares_cloakable_deployed_and_cloak_height_gate_self_and_generator_cloak() {
    let facts = |rules: &RuleSet, deployed: bool, field: bool| {
        let (mut sim, id) = scene(rules, "GHOST");
        if deployed {
            sim.substrate.entities.get_mut(id).unwrap().mission_leaf =
                crate::sim::mission::leaf::MissionLeafState::infantry_raw_for_test(0, 28);
        }
        if field {
            let owner = sim.substrate.entities.get(id).unwrap().owner();
            if !sim.houses.contains_key(&owner) {
                sim.houses
                    .register(crate::sim::house_state::HouseState::new(
                        owner, 0, None, false, 0, 10,
                    ));
            }
            sim.fog.reset_cloaked_by_houses();
            let index = sim.base_reservation_house_index(owner).unwrap() as u8;
            assert!(sim.fog.set_cloaked_by_house(index, 20, 20));
        }
        let facts = stock_cloak_tick_facts(&sim, id, rules, None).unwrap();
        (
            facts.state_zero_head_allows,
            facts.can_auto_cloak,
            facts.should_uncloak,
        )
    };
    let rules_default = rules("");
    // Undeployed: no self-cloak and nothing else to keep it cloaked.
    assert_eq!(facts(&rules_default, false, false), (false, false, true));
    assert_eq!(facts(&rules_default, true, false), (true, true, false));
    // A generator covers the undeployed infantry at height 0 <= HoverHeight,
    // but not when CloakHeight is below the ground.
    assert_eq!(facts(&rules_default, false, true), (false, true, false));
    let low = rules("CloakHeight=-1\n");
    assert_eq!(facts(&low, false, true), (false, false, true));
    assert_eq!(facts(&low, true, true), (true, true, false));
}
