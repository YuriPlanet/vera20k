//! Damage retaliation: ShouldRetaliate gates, the receiver-synchronous
//! retaliation order, grudges and the managers that block it.

use super::*;

/// One hit from `source` (an unarmed tank `source_cells` west of the victim)
/// on a VICTIM tank, through the production receiver. Returns whether the
/// victim turned on the source (`ShouldRetaliate 0x007087C0`, then
/// `ReceiveDamage`'s reach gate `0x00702A58..0x00702B2F`).
struct RetaliationCase {
    human: bool,
    mission: MissionType,
    source_cells: u16,
    /// The source's lepton offset inside its cell (128 is the centre).
    source_sub_x: i32,
    /// The source's health, of Strength=200.
    source_health: i32,
    range_cells: &'static str,
    sight: i32,
    damage: i32,
    verses: &'static str,
    player_return_fire: bool,
    /// The source rides an open-topped transport (id 3, type SOURCE) in its
    /// cell; the victim must then turn on whichever of the two the hook
    /// under test names.
    rider: bool,
}

/// ReceiveDamage702A69 asks CanFireAt before the human Sight fallback. This
/// supplies an already accepted distant hit to the production receiver; it
/// does not claim an E1 can originate that hit across twelve cells. Stock
/// SNIPE/AWP's longer range makes a wall refusal observable outside Sight.
#[test]
fn stock_sniper_damage_retaliation_uses_wall_context_before_sight_fallback() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &fixture.processed_rules,
        Some(&fixture.fixed_art),
    );
    let sniper = rules.object("SNIPE").unwrap();
    assert_eq!(sniper.primary(), Some("AWP"));
    assert_eq!(sniper.sight, 8);
    let weapon = rules.weapon("AWP").unwrap();
    assert_eq!(weapon.range_leptons, 14 * 256);
    assert_eq!(weapon.projectile.as_deref(), Some("InvisibleLow"));
    assert!(rules.projectile("InvisibleLow").unwrap().subject_to_walls);
    assert!(
        !rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .wall
    );
    let wall = registry.id_for_name("GAWALL").unwrap();

    for (blocked, source_x, expected) in [(false, 24, true), (true, 18, true), (true, 24, false)] {
        let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
        let victim = sim
            .spawn_object("SNIPE", "Americans", 12, 12, 0, rules)
            .unwrap();
        let source = sim
            .spawn_object("E1", "Russians", source_x, 12, 0, rules)
            .unwrap();
        let source_owner = sim.substrate.entities.get(source).unwrap().owner();
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
        if blocked {
            overlays.place_overlay(13, 12, wall, 0);
        }
        sim.overlay_grid = Some(overlays);
        assert!(
            combat_targeting::should_retaliate(&sim, rules, victim, source),
            "the non-range refusal gates must admit this source"
        );
        let actor = sim.substrate.entities.get(victim).unwrap();
        assert_eq!(actor.mission.current().known(), Some(MissionType::Guard));
        assert!(actor.attack_target.target().is_none());
        let subject = super::fire_error_world::FireSubject {
            world: &sim,
            rules,
            fog: Some(sim.fog.fire_view(&sim.house_alliances)),
            firer: actor,
            obj: sniper,
            target: Some(TargetKind::Entity(source)),
            weapon_index: 0,
        };
        assert_eq!(
            subject.in_range(),
            !blocked,
            "range is denied only by the wall"
        );
        let before = actor.health.current();
        let warhead = sim.interner.intern("SA");
        sim.commit_direct_damage_receiver(
            rules,
            Some(&registry),
            EntityDamageEvent::area(victim, 1, 0, source, Some(source_owner), warhead),
            &mut crate::sim::world::FrameEffects::default(),
        );
        let actor = sim.substrate.entities.get(victim).unwrap();
        assert!(
            actor.health.current() < before,
            "the supplied hit reached the real receiver"
        );
        assert_eq!(
            actor.attack_target.target(),
            expected.then_some(TargetKind::Entity(source)),
            "blocked={blocked}, source_x={source_x}: human Sight may admit the near source, not the distant one"
        );
    }
}

impl Default for RetaliationCase {
    fn default() -> Self {
        Self {
            human: true,
            mission: MissionType::Guard,
            source_cells: 2,
            source_sub_x: 128,
            source_health: 200,
            range_cells: "8",
            sight: 8,
            damage: 1,
            verses: "100%",
            player_return_fire: false,
            rider: false,
        }
    }
}

fn retaliates(case: RetaliationCase) -> bool {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=IncomingWH\n1=ReturnWH\n\
         [General]\nFixtureOnly=1\n\
         [CombatDamage]\nPlayerReturnFire={}\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nSight={}\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [ReturnGun]\nDamage={}\nROF=50\nRange={}\nWarhead=ReturnWH\n\
         [IncomingWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ReturnWH]\nCellSpread=0\nVerses={v},{v},{v},{v},{v},{v},{v},{v},{v},{v},{v}\n",
        if case.player_return_fire { "yes" } else { "no" },
        case.sight,
        case.damage,
        case.range_cells,
        v = case.verses,
    ));
    let rules = RuleSet::from_ini(&ini).expect("retaliation gate fixture");
    let mut interner = test_interner();
    let source_owner = interner.intern("SourceHouse");
    let victim_owner = interner.intern("VictimHouse");
    let incoming_wh = interner.intern("IncomingWH");
    let incoming_weapon = interner.intern("IncomingGun");
    let (source_rx, victim_rx) = (20 - case.source_cells, 20);

    let mut entities = EntityStore::new();
    let mut source = make_entity(1, "SOURCE", source_rx, 5, case.source_health);
    source.set_owner_for_test(source_owner);
    source.set_type_ref_for_test(interner.intern("SOURCE"));
    source
        .position
        .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(
            case.source_sub_x,
        ));
    source.lifecycle.set_cell_marked_for_test(true);
    if case.rider {
        source.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::boarded_for_test(
            3, true,
        ));
        let mut transport = make_entity(3, "SOURCE", source_rx, 5, case.source_health);
        transport.set_owner_for_test(source_owner);
        transport.set_type_ref_for_test(interner.intern("SOURCE"));
        transport.lifecycle.set_cell_marked_for_test(true);
        entities.insert(transport);
    }
    entities.insert(source);
    let mut victim = make_entity(2, "VICTIM", victim_rx, 5, 100);
    victim.set_owner_for_test(victim_owner);
    victim.set_type_ref_for_test(interner.intern("VICTIM"));
    victim.lifecycle.set_cell_marked_for_test(true);
    set_mission(&mut victim, case.mission, MissionId::NONE);
    victim.body_facing.snap(0xC000, 0);
    entities.insert(victim);

    let mut occupancy = OccupancyGrid::new();
    for (id, rx) in [(1, source_rx), (2, victim_rx)] {
        occupancy.add(
            rx,
            5,
            id,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
    }
    let detonation = ProjectileDetonation {
        projectile_id: 77,
        source_id: 1,
        target: ProjectileTarget::Entity(2),
        impact: ProjectileCoord::new(i32::from(victim_rx) * 256 + 128, 5 * 256 + 128, 0),
        payload: payload_from_weapon(10, incoming_wh, incoming_weapon, &rules, &mut interner),
        reason: ProjectileDetonationReason::ReachedTarget,
    };
    let mut houses = crate::sim::houses::Houses::default();
    houses.register(HouseState::new(victim_owner, 0, None, case.human, 0, 10));
    houses.register(HouseState::new(source_owner, 1, None, false, 0, 10));
    tick_detonations(
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &HouseAllianceMap::new(),
        0,
        &[2],
        &[detonation],
        &mut SimRng::new(11),
        &mut SimRng::new(13),
    );
    let source = if case.rider { 3 } else { 1 };
    entities
        .get(2)
        .and_then(|victim| victim.attack_target.target())
        .is_some_and(|attack| attack == TargetKind::Entity(source))
}

/// Ares 3.0 TechnoClass_ReceiveDamage_OpenTopped (1004AD10, hook 0x702A38):
/// a hit from a rider of an open-topped transport makes the victim
/// retaliate against the transport.
#[test]
fn ares_retaliation_turns_on_an_open_topped_riders_transport() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    assert!(retaliates(RetaliationCase {
        rider: true,
        ..Default::default()
    }));
}

/// `ShouldRetaliate 0x007089E8..0x00708A26`: unless `PlayerReturnFire=`, a
/// human's unit retaliates only on Guard, Area Guard or Patrol. A computer's
/// unit, or any unit under `PlayerReturnFire=yes`, retaliates on the move.
#[test]
fn gsi_04_07_a_human_unit_on_the_move_does_not_retaliate() {
    for mission in [
        MissionType::Guard,
        MissionType::AreaGuard,
        MissionType::Patrol,
    ] {
        assert!(
            retaliates(RetaliationCase {
                mission,
                ..Default::default()
            }),
            "{mission:?}"
        );
    }
    for mission in [MissionType::Move, MissionType::Attack, MissionType::Sticky] {
        assert!(
            !retaliates(RetaliationCase {
                mission,
                ..Default::default()
            }),
            "{mission:?}"
        );
    }
    assert!(retaliates(RetaliationCase {
        mission: MissionType::Move,
        human: false,
        ..Default::default()
    }));
    assert!(retaliates(RetaliationCase {
        mission: MissionType::Move,
        player_return_fire: true,
        ..Default::default()
    }));
}

/// `ReceiveDamage 0x00702A58..0x00702B2F`: a human's unit turns on a source
/// beyond its weapon's range only if the source is within `(Sight + 0.5) *
/// 256` leptons; a computer's always does. So a player's tank on Guard does
/// not charge an artillery piece it cannot see.
#[test]
fn gsi_04_07_a_human_unit_does_not_charge_an_unseen_shooter() {
    let far = |human, source_cells| RetaliationCase {
        human,
        source_cells,
        range_cells: "1",
        sight: 3,
        ..Default::default()
    };
    // Sight 3: 896 leptons; two cells (512) is seen, four (1024) is not.
    assert!(retaliates(far(true, 2)));
    assert!(!retaliates(far(true, 4)));
    assert!(retaliates(far(false, 4)));
    // The compare is inclusive and reads the ftol'd Sqrt_Approx distance: a
    // source whose distance comes out at exactly 896 is seen, 897 is not. Walk
    // raw offsets across the boundary from the victim's centre, x = 20 * 256
    // + 128; both sides must occur.
    let victim_x = 20 * 256 + 128;
    let mut distances = Vec::new();
    for offset in 896..=900 {
        let source_x = victim_x - offset;
        let distance = crate::util::native_x87::distance_3d_leptons(
            [source_x, 5 * 256 + 128, 0],
            [victim_x, 5 * 256 + 128, 0],
        );
        let case = RetaliationCase {
            source_sub_x: source_x % 256,
            ..far(true, (20 - source_x / 256) as u16)
        };
        assert_eq!(
            retaliates(case),
            distance <= 896,
            "offset {offset}, distance {distance}"
        );
        distances.push(distance);
    }
    assert!(
        distances.contains(&896) && distances.contains(&897),
        "{distances:?}"
    );
    // In range needs no sight.
    assert!(retaliates(RetaliationCase {
        source_cells: 4,
        range_cells: "5",
        sight: 1,
        ..Default::default()
    }));
}

/// Stock `ShouldRetaliate 0x00708AF7..0x00708B09` refuses only Verses at or below
/// the single `0.01`: a 1% warhead still retaliates, a bare `0.005` (which
/// GetFireError's zero-Verses test lets through; `0.5%` would parse to 0) and
/// 0% do not. And a weapon whose
/// `Damage + AmbientDamage` is not positive never does (`0x007088A7`): zero
/// damage, or a healer even against a damaged source, which GetFireError's
/// heal test would allow.
#[test]
fn gsi_04_07_retaliation_verses_and_healer_gates() {
    for ares_way in [false, true] {
        let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
        // Ares3.0 parser100523E0 and retaliation hook100543E0 replace the stock
        // threshold: tools/rules_oracle/ares_armor.json `parse_1%` has Retaliate=false.
        assert_eq!(
            retaliates(RetaliationCase {
                verses: "1%",
                ..Default::default()
            }),
            !ares_way,
        );
        assert!(!retaliates(RetaliationCase {
            verses: "0.005",
            ..Default::default()
        }));
        assert!(!retaliates(RetaliationCase {
            verses: "0%",
            ..Default::default()
        }));
        assert!(!retaliates(RetaliationCase {
            damage: 0,
            ..Default::default()
        }));
        for source_health in [200, 100] {
            assert!(
                !retaliates(RetaliationCase {
                    damage: -5,
                    source_health,
                    ..Default::default()
                }),
                "{source_health}"
            );
        }
    }
}

const GATE_SOURCE: u64 = 1;
const GATE_VICTIM: u64 = 2;

/// A world for [`combat_targeting::should_retaliate`] alone: `source_type`
/// (house Source, a computer) two cells west of `victim_type` (house Victim,
/// on Guard).
fn retaliation_gate_world(
    victim_type: &str,
    victim_category: EntityCategory,
    source_type: &str,
    source_category: EntityCategory,
    human: bool,
) -> (crate::sim::world::Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=C4GUY\n1=E1\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=SRCBLDG\n1=EMPBLDG\n2=CAGAS\n\
         [Warheads]\n0=ReturnWH\n\
         [General]\nFixtureOnly=1\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [SRCBLDG]\nStrength=200\nArmor=concrete\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nSight=8\nPrimary=ReturnGun\n\
         [C4GUY]\nStrength=100\nArmor=none\nSpeed=4\nSight=8\nPrimary=ReturnGun\nC4=yes\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=ReturnGun\nOccupyWeapon=ReturnGun\n\
         [EMPBLDG]\nStrength=100\nArmor=concrete\nPrimary=ReturnGun\nEMPulseCannon=yes\n\
         [CAGAS]\nStrength=800\nArmor=wood\nCanBeOccupied=yes\nCanOccupyFire=yes\nMaxNumberOccupants=5\n\
         [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
         [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("retaliation gate fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let victim_owner = sim.interner.intern("Victim");
    let source_owner = sim.interner.intern("Source");
    sim.houses
        .register(HouseState::new(victim_owner, 0, None, human, 0, 10));
    sim.houses
        .register(HouseState::new(source_owner, 1, None, false, 0, 10));
    for (id, type_name, category, owner, rx, health) in [
        (
            GATE_SOURCE,
            source_type,
            source_category,
            source_owner,
            18,
            200,
        ),
        (
            GATE_VICTIM,
            victim_type,
            victim_category,
            victim_owner,
            20,
            100,
        ),
    ] {
        let mut entity = GameEntity::test_default(id, type_name, "Test", rx, 5);
        entity.set_owner_for_test(owner);
        entity.set_type_ref_for_test(sim.interner.intern(type_name));
        entity.set_category_for_test(category);
        entity.health.set_for_test(health);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked_for_test(true);
        set_mission(&mut entity, MissionType::Guard, MissionId::NONE);
        sim.substrate.entities.insert(entity);
    }
    (sim, rules)
}

/// `ShouldRetaliate @ 0x007087C0`'s refusals that read world state. Each case
/// changes one fact of a world whose baseline retaliates.
#[test]
fn gsi_04_07_should_retaliate_world_refusals() {
    use combat_targeting::should_retaliate;
    let tank = |human| {
        retaliation_gate_world(
            "VICTIM",
            EntityCategory::Unit,
            "SOURCE",
            EntityCategory::Unit,
            human,
        )
    };
    for human in [false, true] {
        let (sim, rules) = tank(human);
        assert!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            "baseline, human {human}"
        );
    }
    // `0x00708807..0x0070881F`: a draining object refuses only for a house
    // that is not human (`House+0x1EC`).
    for human in [false, true] {
        let (mut sim, rules) = tank(human);
        sim.substrate
            .entities
            .get_mut(GATE_VICTIM)
            .unwrap()
            .drain
            .set_target_for_test(Some(9));
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            human,
            "draining, human {human}"
        );
    }
    // `0x007087EB`: a slave (SlaveOwner `+0x2DC`). GetFireError's T2
    // refuses a slave as well, so this outcome does not isolate the gate.
    let (mut sim, rules) = tank(false);
    sim.substrate
        .entities
        .get_mut(GATE_VICTIM)
        .unwrap()
        .seldom
        .slave = crate::sim::slave_manager::SlaveLink::for_test(Some(9), Vec::new());
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
    // `0x00708899`: the source is disguised to the victim's house as one of
    // its own (vt+0xC8).
    let (mut sim, rules) = tank(false);
    let victim_owner = sim.interner.intern("Victim");
    sim.substrate
        .entities
        .get_mut(GATE_SOURCE)
        .unwrap()
        .seldom
        .disguise
        .get_or_insert_with(Default::default)
        .acquire(0, None, Some(victim_owner));
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
    // `0x00708905..0x007089A5`: a human's C4 infantryman leaves a building
    // alone; a computer's does not.
    for human in [false, true] {
        let (sim, rules) = retaliation_gate_world(
            "C4GUY",
            EntityCategory::Infantry,
            "SRCBLDG",
            EntityCategory::Structure,
            human,
        );
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            !human,
            "C4 against a building, human {human}"
        );
    }
    // `0x00708A2C..0x00708A54`: a member of a `Suicide=` team.
    for suicide in [false, true] {
        let (mut sim, rules) = tank(false);
        crate::sim::team_script_vm::join_team_for_test(&mut sim, &[GATE_VICTIM], suicide, false);
        assert_eq!(
            should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE),
            !suicide,
            "Suicide={suicide}"
        );
    }
    // `0x007088FC`: GetFireError Cant, from an `EMPulseCannon=` building
    // (`BuildingClass::GetFireError` B3 `0x00447F54`).
    let (sim, rules) = retaliation_gate_world(
        "EMPBLDG",
        EntityCategory::Structure,
        "SOURCE",
        EntityCategory::Unit,
        false,
    );
    assert!(!should_retaliate(&sim, &rules, GATE_VICTIM, GATE_SOURCE));
}

/// A garrisoned building fights back with its occupant's weapon: GetWeapon
/// (`BuildingClass::GetWeapon @ 0x004526F0`) answers the occupant's
/// `OccupyWeapon=` for every slot, so `GetWeaponDamageValue(-1)`, SelectWeapon,
/// GetFireError and the Verses test all read it. An empty civilian building
/// has no weapon and does not.
#[test]
fn gsi_04_07_a_garrison_retaliates_with_its_occupants_weapon() {
    let (mut sim, rules) = retaliation_gate_world(
        "CAGAS",
        EntityCategory::Structure,
        "SOURCE",
        EntityCategory::Unit,
        false,
    );
    assert!(!combat_targeting::should_retaliate(
        &sim,
        &rules,
        GATE_VICTIM,
        GATE_SOURCE
    ));
    let occupant_id = 3;
    let mut occupant = GameEntity::test_default(occupant_id, "E1", "Test", 20, 5);
    occupant.set_owner_for_test(sim.substrate.entities.get(GATE_VICTIM).unwrap().owner());
    occupant.set_type_ref_for_test(sim.interner.intern("E1"));
    occupant.set_category_for_test(EntityCategory::Infantry);
    occupant.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::inside_for_test(
        GATE_VICTIM,
        false,
    ));
    sim.substrate.entities.insert(occupant);
    sim.substrate
        .entities
        .get_mut(GATE_VICTIM)
        .unwrap()
        .passenger_role_mut_for_test()
        .occupants_mut()
        .add_for_test(occupant_id);
    assert!(combat_targeting::should_retaliate(
        &sim,
        &rules,
        GATE_VICTIM,
        GATE_SOURCE
    ));
}

#[test]
fn gsi_04_07_damage_retaliation_is_receiver_synchronous_and_uses_mission_override() {
    #[derive(Debug)]
    struct Outcome {
        health: i32,
        current: MissionId,
        suspended: MissionId,
        target: Option<TargetKind>,
        nav_com: Option<crate::sim::components::NavTargetRef>,
        suspended_nav_com: Option<crate::sim::components::NavTargetRef>,
        has_movement: bool,
        fired: Vec<(u64, TargetKind)>,
        immediate_uninit: Vec<u64>,
    }

    fn run(
        mission: MissionType,
        source_present: bool,
        allied: bool,
        victim_health: i32,
    ) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=IncomingWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=200\nArmor=heavy\n\
             [VICTIM]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\n\
             [IncomingGun]\nDamage=10\nRange=8\nWarhead=IncomingWH\n\
             [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
             [IncomingWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [Harvest]\nRetaliate=no\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("retaliation receiver fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("VICTIM");
        let incoming_wh = interner.intern("IncomingWH");
        let incoming_weapon = interner.intern("IncomingGun");

        let mut entities = EntityStore::new();
        let source = marked_entity(1, "SOURCE", 6, 5, 200, source_owner, source_type);
        entities.insert(source);

        let mut victim = marked_entity(2, "VICTIM", 8, 5, victim_health, victim_owner, victim_type);
        set_mission(&mut victim, mission, MissionId::NONE);
        // The victim retaliates WEST at the source; give it that hull heading up
        // front so the turretless body gate (`UnitClass::GetFireError @
        // 0x00740FD0` step 17) does not spend this pass turning. The subject
        // here is the inline mission Override, not turn-to-fire.
        victim.body_facing.snap(0xC000, 0);
        victim
            .navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(9, 5)));
        // Supply the actual class receiver for the NULL destination below.
        victim.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("VICTIM").unwrap(),
                0,
            ),
        );
        assert!(
            victim
                .locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(
                    crate::sim::movement::DriveLocomotionRuntime::default()
                        .with_destination_for_test(Some(crate::sim::components::DriveCoord::cell(
                            9, 5, 0
                        )))
                ))
        );
        entities.insert(victim);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            6,
            5,
            1,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        occupancy.add(
            8,
            5,
            2,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        let detonation = ProjectileDetonation {
            projectile_id: 77,
            source_id: if source_present { 1 } else { RAD_NO_ATTACKER },
            target: ProjectileTarget::Entity(2),
            impact: ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: payload_from_weapon(10, incoming_wh, incoming_weapon, &rules, &mut interner),
            reason: ProjectileDetonationReason::ReachedTarget,
        };
        let mut alliances = HouseAllianceMap::new();
        if allied {
            alliances
                .entry("VICTIMHOUSE".to_string())
                .or_default()
                .insert("SOURCEHOUSE".to_string());
            alliances
                .entry("SOURCEHOUSE".to_string())
                .or_default()
                .insert("VICTIMHOUSE".to_string());
        }
        align_attackers_to_targets(&mut entities);
        let result = tick_detonations(
            &mut entities,
            &mut occupancy,
            &rules,
            &mut interner,
            &mut crate::sim::houses::Houses::default(),
            &alliances,
            0,
            &[2],
            &[detonation],
            &mut SimRng::new(11),
            &mut SimRng::new(13),
        );
        let victim = entities.get(2).expect("deferred storage keeps victim");
        Outcome {
            health: victim.health.current(),
            current: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim.attack_target.target(),
            nav_com: victim.navigation.nav_com(),
            suspended_nav_com: victim.navigation.suspended_nav_com(),
            has_movement: crate::sim::movement::motion_query::is_moving(&victim) == Some(true),
            fired: result
                .consequences
                .fire_events()
                .iter()
                .map(|event| (event.attacker_id, event.target))
                .collect(),
            immediate_uninit: result.consequences.effects().immediate_uninit_ids.clone(),
        }
    }

    let live = run(MissionType::Guard, true, false, 100);
    assert_eq!(live.health, 90);
    assert_eq!(live.current, MissionId::from_known(MissionType::Attack));
    assert_eq!(live.suspended, MissionId::from_known(MissionType::Guard));
    assert_eq!(live.target, Some(TargetKind::Entity(1)));
    assert_eq!(live.nav_com, None);
    assert_eq!(
        live.suspended_nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
    assert!(
        !live.has_movement,
        "NULL destination stops the represented path"
    );
    assert_eq!(
        live.fired,
        vec![(2, TargetKind::Entity(1))],
        "the later live slot reads the inline Override and fires this pass"
    );

    for (blocked, expected_mission) in [
        (
            run(MissionType::Harvest, true, false, 100),
            MissionType::Harvest,
        ),
        (
            run(MissionType::Guard, false, false, 100),
            MissionType::Guard,
        ),
        (run(MissionType::Guard, true, true, 100), MissionType::Guard),
    ] {
        assert_eq!(blocked.current, MissionId::from_known(expected_mission));
        assert_eq!(blocked.health, 90);
        assert!(blocked.target.is_none());
        assert!(blocked.fired.is_empty());
        assert!(blocked.nav_com.is_some());
        assert!(blocked.has_movement);
    }
    let fatal = run(MissionType::Guard, true, false, 10);
    assert_eq!(fatal.health, 0);
    assert_eq!(fatal.target, None);
    assert!(fatal.fired.is_empty());
    assert_eq!(fatal.immediate_uninit, vec![2]);
}

#[test]
fn gsi_04_07_damage_retaliation_peek_rejects_limbo_attacker() {
    fn run(attacker_in_limbo: bool) -> (i32, bool, MissionId, MissionId, Option<TargetKind>) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=200\nArmor=heavy\n\
             [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\n\
             [ReturnGun]\nDamage=1\nRange=8\nWarhead=ReturnWH\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("retaliation peek fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("VICTIM");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut source = make_entity(1, "SOURCE", 6, 5, 200);
        source.set_owner_for_test(source_owner);
        source.set_type_ref_for_test(source_type);
        source.lifecycle.set_in_limbo_for_test(attacker_in_limbo);
        source
            .lifecycle
            .set_cell_marked_for_test(!attacker_in_limbo);
        entities.insert(source);

        let mut victim = marked_entity(2, "VICTIM", 8, 5, 100, victim_owner, victim_type);
        set_mission(&mut victim, MissionType::Guard, MissionId::NONE);
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 10, 0, 1, Some(source_owner), hit_wh);
        commit_hits(
            &[event],
            &mut entities,
            &rules,
            &mut interner,
            &mut crate::sim::houses::Houses::default(),
            0,
            None,
        );
        let victim = entities.get(2).expect("victim survives");
        (
            victim.health.current(),
            victim.seldom.ai_sell_repair.was_attacked_by_enemy(),
            victim.mission.current(),
            victim.mission.suspended(),
            victim.attack_target.target(),
        )
    }

    let illegal = run(true);
    assert_eq!(illegal.0, 90, "ReceiveDamage still commits before the peek");
    assert!(illegal.1, "the surviving hostile-hit postlude still runs");
    assert_eq!(illegal.2, MissionId::from_known(MissionType::Guard));
    assert_eq!(illegal.3, MissionId::NONE);
    assert_eq!(illegal.4, None, "FIRE_ILLEGAL suppresses Override");

    let legal = run(false);
    assert_eq!(legal.0, 90);
    assert!(legal.1);
    assert_eq!(legal.2, MissionId::from_known(MissionType::Attack));
    assert_eq!(legal.3, MissionId::from_known(MissionType::Guard));
    assert_eq!(legal.4, Some(TargetKind::Entity(1)));
}

#[test]
fn gsi_04_07_damage_invulnerability_impact_precedes_warping_and_postlude() {
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=VICTIM\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=HitWH\n\
         [SOURCE]\nStrength=100\nArmor=heavy\nSpeed=6\n\
         [VICTIM]\nStrength=100\nArmor=heavy\nSpeed=6\n\
         [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("invulnerability impact fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules)
        .expect("source spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner();
    let protected = [
        (8, InvulnKind::IronCurtain, false),
        (9, InvulnKind::ForceShield, false),
        (10, InvulnKind::IronCurtain, true),
    ]
    .map(|(rx, kind, warping)| {
        let id = sim
            .spawn_object("VICTIM", "VictimHouse", rx, 5, 0, &rules)
            .expect("protected victim spawns");
        let victim = sim.substrate.entities.get_mut(id).unwrap();
        victim.invulnerability = Some(InvulnerabilityState::new(
            crate::sim::timer::CdTimer::started(0, 100),
            kind,
        ));
        if warping {
            victim.install_teleport_state_for_test(Some(TeleportState::for_test(
                TeleportPhase::Relocate,
                20,
                20,
                1,
            )));
        }
        id
    });
    let healing_id = sim
        .spawn_object("VICTIM", "VictimHouse", 11, 5, 0, &rules)
        .expect("healing control spawns");
    let ignored_id = sim
        .spawn_object("VICTIM", "VictimHouse", 12, 5, 0, &rules)
        .expect("ignore-defenses control spawns");
    for id in [healing_id, ignored_id] {
        sim.substrate.entities.get_mut(id).unwrap().invulnerability =
            Some(InvulnerabilityState::new(
                crate::sim::timer::CdTimer::started(0, 100),
                InvulnKind::IronCurtain,
            ));
    }
    sim.substrate
        .entities
        .get_mut(healing_id)
        .unwrap()
        .health
        .set_for_test(90);
    let hit_wh = sim.interner.intern("HitWH");
    let hits = vec![
        EntityDamageEvent::area(protected[0], 10, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(protected[1], 20, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(protected[2], 30, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::area(healing_id, -10, 0, source_id, Some(source_owner), hit_wh),
        EntityDamageEvent::direct_receiver(
            ignored_id,
            10,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
    ];

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &hits,
        &mut crate::sim::world::FrameEffects::default(),
    );

    for id in protected {
        let victim = sim.substrate.entities.get(id).unwrap();
        assert_eq!(victim.health.current(), 100);
        assert!(!victim.seldom.ai_sell_repair.was_attacked_by_enemy());
        assert_eq!(victim.seldom.damage_smoke_system_id.system(), None);
    }
    assert_eq!(
        sim.substrate
            .entities
            .get(healing_id)
            .unwrap()
            .health
            .current(),
        100,
        "negative healing bypasses IC without an impact"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(ignored_id)
            .unwrap()
            .health
            .current(),
        90,
        "ignoreDefenses bypasses IC without an impact"
    );

    let effects = &sim.combat_light_requests;
    assert_eq!(effects.len(), 3);
    for (index, effect) in effects.iter().enumerate() {
        let CombatLightRequest::Impact {
            target_id,
            damage,
            warhead_ref,
            coord,
            force_create,
            flags,
        } = effect
        else {
            panic!("receiver must produce an impact light");
        };
        assert_eq!(*target_id, Some(protected[index]), "receiver order");
        assert_eq!(*damage, [20, 40, 60][index]);
        assert_eq!(*flags, [1, 6, 1][index], "IC versus ForceShield");
        assert_eq!(*warhead_ref, hit_wh);
        assert!(*force_create);
        assert_eq!(
            *coord,
            ProjectileCoord::new((8 + index as i32) * 256 + 128, 5 * 256 + 128, 0),
            "the helper receives the protected target coordinate, not source/impact"
        );
    }
}

#[test]
fn gsi_04_07_damage_receiver_smoke_creation_precedes_retaliation() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=SOURCE\n1=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=HitWH\n1=ReturnWH\n\
         [ParticleSystems]\n0=SparkSys\n1=SmallGreySSys\n\
         [SOURCE]\nStrength=200\nArmor=heavy\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=ReturnGun\nCanRetaliate=yes\nDamageParticleSystems=SparkSys,SmallGreySSys\n\
         [ReturnGun]\nDamage=1\nROF=50\nRange=8\nWarhead=ReturnWH\n\
         [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ReturnWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [SparkSys]\nBehavesLike=Spark\nLifetime=5\n\
         [SmallGreySSys]\nBehavesLike=Smoke\nLifetime=-1\nSpawns=no\nSpawnFrames=10\nSpawnCutoff=13\n\
         [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
    ))
    .expect("damage-Smoke receiver fixture");
    let mut sim = crate::sim::world::Simulation::new();
    let source_id = sim
        .spawn_object("SOURCE", "SourceHouse", 6, 5, 0, &rules)
        .expect("source spawns");
    let victim_id = sim
        .spawn_object("MTNK", "VictimHouse", 8, 5, 0, &rules)
        .expect("Grizzly spawns");
    let source_owner = sim.substrate.entities.get(source_id).unwrap().owner();
    let victim_owner = sim.substrate.entities.get(victim_id).unwrap().owner();
    register_computer_houses(&mut sim.houses, &[source_owner, victim_owner]);
    {
        let victim = sim.substrate.entities.get_mut(victim_id).unwrap();
        victim.health = Health::new(180);
        set_mission(victim, MissionType::Guard, MissionId::NONE);
    }
    let hit_wh = sim.interner.intern("HitWH");
    let before_order = sim.live_object_order_snapshot();
    let before_rng = sim.scenario_rng.logical_state();

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            60,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );

    let victim = sim.substrate.entities.get(victim_id).unwrap();
    assert_eq!(victim.health.current(), 120);
    assert_eq!(
        victim.mission.current(),
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        victim.attack_target.target(),
        Some(TargetKind::Entity(source_id)),
        "retaliation sees the receiver after synchronous smoke creation"
    );
    let system_id = victim
        .seldom
        .damage_smoke_system_id
        .system()
        .expect("yellow crossing attaches a Smoke system");
    let system = sim.particle_systems().get(system_id).unwrap();
    assert_eq!(
        rules.particle_system_type(system.type_id()).name,
        "SmallGreySSys"
    );
    assert_eq!(system.owner_entity(), Some(victim_id));
    assert_eq!(system.target_ref(), None);
    assert_eq!(system.owner_house(), None);
    assert_eq!(
        system.coords(),
        glam::IVec3::new(8 * 256 + 128, 5 * 256 + 128, 0)
    );
    let mut expected_order = before_order;
    expected_order.push(system_id);
    assert_eq!(sim.live_object_order_snapshot(), expected_order);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before_rng,
        "reverse filtering leaves one Smoke choice, so RandomRanged(0,0) draws nothing"
    );
    // The actual ReceiveDamage-created +310 system follows the same retained
    // owner as the native constructor/AI controls in damage_smoke_following.
    // Frame1 is not an emission frame; moving the source consumes no RNG.
    let moved = crate::sim::components::DriveCoord {
        x: 9 * 256 + 37,
        y: 6 * 256 + 91,
        z: 74,
    };
    sim.substrate
        .entities
        .get_mut(victim_id)
        .unwrap()
        .position
        .put_location(moved);
    sim.session.set_binary_frame_for_test(1);
    assert!(crate::sim::particles::system_ai::tick_particle_system(
        &mut sim,
        &rules,
        system_id,
        crate::sim::world::UninitContext::new(&rules)
    ));
    assert_eq!(
        sim.particle_systems().get(system_id).unwrap().coords(),
        glam::IVec3::new(moved.x, moved.y, moved.z)
    );
    assert_eq!(sim.scenario_rng.logical_state(), before_rng);

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            1,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.particle_systems().len(),
        1,
        "live +0x310 suppresses duplicates"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(system_id)
    );

    sim.commit_noncombat_aoe_hits(
        &rules,
        None,
        &[EntityDamageEvent::area(
            victim_id,
            -61,
            0,
            source_id,
            Some(source_owner),
            hit_wh,
        )],
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .health
            .current(),
        180
    );
    assert!(
        sim.particle_systems()
            .get(system_id)
            .unwrap()
            .done_spawning(),
        "recovery above ConditionYellow invokes mark-only ParticleSystem Destroy"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        Some(system_id),
        "mark-only Destroy retains +0x310 until the system's own Limbo"
    );
    sim.retire_particle_system(system_id, crate::sim::world::UninitContext::new(&rules));
    assert!(sim.particle_systems().get(system_id).is_some());
    assert_eq!(
        sim.substrate
            .entities
            .get(victim_id)
            .unwrap()
            .seldom
            .damage_smoke_system_id
            .system(),
        None,
        "done-empty Limbo clears +0x310 before physical finalization"
    );
    sim.process_pending_delete();
    assert!(sim.particle_systems().get(system_id).is_none());
}

#[test]
fn gsi_04_07_damage_ai_retaliation_keeps_higher_scored_current_target() {
    #[derive(Debug)]
    struct Outcome {
        current_score: i64,
        attacker_score: i64,
        mission: MissionId,
        suspended: MissionId,
        target: TargetKind,
        nav_com: Option<crate::sim::components::NavTargetRef>,
        suspended_nav_com: Option<crate::sim::components::NavTargetRef>,
    }

    fn run(current_id: u64, attacker_id: u64) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=TANY\n1=E1\n\
             [VehicleTypes]\n0=HTNK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=AP\n2=HollowPoint2\n3=SA\n\
             [General]\nMyEffectivenessCoefficientDefault=200\nTargetEffectivenessCoefficientDefault=-200\nTargetSpecialThreatCoefficientDefault=200\nTargetStrengthCoefficientDefault=-200\nTargetDistanceCoefficientDefault=-10\n\
             [HTNK]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=120mm\nCanRetaliate=yes\n\
             [TANY]\nStrength=200\nArmor=flak\nPrimary=DoublePistols\nSpecialThreatValue=1\n\
             [E1]\nStrength=125\nArmor=none\nPrimary=M60\n\
             [120mm]\nDamage=90\nRange=5.75\nWarhead=AP\n\
             [DoublePistols]\nDamage=125\nRange=6\nWarhead=HollowPoint2\n\
             [M60]\nDamage=15\nRange=4\nWarhead=SA\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [AP]\nVerses=25%,25%,15%,75%,100%,100%,65%,45%,60%,60%,100%\n\
             [HollowPoint2]\nVerses=100%,100%,100%,0%,0%,0%,1%,1%,1%,1%,100%\n\
             [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("stock threat-score fixture");
        let mut interner = test_interner();
        let ai_owner = interner.intern("AI");
        let tanya_owner = interner.intern("TanyaHouse");
        let gi_owner = interner.intern("GiHouse");
        let htnk_type = interner.intern("HTNK");
        let tanya_type = interner.intern("TANY");
        let gi_type = interner.intern("E1");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let mut victim = marked_entity(10, "HTNK", 8, 5, 400, ai_owner, htnk_type);
        victim
            .attack_target
            .set_for_test(Some(TargetKind::Entity(current_id)));
        victim
            .navigation
            .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::cell(9, 5)));
        set_mission(&mut victim, MissionType::Guard, MissionId::NONE);
        entities.insert(victim);

        let mut tanya = marked_entity(20, "TANY", 6, 5, 200, tanya_owner, tanya_type);
        tanya.set_category_for_test(EntityCategory::Infantry);
        entities.insert(tanya);

        let mut gi = marked_entity(30, "E1", 10, 5, 125, gi_owner, gi_type);
        gi.set_category_for_test(EntityCategory::Infantry);
        entities.insert(gi);

        let current_score = crate::sim::combat::combat_targeting::calculate_ai_threat_score(
            &entities, 10, current_id, &rules, &interner, None, None, None,
        )
        .map(|score| {
            i64::from(crate::util::native_x87::MaskedX87Chop53::ftol_i32_low_masked(score))
        })
        .expect("current target score");
        let attacker_score = crate::sim::combat::combat_targeting::calculate_ai_threat_score(
            &entities,
            10,
            attacker_id,
            &rules,
            &interner,
            None,
            None,
            None,
        )
        .map(|score| {
            i64::from(crate::util::native_x87::MaskedX87Chop53::ftol_i32_low_masked(score))
        })
        .expect("attacker score");

        let mut houses = computer_houses(&[ai_owner]);
        let source_house = entities.get(attacker_id).expect("attacker").owner();
        let event = EntityDamageEvent::area(10, 1, 0, attacker_id, Some(source_house), hit_wh);
        commit_hits(
            &[event],
            &mut entities,
            &rules,
            &mut interner,
            &mut houses,
            0,
            None,
        );
        let victim = entities.get(10).expect("victim retained");
        Outcome {
            current_score,
            attacker_score,
            mission: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim.attack_target.target().expect("target retained"),
            nav_com: victim.navigation.nav_com(),
            suspended_nav_com: victim.navigation.suspended_nav_com(),
        }
    }

    // Scored on the per-type coefficient set (200 / -200 / 200 / -200 / -10),
    // which `HouseClass+0x1FB` selects for every skirmish house from creation.
    // Tanya: base 100000, C = 200*SpecialThreatValue 1 = +200, A = 200 * AP-vs-
    // flak 25% = +50, D = -200 * (200/200) = -200, B absent (HollowPoint2 is 0%
    // against heavy, so no weapon selects), E = 0 (2 cells, range 5) → 100050.
    // GI: B = -200 * SA-vs-heavy 25% = -50, C = 0, A = 200 * AP-vs-none 25% =
    // +50, D = -200, E = 0 → 99800.
    let keep_tanya = run(20, 30);
    assert_eq!(
        (keep_tanya.current_score, keep_tanya.attacker_score),
        (100_050, 99_800)
    );
    assert_eq!(
        keep_tanya.mission,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(keep_tanya.suspended, MissionId::NONE);
    assert_eq!(keep_tanya.target, TargetKind::Entity(20));
    assert_eq!(
        keep_tanya.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
    assert_eq!(keep_tanya.suspended_nav_com, None);

    let switch_to_tanya = run(30, 20);
    assert_eq!(
        (
            switch_to_tanya.current_score,
            switch_to_tanya.attacker_score
        ),
        (99_800, 100_050)
    );
    assert_eq!(
        switch_to_tanya.mission,
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        switch_to_tanya.suspended,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(switch_to_tanya.target, TargetKind::Entity(20));
    assert_eq!(switch_to_tanya.nav_com, None);
    assert_eq!(
        switch_to_tanya.suspended_nav_com,
        Some(crate::sim::components::NavTargetRef::cell(9, 5))
    );
}

#[test]
fn gsi_04_07_damage_spawn_and_slave_managers_block_retaliation() {
    fn run(slave_manager_shaped: bool) -> (MissionId, MissionId, Option<TargetKind>, bool) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=SLAV\n\
             [VehicleTypes]\n0=SOURCE\n1=CARRIER\n2=SLAVEMASTER\n\
             [AircraftTypes]\n0=HORNET\n\
             [BuildingTypes]\n\
             [Warheads]\n0=HitWH\n1=ReturnWH\n\
             [SOURCE]\nStrength=100\nArmor=heavy\n\
             [CARRIER]\nStrength=100\nArmor=heavy\nPrimary=ReturnGun\nSpawns=HORNET\nSpawnsNumber=1\n\
             [SLAVEMASTER]\nStrength=100\nArmor=heavy\nPrimary=ReturnGun\nEnslaves=SLAV\nSlavesNumber=1\n\
             [HORNET]\nStrength=75\nArmor=light\n\
             [SLAV]\nStrength=100\nArmor=none\n\
             [ReturnGun]\nDamage=1\nRange=8\nWarhead=ReturnWH\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("manager identity fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type_name = if slave_manager_shaped {
            "SLAVEMASTER"
        } else {
            "CARRIER"
        };
        let victim_type = interner.intern(victim_type_name);
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let source = marked_entity(1, "SOURCE", 6, 5, 100, source_owner, source_type);
        entities.insert(source);

        let mut victim = marked_entity(2, victim_type_name, 8, 5, 100, victim_owner, victim_type);
        set_mission(&mut victim, MissionType::Guard, MissionId::NONE);
        if !slave_manager_shaped {
            victim.spawn_manager = crate::sim::spawn_manager::init_spawn_manager(
                rules.object("CARRIER").expect("carrier type"),
                &rules,
                &mut interner,
                0,
            );
            assert!(victim.spawn_manager.is_some(), "live SpawnManager fixture");
        } else {
            let slave_type = rules
                .object("SLAVEMASTER")
                .and_then(|object| object.enslaves.as_deref())
                .expect("resolved Enslaves profile creates native SlaveManager");
            victim.slave_manager = Some(Box::new(crate::sim::slave_manager::SlaveManager::new(
                interner.intern(slave_type),
                [None; 0],
                0,
                0,
                0,
            )));
        }
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 1, 0, 1, Some(source_owner), hit_wh);
        commit_hits(
            &[event],
            &mut entities,
            &rules,
            &mut interner,
            &mut crate::sim::houses::Houses::default(),
            0,
            None,
        );
        let victim = entities.get(2).expect("victim survives");
        assert_eq!(victim.health.current(), 99);
        (
            victim.mission.current(),
            victim.mission.suspended(),
            victim.attack_target.target(),
            victim.spawn_manager.is_some(),
        )
    }

    let carrier = run(false);
    assert_eq!(carrier.0, MissionId::from_known(MissionType::Guard));
    assert_eq!(carrier.1, MissionId::NONE, "no override archives Guard");
    assert_eq!(carrier.2, None);
    assert!(carrier.3, "rejection preserves the live SpawnManager");

    let slave_master = run(true);
    assert_eq!(slave_master.0, MissionId::from_known(MissionType::Guard));
    assert_eq!(
        slave_master.1,
        MissionId::NONE,
        "no override archives Guard"
    );
    assert_eq!(slave_master.2, None);
    assert!(!slave_master.3);
}

#[test]
fn gsi_04_07_damage_full_capture_manager_blocks_retaliation() {
    struct Outcome {
        health: i32,
        mission: MissionId,
        suspended: MissionId,
        target: Option<TargetKind>,
        links: Vec<u64>,
    }

    fn run(link_count: usize) -> Outcome {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=SOURCE\n1=LINK\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=YAPSYT\n\
             [Warheads]\n0=HitWH\n1=Controller\n\
             [SOURCE]\nStrength=100\nArmor=heavy\n\
             [LINK]\nStrength=100\nArmor=heavy\n\
             [YAPSYT]\nStrength=100\nArmor=heavy\nPrimary=MultipleMindControlTower\n\
             [MultipleMindControlTower]\nDamage=3\nRange=7\nWarhead=Controller\n\
             [Controller]\nMindControl=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [HitWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("Psychic Tower manager fixture");
        let mut interner = test_interner();
        let source_owner = interner.intern("SourceHouse");
        let victim_owner = interner.intern("VictimHouse");
        let source_type = interner.intern("SOURCE");
        let victim_type = interner.intern("YAPSYT");
        let link_type = interner.intern("LINK");
        let hit_wh = interner.intern("HitWH");

        let mut entities = EntityStore::new();
        let source = marked_entity(1, "SOURCE", 6, 5, 100, source_owner, source_type);
        entities.insert(source);

        let mut victim = make_structure_entity(2, "YAPSYT", 8, 5, 100, 100);
        victim.set_owner_for_test(victim_owner);
        victim.set_type_ref_for_test(victim_type);
        victim.lifecycle.set_in_limbo_for_test(false);
        victim.lifecycle.set_cell_marked_for_test(true);
        set_mission(&mut victim, MissionType::Guard, MissionId::NONE);
        let links: Vec<u64> = (0..link_count).map(|offset| 10 + offset as u64).collect();
        for (offset, &id) in links.iter().enumerate() {
            let mut controlled = make_entity(id, "LINK", 10 + offset as u16, 5, 100);
            controlled.set_owner_for_test(victim_owner);
            controlled.set_type_ref_for_test(link_type);
            controlled.seldom.mind_control =
                crate::sim::capture_manager::MindControlLink::controlled_by_for_test(2);
            controlled.lifecycle.set_in_limbo_for_test(false);
            controlled.lifecycle.set_cell_marked_for_test(true);
            entities.insert(controlled);
        }
        // The stock tower's link limit (`[MultipleMindControlTower] Damage=3`).
        victim.capture_manager = Some(Box::new(
            crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(
                3, false, &links,
            ),
        ));
        entities.insert(victim);

        let event = EntityDamageEvent::area(2, 1, 0, 1, Some(source_owner), hit_wh);
        commit_hits(
            &[event],
            &mut entities,
            &rules,
            &mut interner,
            &mut crate::sim::houses::Houses::default(),
            0,
            None,
        );

        let victim = entities.get(2).expect("tower survives");
        Outcome {
            health: victim.health.current(),
            mission: victim.mission.current(),
            suspended: victim.mission.suspended(),
            target: victim.attack_target.target(),
            links: victim
                .capture_manager
                .as_ref()
                .expect("manager retained")
                .victims()
                .collect(),
        }
    }

    let full = run(3);
    assert_eq!(full.health, 99, "receiver still commits the hostile hit");
    // The full manager rejects the Override: the mission stays Guard.
    assert_eq!(full.mission, MissionId::from_known(MissionType::Guard));
    assert_eq!(full.suspended, MissionId::NONE);
    // BuildingClass::ReceiveDamage's retaliation block then offers the
    // computer's tower the in-range source (`0x00442A39`).
    assert_eq!(full.target, Some(TargetKind::Entity(1)));
    assert_eq!(full.links, vec![10, 11, 12]);

    let below_capacity = run(2);
    assert_eq!(below_capacity.health, 99);
    assert_eq!(
        below_capacity.mission,
        MissionId::from_known(MissionType::Attack)
    );
    assert_eq!(
        below_capacity.suspended,
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(below_capacity.target, Some(TargetKind::Entity(1)));
    assert_eq!(below_capacity.links, vec![10, 11]);
}

#[test]
fn gsi_04_07_damage_receiver_updates_grudge_before_retaliation() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nConditionRed=25%\n\
         [VehicleTypes]\n0=TARGET\n1=ZERO\n2=SOURCE\n\
         [Warheads]\n0=HitWH\n1=ZeroWH\n2=ReturnWH\n\
         [TARGET]\nStrength=1000\nCost=700\nArmor=heavy\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [ZERO]\nStrength=1000\nCost=700\nArmor=heavy\nPrimary=ReturnGun\nCanRetaliate=yes\n\
         [SOURCE]\nStrength=1000\nCost=300\nArmor=heavy\n\
         [HitWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ZeroWH]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n\
         [ReturnGun]\nDamage=1\nROF=1\nRange=8\nWarhead=ReturnWH\n\
         [ReturnWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Guard]\nRetaliate=yes\n",
    ))
    .expect("anger feedback rules parse");
    let mut entities = EntityStore::new();
    let mut source = make_entity_owned(1, "SOURCE", 7, 5, 1000, "B");
    source.lifecycle.set_in_limbo_for_test(false);
    entities.insert(source);
    let mut damaged = make_entity_owned(2, "TARGET", 5, 5, 1000, "A");
    damaged.lifecycle.set_in_limbo_for_test(false);
    entities.insert(damaged);
    let mut zero = make_entity_owned(3, "ZERO", 6, 5, 1000, "A");
    zero.lifecycle.set_in_limbo_for_test(false);
    entities.insert(zero);

    // `GameEntity::test_default` interns through the thread-local test
    // registry. Clone it only after constructing the fixture so every entity
    // type/owner ID resolves through the receiver's local registry too.
    let mut interner = test_interner();
    let victim_house = interner.intern("A");
    let source_house = interner.intern("B");
    let hit_wh = interner.intern("HitWH");
    let zero_wh = interner.intern("ZeroWH");

    let mut houses = computer_houses(&[victim_house, source_house]);
    let threat_persistence = |houses: &crate::sim::houses::Houses| {
        let victim = &houses[&victim_house];
        let snapshot =
            save_codec::serialize(&(victim.grudge_scores().clone(), victim.enemy_house()))
                .expect("threat state serializes");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&victim.grudge_scores().len(), &mut hasher);
        for (other, score) in victim.grudge_scores() {
            std::hash::Hash::hash(other, &mut hasher);
            std::hash::Hash::hash(score, &mut hasher);
        }
        std::hash::Hash::hash(&victim.enemy_house(), &mut hasher);
        (snapshot, std::hash::Hasher::finish(&hasher))
    };
    let mut occupancy = OccupancyGrid::new();
    let mut main_rng = SimRng::new(5);
    let mut scenario_rng = SimRng::new(7);
    let mut handled_deaths = Vec::new();
    let mut fatal_lifecycle = None;
    let mut sound_sink = None;
    let threat_before_zero = threat_persistence(&houses);
    let (zero_death, _) = commit_damage_events(
        &[EntityDamageEvent::area(
            3,
            500,
            0,
            1,
            Some(source_house),
            zero_wh,
        )],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(3).unwrap().health.current(), 1000);
    assert_eq!(threat_persistence(&houses), threat_before_zero);
    let victim = &houses[&victim_house];
    assert!(!victim.grudge_scores().contains_key(&source_house));
    assert_eq!(victim.enemy_house(), None);
    assert_eq!(
        zero_death.receiver_stage_trace,
        [
            ReceiverStageTrace::HouseThreat {
                target_id: 3,
                delta: 0,
            },
            ReceiverStageTrace::ShouldRetaliate { target_id: 3 },
        ],
        "fresh zero feedback rescans before retaliation without materializing a sparse node"
    );

    let (death, _) = commit_damage_events(
        &[EntityDamageEvent::area(
            2,
            500,
            0,
            1,
            Some(source_house),
            hit_wh,
        )],
        &mut entities,
        &mut occupancy,
        &rules,
        &mut interner,
        &mut houses,
        &HouseAllianceMap::new(),
        &mut main_rng,
        &mut scenario_rng,
        &mut handled_deaths,
        None,
        None,
        None,
        0,
        &mut fatal_lifecycle,
        &mut sound_sink,
    );
    assert_eq!(entities.get(2).unwrap().health.current(), 500);
    let victim = &houses[&victim_house];
    assert_eq!(victim.grudge_scores().get(&source_house), Some(&350));
    assert_eq!(victim.enemy_house(), Some(source_house));
    assert_eq!(
        death.receiver_stage_trace,
        [
            ReceiverStageTrace::HouseThreat {
                target_id: 2,
                delta: 350,
            },
            ReceiverStageTrace::ShouldRetaliate { target_id: 2 },
        ],
        "the first nonzero feedback materializes its sparse node before retaliation"
    );
}
