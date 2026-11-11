//! Ares firestorm walls through their production owners
//! (`docs/ares-firestorm.md`): the House firestorm's wall updates, the
//! passability and sale gates, the burns of Feet and bullets, and the line
//! building of a placed section.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectilePayload, ProjectileSpawn, ProjectileTrajectory,
    ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
};

const RULES: &str = "[Countries]\n0=Americans\n1=Russians\n\
     [BuildingTypes]\n0=FSWALL\n1=TRENCHA\n2=TRENCHB\n\
     [FSWALL]\nStrength=500\nFirestorm.Wall=yes\nGuardRange=4\nTurret=no\nSight=0\n\
     [TRENCHA]\nStrength=300\nIsTrench=Sand\nGuardRange=4\nSight=0\n\
     [TRENCHB]\nStrength=300\nIsTrench=sand\nGuardRange=4\nSight=0\n\
     [InfantryTypes]\n0=E1\n1=CYBORG\n\
     [E1]\nStrength=125\nSight=0\n[CYBORG]\nStrength=125\nSight=0\nIgnoresFirestorm=yes\n\
     [Animations]\n0=FSACT\n1=FSBURN\n\
     [AudioVisual]\nFirestormActiveAnim=FSACT\nFirestormGroundAnim=FSBURN\n\
     [Warheads]\n0=C4WH\n[C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [CombatDamage]\nC4Warhead=C4WH\n\
     [Projectiles]\n0=SHOT\n[SHOT]\nInviso=yes\n";

fn arena() -> (Simulation, RuleSet) {
    let art = IniFile::from_str(
        "[FSWALL]\nFoundation=1x1\n[TRENCHA]\nFoundation=1x1\n[TRENCHB]\nFoundation=1x1\n\
         [FSACT]\nImage=FSACT\nRate=900\n\
         [FSBURN]\nImage=FSBURN\nRate=900\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &art,
    )
    .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    for anim in ["FSACT", "FSBURN"] {
        rules.bind_anim_frame_count_for_test(anim, 8);
    }
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x10013280);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    (sim, rules)
}

fn house(sim: &Simulation, name: &str) -> InternedId {
    sim.interner.get(name).unwrap()
}

fn frame(sim: &Simulation, id: u64) -> u32 {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .ares_firestorm_wall
        .frame()
}

fn alive(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.is_object_alive() && entity.health.current() > 0)
}

fn burns(sim: &Simulation) -> usize {
    let burn = sim.interner.get("FSBURN");
    sim.substrate
        .anims
        .iter()
        .filter(|(_, anim)| Some(anim.type_id()) == burn)
        .count()
}

fn firestorm(sim: &mut Simulation, rules: &RuleSet, owner: &str, active: bool) {
    let owner = house(sim, owner);
    sim.ares_set_firestorm_state(owner, active, rules, None, FrameEffects::default());
}

/// SetFirestormState (`10022590`) updates each section of the House
/// (`UpdateFirewall(true)`, `10013280`): the connections Put found plus 32
/// while powered, and the powered anim in the Special slot of a section
/// that is not a straight piece. A powered section refuses entry (hook
/// `0x51C4C8`) and sale (hook `0x449518`); an unpowered one is ignored by
/// entry and may be sold.
#[test]
fn a_raised_firestorm_powers_its_sections_which_then_block_and_cannot_be_sold() {
    let (mut sim, rules) = arena();
    let west = sim
        .spawn_object("FSWALL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let east = sim
        .spawn_object("FSWALL", "Americans", 11, 10, 0, &rules)
        .unwrap();
    let man = sim
        .spawn_object("E1", "Russians", 13, 13, 0, &rules)
        .unwrap();
    assert_eq!(
        (frame(&sim, west), frame(&sim, east)),
        (0b0010, 0b1000),
        "east and west neighbours"
    );
    let entry = |sim: &Simulation| {
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        sim.foot_can_enter(
            man,
            cells.lookup((10, 10)),
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            &rules,
            None,
        )
        .unwrap()
    };
    assert_eq!(entry(&sim), 0);
    assert!(crate::sim::production::can_sell_building(
        &sim, &rules, west
    ));

    firestorm(&mut sim, &rules, "Americans", true);
    assert_eq!((frame(&sim, west), frame(&sim, east)), (34, 40));
    let anim = sim
        .substrate
        .entities
        .get(west)
        .unwrap()
        .building_art
        .anim_slots()[10]
        .expect("the powered anim");
    let anim = sim.substrate.anims.get(anim).unwrap();
    assert!(anim.is_building_anim());
    assert_eq!(anim.building_slot(), Some((west, 10)));
    assert_eq!(entry(&sim), 7);
    assert!(!crate::sim::production::can_sell_building(
        &sim, &rules, west
    ));

    firestorm(&mut sim, &rules, "Americans", false);
    assert_eq!((frame(&sim, west), frame(&sim, east)), (0b0010, 0b1000));
    assert!(
        sim.substrate
            .entities
            .get(west)
            .unwrap()
            .building_art
            .anim_slots()[10]
            .is_none()
    );
    assert_eq!(entry(&sim), 0);
}

/// A powered section burns the Feet in its cell (ImmolateVictims, end of
/// `10013280`) with its Health as `C4Warhead=` damage and the ground anim,
/// sparing an `IgnoresFirestorm=` type; a Foot that later stands there burns
/// at its own update (hook `0x4DA53E`).
#[test]
fn a_powered_section_burns_the_feet_in_its_cell_but_spares_ignores_firestorm() {
    let (mut sim, rules) = arena();
    sim.spawn_object("FSWALL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let man = sim
        .spawn_object("E1", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let cyborg = sim
        .spawn_object("CYBORG", "Russians", 10, 10, 0, &rules)
        .unwrap();
    assert_eq!(burns(&sim), 0);
    firestorm(&mut sim, &rules, "Americans", true);
    assert!(!alive(&sim, man));
    assert!(alive(&sim, cyborg));
    assert_eq!(burns(&sim), 1);

    let late = sim.with_object_placement_scope(|sim| {
        sim.spawn_object("E1", "Russians", 10, 10, 0, &rules)
            .unwrap()
    });
    assert!(alive(&sim, late));
    sim.ares_foot_firestorm(late, &rules, None, FrameEffects::default());
    assert!(!alive(&sim, late));
    assert_eq!(burns(&sim), 2);
}

fn shot(source_id: u64, origin: ProjectileCoord, target: (u16, u16)) -> ProjectileSpawn {
    ProjectileSpawn {
        native_unique_id: 0,
        line_trail: None,
        flat: false,
        source_id,
        origin,
        target: ProjectileTarget::Cell {
            rx: target.0,
            ry: target.1,
        },
        initial_target_position: ProjectileCoord::new(
            i32::from(target.0) * 256 + 128,
            i32::from(target.1) * 256 + 128,
            0,
        ),
        payload: ProjectilePayload::new(10, InternedId::from_index(0), None, false),
        speed_leptons_per_frame: 128,
        velocity: ProjectileVelocity::new(128, 0, 0),
        trajectory: ProjectileTrajectory::Straight,
        guidance: None,
        visual: ProjectileVisualState::new(0, 0, 0),
        arm_frames: 0,
        fuse_frames: None,
        ranged_fuse: false,
        tracks_target: false,
        target_expiry: TargetExpiryPolicy::Expire,
        collision: ProjectileCollisionPolicy::NONE,
    }
}

/// TechnoClass_Fire_FSW (`10024C00`) retargets a shot whose path crosses a
/// powered section of another House to the cell's first object and marks
/// the firer (`+0x50C`); the Inviso arm of `1001A940` burns such a shot
/// before its obstacle walk and UnInits it. A firer's own section, or one
/// whose firestorm is down, lets the shot pass.
#[test]
fn a_powered_section_of_another_house_takes_a_shot_across_it() {
    let (mut sim, rules) = arena();
    let wall = sim
        .spawn_object("FSWALL", "Russians", 4, 2, 0, &rules)
        .unwrap();
    let gunner = sim
        .spawn_object("E1", "Americans", 0, 2, 0, &rules)
        .unwrap();
    let origin = ProjectileCoord::new(128, 2 * 256 + 128, 0);
    let target = ProjectileCoord::new(7 * 256 + 128, 2 * 256 + 128, 0);
    let americans = Some(house(&sim, "Americans"));
    let russians = Some(house(&sim, "Russians"));
    assert_eq!(
        sim.ares_find_first_firestorm(origin, target, americans, &rules),
        None,
        "the firestorm is down"
    );
    firestorm(&mut sim, &rules, "Russians", true);
    assert_eq!(
        sim.ares_find_first_firestorm(origin, target, americans, &rules),
        Some((4, 2))
    );
    assert_eq!(
        sim.ares_find_first_firestorm(origin, target, russians, &rules),
        None,
        "the firer's own section"
    );

    let bullet = sim.substrate.allocate_stable_id();
    sim.admit_projectile(bullet, shot(gunner, origin, (7, 2)));
    sim.ares_fire_at_firestorm(bullet, origin, target, &rules);
    assert_eq!(
        sim.projectiles.get(bullet).unwrap().target(),
        ProjectileTarget::Entity(wall)
    );
    assert!(
        sim.substrate
            .entities
            .get(gunner)
            .unwrap()
            .passive_scan
            .acquired()
    );

    let before = burns(&sim);
    assert!(sim.ares_inviso_firestorm(bullet, target, &rules));
    sim.flush_pending_delete();
    assert!(sim.projectiles.get(bullet).is_none(), "UnInit'ed");
    assert_eq!(burns(&sim), before + 1);
}

/// buildLines (`100113D0`) after a section's placement fills the gap to a
/// same-House section within `GuardRange / 256` cells with new sections
/// queued for Construction; the placement shadow (`0x006D59D0` under the
/// Ares hooks) marks the same cells. A building of another House in the
/// way leaves the line out.
#[test]
fn a_placed_section_fills_the_gap_to_a_linked_section() {
    let (mut sim, rules) = arena();
    sim.spawn_object("FSWALL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let owner = house(&sim, "Americans");
    let object = rules.object("FSWALL").unwrap();
    let terrain = sim.resolved_terrain.clone().unwrap();
    let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(&terrain);
    assert_eq!(
        sim.ares_firestorm_placement_shadow(object, (13, 10), owner, &rules, None, &cells),
        vec![(12, 10), (11, 10)]
    );
    let placed = sim
        .spawn_object("FSWALL", "Americans", 13, 10, 0, &rules)
        .unwrap();
    sim.ares_build_lines(
        placed,
        (13, 10),
        owner,
        &rules,
        None,
        FrameEffects::default(),
    );
    let sections: Vec<_> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| sim.interner.resolve(entity.type_ref()) == "FSWALL" && !entity.in_limbo())
        .map(|entity| {
            (
                (entity.position.rx(), entity.position.ry()),
                entity.mission.effective().known(),
            )
        })
        .collect();
    assert_eq!(sections.len(), 4);
    for cell in [(11, 10), (12, 10)] {
        assert!(sections.iter().any(|&(at, mission)| at == cell
            && mission == Some(crate::sim::mission::MissionType::Construction)));
    }

    sim.spawn_object("FSWALL", "Americans", 20, 20, 0, &rules)
        .unwrap();
    sim.spawn_object("FSWALL", "Russians", 22, 20, 0, &rules)
        .unwrap();
    let blocked = sim
        .spawn_object("FSWALL", "Americans", 23, 20, 0, &rules)
        .unwrap();
    let before = sim.substrate.entities.len();
    sim.ares_build_lines(
        blocked,
        (23, 20),
        owner,
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.len(),
        before,
        "another House's section"
    );
}

/// buildLines (`100113D0`) links an `IsTrench` type to a trench of its own
/// kind (`sameTrench`): the kind names compare case-sensitively, and a
/// firestorm section is no trench.
#[test]
fn a_placed_trench_fills_the_gap_only_to_its_own_kind() {
    let (mut sim, rules) = arena();
    let owner = house(&sim, "Americans");
    let count = |sim: &Simulation| {
        sim.substrate
            .entities
            .values()
            .filter(|entity| sim.interner.resolve(entity.type_ref()) == "TRENCHA")
            .count()
    };
    sim.spawn_object("TRENCHA", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let placed = sim
        .spawn_object("TRENCHA", "Americans", 13, 10, 0, &rules)
        .unwrap();
    sim.ares_build_lines(
        placed,
        (13, 10),
        owner,
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(count(&sim), 4);

    sim.spawn_object("TRENCHB", "Americans", 10, 20, 0, &rules)
        .unwrap();
    sim.spawn_object("FSWALL", "Americans", 16, 20, 0, &rules)
        .unwrap();
    let unlinked = sim
        .spawn_object("TRENCHA", "Americans", 13, 20, 0, &rules)
        .unwrap();
    sim.ares_build_lines(
        unlinked,
        (13, 20),
        owner,
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        count(&sim),
        5,
        "another trench kind and a firestorm section"
    );
}

/// SetFirestormState's closing Update_Pathfinding_2 (`0x00586990`) recalcs
/// each section's cell, and RecalcZoneType's building arm (Ares
/// `10014B70`, hook `0x483D94`) makes it Impassable while the House's
/// firestorm is up; the retained zone class follows. Dropping the
/// firestorm gives the cell back its ground class.
#[test]
fn a_powered_section_cell_takes_the_impassable_zone_class() {
    use crate::map::resolved_terrain::zone_class;
    let (mut sim, rules) = arena();
    let _west = sim
        .spawn_object("FSWALL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let class = |sim: &Simulation, cell: (u16, u16)| {
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let cached = terrain.cell(cell.0, cell.1).unwrap().zone_type;
        let retained = sim
            .zone_grid
            .as_deref()
            .unwrap()
            .base_movement_class_at(cell.0, cell.1)
            .unwrap();
        (cached, retained)
    };
    assert_eq!(
        class(&sim, (10, 10)),
        (zone_class::GROUND, zone_class::GROUND)
    );

    firestorm(&mut sim, &rules, "Americans", true);
    assert_eq!(
        class(&sim, (10, 10)),
        (zone_class::IMPASSABLE, zone_class::IMPASSABLE)
    );
    assert_eq!(
        class(&sim, (11, 10)),
        (zone_class::GROUND, zone_class::GROUND)
    );

    firestorm(&mut sim, &rules, "Americans", false);
    assert_eq!(
        class(&sim, (10, 10)),
        (zone_class::GROUND, zone_class::GROUND)
    );
}
