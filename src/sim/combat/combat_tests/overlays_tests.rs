//! Combat against overlays: Tiberium prelude, ore destruction in spread,
//! wall damage and crushers.

use super::*;

#[test]
fn gsi_04_11_tiberium_prelude_gates_and_signed_large_quotient() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=TIBWH\n1=PLAINWH\n\
         [OverlayTypes]\n0=DEFAULTORE\n1=CHAINORE\n2=CHAINPLAIN\n\
         [TIBWH]\nTiberium=yes\n\
         [PLAINWH]\nTiberium=no\n\
         [DEFAULTORE]\nTiberium=yes\n\
         [CHAINORE]\nTiberium=yes\nChainReaction=yes\n\
         [CHAINPLAIN]\nTiberium=no\nChainReaction=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("tiberium prelude gate rules");
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut overlay = OverlayGrid::new(3, 1);
    overlay.place_overlay(0, 0, registry.id_for_name("DEFAULTORE").unwrap(), 0);
    overlay.place_overlay(1, 0, registry.id_for_name("CHAINORE").unwrap(), 0);
    overlay.place_overlay(2, 0, registry.id_for_name("CHAINPLAIN").unwrap(), 0);

    assert!(!combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        0,
        0
    ));
    assert!(combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        1,
        0
    ));
    assert!(!combat_aoe::tiberium_reduction_cell_admitted(
        Some(&overlay),
        Some(&registry),
        2,
        0
    ));

    let tib = rules.warhead("TIBWH").unwrap();
    let plain = rules.warhead("PLAINWH").unwrap();
    assert_eq!(
        tiberium_reduction_amount(655_360, true, tib),
        Some(65_536),
        "native signed quotient must not wrap through u16"
    );
    assert_eq!(tiberium_reduction_amount(100, true, plain), None);
    assert_eq!(tiberium_reduction_amount(100, false, tib), None);
    assert_eq!(tiberium_reduction_amount(-100, true, tib), None);
}

// --- Ore destruction integration tests ---

/// Build a RuleSet with a CellSpread=2 AoE weapon for ore destruction testing.
fn test_rules_with_spread() -> RuleSet {
    let ini_str: &str = "\
[InfantryTypes]\n\n\
[VehicleTypes]\n0=MTNK\n\n\
[AircraftTypes]\n\n\
[BuildingTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=120mm\n\n\
[120mm]\nDamage=120\nROF=50\nRange=6\nWarhead=HE\n\n\
[HE]\nCellSpread=2\nTiberium=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let ini = IniFile::from_str(ini_str);
    RuleSet::from_ini(&ini).expect("test rules should parse")
}

#[test]
fn test_weapon_fire_destroys_ore_in_spread() {
    let rules = test_rules_with_spread();
    let mut store = EntityStore::new();
    store.insert(make_entity(1, "MTNK", 5, 5, 300));
    store.insert(make_entity(2, "MTNK", 8, 5, 300));
    let mut interner = test_interner();
    install_entity_attack_target_for_test(&mut store, 1, 2);

    // Place ore at the target cell and a neighbor within CellSpread=2.
    // 6 density levels of ore at target (8,5): remaining = 6 * 120 = 720.
    // 3 density levels at (9,5): remaining = 3 * 120 = 360.

    let ore_ini =
        IniFile::from_str("[OverlayTypes]\n0=ORE\n[ORE]\nTiberium=yes\nChainReaction=yes\n");
    let ore_registry = OverlayTypeRegistry::from_ini(&ore_ini, None);
    let mut overlays = OverlayGrid::new(16, 16);
    overlays.place_overlay(8, 5, 0, 5);
    overlays.place_overlay(9, 5, 0, 2);

    align_attackers_to_targets(&mut store);
    let result = tick_with_map(
        &mut store,
        &rules,
        &mut interner,
        Some(&mut overlays),
        Some(&ore_registry),
        None,
        &[],
        &mut SimRng::new(1),
    );

    // Combat emits TiberiumReductionRequests (applied later by World via the
    // shared cell reducer).
    // Damage=120 → ore_damage = 120/10 = 12 density levels at each cell within
    // CellSpread=2. Both ore cells (8,5) and (9,5) get a reduction request.
    let req_amount = |rx: u16, ry: u16| {
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .iter()
            .find(|r| r.rx == rx && r.ry == ry)
            .map(|r| r.amount)
    };
    assert_eq!(
        req_amount(8, 5),
        Some(12),
        "target cell should get a 12-level reduction request"
    );
    assert_eq!(
        req_amount(9, 5),
        Some(12),
        "neighbor cell within CellSpread=2 should get a 12-level reduction request"
    );
}

// ---- Wall damage integration tests ----------------------------------------

/// INI containing GAWALL as both a [BuildingTypes] entry (so it has an
/// ObjectType with Wall=yes) and an [OverlayTypes] entry (so the overlay
/// registry knows it as a wall overlay). Strength=400, DamageLevels=4 are
/// representative of the real GAWALL.
fn wall_test_ini() -> &'static str {
    "[InfantryTypes]\n\
     [VehicleTypes]\n\
     [AircraftTypes]\n\
     [BuildingTypes]\n0=GAWALL\n\
     [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
     [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\n\
     [GASAND]\nWall=yes\nStrength=400\n\
     [CYCL]\nWall=yes\nStrength=400\n"
}

/// Build a Simulation with an ephemeral GAWALL overlay at `(rx, ry)`.
fn build_minimal_sim_with_gawall(rx: u16, ry: u16) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let ini = IniFile::from_str(wall_test_ini());
    let rules = RuleSet::from_ini(&ini).expect("wall rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    let mut sim = Simulation::new();
    let mut grid = OverlayGrid::new(10, 10);
    // Place GAWALL (overlay_id=2). Initial frame = 0 (isolated, stage 0).
    grid.place_overlay(rx, ry, 2, 0);
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(10, 10));

    (sim, rules, registry)
}

/// A world with overlay `overlay_id` at (5,5) on flat 10x10 terrain and a
/// `veh_type` vehicle (id 2) standing on it, its crusher flag and locomotor
/// taken from the type.
fn vehicle_on_overlay(rules: &RuleSet, veh_type: &str, overlay_id: u8) -> Simulation {
    let mut sim = Simulation::new();
    let mut grid = OverlayGrid::new(10, 10);
    grid.place_overlay(5, 5, overlay_id, 0);
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(crate::sim::tiberium::test_support::flat_terrain(10, 10));
    let owner_id = sim.interner.intern("Test");
    let obj = rules.object(veh_type).expect("veh object");
    let veh_type_id = sim.interner.intern(veh_type);
    let mut veh = GameEntity::test_default(2, veh_type, "Test", 5, 5);
    // PerCell belongs to an on-map live owner. The old global sweep also
    // inspected ctor-limbo fixtures; the canonical callback must not do so.
    veh.lifecycle.set_in_limbo_for_test(false);
    veh.set_owner_for_test(owner_id);
    veh.set_type_ref_for_test(veh_type_id);
    veh.mover_facts.set_regular_crusher_for_test(obj.crusher);
    veh.locomotor = Some(crate::sim::movement::locomotor::LocomotorState::from_object_type(obj, 0));
    sim.substrate.entities.insert(veh);
    sim.substrate.entities.rebuild_class_registries();
    sim
}

#[test]
fn wall_warhead_damages_and_destroys_wall_overlay() {
    let (mut sim, rules, registry) = build_minimal_sim_with_gawall(5, 5);

    let initial_wall_entities = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            sim.interner
                .object_type(e.type_ref(), &rules)
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(initial_wall_entities, 0, "wall state is cell-owned");

    // Forced destruction (literal -1 bypasses the probabilistic gate).
    sim.apply_wall_damage(WallPointerTarget::Real(5, 5), -1, &registry);
    // Overlay cleared.
    let grid = sim
        .overlay_grid
        .as_ref()
        .expect("grid should still be present");
    assert!(
        grid.cell(5, 5).overlay_id.is_none(),
        "overlay should be cleared"
    );
    assert_eq!(
        sim.dirty_cells.radar(),
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (4, 4),
            (5, 4),
            (4, 6),
            (3, 5),
            (4, 5),
            (6, 6),
            (5, 7),
            (5, 6),
            (7, 5),
            (6, 5),
        ],
        "direct wall damage uses the terminal DestroyOverlay visit stencil",
    );
    assert_eq!(sim.dirty_cells.radar_generation(), 13);
    assert_eq!(
        sim.dirty_cells.tactical_for_test(),
        vec![
            (5, 5),
            (5, 3),
            (6, 4),
            (5, 5),
            (4, 4),
            (5, 4),
            (4, 4),
            (5, 5),
            (4, 6),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 6),
            (5, 7),
            (4, 6),
            (5, 6),
            (6, 4),
            (7, 5),
            (6, 6),
            (5, 5),
            (6, 5),
        ]
    );

    // No persistent wall entity is created or removed.
    let remaining = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            sim.interner
                .object_type(e.type_ref(), &rules)
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(remaining, 0);
}

#[test]
fn crusher_driveover_destroys_wall_but_noncrusher_does_not() {
    // A `Crusher=yes` drive vehicle standing on a wall cell after ground movement
    // flattens the wall (gamemd movement-side PerCellProcess crush), taking no
    // damage itself; a non-crusher on the same cell leaves the wall intact.
    // GAWALL is both a BuildingType (Wall=yes ObjectType) and an OverlayType;
    // BFRT is a Crusher drive vehicle, MTNK a plain drive vehicle.
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=BFRT\n1=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [MTNK]\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("rules parse");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    // The vehicle on the GAWALL overlay (id 2) also carries its type's
    // omni-crusher flag and 300 health.
    let build = |veh_type: &str| -> Simulation {
        let mut sim = vehicle_on_overlay(&rules, veh_type, 2);
        let obj = rules.object(veh_type).expect("veh object");
        let veh = sim.substrate.entities.get_mut(2).unwrap();
        veh.mover_facts.set_omni_crusher_for_test(obj.omni_crusher);
        veh.health = Health::new(300);
        sim
    };

    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id
            .is_some()
    };

    // Crusher (BFRT): wall destroyed and crusher unharmed.
    let mut sim = build("BFRT");
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.flush_pending_delete();
    assert!(
        !wall_present(&sim),
        "crusher drive-over must remove the wall overlay"
    );
    assert_eq!(
        sim.dirty_cells.radar().len(),
        13,
        "crusher uses the terminal DestroyOverlay radar-dirty stencil",
    );
    assert_eq!(sim.dirty_cells.radar()[0], (5, 5));
    assert_eq!(sim.dirty_cells.radar_generation(), 13);
    assert_eq!(sim.dirty_cells.tactical_for_test().len(), 21);
    assert!(
        sim.substrate
            .entities
            .get(2)
            .is_some_and(|e| e.health.current() == 300),
        "crusher takes no damage from crushing the wall"
    );
    let walls_left = sim
        .substrate
        .entities
        .iter_sorted()
        .filter(|(_, e)| {
            sim.interner
                .object_type(e.type_ref(), &rules)
                .is_some_and(|o| o.wall)
        })
        .count();
    assert_eq!(walls_left, 0, "walls never create persistent entities");

    // Non-crusher (MTNK): wall stays intact.
    let mut sim = build("MTNK");
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.flush_pending_delete();
    assert!(
        wall_present(&sim),
        "a non-crusher drive vehicle must not remove the wall"
    );
}

/// `UnitClass::PerCellProcess @ 0x0073B013..B034`: a `Crushable=yes` overlay
/// (fences, sandbags) falls to any crusher whatever its locomotor, a plain
/// `Wall=yes` overlay only to a `MovementZone=CrusherAll` one; the overlay
/// `CrushSound=` is queued at the crusher (`0x0073B045..B04D`).
#[test]
fn crushable_fence_falls_to_any_crusher_and_plays_its_crush_sound() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=ROBO\n1=BFRT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n1=CAFNCB\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n3=CAFNCB\n4=CUSTOM\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [CAFNCB]\nStrength=100\nArmor=wood\nWall=yes\nCrushable=yes\nCrushSound=WallCrushBlack\n\
         [CUSTOM]\nCrushable=yes\nWall=no\nCrushSound=WallCrushBlack\n\
         [ROBO]\nCrusher=yes\nLocomotor={4A582742-9839-11D1-B709-00A024DDAFD1}\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini_with_sounds_for_test(&ini, &["WallCrushBlack"])
        .expect("rules and fixed sound catalog parse");
    let registry = rules.overlay_registry().clone();
    assert_eq!(
        registry.flags(3).and_then(|f| f.crush_sound.as_deref()),
        Some("WallCrushBlack")
    );

    let build = |veh_type: &str, overlay_id: u8| vehicle_on_overlay(&rules, veh_type, overlay_id);
    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .overlay_id
            .is_some()
    };
    let crush_sounds = |sim: &Simulation| -> Vec<String> {
        sim.sound_events
            .iter()
            .filter_map(|event| match event {
                crate::sim::world::SimSoundEvent::WallCrushed {
                    sound_id, rx, ry, ..
                } => {
                    assert_eq!((*rx, *ry), (5, 5));
                    Some(sound_id.name_in(&rules).to_string())
                }
                _ => None,
            })
            .collect()
    };

    // Hover crusher over a crushable fence: crushed, cue queued.
    let mut sim = build("ROBO", 3);
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.flush_pending_delete();
    assert!(
        !wall_present(&sim),
        "a crushable fence falls to a hover crusher"
    );
    assert_eq!(crush_sounds(&sim), vec!["WallCrushBlack".to_string()]);

    // Crusher over a concrete wall WITHOUT MovementZone=CrusherAll: survives.
    let mut sim = build("ROBO", 2);
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.flush_pending_delete();
    assert!(
        wall_present(&sim),
        "a plain wall needs MovementZone=CrusherAll, not merely Crusher=yes"
    );
    assert!(crush_sounds(&sim).is_empty());

    // CrusherAll vehicle over a concrete wall without a CrushSound: crushed,
    // silent. Exactly one stock vehicle qualifies - BFRT, the Battle Fortress.
    let mut sim = build("BFRT", 2);
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.flush_pending_delete();
    assert!(!wall_present(&sim));
    assert!(
        crush_sounds(&sim).is_empty(),
        "no CrushSound= means silence"
    );
    // The native sound/recoil precede/ignore the Cell damage return. A
    // Crushable non-Wall is admitted by Unit but rejected by Cell480CB0.
    let mut sim = build("ROBO", 4);
    sim.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert!(wall_present(&sim));
    assert_eq!(crush_sounds(&sim), vec!["WallCrushBlack".to_string()]);
    let native = crate::test_fixture::json("tools/spatial_oracle/unit_per_cell_overlay.json");
    let expected = native
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "numeric_00000000_r0_d0")
        .unwrap();
    assert_eq!(
        sim.entities()
            .get(2)
            .unwrap()
            .rocking
            .unwrap()
            .native_bits()[3],
        u32::from_str_radix(expected["output"]["velocity_bits"].as_str().unwrap(), 16).unwrap()
    );
}

/// Seeded variant of `build_minimal_sim_with_gawall` — used for determinism
/// replay tests where two sims must produce byte-identical state given the
/// same input event sequence.
fn build_minimal_sim_with_gawall_seeded(
    rx: u16,
    ry: u16,
    seed: u64,
) -> (Simulation, RuleSet, OverlayTypeRegistry) {
    let (mut sim, rules, registry) = build_minimal_sim_with_gawall(rx, ry);
    sim.initialize_random_number_system(seed as u32);
    (sim, rules, registry)
}

#[test]
fn wall_damage_deterministic_across_replays() {
    let seed: u64 = 0x1234_5678;
    let snapshot_a: (Option<u8>, u8) = {
        let (mut sim, _rules, registry) = build_minimal_sim_with_gawall_seeded(5, 5, seed);
        for _ in 0..5 {
            sim.apply_wall_damage(WallPointerTarget::Real(5, 5), 100, &registry);
        }
        let cell = sim.overlay_grid.as_ref().unwrap().cell(5, 5);
        (cell.overlay_id, cell.overlay_data)
    };
    let snapshot_b: (Option<u8>, u8) = {
        let (mut sim, _rules, registry) = build_minimal_sim_with_gawall_seeded(5, 5, seed);
        for _ in 0..5 {
            sim.apply_wall_damage(WallPointerTarget::Real(5, 5), 100, &registry);
        }
        let cell = sim.overlay_grid.as_ref().unwrap().cell(5, 5);
        (cell.overlay_id, cell.overlay_data)
    };

    assert_eq!(
        snapshot_a, snapshot_b,
        "wall damage must be RNG-deterministic"
    );
}

/// A Drive `Crusher=yes` vehicle that is not `CrusherAll` leaves a plain wall
/// standing.
///
/// This is the case the change is about, and the one the sibling tests cannot
/// isolate: BFRT is Drive *and* CrusherAll, so it crushed under the old
/// locomotor gate too, and ROBO is Hover *and* not CrusherAll, so it differs on
/// both axes. HTNK differs on exactly one - Drive, `Crusher=yes`, and NOT
/// `CrusherAll` - so reverting the gate to `LocomotorKind::Drive` makes this
/// fail and nothing else in the suite notices.
///
/// Native: `0x0073B027` loads the type from `+0x6C4`, `0x0073B02D` compares
/// `TechnoTypeClass+0x5B4` (MovementZone) against `0xC` (CrusherAll), and
/// `0x0073B034 JNZ` leaves the wall alone.
#[test]
fn a_drive_crusher_without_crusherall_leaves_a_plain_wall_standing() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=HTNK\n1=BFRT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAWALL\n\
         [OverlayTypes]\n0=GASAND\n1=CYCL\n2=GAWALL\n\
         [GAWALL]\nStrength=400\nArmor=concrete\nWall=yes\nDamageLevels=4\n\
         [HTNK]\nCrusher=yes\nMovementZone=Destroyer\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [BFRT]\nCrusher=yes\nMovementZone=CrusherAll\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("wall crush rules");
    let registry = OverlayTypeRegistry::from_ini(&ini, None);

    let build = |veh_type: &str| vehicle_on_overlay(&rules, veh_type, 2);

    let wall_present = |sim: &Simulation| -> bool {
        sim.overlay_grid
            .as_ref()
            .map(|g| g.cell(5, 5).overlay_id == Some(2))
            .unwrap_or(false)
    };

    let mut tank = build("HTNK");
    tank.per_cell_process(
        2,
        crate::sim::movement::PerCellReason::TurnComplete,
        &rules,
        Some(&registry),
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    tank.flush_pending_delete();
    assert!(
        wall_present(&tank),
        "a Rhino is Crusher=yes and Drive, but MovementZone=Destroyer: the wall stands"
    );

    let mut fortress = build("BFRT");
    fortress
        .per_cell_process(
            2,
            crate::sim::movement::PerCellReason::TurnComplete,
            &rules,
            Some(&registry),
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    fortress.flush_pending_delete();
    assert!(
        !wall_present(&fortress),
        "the Battle Fortress is CrusherAll, so the same wall falls to it"
    );
}
