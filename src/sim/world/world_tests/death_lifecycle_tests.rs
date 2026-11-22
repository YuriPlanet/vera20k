//! Death and removal lifecycle: fatal transport damage with nested death weapons,
//! ore reduction around crater anims, the deferred-delete dying window, and death
//! debris joining the live order, the hash and the snapshot.

use super::*;

#[test]
fn gsi_04_07_damage_fatal_transport_lifecycle_brackets_nested_death_weapon() {
    fn run(carrier_hp: i32) -> (Simulation, crate::sim::combat::CombatTickResult, u64) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=PASSENGER\n\
             [VehicleTypes]\n0=BOOMER\n1=SHOOTER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=LISTENER\n\
             [Warheads]\n0=KillWH\n1=NoDamageWH\n2=WallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [BOOMER]\nStrength=11\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
             [SHOOTER]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\
             [PASSENGER]\nStrength=50\nArmor=none\n\
             [LISTENER]\nStrength=300\nArmor=wood\n\
             [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
             [Gun]\nDamage=1\nROF=50\nRange=8\nWarhead=NoDamageWH\n\
             [KillWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,0%,100%,100%,100%,100%\n\
             [NoDamageWH]\nCellSpread=0\nVerses=100%,100%,100%,0%,100%,100%,100%,100%,100%,100%,100%\n\
             [WallWH]\nCellSpread=.5\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,50%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
        );
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("fatal lifecycle rules");
        let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
        let mut sim = Simulation::with_seed(1);
        let owner = sim.interner.intern("Americans");
        let enemy = sim.interner.intern("Soviet");

        let mut cargo = crate::sim::passenger::PassengerCargo::new(2, 1.0);
        cargo.board_forced_chain(&[(11, 1.0)]);
        let mut carrier = GameEntity::test_default(10, "BOOMER", "Soviet", 8, 5);
        carrier.set_owner_for_test(enemy);
        carrier.set_type_ref_for_test(sim.interner.intern("BOOMER"));
        carrier.health.set_for_test(carrier_hp);
        carrier.set_passenger_role_for_test(crate::sim::passenger::PassengerRole::transport(cargo));
        sim.substrate.entities.insert(carrier);
        let _ = sim.reveal(10);

        let mut passenger = GameEntity::test_default(11, "PASSENGER", "Soviet", 8, 5);
        passenger.set_owner_for_test(enemy);
        passenger.set_type_ref_for_test(sim.interner.intern("PASSENGER"));
        passenger.set_category_for_test(EntityCategory::Infantry);
        passenger.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        passenger.set_is_voxel_for_test(false);
        passenger.set_passenger_role_for_test(
            crate::sim::passenger::PassengerRole::inside_for_test(10, false),
        );
        sim.substrate.entities.insert(passenger);

        let mut listener = GameEntity::test_default_of_category(
            30,
            "LISTENER",
            "Americans",
            8,
            5,
            EntityCategory::Structure,
        );
        listener.set_owner_for_test(owner);
        listener.set_type_ref_for_test(sim.interner.intern("LISTENER"));
        listener.set_is_voxel_for_test(false);
        listener.health.set_for_test(300);
        sim.substrate.entities.insert(listener);
        let _ = sim.reveal(30);

        let mut nested_fatal = GameEntity::test_default_of_category(
            31,
            "LISTENER",
            "Americans",
            8,
            5,
            EntityCategory::Structure,
        );
        nested_fatal.set_owner_for_test(owner);
        nested_fatal.set_type_ref_for_test(sim.interner.intern("LISTENER"));
        nested_fatal.set_is_voxel_for_test(false);
        nested_fatal.health.set_for_test(107);
        sim.substrate.entities.insert(nested_fatal);
        let _ = sim.reveal(31);

        let mut attacker = GameEntity::test_default(20, "SHOOTER", "Americans", 6, 5);
        attacker.set_owner_for_test(owner);
        attacker.set_type_ref_for_test(sim.interner.intern("SHOOTER"));
        sim.substrate.entities.insert(attacker);
        let _ = sim.reveal(20);
        let attacker = sim.substrate.entities.get_mut(20).unwrap();
        attacker
            .attack_target
            .set_for_test(Some(TargetKind::Entity(10)));
        attacker.seldom.radio_contacts.insert(10);
        attacker
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Attack,
                ),
                suspended: crate::sim::mission::MissionId::NONE,
                queued: crate::sim::mission::MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });

        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 5, 0, 0);
        sim.overlay_grid = Some(overlays);
        sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(16, 16));
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: 1,
            source_id: 99,
            target: crate::sim::projectile::ProjectileTarget::Entity(10),
            impact: crate::sim::projectile::ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: crate::sim::projectile::ProjectilePayload::new(
                10,
                sim.interner.intern("KillWH"),
                None,
                false,
            ),
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        let result = sim.tick_combat_with_fatal_lifecycle(
            &rules,
            Some(&registry),
            100,
            &[10, 20],
            &BTreeSet::new(),
            &[detonation],
            &mut crate::sim::world::FrameEffects::default(),
        );
        let rng = sim.scenario_rng.state();
        (sim, result, rng)
    }

    let (fatal, result, fatal_rng) = run(10);
    assert_eq!(
        fatal.substrate.pending_delete.object_ids(),
        vec![11, 31, 10]
    );
    for id in [11, 31, 10] {
        let entity = fatal.substrate.entities.get(id).unwrap();
        // KillPassengers (`0x00707CB0`) stores no Health: the passenger
        // keeps its own through RecordKill and UnInit.
        let health = if id == 11 { 100 } else { 0 };
        assert_eq!(entity.health.current(), health, "object {id}");
        assert!(!entity.lifecycle.object_alive());
        assert!(!entity.seldom.in_logic_vector.is_member());
    }
    let listener = fatal.substrate.entities.get(30).unwrap();
    assert_eq!(
        listener.health.current(),
        193,
        "wood Verses scales 214 to 107"
    );
    let nested_fatal = fatal.substrate.entities.get(31).unwrap();
    assert_eq!(
        fatal
            .interner
            .resolve(nested_fatal.seldom.kill_record.killer().unwrap()),
        "Soviet"
    );
    // Only the surviving listener (30) pings. The nested-fatal building (31)
    // takes `BuildingClass::ReceiveDamage` case 4, whose stock 8-frame timer
    // runs `ObjectClass::UnInit` (`0x005F6625` clears `IsAlive +0x90`) before
    // the `0x00442905` re-test, so `NotifyUnderAttack` never runs for it.
    assert!(result.consequences.effects().under_attack_events.is_empty());
    assert_eq!(
        fatal
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnderAttack {
                    owner,
                    miner,
                    rx,
                    ry,
                    ..
                } => Some((*owner, *miner, *rx, *ry)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(listener.owner(), false, 8, 5)]
    );
    assert!(!fatal.substrate.occupancy.contains_entity(8, 5, 10));
    let attacker = fatal.substrate.entities.get(20).unwrap();
    assert!(!attacker.seldom.radio_contacts.contains(10));
    assert!(attacker.attack_target.target().is_none());
    assert!(
        result
            .consequences
            .effects()
            .immediate_uninit_ids
            .is_empty()
    );
    assert_eq!(
        fatal.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        None
    );
    assert_eq!(
        fatal.dirty_cells.radar(),
        vec![
            (8, 5),
            (8, 3),
            (9, 4),
            (7, 4),
            (8, 4),
            (7, 6),
            (6, 5),
            (7, 5),
            (9, 6),
            (8, 7),
            (8, 6),
            (10, 5),
            (9, 5),
        ],
        "combat-result commit projects the complete DestroyOverlay visit stencil",
    );
    // ObjectClass::ReceiveDamage's exact-zero Destroy broadcast (0x005F57AF)
    // re-arms the attacker's passive scan (4..8) before TechnoClass's death
    // arm fires the DeathWeapon whose wall hit draws 0..400.
    let mut one_draw = SimRng::new(1);
    let _ = one_draw.next_range_u32_inclusive(4, 8);
    let _ = one_draw.next_range_u32_inclusive(0, 400);
    assert_eq!(fatal_rng, one_draw.state());

    let (boundary, boundary_result, boundary_rng) = run(11);
    assert!(boundary.substrate.pending_delete.is_empty());
    assert!(boundary.substrate.occupancy.contains_entity(8, 5, 10));
    for id in [30, 31] {
        let listener = boundary.substrate.entities.get(id).unwrap();
        assert_eq!(listener.health.current(), if id == 30 { 300 } else { 107 });
    }
    assert!(
        boundary_result
            .consequences
            .effects()
            .under_attack_events
            .is_empty()
    );
    assert_eq!(
        boundary
            .substrate
            .entities
            .get(10)
            .unwrap()
            .passenger_role()
            .cargo()
            .unwrap()
            .passengers(),
        vec![11]
    );
    assert!(boundary.substrate.entities.get(11).unwrap().is_alive());
    assert!(
        boundary
            .substrate
            .entities
            .get(20)
            .unwrap()
            .seldom
            .radio_contacts
            .contains(10)
    );
    assert_eq!(
        boundary
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(8, 5)
            .overlay_id,
        Some(0)
    );
    assert!(boundary.dirty_cells.radar().is_empty());
    assert_eq!(boundary_rng, SimRng::new(1).state());
}

#[test]
fn gsi_04_11_bullet_ore_reduction_precedes_outer_crater_anim_start() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=OREWH\n\
         [OverlayTypes]\n0=ORE\n\
         [SmudgeTypes]\n0=CR1\n\
         [Tiberiums]\n0=Riparius\n\
         [Animations]\n0=EXPLOSION\n\
         [OREWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ORE]\nTiberium=yes\nChainReaction=yes\n\
         [Riparius]\nImage=1\nValue=25\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
    let mut rules = RuleSet::from_ini(&ini).expect("bullet ore-order rules");
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[EXPLOSION]\nCrater=yes\nScorch=no\n"),
    ));
    // One raw frame: no middle frame, so Start runs Middle at construction.
    rules.bind_anim_frame_count_for_test("EXPLOSION", 1);
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = registry.id_for_name("ORE").expect("ORE overlay id");

    let mut sim = Simulation::with_seed(1);
    let mut terrain = gsi_04_10_clear_terrain(10, 10);
    for cell in terrain.cells_mut_for_test() {
        cell.filled_clear = true;
        cell.accepts_smudge = true;
        cell.allows_tiberium = true;
    }
    sim.resolved_terrain = Some(terrain);
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    overlay.place_overlay(5, 5, ore_id, 9);
    sim.overlay_grid = Some(overlay);
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 1,
        source_id: crate::sim::combat::RAD_NO_ATTACKER,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: crate::sim::projectile::ProjectilePayload::new(
            100,
            sim.interner.intern("OREWH"),
            None,
            false,
        ),
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };

    let result = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        Some(&registry),
        100,
        &[],
        &BTreeSet::new(),
        &[detonation],
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None,
        "Apply_area_damage must clear ten-density ore before Bullet impact AnimClass::Start"
    );
    assert!(
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .is_empty()
    );
    assert!(
        result
            .consequences
            .effects()
            .smudge_spawn_requests
            .is_empty()
    );
    // The receiver transaction's commit constructs the AnimList anim.
    let _ = result.consequences.commit(
        &mut sim,
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(
        sim.smudge_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .type_id
            .is_some(),
        "the outer crater must observe the already-cleared overlay cell"
    );
    let mut expected_rng = crate::sim::rng::SimRng::new(1);
    // When Ares plays, Ares 3.0 CellChainReact (10050DD0, hook 0x48964F):
    // the spread walk rolls RandomRanged(0, 99) for the ChainReaction ore
    // before its reduction; with Power 0 a reaction deals nothing and the
    // ore clears.
    if rules.ares() {
        let _ = expected_rng.next_range_i32_inclusive(0, 99);
    }
    let _ = expected_rng.next_range_u32(1);
    assert_eq!(sim.scenario_rng.state(), expected_rng.state());
}

/// Rocket Detonate (`0x00663030`) constructs its explosion before
/// `Apply_area_damage`: the crater Anim starts while the ore still lies in the
/// cell, and the area damage then reduces it in the same Process.
#[test]
fn gsi_04_11_missile_outer_anim_precedes_per_cell_ore_reduction() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [General]\nDMislDamage=100\nDMislBodyLength=0\n\
         [CombatDamage]\nDMislWarhead=MISSILEWH\n\
         [Warheads]\n0=MISSILEWH\n[OverlayTypes]\n0=ORE\n\
         [SmudgeTypes]\n0=CR1\n[Tiberiums]\n0=Riparius\n\
         [MISSILEWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ORE]\nTiberium=yes\nChainReaction=yes\n\
         [Riparius]\nImage=1\nValue=25\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let ini = crate::rules::tiberium_type::with_tiberium_image_slots(&ini);
    let mut rules = RuleSet::from_ini(&ini).expect("missile ore-order rules");
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[EXPLOSION]\nCrater=yes\nScorch=no\nFrameWidth=100\nFrameHeight=100\n"),
    ));
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = registry.id_for_name("ORE").unwrap();
    let mut sim = Simulation::with_seed(7);
    let mut terrain = gsi_04_10_clear_terrain(10, 10);
    for cell in terrain.cells_mut_for_test() {
        cell.filled_clear = true;
        cell.accepts_smudge = true;
        cell.allows_tiberium = true;
    }
    sim.resolved_terrain = Some(terrain);
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    overlay.place_overlay(5, 5, ore_id, 9);
    sim.overlay_grid = Some(overlay);
    // A cruising missile on the ground of the ore cell detonates at once.
    let mut missile = GameEntity::test_default_of_category(
        1,
        "DMISL",
        "Americans",
        5,
        5,
        EntityCategory::Aircraft,
    );
    missile.set_owner_for_test(sim.interner.intern("Americans"));
    missile.set_type_ref_for_test(sim.interner.intern("DMISL"));
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Rocket);
    locomotor
        .rocket_runtime_mut()
        .unwrap()
        .set_mission_state_for_test(4);
    missile.locomotor = Some(locomotor);
    sim.substrate.entities.insert(missile);
    sim.reveal(1);
    let mut expected_rng = sim.scenario_rng.clone();
    // When Ares plays, Ares 3.0 CellChainReact (10050DD0, hook 0x48964F):
    // the ChainReaction ore's RandomRanged(0, 99) roll in the spread walk;
    // with Power 0 a reaction deals nothing and the ore still clears.
    if rules.ares() {
        let _ = expected_rng.next_range_i32_inclusive(0, 99);
    }

    sim.process_rocket(1, &rules, &mut crate::sim::world::FrameEffects::default());

    assert!(
        !sim.substrate.entities.get(1).unwrap().is_object_alive(),
        "Detonate UnInits the missile"
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None
    );
    assert!(
        sim.smudge_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .type_id
            .is_none(),
        "RocketLocomotion starts its crater Anim before the later ore sweep"
    );
    assert_eq!(sim.scenario_rng.state(), expected_rng.state());
    assert_eq!(sim.combat_light_requests.len(), 1);
}

// --- Slice 6: deferred-delete Dying-window behavior ---

/// Insert a revealed, occupancy-marked 2x2 structure owned by `Americans`.
fn insert_revealed_structure(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
    let mut s = GameEntity::test_default_of_category(
        id,
        "GAPOWR",
        "Americans",
        rx,
        ry,
        EntityCategory::Structure,
    );
    s.set_owner_for_test(sim.interner.intern("Americans"));
    s.set_type_ref_for_test(sim.interner.intern("GAPOWR"));
    s.seldom
        .type_facts
        .set_foundation_for_test("2x2".to_string().into());
    sim.substrate.entities.insert(s);
    sim.reveal(id);
}

/// Mutual same-tick death: both structures resolve as `Dying` in death order until
/// the flush, and the pre-flush state is replay-deterministic across two runs.
#[test]
fn mutual_same_tick_death_both_dying_then_flushed() {
    fn build() -> Simulation {
        let mut sim = Simulation::new();
        insert_revealed_structure(&mut sim, 1, 4, 5);
        insert_revealed_structure(&mut sim, 2, 8, 5);
        sim
    }

    let mut a = build();
    a.uninit(1);
    a.uninit(2);
    assert!(
        a.substrate
            .entities
            .get(1)
            .is_some_and(|e| e.death.is_dying())
    );
    assert!(
        a.substrate
            .entities
            .get(2)
            .is_some_and(|e| e.death.is_dying())
    );
    // Drain order = death (enqueue) order, deterministic.
    assert_eq!(a.substrate.pending_delete.object_ids(), vec![1, 2]);

    // Determinism: an identical second run hashes equal at the pre-flush point.
    let mut b = build();
    b.uninit(1);
    b.uninit(2);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "pre-flush mutual-death state must be replay-deterministic",
    );

    a.flush_pending_delete();
    assert!(a.substrate.entities.get(1).is_none());
    assert!(a.substrate.entities.get(2).is_none());
    assert!(a.substrate.pending_delete.is_empty());
}

/// A death in the object pass (here: a sold power plant, UnInit'd by the
/// Selling mission's completing visit in its LogicVector slot) remains
/// resolvable until the ordinary tail drain. Later systems of the frame must
/// gate on lifecycle authority rather than counting the dead-limbo object
/// merely because it is still stored.
#[test]
fn sale_death_is_ignored_before_ordinary_tail_drain() {
    use crate::sim::combat::Health;

    let ini_str: &str = "\
[VehicleTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\nPower=100\n";
    let ini = IniFile::from_str(ini_str);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("power rules parse");
    // Retail GAPOWRMK: a building sells only with a Buildup SHP.
    rules.set_buildup_control_for_test("GAPOWR", [0, 25, 2]);

    let mut sim = Simulation::new();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.input_delay_ticks = 0;
    let grid = PathGrid::test_all_passable(64, 64);

    // A represented House retains its power assessment after one of its two
    // plants sells. Its ordered AI visit must skip the dead-limbo plant.
    // Force the strings into the thread-local interner before snapshotting it.
    let _ = (
        crate::sim::intern::test_intern("GAPOWR"),
        crate::sim::intern::test_intern("Americans"),
    );
    sim.interner = crate::sim::intern::test_interner();
    let owner_id = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner_id, 0, None, true, 0, 10,
        ));
    for (id, rx, ry) in [(1u64, 10u16, 10u16), (2u64, 20u16, 20u16)] {
        let mut bld = GameEntity::test_default_of_category(
            id,
            "GAPOWR",
            "Americans",
            rx,
            ry,
            EntityCategory::Structure,
        );
        bld.seldom
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        bld.health = Health::new(750);
        sim.substrate.entities.insert(bld);
        sim.reveal(id);
    }

    // Tick 1: power registers both plants.
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    assert_eq!(
        sim.power_states.get(&owner_id).map(|s| s.total_output()),
        Some(200),
        "two power plants should produce 200 before sale",
    );

    // Tick 2: sell plant 1 via command: its Selling mission starts at the
    // command tail and packs it up; power still counts it.
    let sell = CommandEnvelope::new(
        owner_id,
        sim.session.tick() + 1,
        Command::SellBuilding { entity_id: 1 },
    );
    sim.advance_tick(&[sell], Some(&rules), Some(&grid), None, 100);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .is_some_and(|plant| plant.building_down()),
        "the sale packs the plant up"
    );

    // The completing visit (2 + 24 * 2 frames on) UnInits it in the object
    // pass; the power phase runs while it is still stored, and the tail
    // drains it the same tick.
    let mut frames = 0;
    while sim.substrate.entities.get(1).is_some() && frames < 60 {
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        frames += 1;
    }
    assert_eq!(frames, 50, "sold plant freed at its completing visit");
    assert!(
        sim.substrate.entities.get(2).is_some(),
        "surviving plant still present"
    );
    assert!(
        sim.substrate.pending_delete.is_empty(),
        "sale-death queue drained"
    );
    assert_eq!(
        sim.power_states.get(&owner_id).map(|s| s.total_output()),
        Some(100),
        "power skips the dead-limbo plant the tail has not yet drained",
    );
}

/// Combat-death counterpart: a structure killed in combat in the object pass
/// lives in the Dying window through the factory sweep and is freed only by the
/// single end-of-tick drain. Its repair took its step in the building's own
/// visit, before its death, and none after.
#[test]
fn combat_death_after_its_repair_visit_is_freed_at_end_of_tick() {
    use crate::sim::combat::Health;
    use crate::sim::house_state::HouseState;

    let ini_str: &str = "\
[VehicleTypes]\n0=MTNK\n\n\
[BuildingTypes]\n0=TARGB\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[TARGB]\nStrength=750\nArmor=wood\nCost=1000\n\n\
[105mm]\nDamage=65\nROF=20\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let ini = IniFile::from_str(ini_str);
    let rules = RuleSet::from_ini(&ini).expect("repair rules parse");

    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    let grid = PathGrid::test_all_passable(64, 64);

    let mut atk = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    // The fixture `MTNK` authors no `Turret=`, so the native body gate
    // (`UnitClass::GetFireError @ 0x00740FD0` step 17) compares its HULL
    // `+0x388`. Face it east at the building so the death/drain ordering under
    // test happens on the first tick instead of after a turn-to-fire.
    atk.body_facing.snap(0x4000, 0);
    atk.health = Health::new(300);
    // Damaged, auto-repairing enemy building MTNK destroys this tick in the
    // object pass.
    let mut bld =
        GameEntity::test_default_of_category(2, "TARGB", "Russia", 7, 5, EntityCategory::Structure);
    bld.seldom
        .type_facts
        .set_foundation_for_test("1x1".to_string().into());
    bld.health = Health::new(50);
    bld.building.repairing.set_for_test(true);
    sim.interner = crate::sim::intern::test_interner();
    let russia = sim.interner.intern("Russia");
    sim.houses
        .register(HouseState::new(russia, 0, None, false, 1000, 10));
    sim.substrate.entities.insert(atk);
    sim.substrate.entities.insert(bld);
    sim.reveal_entity_with_rules(1, &rules);
    sim.reveal_entity_with_rules(2, &rules);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(2)));

    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);

    assert!(
        sim.substrate.entities.get(2).is_none(),
        "building destroyed + freed by the end-of-tick drain"
    );
    assert!(
        sim.substrate.pending_delete.is_empty(),
        "end-of-tick drain emptied the queue"
    );
    // The repair step belongs to the building's own LogicVector visit
    // (`BuildingClass::UpdateRepairAndPower`, `0x004401B6`), which runs
    // before the MTNK's shot kills it: frame 0 takes one step (Cost 1000 over
    // 150 steps at 25% costs 1) while the building lives, and none after.
    assert_eq!(
        sim.houses.get(&russia).map(|h| h.economy().credits()),
        Some(999),
        "one repair step in the building's visit, none after its death",
    );
}

/// The `VoxelAnimClass` store is a live scheduler citizen, not a side table.
///
/// gamemd-derived: `VoxelAnimClass::Constructor @ 0x007493B0` assigns the
/// shared unique id, appends to the VoxelAnim registry, and `ObjectClass::
/// Unlimbo` reveals the piece into the LogicClass vector; `LogicClass::AI`
/// then visits it in that order every frame until `VoxelAnimClass::AI @
/// 0x00749F30` reaches `vtable+0xF8`. This pins the four wiring facts a
/// desync would ride in on: the id comes from the shared allocator, the piece
/// enters the live order, the state hash folds it, and a snapshot restores
/// both the store and its logic membership.
#[test]
fn gsi_05_14_death_debris_joins_the_live_order_the_hash_and_the_snapshot() {
    // The visits read the piece's type (its IsMeteor at expiry) from the rules
    // they are given, so the spawn and the visits share one RuleSet.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VoxelAnims]\n0=TIRE\n\
         [TIRE]\nElasticity=0.8\nMinAngularVelocity=12.0\nMaxAngularVelocity=24.0\n\
         MinZVel=28.0\nMaxZVel=32.0\nMaxXYVel=10.0\nDuration=150\n",
    ))
    .expect("TIRE rules parse");
    let tire = rules.voxel_anim_type_id_by_name("TIRE").unwrap();
    let voxel_type = rules.voxel_anim_type(tire);

    let mut sim = Simulation::new();
    let empty_hash = sim.state_hash();
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
    let mut deferred = Vec::new();
    for _ in 0..3 {
        sim.emit_debris_voxel(
            tire,
            voxel_type,
            None,
            glam::IVec3::new(1280, 2560, 0),
            true,
            &mut deferred,
            &rules,
        )
        .expect("in domain");
    }
    assert!(deferred.is_empty());
    assert_eq!(sim.native_identity_cursor(), Some(1003));
    assert_eq!(
        sim.voxel_anims()
            .iter()
            .map(|(_, piece)| piece.native_unique_id())
            .collect::<Vec<_>>(),
        [1001, 1002, 1003],
        "constructor native IDs are independent of stable handles"
    );

    assert_eq!(sim.substrate.voxel_anims.len(), 3);
    let ids: Vec<u64> = sim.substrate.voxel_anims.ids();
    assert_eq!(
        sim.substrate
            .display
            .members(super::display_layers::DisplayLayer::AIR),
        ids
    );
    assert_eq!(
        sim.live_object_order_snapshot(),
        ids,
        "each piece is revealed into the live order in spawn order"
    );
    assert_ne!(
        sim.state_hash(),
        empty_hash,
        "the store folds into the state hash"
    );

    // The fold reads the physics body, not just the identity: a single tick of
    // the duration has to move the hash.
    let before_tick = sim.state_hash();
    let first_id = ids[0];
    let debris = sim.substrate.voxel_anims.get_mut(first_id).unwrap();
    debris.set_duration_for_test(debris.duration() - 1);
    assert_ne!(sim.state_hash(), before_tick);
    let debris = sim.substrate.voxel_anims.get_mut(first_id).unwrap();
    debris.set_duration_for_test(debris.duration() + 1);
    assert_eq!(sim.state_hash(), before_tick);

    let expected_order = sim.live_object_order_snapshot();
    let expected_store = sim.substrate.voxel_anims.clone();
    let bytes =
        crate::sim::snapshot::GameSnapshot::save_validated(&sim, 17, 23, "live voxel debris", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load_validated(&bytes, 17, 23, "")
        .expect("current envelope preserves VoxelAnimStore")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("restore live debris");
    assert_eq!(restored.live_object_order_snapshot(), expected_order);
    assert_eq!(
        restored.substrate.voxel_anims, expected_store,
        "every piece and its physics body survive the snapshot"
    );
    restored.debug_assert_logic_membership_consistent();
    assert_eq!(
        restored.native_identity_cursor(),
        sim.native_identity_cursor()
    );
    assert_eq!(restored.next_native_runtime_id(), 1004);

    // The scheduler slot advances the physics body, and a piece leaves the live
    // order once its AI reaches Delete.
    let first = first_id;
    let before = sim.substrate.voxel_anims.get(first).unwrap().world_coord();
    sim.visit_voxel_anim(
        first,
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.process_pending_delete_with(
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
        1,
        sim.session().started(),
    );
    let after = sim.substrate.voxel_anims.get(first).unwrap().world_coord();
    assert_ne!(before, after, "one AI visit moves the body");
    for _ in 0..200 {
        if !sim.substrate.voxel_anims.contains_key(first) {
            break;
        }
        sim.visit_voxel_anim(
            first,
            &rules,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        sim.process_pending_delete_with(
            &rules,
            &mut crate::sim::world::FrameEffects::default(),
            1,
            sim.session().started(),
        );
    }
    assert!(
        !sim.substrate.voxel_anims.contains_key(first),
        "the piece expires inside its Duration"
    );
    assert!(
        !sim.live_object_order_snapshot().contains(&first),
        "Delete leaves the LogicVector"
    );
    assert_eq!(
        sim.substrate.display.layer_of(first),
        None,
        "Delete leaves Display too"
    );
    sim.debug_assert_logic_membership_consistent();
}
