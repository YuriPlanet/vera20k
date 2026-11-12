//! Ares 3.0 gameplay bugfixes (`ares-src/src/Misc/Bugfixes.cpp`) through the
//! production owners; each test names its hook. The owners' own test modules
//! hold the fixes that need their private state.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::DriveCoord;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

const RULES: &str = "\
[General]
AnimToInfantry=GI
Engineer=ENGI
[InfantryTypes]
0=GI
1=ENGI
2=CLEG
[VehicleTypes]
0=TANK
1=SIMPLE
[AircraftTypes]
0=JET
[BuildingTypes]
0=SHED
1=COIL
2=CLKB
[Warheads]
0=AP
1=HE
[Animations]
0=EXPA
1=EXPB
[GI]
Strength=100
Speed=4
Deployer=yes
UndeployDelay=0
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[ENGI]
Strength=75
Speed=4
Engineer=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[CLEG]
Strength=125
Speed=4
Teleporter=yes
Locomotor={4A582747-9839-11D1-B709-00A024DDAFD1}
[TANK]
Strength=400
Speed=4
ROT=5
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
Primary=Gun
[SIMPLE]
Strength=400
Speed=4
IsSimpleDeployer=yes
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[JET]
Strength=200
Speed=12
Cloakable=yes
Primary=Gun
[SHED]
Strength=1000
Crewed=yes
Crew.EngineerChance=100
[COIL]
Strength=1000
Overpowerable=yes
[CLKB]
Strength=1000
Cloakable=yes
[Gun]
Damage=30
ROF=50
Range=5
Projectile=Invisible
Warhead=AP
[Invisible]
Inviso=yes
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[HE]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
AnimList=EXPA,EXPB
";

fn rules() -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(RULES)).expect("bugfix rules")
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// BuildingClass_Init_Cloakable (1005C4F0, hook 0x442CE0) and
/// AircraftClass_Init_Cloakable (1005C2D0, hook 0x413FA3): a building's and
/// an aircraft's `Cloakable=` seed their stealth ability as a unit's does.
#[test]
fn a_cloakable_building_or_aircraft_type_seeds_its_stealth() {
    let rules = rules();
    let seeds = |category, kind: &str| {
        crate::sim::cloak_disguise::type_seeds_stealth(
            category,
            rules.object(kind).unwrap(),
            crate::rules::ARES_BUILD,
        )
    };
    assert!(seeds(EntityCategory::Structure, "CLKB"));
    assert!(seeds(EntityCategory::Aircraft, "JET"));
    assert!(!seeds(EntityCategory::Structure, "SHED"));
}

/// AnimClass_Update (1005C3E0, hook 0x424B23): a MakeInfantry anim whose
/// infantryman cannot be placed ends, and the infantryman is UnInit instead
/// of left in limbo for the next try.
#[test]
fn a_refused_make_infantry_anim_ends_with_its_infantryman() {
    let mut rules = rules();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[GENDEATH]\nRate=900\nMakeInfantry=0\n",
    ));
    art.bind_anim_frame_count_for_test("GENDEATH", 20);
    rules.replace_art_registry_for_test(art);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x424B23);
    sim.substrate.raw_cell_occupation.mark_ground(
        10,
        10,
        crate::sim::cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT,
    );
    let world = crate::sim::anim_class::AnimWorldCoord {
        x: 10 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    let (rx, ry, sub_x, sub_y, level) = world.to_cell_sub_z();
    let type_id = sim.interner.intern("GENDEATH");
    let descriptor =
        crate::sim::components::AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level);
    let anim = sim
        .spawn_anim_at_world(&rules, descriptor, world)
        .expect("the MakeInfantry anim");
    let russians = sim.interner.get("Russians").unwrap();
    sim.substrate.set_anim_owner_house(anim, russians);
    let before = sim.substrate.entities.keys_sorted();

    assert!(sim.anim_make_infantry(anim, 0, &rules, None, FrameEffects::default()));
    let created: Vec<u64> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(created.len(), 1, "the GI was constructed");
    let gi = sim.substrate.entities.get(created[0]).unwrap();
    assert!(!gi.is_object_alive(), "and UnInit, not left in limbo");
}

/// BuildingClass_Mi_Selling_EngineerFreeze (1005C550, hook 0x44A5F0): with
/// every pick an Engineer, the first survivor is one and the others are left
/// out after nine more picks each, where the original picks forever.
#[test]
fn a_sale_that_only_picks_engineers_lets_one_out() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x44A5F0);
    let shed = spawn(&mut sim, &rules, "SHED", "Russians", 10, 10);
    let before = sim.substrate.entities.keys_sorted();
    sim.spawn_sale_crew(
        &rules,
        None,
        shed,
        3,
        &[(10, 10), (11, 10), (10, 11)],
        FrameEffects::default(),
    );
    let crew: Vec<u64> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(crew.len(), 1);
    let engineer = sim.substrate.entities.get(crew[0]).unwrap();
    assert_eq!(sim.interner.resolve(engineer.type_ref()), "ENGI");
}

/// BuildingClass_Mi_Selling_Crew (1005C530, hook 0x44A8A2): once the sale's
/// survivors are out the building has none left for a death in the pack-up.
#[test]
fn a_sale_past_its_survivors_leaves_none_for_its_death() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x44A8A2);
    let shed = spawn(&mut sim, &rules, "SHED", "Russians", 10, 10);
    crate::sim::production::begin_selling(&mut sim, &rules, shed, false);
    assert!(sim.substrate.entities.get(shed).unwrap().building_down());
    crate::sim::production::sell_stage_one(&mut sim, &rules, None, shed, FrameEffects::default());
    assert!(
        sim.substrate
            .entities
            .get(shed)
            .unwrap()
            .no_survivor
            .is_set()
    );
    assert_eq!(sim.building_survivor_count(&rules, shed), 0);
}

/// BulletClass_Fire_Overpower (1005C650, hook 0x4693B0): a vehicle's
/// ElectricAssault shot charges an Overpowerable building too.
#[test]
fn a_vehicle_charges_a_coil() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4693B0);
    let coil = spawn(&mut sim, &rules, "COIL", "Russians", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Russians", 13, 10);
    sim.substrate
        .electric_assault_detonation(tank, Some(coil), crate::rules::ARES_BUILD);
    assert_eq!(
        sim.substrate
            .entities
            .get(coil)
            .unwrap()
            .tesla_charge
            .chargers(),
        &[tank]
    );
}

/// Networking_RespondToEvent_Selling (1005CE40, hook 0x4C6DDB): an object
/// already on Selling with no archive keeps none.
#[test]
fn a_selling_building_takes_no_new_archive() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4C6DDB);
    let shed = spawn(&mut sim, &rules, "SHED", "Americans", 10, 10);
    let tank = spawn(&mut sim, &rules, "TANK", "Americans", 14, 10);
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(shed, MissionId::from_known(MissionType::Selling), now)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &Command::ArchiveObject {
            entity_id: shed,
            target_id: tank,
        },
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate.entities.get(shed).unwrap().archive_target(),
        None
    );
}

/// FootClass_SetLocation_Parasite (1005CA50, hook 0x4DB87E): the parasite
/// eating a Foot takes the Foot's new Location.
#[test]
fn a_parasite_moves_with_its_victim() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4DB87E);
    let victim = spawn(&mut sim, &rules, "TANK", "Americans", 10, 10);
    let eater = spawn(&mut sim, &rules, "TANK", "Russians", 14, 14);
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .parasite
        .set_eater_for_test(Some(eater));
    let to = DriveCoord {
        x: 11 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    crate::sim::movement::ground_pose::foot_set_location(
        &mut sim.substrate.entities,
        victim,
        to,
        &rules,
        &sim.interner,
    );
    let eater = sim.substrate.entities.get(eater).unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&eater.position),
        to
    );
}

/// InfantryClass_Remove_Teleport (1005CD50, hook 0x51DF27): a Teleporter's
/// Limbo clears its Teleport locomotor's Marked coordinate (`+0x28`).
#[test]
fn a_teleporter_forgets_its_marked_cell_in_limbo() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x51DF27);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Americans", 10, 10);
    let marked = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(cleg)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.teleport_runtime())
            .and_then(|runtime| runtime.resolved_destination())
    };
    sim.substrate
        .entities
        .get_mut(cleg)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .teleport_runtime_mut()
        .unwrap()
        .set_resolved_destination(DriveCoord {
            x: 12 * 256 + 128,
            y: 10 * 256 + 128,
            z: 0,
        });
    assert!(marked(&sim).is_some());
    let _ = sim.techno_limbo_with_context(cleg, UninitContext::new(&rules, None));
    assert_eq!(marked(&sim), None);
}

/// InfantryClass_Mi_Unload_Undeploy (1005CD20, hook 0x51F716): a deployed
/// man's Unload returns after one frame instead of Foot Unload's 450.
#[test]
fn a_deployed_man_undeploys_with_a_one_frame_delay() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x51F716);
    let gi = spawn(&mut sim, &rules, "GI", "Americans", 10, 10);
    sim.substrate
        .entities
        .get_mut(gi)
        .unwrap()
        .mission_leaf
        .set_infantry_doing_verified(27)
        .unwrap();
    assert_eq!(
        sim.infantry_mission_unload(gi, &rules, FrameEffects::default()),
        Ok(1)
    );
}

/// TechnoClass_CanDeploySlashUnload_Immobile (1005D1C0, hook 0x700E47): a
/// falling unit cannot deploy.
#[test]
fn a_falling_unit_cannot_deploy() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x700E47);
    let unit = spawn(&mut sim, &rules, "SIMPLE", "Americans", 10, 10);
    assert!(sim.can_deploy_slash_unload(unit, &rules));
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .set_falling_down_for_test(true);
    assert!(!sim.can_deploy_slash_unload(unit, &rules));
}

/// AbstractClass_Distance2DSquared_1/2 (1005C1A0, hook 0x5F6515; 1005C240,
/// hook 0x5F6560): a squared distance past `i32::MAX` clamps to it instead
/// of wrapping.
#[test]
fn a_far_squared_distance_clamps() {
    use crate::util::native_coord::squared_xy_distance;
    assert_eq!(
        squared_xy_distance([0, 0], [3, 4], crate::rules::ARES_BUILD),
        25
    );
    assert_eq!(
        squared_xy_distance([0, 0], [50_000, 0], crate::rules::ARES_BUILD),
        i32::MAX
    );
}

/// SelectDamageAnimation_FixNegatives (1005D070, hook 0x48A4F9): a negative
/// packet selects its explosion by its magnitude.
#[test]
fn a_negative_packet_selects_an_explosion() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x48A4F9);
    let warhead = rules.warhead("HE").unwrap();
    let coord = crate::sim::projectile::ProjectileCoord {
        x: 10 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    let picked = crate::sim::combat::detonation_anim::select(
        &mut sim, &rules, warhead, -50, 0, coord, false,
    )
    .map(|name| sim.interner.resolve(name).to_owned());
    assert_eq!(picked.as_deref(), Some("EXPB"));
}

/// Arcing_Aircraft (1005C450, hook 0x6F7561): an aircraft measures its
/// direct reach in the plane, so a target under its range in XY is in range
/// from any height.
#[test]
fn an_aircraft_measures_its_reach_in_the_plane() {
    let rules = rules();
    let mut jet = crate::sim::game_entity::GameEntity::test_default(1, "JET", "Americans", 10, 10);
    jet.set_category_for_test(EntityCategory::Aircraft);
    jet.position.set_exact_z_leptons_for_test(Some(2000));
    let interner = crate::sim::intern::test_interner();
    let source = crate::sim::movement::ground_pose::object_get_coords(&jet, None);
    let target = DriveCoord {
        x: source.x + 1000,
        y: source.y,
        z: 0,
    };
    assert!(crate::sim::combat::in_range::mapless_in_range(
        &jet,
        crate::sim::combat::in_range::MaplessTarget::Point(target),
        rules.weapon("Gun").unwrap(),
        &rules,
        &interner,
        None,
    ));
}

/// TeleportLocomotionClass_ILocomotion_MoveTo_Deactivated (1005D3E0, hook
/// 0x71810D): a teleporter whose locomotor is powered off refuses a move and
/// loses its NavCom.
#[test]
fn an_unpowered_teleporter_refuses_a_move() {
    let rules = rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x71810D);
    let cleg = spawn(&mut sim, &rules, "CLEG", "Americans", 10, 10);
    sim.substrate
        .entities
        .get_mut(cleg)
        .unwrap()
        .locomotor
        .as_mut()
        .unwrap()
        .power_off();
    let to = DriveCoord {
        x: 14 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    assert_eq!(
        sim.teleport_move_to(cleg, to, &rules, false, None, FrameEffects::default()),
        Ok(false)
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(cleg)
            .unwrap()
            .navigation
            .nav_com(),
        None
    );
}
