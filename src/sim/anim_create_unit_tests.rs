//! `CreateUnit.*` through the production anim AI and spawn owners.
use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::mission_data::MissionType;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::mission::MissionId;

const RULES: &str = "[Countries]\n0=Americans\n1=Russians\n2=Civilians\n\
    [Sides]\nCivilian=Civilians\n\
    [VehicleTypes]\n0=MTNK\n[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=LAB\n\
    [Animations]\n0=MAKE\n1=SPAWN\n\
    [MTNK]\nStrength=100\nSpeed=4\nSight=4\nPrimary=GUN\n\
    [LAB]\nStrength=1000\nFoundation=1x1\n\
    [Weapons]\n0=GUN\n[GUN]\nDamage=5\nROF=20\nRange=5\nWarhead=WH\nProjectile=PIFF\n\
    [Warheads]\n0=WH\n[WH]\n\
    [PIFF]\nInviso=yes\n\
    [Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\nWheel=100%\n";
fn scene(make_lines: &str) -> (Simulation, RuleSet) {
    let art_text = format!(
        "[MAKE]\nRate=900\nEnd=1\nLoopCount=1\n{make_lines}\n\
         [SPAWN]\nRate=900\nEnd=1\nLoopCount=1\n"
    );
    let art = IniFile::from_str(&art_text);
    let rules_text = crate::sim::arena_fixture::clear_ground_ini(RULES);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&rules_text, &art).expect("create-unit rules");
    let mut registry = ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("MAKE", 1);
    registry.bind_anim_frame_count_for_test("SPAWN", 1);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x424932);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    (sim, rules)
}

fn spawn_make(sim: &mut Simulation, rules: &RuleSet, owner: Option<&str>) -> AnimId {
    spawn_make_at_z(sim, rules, owner, 0)
}

/// [`scene`] with a second anim type `NEXT` whose ART body is `next_lines`.
fn scene_chain(make_lines: &str, next_lines: &str) -> (Simulation, RuleSet) {
    let art_text = format!(
        "[MAKE]\nRate=900\nEnd=1\nLoopCount=1\n{make_lines}\n\
         [NEXT]\nRate=900\nEnd=1\nLoopCount=1\n{next_lines}\n\
         [SPAWN]\nRate=900\nEnd=1\nLoopCount=1\n"
    );
    let art = IniFile::from_str(&art_text);
    let rules_text = crate::sim::arena_fixture::clear_ground_ini(RULES);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&rules_text, &art).expect("create-unit rules");
    let mut registry = ArtRegistry::from_ini(&art);
    for name in ["MAKE", "NEXT", "SPAWN"] {
        registry.bind_anim_frame_count_for_test(name, 1);
    }
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x424932);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    (sim, rules)
}

fn spawn_make_at_z(sim: &mut Simulation, rules: &RuleSet, owner: Option<&str>, z: i32) -> AnimId {
    let type_name = sim.interner.intern("MAKE");
    let id = sim
        .spawn_anim_at_world(
            rules,
            AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(
                    type_name,
                    20,
                    10,
                    crate::util::fixed_math::SimFixed::from_num(128),
                    crate::util::fixed_math::SimFixed::from_num(128),
                    0,
                )
            },
            AnimWorldCoord {
                x: 20 * 256 + 128,
                y: 10 * 256 + 128,
                z,
            },
        )
        .expect("the MAKE anim constructs");
    if let Some(owner) = owner {
        sim.substrate
            .set_anim_owner_house(id, sim.interner.intern(owner));
    }
    id
}

fn draws_until(mut rng: crate::sim::rng::SimRng, target: u64) -> usize {
    for count in 0..8 {
        if rng.state() == target {
            return count;
        }
        rng.next_u32();
    }
    panic!("the post-drive RNG state is not on the pre-drive stream");
}

fn draws_during_drive(sim: &mut Simulation, rules: &RuleSet, id: AnimId) -> usize {
    let checkpoint = sim.scenario_rng.clone();
    drive(sim, rules, id);
    draws_until(checkpoint, sim.scenario_rng.state())
}

fn add_civilian_house(sim: &mut Simulation) {
    let id = sim.interner.intern("Civilians");
    let house = crate::sim::house_state::HouseState::new(id, 0, None, false, 0, 10);
    sim.houses.register(house);
}

fn drive(sim: &mut Simulation, rules: &RuleSet, id: AnimId) {
    for frame in 0..12 {
        sim.session.set_binary_frame_for_test(frame);
        let _ = sim.visit_anim(id, rules, None, FrameEffects::default());
        if sim.anim(id).is_none() {
            break;
        }
    }
}

fn units(sim: &Simulation, rules: &RuleSet) -> Vec<u64> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| entity.category() == crate::map::entities::EntityCategory::Unit)
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "MTNK")
        .map(|entity| entity.stable_id())
        .filter(|id| {
            sim.object_type(sim.substrate.entities.get(*id).unwrap().type_ref(), rules)
                .is_some()
        })
        .collect()
}

#[test]
fn create_unit_spawns_one_unit_for_the_anim_owner() {
    for (keys, expect) in [("CreateUnit=MTNK\n", 1), ("", 0), ("CreateUnit=none\n", 0)] {
        let (mut sim, rules) = scene(keys);
        let id = spawn_make(&mut sim, &rules, Some("Americans"));
        drive(&mut sim, &rules, id);
        let spawned = units(&sim, &rules);
        assert_eq!(spawned.len(), expect, "keys {keys:?}");
        if expect == 1 {
            let unit = sim
                .substrate
                .entities
                .get(spawned[0])
                .expect("spawned unit");
            assert_eq!(
                sim.interner.resolve(unit.owner()),
                "Americans",
                "CreateUnit.Owner defaults to the anim's house (Victim)"
            );
            assert_eq!(
                (unit.position.rx(), unit.position.ry()),
                (20, 10),
                "spawned at the anim's cell"
            );
        }
    }
}

#[test]
fn create_unit_facing_random_flag_and_death_inheritance() {
    // RandomFacing=no: the configured facing; body_facing_dir round-trips.
    let (mut sim, rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.Facing=64\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    let frame = sim.session.binary_frame();
    assert_eq!(unit.body_facing_dir(frame), 64, "CreateUnit.Facing");

    // InheritFacings + FromDeathUnit uses the stored death facing.
    let (mut sim, rules) = scene(
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
         CreateUnit.InheritFacings=yes\n",
    );
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    sim.substrate
        .set_anim_death_facts(id, true, Some(100), false, None);
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    assert_eq!(unit.body_facing_dir(frame), 100, "death-unit facing");
}

#[test]
fn create_unit_spawn_height_and_ground_gate() {
    let (mut sim, rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=100\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    let coords =
        crate::sim::movement::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref());
    assert_eq!(coords.z, 100, "cell floor 0 + SpawnHeight");

    let (mut sim, rules) = scene(
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
         CreateUnit.SpawnHeight=100\nCreateUnit.AlwaysSpawnOnGround=yes\n",
    );
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    let coords =
        crate::sim::movement::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref());
    assert_eq!(coords.z, 0, "AlwaysSpawnOnGround uses the cell floor");
}

/// `Ext/TechnoType/Body.cpp:702-706`: a spawn whose `Unlimbo` left the object
/// in the air, on a type with neither a Fly nor a Jumpjet locomotor and with
/// no `SpawnParachutedInAir`, is let go into free fall and marked
/// `OnParachuted` — so its landing skips the `FallingDownDamage` ratio
/// (`Ext/Techno/Hooks.cpp:1067`). `AlwaysSpawnOnGround` keeps it on the floor
/// and takes no fall.
#[cfg(feature = "phobos")]
#[test]
fn an_air_spawned_unit_is_let_go_into_a_parachuted_fall() {
    fn spawn(keys: &str) -> (bool, bool) {
        let (mut sim, rules) = scene(keys);
        let id = spawn_make(&mut sim, &rules, Some("Americans"));
        drive(&mut sim, &rules, id);
        let unit = sim
            .substrate
            .entities
            .get(units(&sim, &rules)[0])
            .expect("spawned unit");
        (unit.is_falling_down(), unit.on_parachuted())
    }

    let (falling, marked) = spawn(
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=1200\n",
    );
    assert!(falling, "an airborne ground type starts a free fall");
    assert!(marked, "and is marked OnParachuted");

    let (falling, marked) = spawn(
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
         CreateUnit.SpawnHeight=1200\nCreateUnit.AlwaysSpawnOnGround=yes\n",
    );
    assert!(!falling && !marked, "AlwaysSpawnOnGround stays on the floor");
}

#[test]
fn create_unit_refuses_a_building_type() {
    let (mut sim, rules) = scene("CreateUnit=LAB\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    assert!(
        units(&sim, &rules).is_empty()
            && sim
                .substrate
                .entities
                .values()
                .all(|entity| sim.interner.resolve(entity.type_ref()) != "LAB"),
        "a TechnoType Building spawns nothing"
    );
}

#[test]
fn create_unit_plays_its_spawn_anim_with_the_anim_house() {
    let (mut sim, rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnAnim=SPAWN\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    let invoker = sim
        .spawn_object("MTNK", "Americans", 22, 10, 0, &rules)
        .expect("live invoker");
    sim.substrate
        .set_anim_invoker(id, Some(invoker), Some(sim.interner.intern("Americans")));
    drive(&mut sim, &rules, id);
    let spawn = sim
        .substrate
        .anims
        .iter()
        .find(|(_, anim)| sim.interner.resolve(anim.type_id()) == "SPAWN")
        .map(|(_, anim)| (anim.owner_house(), anim.owner_entity(), anim.invoker()));
    assert_eq!(
        spawn.as_ref().map(|(house, ..)| *house),
        Some(Some(sim.interner.intern("Americans"))),
        "SpawnAnim plays with the invoker/anim house"
    );
    assert_eq!(
        spawn.as_ref().map(|(_, owner, _)| *owner),
        Some(None),
        "CreateRandomAnim passes no owned object for SpawnAnim"
    );
    assert_eq!(
        spawn.as_ref().map(|(_, _, invoker)| *invoker),
        Some(Some(invoker)),
        "the invoker itself is stored"
    );
}

#[test]
fn a_next_chain_of_two_create_unit_anims_spawns_both() {
    let (mut sim, rules) = scene_chain(
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nNext=NEXT\n",
        "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n",
    );
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    assert_eq!(
        units(&sim, &rules).len(),
        2,
        "the one-shot latch resets on the Next type switch"
    );
}

#[test]
fn an_unresolvable_create_unit_type_marks_and_draws_nothing() {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::RawCellKey;
    let (mut sim, rules) = scene("CreateUnit=NOSUCH\n");
    let before = sim.scenario_rng.state();
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .bits_at(RawCellKey::Real(20, 10), MovementLayer::Ground),
        0,
        "the constructor marks no sub-cell bits for a null unit type"
    );
    drive(&mut sim, &rules, id);
    assert_eq!(
        sim.scenario_rng.state(),
        before,
        "a null unit type draws no facing"
    );
    assert!(units(&sim, &rules).is_empty());
}

#[test]
fn create_unit_random_facing_draws_one_scenario_value() {
    let (mut sim, rules) = scene("CreateUnit=MTNK\nCreateUnit.Facing=64\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    let checkpoint = sim.scenario_rng.clone();
    let mut reference = checkpoint.clone();
    let expected = reference.next_range_i32_inclusive(0, 255) as u8;
    drive(&mut sim, &rules, id);
    let draws = draws_until(checkpoint, sim.scenario_rng.state());
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    assert_eq!(
        unit.body_facing_dir(sim.session.binary_frame()),
        expected,
        "RandomFacing constructs yes, overriding CreateUnit.Facing"
    );
    // The unit constructor takes one draw of its own on this shared owner;
    // RandomFacing adds exactly one more.
    let (mut control, control_rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.Facing=64\n");
    let control_id = spawn_make(&mut control, &control_rules, Some("Americans"));
    let control_draws = draws_during_drive(&mut control, &control_rules, control_id);
    assert_eq!(
        draws,
        control_draws + 1,
        "one RandomRanged(0, 255) draw for RandomFacing"
    );
}

#[test]
fn create_unit_mission_and_ai_mission_follow_the_owner() {
    let keys = "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
                CreateUnit.Mission=Move\nCreateUnit.AIMission=Hunt\n";
    // rival_arena: Americans is human, Russians is the computer.
    let (mut sim, rules) = scene(keys);
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    assert_eq!(
        unit.mission.queued(),
        MissionId::from_known(MissionType::Move),
        "a human owner queues CreateUnit.Mission (current {:?}, queued {:?})",
        unit.mission.current(),
        unit.mission.queued()
    );

    let (mut sim, rules) = scene(keys);
    let id = spawn_make(&mut sim, &rules, Some("Russians"));
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("spawn");
    assert_eq!(
        unit.mission.queued(),
        MissionId::from_known(MissionType::Hunt),
        "a computer owner queues CreateUnit.AIMission"
    );
}

#[test]
fn create_unit_require_owner_and_civilian_fallback() {
    let (mut sim, rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.RequireOwner=yes\n");
    let id = spawn_make(&mut sim, &rules, None);
    drive(&mut sim, &rules, id);
    assert!(
        units(&sim, &rules).is_empty(),
        "RequireOwner refuses a missing anim house"
    );

    let (mut sim, rules) = scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n");
    add_civilian_house(&mut sim);
    let id = spawn_make(&mut sim, &rules, None);
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("civilian-owned spawn");
    assert_eq!(
        sim.interner.resolve(unit.owner()),
        "Civilians",
        "without RequireOwner a missing owner falls to the civilian side"
    );

    let (mut sim, rules) =
        scene("CreateUnit=MTNK\nCreateUnit.RandomFacing=no\nCreateUnit.RequireOwner=yes\n");
    add_civilian_house(&mut sim);
    let americans = sim.interner.intern("Americans");
    sim.houses
        .get_mut(&americans)
        .expect("Americans house")
        .set_is_defeated_for_test(true);
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    assert!(
        units(&sim, &rules).is_empty(),
        "a defeated anim house counts as missing"
    );
}

#[test]
fn create_unit_spawn_parachuted_in_air_uses_the_paradrop_path() {
    let keys = "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
                CreateUnit.SpawnParachutedInAir=yes\n";
    let (mut sim, rules) = scene(keys);
    let id = spawn_make_at_z(&mut sim, &rules, Some("Americans"), 900);
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("parachuted spawn");
    assert!(
        unit.is_falling_down(),
        "2 CellHeights up with SpawnParachutedInAir uses SpawnParachuted"
    );
    assert_eq!(
        (unit.position.rx(), unit.position.ry()),
        (20, 10),
        "the paradrop lands on the anim's cell"
    );

    let keys = "CreateUnit=MTNK\nCreateUnit.RandomFacing=no\n\
                CreateUnit.SpawnParachutedInAir=yes\nCreateUnit.AlwaysSpawnOnGround=yes\n";
    let (mut sim, rules) = scene(keys);
    let id = spawn_make_at_z(&mut sim, &rules, Some("Americans"), 900);
    drive(&mut sim, &rules, id);
    let unit = sim
        .substrate
        .entities
        .get(units(&sim, &rules)[0])
        .expect("grounded spawn");
    assert!(
        !unit.is_falling_down(),
        "AlwaysSpawnOnGround overrides SpawnParachutedInAir"
    );
    let coords =
        crate::sim::movement::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref());
    assert_eq!(coords.z, 0, "the cell floor");
}

#[test]
fn create_unit_marks_all_infantry_bits_until_the_actual_effects() {
    let (mut sim, rules) = scene("CreateUnit=MTNK\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(20, 10) & 0x1C,
        0x1C,
        "the constructor marks all three infantry sub-cells"
    );
    drive(&mut sim, &rules, id);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(20, 10) & 0x1C,
        0,
        "the actual-effects frame releases them"
    );
}

/// [`scene`] over a rules text that also defines a Fly aircraft (`ORCA`), an
/// `AirportBound` Fly aircraft (`PLANE`), a Jumpjet (`ZEP`) and a BalloonHover
/// jumpjet vehicle (`BHOV`), for `CreateUnit`'s per-locomotor spawn orders.
#[cfg(feature = "phobos")]
fn scene_air(make_lines: &str) -> (Simulation, RuleSet) {
    const AIR_RULES: &str = "[Countries]\n0=Americans\n1=Russians\n2=Civilians\n\
        [Sides]\nCivilian=Civilians\n\
        [VehicleTypes]\n0=MTNK\n1=BHOV\n[InfantryTypes]\n[BuildingTypes]\n0=LAB\n\
        [AircraftTypes]\n0=ORCA\n1=PLANE\n2=ZEP\n\
        [Animations]\n0=MAKE\n1=SPAWN\n\
        [MTNK]\nStrength=100\nSpeed=4\nSight=4\nPrimary=GUN\n\
        [ORCA]\nStrength=200\nSpeed=8\nSight=6\n\
        Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
        [PLANE]\nStrength=200\nSpeed=8\nSight=6\n\
        Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\nAirportBound=yes\n\
        [ZEP]\nStrength=200\nSpeed=8\nSight=6\n\
        Locomotor={92612C46-F71F-11D1-AC9F-006008055BB5}\nJumpjetHeight=400\n\
        [BHOV]\nStrength=200\nSpeed=8\nSight=6\n\
        Locomotor={92612C46-F71F-11D1-AC9F-006008055BB5}\nJumpjetHeight=400\n\
        BalloonHover=yes\n\
        [LAB]\nStrength=1000\nFoundation=1x1\n\
        [Weapons]\n0=GUN\n[GUN]\nDamage=5\nROF=20\nRange=5\nWarhead=WH\nProjectile=PIFF\n\
        [Warheads]\n0=WH\n[WH]\n\
        [PIFF]\nInviso=yes\n\
        [Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\nWheel=100%\n";
    let art_text = format!(
        "[MAKE]\nRate=900\nEnd=1\nLoopCount=1\n{make_lines}\n\
         [SPAWN]\nRate=900\nEnd=1\nLoopCount=1\n"
    );
    let art = IniFile::from_str(&art_text);
    let rules_text = crate::sim::arena_fixture::clear_ground_ini(AIR_RULES);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&rules_text, &art).expect("create-unit rules");
    let mut registry = ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("MAKE", 1);
    registry.bind_anim_frame_count_for_test("SPAWN", 1);
    rules.install_art_data(registry);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x424932);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    (sim, rules)
}

/// The single spawned object of `name`, if any.
#[cfg(feature = "phobos")]
fn spawned<'a>(
    sim: &'a Simulation,
    rules: &RuleSet,
    name: &str,
) -> Option<&'a crate::sim::game_entity::GameEntity> {
    sim.substrate
        .entities
        .values()
        .find(|entity| {
            sim.interner.resolve(entity.type_ref()) == name
                && sim.object_type(entity.type_ref(), rules).is_some()
        })
}

/// `Ext/TechnoType/Body.cpp:674-683`: a Fly spawn on a free cell takes
/// `pFlyLoco->Move_To(pCell->GetCoordsWithBridge())`, so the destination is the
/// cell centre and the aircraft starts moving.
#[cfg(feature = "phobos")]
#[test]
fn a_fly_spawn_on_a_free_cell_is_ordered_to_the_cell_centre() {
    let (mut sim, rules) =
        scene_air("CreateUnit=ORCA\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=1000\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let plane = spawned(&sim, &rules, "ORCA").expect("the ORCA constructs");
    let fly = plane
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.fly_runtime())
        .expect("Fly locomotor");
    assert!(fly.moving(), "Move_To marks the Fly loco moving");
    let destination = fly.destination();
    assert_eq!(
        (destination.x, destination.y),
        (20 * 256 + 128, 10 * 256 + 128),
        "the destination is the cell's centre"
    );
}

/// The same arm with `AirportBound=yes` and an empty cell: native calls
/// `EnterIdleMode(false, true)` instead of `Move_To`, so the aircraft does not
/// start moving.
#[cfg(feature = "phobos")]
#[test]
fn an_airport_bound_fly_spawn_enters_idle_mode() {
    let (mut sim, rules) =
        scene_air("CreateUnit=PLANE\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=1000\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let plane = spawned(&sim, &rules, "PLANE").expect("the PLANE constructs");
    let fly = plane
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.fly_runtime())
        .expect("Fly locomotor");
    assert!(
        !fly.moving(),
        "an AirportBound aircraft idles on a free cell"
    );
}

/// `pCell->GetContent()`: a ground object on the spawn cell also takes the
/// idle arm, so a Fly spawn does not queue a `Move_To` onto it.
#[cfg(feature = "phobos")]
#[test]
fn a_fly_spawn_on_an_occupied_cell_enters_idle_mode() {
    let (mut sim, rules) =
        scene_air("CreateUnit=ORCA\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=1000\n");
    sim.spawn_object("MTNK", "Americans", 20, 10, 0, &rules)
        .expect("a ground object on the spawn cell");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let plane = spawned(&sim, &rules, "ORCA").expect("the ORCA constructs");
    let fly = plane
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.fly_runtime())
        .expect("Fly locomotor");
    assert!(!fly.moving(), "cell content takes the EnterIdleMode arm");
}

/// `Ext/TechnoType/Body.cpp:686-687`: a Jumpjet spawn snaps its locomotion
/// facing to `CreateUnit.Facing` before any movement order.
#[cfg(feature = "phobos")]
#[test]
fn a_jumpjet_spawn_snaps_its_locomotion_facing() {
    let (mut sim, rules) =
        scene_air("CreateUnit=ZEP\nCreateUnit.RandomFacing=no\nCreateUnit.Facing=64\nCreateUnit.SpawnHeight=500\n");
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let zep = spawned(&sim, &rules, "ZEP").expect("the ZEP constructs");
    let frame = sim.session.binary_frame();
    let jumpjet = zep
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.jumpjet_runtime())
        .expect("Jumpjet locomotor");
    assert_eq!(
        jumpjet.flight().facing().current(frame),
        64,
        "LocomotionFacing.SetCurrent(DirStruct(facing))"
    );
}

/// `Ext/TechnoType/Body.cpp:689-695`: a BalloonHover jumpjet is ordered to
/// ascend straight up — `IsMoving = true`, `DestinationCoords = GetCoords()`,
/// and the `FootExt::JumpjetStraightAscend` latch its locomotion consumes
/// (`Ext/Unit/Hooks.Jumpjet.cpp:394-437`).
#[cfg(feature = "phobos")]
#[test]
fn a_balloon_hover_jumpjet_spawn_holds_its_own_coords_and_the_latch() {
    let (mut sim, rules) = scene_air(
        "CreateUnit=BHOV\nCreateUnit.RandomFacing=no\nCreateUnit.SpawnHeight=1000\n",
    );
    let id = spawn_make(&mut sim, &rules, Some("Americans"));
    drive(&mut sim, &rules, id);
    let bhov = spawned(&sim, &rules, "BHOV").expect("the BHOV constructs");
    let jumpjet = bhov
        .locomotor
        .as_ref()
        .and_then(|locomotor| locomotor.jumpjet_runtime())
        .expect("Jumpjet locomotor");
    assert!(jumpjet.moving(), "IsMoving = true");
    let destination = jumpjet.destination();
    assert_eq!(
        (destination.x, destination.y, destination.z),
        (20 * 256 + 128, 10 * 256 + 128, 1000),
        "DestinationCoords = GetCoords() at the spawn height"
    );
    assert!(
        bhov.jumpjet_straight_ascend(),
        "FootExt::JumpjetStraightAscend = true"
    );
}
