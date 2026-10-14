//! Rust regression tests for the arms that send a computer house's
//! infiltrators and occupiers into buildings: `InfantryClass::Assign_Target`'s
//! infiltration arm (`0x0051B2B1..0x0051B33F`), the computer capture arm
//! `Mission_Attack` (`0x0051F456..0x0051F4D2`) and `Mission_Hunt`
//! (`0x0051F540..0x0051F5BF`) share, and Mission_Hunt's deployed undeploy
//! (`0x0051F5C0..0x0051F609`). Expected values come from instruction
//! reading; none of these arms draws.

use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::movement::infantry_action::{DO_DEPLOYED_IDLE, DO_UNDEPLOY};
use crate::sim::world::ObjectAiCtx;

/// A spy, a non-`Agent=` infiltrator, an occupier, a deployer and the
/// buildings they test. Hunt's and Attack's rates are not Capture's, so
/// the cadence shows the arm's own return.
const RULES: &str = "\
[InfantryTypes]
0=SPY
1=THIEF
2=GI
3=DGI
[BuildingTypes]
0=NAPOWR
1=DEPOT
2=BUNKER
[Attack]
Rate=.030
[Hunt]
Rate=.030
[Capture]
Rate=.016
[SPY]
Strength=100
Speed=4
Infiltrate=yes
Agent=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[THIEF]
Strength=100
Speed=4
Infiltrate=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[GI]
Strength=125
Speed=4
Occupier=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[DGI]
Strength=125
Speed=4
Deployer=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[NAPOWR]
Strength=750
Power=150
Spyable=yes
Capturable=yes
[DEPOT]
Strength=750
Capturable=yes
[BUNKER]
Strength=500
CanBeOccupied=yes
MaxNumberOccupants=5
";

const ART: &str = "\
[NAPOWR]
Foundation=2x2
[DEPOT]
Foundation=2x2
[BUNKER]
Foundation=2x2
[DGI]
Sequence=DGISequence
[DGISequence]
Ready=0,1,1
Guard=0,1,1
Walk=8,6,6
Deploy=56,8,0
Deployed=64,1,1
DeployedIdle=64,1,1
Undeploy=56,8,0
";

const CAPTURE: MissionId = MissionId::from_known(MissionType::Capture);

fn rules() -> RuleSet {
    let art = IniFile::from_str(ART);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &art,
    )
    .expect("capture arm rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules
}

/// The flat arena with the computer Russians and the human Americans, on a
/// frame that is no multiple of 16: on those, `TechnoClass::AI_Update` lets
/// go of a target its object cannot fire at before the mission runs
/// (`0x006FA472..0x006FA4CB`), and these actors carry no weapon.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0x51F3E0);
    for (name, side, human) in [("Russians", 1, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.session.binary_frame = 1;
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn assign(sim: &mut Simulation, id: u64, mission: MissionType) {
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(id, MissionId::from_known(mission), now)
        .unwrap();
}

fn heading(sim: &Simulation, id: u64) -> Option<NavTargetRef> {
    sim.substrate.entities.get(id)?.navigation.nav_com
}

/// The setter heads an `Infiltrate=` infantryman with no NavCom for a
/// building it may enter: `Spyable=` for an `Agent=` one, `Capturable=` for
/// any other. It asks neither house's control nor their alliance.
#[test]
fn the_target_setter_sends_an_infiltrator_into_a_building_it_may_enter() {
    let rules = rules();
    for (kind, owner, building, building_owner, heads) in [
        ("SPY", "Russians", "NAPOWR", "Americans", true),
        ("SPY", "Americans", "NAPOWR", "Russians", true),
        ("SPY", "Russians", "NAPOWR", "Russians", true),
        ("SPY", "Russians", "DEPOT", "Americans", false),
        ("THIEF", "Russians", "DEPOT", "Americans", true),
        ("THIEF", "Russians", "BUNKER", "Americans", false),
        ("GI", "Russians", "NAPOWR", "Americans", false),
    ] {
        let mut sim = arena(&rules);
        let target = spawn(&mut sim, &rules, building, building_owner, 16, 10);
        let actor = spawn(&mut sim, &rules, kind, owner, 10, 10);
        sim.assign_target_represented(
            actor,
            Some(TargetKind::Entity(target)),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        let expected = heads.then(|| NavTargetRef::object(target));
        assert_eq!(
            heading(&sim, actor),
            expected,
            "{owner} {kind} handed the {building_owner}' {building}"
        );
    }

    // An infiltrator already heading somewhere keeps its NavCom, and a
    // target that is no building sends it nowhere.
    let mut sim = arena(&rules);
    let plant = spawn(&mut sim, &rules, "NAPOWR", "Americans", 16, 10);
    let gi = spawn(&mut sim, &rules, "GI", "Americans", 14, 16);
    let busy = spawn(&mut sim, &rules, "SPY", "Russians", 10, 10);
    let idle = spawn(&mut sim, &rules, "SPY", "Russians", 10, 14);
    let cell = NavTargetRef::cell(4, 4);
    sim.assign_destination_represented(
        busy,
        Some(cell),
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    for (actor, target) in [(busy, plant), (idle, gi)] {
        sim.assign_target_represented(
            actor,
            Some(TargetKind::Entity(target)),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
    }
    assert_eq!(heading(&sim, busy), Some(cell));
    assert_eq!(heading(&sim, idle), None);

    // The arm runs after the base write whether or not that changed the
    // target, so the same plant handed again heads the spy there again.
    let mut sim = arena(&rules);
    let plant = spawn(&mut sim, &rules, "NAPOWR", "Americans", 16, 10);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 10, 10);
    for handed in ["first", "again"] {
        sim.assign_target_represented(
            spy,
            Some(TargetKind::Entity(plant)),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        assert_eq!(
            heading(&sim, spy),
            Some(NavTargetRef::object(plant)),
            "{handed}"
        );
        sim.assign_destination_represented(spy, None, Some(&rules), None, FrameEffects::default())
            .unwrap();
    }
}

/// A computer house's infiltrator holding a building, or its occupier
/// holding one that `CanBeOccupiedBy` admits, takes the building as its
/// destination and Capture as its mission, and the arm returns 1 on Attack
/// and Hunt alike. A human's, or an occupier the building refuses, does not.
#[test]
fn a_computer_infiltrator_or_occupier_holding_a_building_takes_capture() {
    let rules = rules();
    for mission in [MissionType::Attack, MissionType::Hunt] {
        for (kind, owner, building, building_owner, captures) in [
            ("SPY", "Russians", "NAPOWR", "Americans", true),
            ("SPY", "Russians", "DEPOT", "Americans", true),
            ("SPY", "Americans", "NAPOWR", "Russians", false),
            ("GI", "Russians", "BUNKER", "Russians", true),
            ("GI", "Russians", "BUNKER", "Americans", false),
            ("GI", "Russians", "NAPOWR", "Russians", false),
        ] {
            let mut sim = arena(&rules);
            let target = spawn(&mut sim, &rules, building, building_owner, 16, 10);
            let actor = spawn(&mut sim, &rules, kind, owner, 10, 10);
            // The Target alone, without the setter's own destination arm.
            sim.substrate
                .entities
                .get_mut(actor)
                .unwrap()
                .attack_target
                .set_for_test(Some(crate::sim::combat::TargetKind::Entity(target)));
            assign(&mut sim, actor, mission);

            sim.object_ai_visit_one(actor, Some(&rules), ObjectAiCtx::default());

            let entity = sim.substrate.entities.get(actor).unwrap();
            let case = format!("{mission:?}: {owner} {kind} holding {building_owner}' {building}");
            if captures {
                assert_eq!(
                    (entity.mission.current(), entity.mission.queued()),
                    (CAPTURE, MissionId::NONE),
                    "{case}"
                );
                assert_eq!(heading(&sim, actor), Some(NavTargetRef::object(target)));
                assert_eq!(entity.mission.dispatch_timer().delay(), 1, "{case}");
            } else {
                assert_ne!(entity.mission.current(), CAPTURE, "{case}");
            }
        }
    }
}

/// A building already dying is a Target the native detach has cleared
/// (`attack_target_is_stale`), so a computer spy holding one keeps its
/// mission and takes no destination.
#[test]
fn a_computer_spy_holding_a_dying_building_keeps_its_mission() {
    let rules = rules();
    for mission in [MissionType::Attack, MissionType::Hunt] {
        let mut sim = arena(&rules);
        let target = spawn(&mut sim, &rules, "NAPOWR", "Americans", 16, 10);
        let actor = spawn(&mut sim, &rules, "SPY", "Russians", 10, 10);
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .death
            .set_dying_for_test(true);
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(target)));
        assign(&mut sim, actor, mission);

        sim.object_ai_visit_one(actor, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(actor).unwrap();
        assert_ne!(entity.mission.current(), CAPTURE, "{mission:?}");
        assert_eq!(heading(&sim, actor), None, "{mission:?}");
    }
}

/// A computer house's deployed infantryman hunting with no live Target (none,
/// or one already dying) undeploys, lets a stale one go, and the arm returns
/// 1. A human's takes the Foot body instead.
#[test]
fn a_deployed_computer_infantryman_hunting_without_a_target_undeploys() {
    let rules = rules();
    for (owner, stale_target, undeploys) in [
        ("Russians", false, true),
        ("Russians", true, true),
        ("Americans", false, false),
    ] {
        let mut sim = arena(&rules);
        let actor = spawn(&mut sim, &rules, "DGI", owner, 10, 10);
        sim.substrate.entities.get_mut(actor).unwrap().mission_leaf =
            MissionLeafState::infantry_raw_for_test(0, DO_DEPLOYED_IDLE);
        if stale_target {
            // A dying Target, which the native detach has already cleared.
            let victim = spawn(&mut sim, &rules, "GI", "Americans", 14, 10);
            sim.substrate
                .entities
                .get_mut(victim)
                .unwrap()
                .death
                .set_dying_for_test(true);
            sim.substrate
                .entities
                .get_mut(actor)
                .unwrap()
                .attack_target
                .set_for_test(Some(crate::sim::combat::TargetKind::Entity(victim)));
        }
        assign(&mut sim, actor, MissionType::Hunt);

        sim.object_ai_visit_one(actor, Some(&rules), ObjectAiCtx::default());

        let entity = sim.substrate.entities.get(actor).unwrap();
        let doing = entity.mission_leaf.as_infantry().unwrap().doing();
        let case = format!("{owner}, stale Target {stale_target}");
        if undeploys {
            assert_eq!(doing, DO_UNDEPLOY, "{case}");
            assert_eq!(entity.mission.dispatch_timer().delay(), 1, "{case}");
            assert!(entity.attack_target.target().is_none(), "{case}");
        } else {
            assert_ne!(doing, DO_UNDEPLOY, "{case}");
        }
    }
}
