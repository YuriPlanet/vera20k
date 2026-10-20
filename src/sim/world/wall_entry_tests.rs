//! The class `Can_Enter_Cell` wall arm (Infantry `0x0051C17C..0x0051C223`,
//! Unit `0x0073F3D7..0x0073F51F`) reads the live overlay cell: the overlay
//! index `CellClass+0x44`, data `+0x11E` and wall owner `+0x50` that
//! `OverlayClass::Mark` (`0x005FC570`) and `CellClass::DestroyOverlay`
//! (`0x00480CB0`) write during play.

use super::*;

#[path = "wall_replacement_retail_tests.rs"]
mod replacement_retail_tests;
use crate::rules::ini_parser::IniFile;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::overlay_grid::damage_wall_overlay_with_terrain;
use crate::sim::rng::SimRng;
use crate::sim::world::entry_test_fixture;

/// Wall overlays `O2` (stone, `DamageLevels=3`) and `O3` (wooden), the wall
/// BuildingType `WALLB`, and movers armed with a `Wall=` or a `Wood=` warhead.
const RULES: &str = "[BuildingTypes]\n1=WALLB\n[WALLB]\nWall=yes\nFoundation=1x1\nStrength=300\n\
     [InfantryTypes]\n2=WALLI\n3=WOODI\n\
     [WALLI]\nSpeedType=Foot\nStrength=100\nPrimary=WALLGUN\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [WOODI]\nSpeedType=Foot\nStrength=100\nPrimary=WOODGUN\n\
     Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
     [VehicleTypes]\n0=TRUCK\n1=WALLV\n2=WOODV\n\
     [TRUCK]\nSpeedType=Wheel\nStrength=100\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WALLV]\nSpeedType=Wheel\nStrength=100\nPrimary=WALLGUN\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WOODV]\nSpeedType=Wheel\nStrength=100\nPrimary=WOODGUN\n\
     Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
     [WALLGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WALLWH\n\
     [WOODGUN]\nDamage=10\nROF=20\nRange=5\nProjectile=TESTPROJ\nWarhead=WOODWH\n\
     [TESTPROJ]\nAG=yes\n\
     [Warheads]\n2=WALLWH\n3=WOODWH\n\
     [WALLWH]\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [WOODWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
     [O2]\nWall=yes\nStrength=300\n\
     [O3]\nWall=yes\nArmor=wood\nStrength=100\n";

const ENGINEER: u64 = 90;
const TRUCK: u64 = 91;
const WALLI: u64 = 92;
const WOODI: u64 = 93;
const WALLV: u64 = 94;
const WOODV: u64 = 95;

/// The entry fixture with every mover at (15, 15), owned by `Americans`;
/// ENGINEER and TRUCK are unarmed.
fn wall_fixture() -> (
    Simulation,
    RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
) {
    let (mut sim, rules, registry) = entry_test_fixture::fixture_with_rules_and_fixed_art(
        RULES,
        &IniFile::from_str("[WALLB]\nToOverlay=O2\n[O2]\nDamageLevels=3\n"),
    );
    let owner = sim.intern("Americans");
    for (id, name, category, kind) in [
        (
            ENGINEER,
            "ENGINEER",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (TRUCK, "TRUCK", EntityCategory::Unit, LocomotorKind::Drive),
        (
            WALLI,
            "WALLI",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (
            WOODI,
            "WOODI",
            EntityCategory::Infantry,
            LocomotorKind::Walk,
        ),
        (WALLV, "WALLV", EntityCategory::Unit, LocomotorKind::Drive),
        (WOODV, "WOODV", EntityCategory::Unit, LocomotorKind::Drive),
    ] {
        let mut mover = GameEntity::test_default(id, name, "Americans", 15, 15);
        mover.set_owner_for_test(owner);
        mover.set_type_ref_for_test(sim.intern(name));
        mover.set_category_for_test(category);
        mover.set_in_playfield(true);
        mover.locomotor = Some(LocomotorState::for_test_kind(kind));
        sim.substrate.entities.insert(mover);
    }
    (sim, rules, registry)
}

/// A wall as the map loader leaves it: the overlay cell and the bridge facts.
fn map_wall(sim: &mut Simulation, overlay: u8, data: u8) {
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 15, overlay, data);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(16, 15)
        .unwrap()
        .bridge_facts
        .overlay_id = Some(overlay);
}

/// The class `Can_Enter_Cell` code of mover `id` for the wall cell (16, 15).
fn entry(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    id: u64,
) -> Result<u8, String> {
    let cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((16, 15));
    sim.foot_can_enter(id, cell, InfantryEntryArgs::REPAIR, rules, Some(registry))
}

#[test]
fn a_wall_built_in_play_refuses_a_mover_that_cannot_shoot_it() {
    let (mut sim, rules, registry) = wall_fixture();
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0));
    }
    assert!(
        sim.spawn_object_at_height_with_overlay_registry(
            "WALLB",
            "Americans",
            16,
            15,
            0,
            0,
            &rules,
            &registry,
        )
        .is_some()
    );
    // Unarmed: Infantry `0x0051C1EB`, Unit `0x0073F48F`.
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(7), "{id}");
    }
}

#[test]
fn a_map_wall_destroyed_in_play_admits_movers_again() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(7), "{id}");
    }
    let destroyed = damage_wall_overlay_with_terrain(
        sim.overlay_grid.as_mut().unwrap(),
        &registry,
        sim.resolved_terrain.as_mut(),
        16,
        15,
        -1,
        &mut SimRng::new(1),
    );
    assert_eq!(destroyed.destroyed_cells, vec![(16, 15)]);
    for id in [ENGINEER, TRUCK] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0), "{id}");
    }
}

#[test]
fn a_mover_that_can_shoot_a_wall_prices_it_by_its_owner() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    // Unowned: `Is_Ally_ByIndex` (`0x004F9A10`) rejects index -1.
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(5), "{id}");
    }
    let owner = sim.intern("Americans");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_wall_owner(16, 15, owner);
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(4), "{id}");
    }
    // A house that names the mover's House an ally: only the mover's own
    // Allies bits count.
    let other = sim.intern("Russians");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .set_wall_owner(16, 15, other);
    sim.house_alliances
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(5), "{id}");
    }
    sim.house_alliances
        .entry("AMERICANS".into())
        .or_default()
        .insert("RUSSIANS".into());
    for id in [WALLI, WALLV] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(4), "{id}");
    }
}

#[test]
fn only_a_unit_takes_the_wood_route_against_a_wooden_wall() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 3, 0);
    // Unit `0x0073F4B3..0x0073F4C4`: `Wood=` against `Armor=wood`.
    assert_eq!(entry(&sim, &rules, &registry, WOODV), Ok(5));
    // Infantry asks `0x00772AC0`, which tests `Wall=` alone.
    assert_eq!(entry(&sim, &rules, &registry, WOODI), Ok(7));
    map_wall(&mut sim, 2, 0);
    assert_eq!(entry(&sim, &rules, &registry, WOODV), Ok(7));
}

#[test]
fn infantry_ignores_a_wall_at_its_last_damage_stage() {
    let (mut sim, rules, registry) = wall_fixture();
    // Damage stage 3 of `DamageLevels=3` (`0x0051C1C9..0x0051C1DC`).
    map_wall(&mut sim, 2, 0x30);
    for id in [WALLI, ENGINEER] {
        assert_eq!(entry(&sim, &rules, &registry, id), Ok(0), "{id}");
    }
    // The Unit arm has no such test.
    assert_eq!(entry(&sim, &rules, &registry, WALLV), Ok(5));
    map_wall(&mut sim, 2, 0x20);
    assert_eq!(entry(&sim, &rules, &registry, ENGINEER), Ok(7));
}

#[test]
fn the_land_row_refuses_after_the_wall_arm() {
    let (mut sim, rules, registry) = wall_fixture();
    map_wall(&mut sim, 2, 0);
    assert_eq!(entry(&sim, &rules, &registry, WALLI), Ok(5));
    // The arm accumulates 5, then the ground row read after the object list
    // (Infantry `0x0051C750`) refuses whatever it accumulated.
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut(16, 15)
        .unwrap()
        .speed_costs
        .foot = Some(0);
    assert_eq!(entry(&sim, &rules, &registry, WALLI), Ok(7));
}

/// Ordinary authored CAFNCB and MTNK/AP, without spawned actors, supplied
/// wall bytes or forced damage rolls. This is production integration coverage;
/// native480CB0/480630/480510 comparisons have their own independent inputs.
#[test]
fn retail_authored_fence_fire_publishes_damage_removal_and_cell_target_expiry() {
    assert_retail_wall_fire_profile(
        "tools/map_observation.authored-fence.example.json",
        "CAFNCB",
        "tools/spatial_oracle/anytown_damage/authored_fence.json",
    );
}

/// Paid GAWALL admission/autofill feeds the same Cell480CB0 wall owner.
/// Its stage-two branch (480E21..480EAF) admits pristine cardinal children
/// for inline damage200; the original component corpus separately pins the
/// recursive requests, results and RNG order, not this World's frame numbers.
#[test]
fn retail_paid_concrete_wall_fire_publishes_chain_and_target_expiry() {
    assert_retail_wall_fire_profile(
        "tools/map_observation.concrete-wall.example.json",
        "GAWALL",
        "tools/spatial_oracle/anytown_damage/concrete_wall.json",
    );
}

fn assert_retail_wall_fire_profile(
    profile_path: &'static str,
    wall_name: &str,
    native_path: &'static str,
) {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::{FrameEffects, TickLane};

    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json(profile_path);
    let native = crate::test_fixture::json(native_path);
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map_name = launch.selected_map_file.as_deref().unwrap().to_owned();
    let selected =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
            .unwrap();
    assert_eq!(
        serde_json::to_value(selected.source).unwrap()["source_sha256"],
        "7a390de363f79743dd54897a49302869a795f839f3387ff03e8c0b70a519e17e",
        "physical XMP source, including loose-file precedence"
    );
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map_name,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    // App apply_map_load_result installs these execution modes before the
    // first Logic turn. Headless load deliberately defaults to serial mode.
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    // Normal play's frame loop hashes every PLAY_HASH_INTERVAL frames; a
    // headless launch would hash every frame, which nothing here reads.
    scene
        .runtime
        .simulation
        .hash_state_every(crate::sim::replay::PLAY_HASH_INTERVAL);
    let initial_hash = scene.sim().state_hash();
    let owner = scene.sim().session.current_house().unwrap();
    let mut orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            assert_eq!(record["owner"], "VERA-OBSERVER");
            CommandEnvelope::new(
                owner,
                record["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(record["payload"].clone()).unwrap(),
            )
        })
        .collect();
    if wall_name == "GAWALL" {
        // Target the paid endpoint using an ordinary attack order. Filler
        // constructor draws change the later damage history; retain this
        // witness's pristine-child branch without seeding RNG or wall state.
        for order in &mut orders {
            if let Command::ForceAttackCell { target_ry, .. } = &mut order.payload {
                *target_ry = 87;
            }
        }
    }
    let (attacker_id, source) = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => Some((attacker_id, (target_rx, target_ry))),
            _ => None,
        })
        .expect("the profile must exercise an ordinary Cell attack");
    let tank = scene.sim().entities().get(attacker_id).unwrap();
    assert_eq!(scene.sim().resolve(tank.type_ref()), "MTNK");
    assert_eq!(tank.owner(), owner);
    let registry = &scene.runtime.resources.overlay_registry;
    let wall = registry.id_for_name(wall_name).unwrap();
    let flags = registry.flags(wall).unwrap();
    let native_type = native["cases"][0]["input"]["wall_types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["index"].as_u64() == Some(u64::from(wall)))
        .unwrap();
    assert_eq!(native_type["name"], wall_name);
    assert!(flags.wall);
    assert_eq!(flags.armor_is_wood, native_type["armor"] == 6);
    assert_eq!(
        u64::from(flags.strength),
        native_type["strength"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(flags.damage_levels),
        native_type["damage_levels"].as_u64().unwrap()
    );
    let damage_levels = flags.damage_levels;
    if wall_name == "GAWALL" {
        let rules = &scene.runtime.resources.rules;
        let building = rules.object(wall_name).unwrap();
        // Selected physical RULESMD values through Building46048E (Wall),
        // Techno71469F/714570/7141C5 (Cost/TechLevel/Prerequisite). The
        // selected mode/map lack this section. ART4611DD/461225 reads fixed
        // ToOverlay/Foundation; General67187F reads the wall time factor.
        assert!(building.wall);
        assert_eq!((building.cost, building.tech_level), (100, 1));
        assert_eq!(building.image, "GAWALL");
        assert_eq!(
            building.prerequisite,
            [crate::rules::prerequisite::Prerequisite::Building(
                rules.building_type_index("GAPILE").unwrap(),
            )]
        );
        assert!(building.prerequisite_override.is_empty());
        let foundation = crate::rules::foundation::foundation_def(&building.foundation);
        assert_eq!((foundation.width, foundation.height), (1, 1));
        assert_eq!(building.to_overlay.as_deref(), Some("GAWALL"));
        assert_eq!(
            building
                .to_overlay
                .as_deref()
                .and_then(|name| registry.id_for_name(name)),
            Some(wall)
        );
        assert_eq!(
            rules.production.wall_build_speed_coefficient,
            crate::util::native_x87::NativeF64Bits::from_bits(3.0_f64.to_bits())
        );
    }
    let weapon = scene.runtime.resources.rules.weapon("105mm").unwrap();
    assert_eq!(weapon.damage, 65);
    // GetROF adds a 0..=2 jitter; a further shot at the expired Cell would
    // come within one such rearm of the last one.
    let rearm = u64::try_from(weapon.rof).unwrap() + 3;
    assert_eq!(weapon.warhead.as_deref(), Some("AP"));
    let warhead = scene.runtime.resources.rules.warhead("AP").unwrap();
    assert!(warhead.wall && warhead.wood && !warhead.wall_absolute_destroyer);
    let initial = scene
        .sim()
        .overlay_grid
        .as_ref()
        .unwrap()
        .cell(source.0, source.1);
    let paid_wall = wall_name == "GAWALL";
    assert_eq!(initial.overlay_id, (!paid_wall).then_some(wall));
    assert_eq!(initial.overlay_data, if paid_wall { 0 } else { 5 });
    let cell = scene
        .sim()
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(source.0, source.1)
        .unwrap();
    assert_eq!((cell.level, cell.slope_type), (4, 0));
    let mut source_present = !paid_wall;
    let mut partial_seen = false;
    let mut inline_child_admission_seen = false;
    let mut first_partial_at = None;
    let mut shot_frames = Vec::new();
    let mut removed_at = None;
    let mut shots = 0;
    let mut shots_at_removal = None;
    let patch: Vec<_> = profile["terrain_cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|at| {
            (
                at[0].as_u64().unwrap() as u16,
                at[1].as_u64().unwrap() as u16,
            )
        })
        .collect();
    // Wall damage (Cell480CB0) and that jitter draw the Scenario RNG, so other
    // Houses' play moves the removal by whole shots. Observe at least the
    // profile's frames, and one full rearm past the removal.
    let profile_ticks = profile["ticks"].as_u64().unwrap();
    let mut step = 0;
    while step < 4 * profile_ticks
        && (step < profile_ticks || removed_at.is_none_or(|at| step < at + rearm))
    {
        step += 1;
        let before: Vec<_> = patch
            .iter()
            .map(|&(x, y)| *scene.sim().overlay_grid.as_ref().unwrap().cell(x, y))
            .collect();
        let due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step - 1)
            .cloned()
            .collect();
        let output = scene
            .runtime
            .advance_frame(
                &due,
                22, // Normal client interval; the headless convenience tick is 66ms.
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(output.tick.frame_committed);
        for _ in output
            .fire_events
            .iter()
            .filter(|event| event.attacker_id == attacker_id)
        {
            shots += 1;
            shot_frames.push(step);
        }
        let sim = scene.sim();
        let grid = sim.overlay_grid.as_ref().unwrap();
        for (&at, old) in patch.iter().zip(&before) {
            let now = grid.cell(at.0, at.1);
            if now == old {
                continue;
            }
            if damage_levels == 3
                && old.overlay_id == Some(wall)
                && old.overlay_data >> 4 == 1
                && now.overlay_id == Some(wall)
                && now.overlay_data >> 4 == 2
            {
                inline_child_admission_seen |=
                    patch.iter().zip(&before).any(|(&neighbor, prior)| {
                        at.0.abs_diff(neighbor.0) + at.1.abs_diff(neighbor.1) == 1
                            && prior.overlay_id == Some(wall)
                            && prior.overlay_data < 0x10
                    });
            }
            if now.overlay_id.is_none() {
                // Wall clears recalculate their cells; the frame's explicit
                // removal channel is for erasures without that recalc (crates).
                // The renderer rejects a retained map entry via the live grid.
                assert!(output.overlay_updates.iter().all(|e| (e.rx, e.ry) != at));
            } else {
                let updates: Vec<_> = output
                    .overlay_updates
                    .iter()
                    .filter(|e| (e.rx, e.ry) == at)
                    .collect();
                assert!(
                    !updates.is_empty(),
                    "live frame publication for {at:?} at {step}"
                );
                // Several synchronous cleanup visits may publish the same
                // coordinate; the app index owns slot uniqueness.
                for update in updates {
                    assert_eq!(Some(update.overlay_id), now.overlay_id);
                    assert_eq!(update.frame, now.overlay_data);
                }
            }
        }
        let live = grid.cell(source.0, source.1);
        if live.overlay_id == Some(wall) {
            source_present = true;
            if paid_wall {
                assert_eq!(live.wall_owner, Some(owner), "paid wall retains its owner");
            }
        }
        if live.overlay_id == Some(wall) && live.overlay_data >> 4 != 0 {
            partial_seen = true;
            first_partial_at.get_or_insert(step);
        }
        if source_present && live.overlay_id.is_none() && removed_at.is_none() {
            assert!(
                partial_seen,
                "ordinary fire first damages the connected source"
            );
            assert!(
                sim.entities()
                    .get(attacker_id)
                    .unwrap()
                    .attack_target
                    .target()
                    .is_none(),
                "Cell7258D0 target expiry is synchronous with removal"
            );
            assert!(sim.radar_terrain_dirty_cells.contains(&source));
            assert!(sim.tactical_dirty_cells.contains(&source));
            removed_at = Some(step);
            shots_at_removal = Some(shots);
        }
    }
    println!(
        "authored_fence_integration initial_hash={initial_hash} partial={first_partial_at:?} removed={removed_at:?} shot_frames={shot_frames:?}"
    );
    assert!(removed_at.is_some(), "ordinary AP fire opens the fence");
    if paid_wall {
        assert!(
            inline_child_admission_seen,
            "ordinary AP reaches stage two beside a pristine same-type child"
        );
        let house = scene.sim().houses.get(&owner).unwrap();
        assert_eq!(house.economy().credits(), 8500);
        assert_eq!(house.economy().spent_credits(), 1500);
    }
    assert!(shots >= 2, "the real weapon reaches repeated impacts");
    assert_eq!(
        Some(shots),
        shots_at_removal,
        "the tank stops firing at the expired Cell target"
    );
    let tank = scene.sim().entities().get(attacker_id).unwrap();
    assert!(tank.attack_target.target().is_none());
    assert_eq!(
        tank.mission.current().known(),
        Some(crate::rules::mission_data::MissionType::Guard)
    );
}

/// Unit739EC0's common tail calls Cell480CB0(-1) at73B056 before
/// object crush/Foot continuation. Supply only an arrival through the existing
/// marked-location owner; the separate normal Move capture reaches this same
/// authored fence without relocating the actor.
#[test]
fn retail_unit_per_cell_removes_authored_fence_before_returning() {
    use crate::sim::world::FrameEffects;

    struct CueBoundary {
        delivered: std::cell::Cell<usize>,
        crushed: std::cell::Cell<usize>,
    }
    impl crate::sim::world::SoundBoundary for CueBoundary {
        fn consume(
            &self,
            sim: &Simulation,
            _: &RuleSet,
            events: &[crate::sim::world::SimSoundEvent],
        ) {
            for event in &events[self.delivered.get()..] {
                if let crate::sim::world::SimSoundEvent::WallCrushed {
                    rx, ry, sound_id, ..
                } = event
                {
                    assert_eq!((*rx, *ry), (43, 82));
                    assert_eq!(sound_id, "WallCrushBlack");
                    assert!(
                        sim.overlay_grid
                            .as_ref()
                            .unwrap()
                            .cell(43, 82)
                            .overlay_id
                            .is_some(),
                        "7509E0 playback boundary precedes Cell480CB0"
                    );
                    assert_eq!(
                        sim.entities()
                            .get(1376)
                            .unwrap()
                            .rocking
                            .map_or(0, |r| r.native_bits()[3]),
                        0,
                        "recoil follows sound/damage"
                    );
                    self.crushed.set(self.crushed.get() + 1);
                }
            }
            self.delivered.set(events.len());
        }
        fn delivered(&self) -> usize {
            self.delivered.get()
        }
    }
    let cue = CueBoundary {
        delivered: std::cell::Cell::new(0),
        crushed: std::cell::Cell::new(0),
    };
    let Some((root, _assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.authored-fence.example.json");
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map = launch.selected_map_file.as_deref().unwrap().to_owned();
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    let rules = &scene.runtime.resources.rules;
    let registry = &scene.runtime.resources.overlay_registry;
    let sim = &mut scene.runtime.simulation;
    let id = 1376;
    let at = (43, 82);
    let wall = registry.id_for_name("CAFNCB").unwrap();
    let flags = registry.flags(wall).unwrap();
    assert!(flags.wall && flags.crushable);
    let object = rules.object("MTNK").unwrap();
    assert!(object.crusher && object.tilts_when_crushes);
    assert_eq!(
        sim.resolve(sim.entities().get(id).unwrap().type_ref()),
        "MTNK"
    );
    let position = {
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
            sim.resolved_terrain.as_ref().unwrap(),
        );
        crate::sim::movement::target_cell_coord(at.0, at.1, Some(&cells))
    };
    sim.foot_set_location_marked(
        id,
        position,
        Some(rules),
        Some(registry),
        FrameEffects::default(),
    );
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_id,
        Some(wall)
    );
    sim.per_cell_process(
        id,
        crate::sim::movement::PerCellReason::Arrival,
        Some(rules),
        Some(registry),
        FrameEffects::for_sound(&cue),
    )
    .unwrap();
    assert_eq!(
        cue.crushed.get(),
        1,
        "cue delivered synchronously through the existing owner"
    );
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_id,
        None,
        "native73B056 removal is visible when Per_Cell_Process returns, before the frame tail"
    );
}

/// Full AreaDamage489280 from an independently seeded, null-attacker boundary.
/// Normal map loading must produce the native wall/122/terrain priors first;
/// no wall state is supplied. This is separate from the ordinary shot witness.
#[test]
fn retail_authored_fence_area_damage_matches_original_histories() {
    assert_retail_wall_history(
        "tools/spatial_oracle/anytown_damage/authored_fence.json",
        "CAFNCB",
        false,
    );
}

/// Direct Cell480CB0(-1), independent of the actor/audio boundary. The
/// original forced controls retain the same physical map priors as the AP
/// controls and exercise real lifecycle/navigation/cleanup rather than stubs.
#[test]
fn retail_authored_fence_forced_removal_matches_original_histories() {
    assert_retail_wall_history(
        "tools/spatial_oracle/anytown_damage/fence_driveover.json",
        "CAFNCB",
        true,
    );
}

/// Original480CB0's recursive200 calls and480630's isolated concrete-wall
/// removal use the same runtime host as ordinary AP. Original construction
/// is a declared Overlay seam; paid production is exercised separately above.
#[test]
fn retail_concrete_wall_area_damage_matches_original_histories() {
    assert_retail_wall_history(
        "tools/spatial_oracle/anytown_damage/concrete_wall.json",
        "GAWALL",
        false,
    );
}

fn assert_retail_wall_history(native_path: &'static str, wall_name: &str, direct: bool) {
    use crate::sim::combat::world_receiver::apply_area_damage;
    use crate::sim::projectile::ProjectileCoord;
    use crate::sim::world::FrameEffects;
    use serde_json::{Value, json};

    fn fields(native: &Value) -> Value {
        let keys = [
            "cell",
            "overlay",
            "data",
            "neighbors",
            "land",
            "zone",
            "tile",
            "subtile",
            "level",
            "slope",
        ];
        Value::Object(
            keys.into_iter()
                .map(|key| (key.to_owned(), native[key].clone()))
                .collect(),
        )
    }
    fn cell(sim: &Simulation, at: (u16, u16)) -> Value {
        let grid = sim.overlay_grid.as_ref().unwrap();
        let overlay = grid.cell(at.0, at.1);
        let ground = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .unwrap();
        json!({"cell":[at.0,at.1], "overlay":overlay.overlay_id.map_or(-1,i32::from),
            "data":overlay.overlay_data,
            "neighbors":grid.retained_neighbor_counts()[usize::from(at.1)*usize::from(grid.width())+usize::from(at.0)],
            "land":ground.land_type,"zone":ground.zone_type,
            "tile":ground.final_tile_index,"subtile":ground.final_sub_tile,
            "level":ground.level,"slope":ground.slope_type})
    }
    fn at(native: &Value) -> (u16, u16) {
        (
            native["cell"][0].as_u64().unwrap() as u16,
            native["cell"][1].as_u64().unwrap() as u16,
        )
    }
    fn assert_cell(
        sim: &Simulation,
        expected: &Value,
        neighbor_domain: &std::collections::BTreeSet<(u16, u16)>,
    ) {
        let mut actual = cell(sim, at(expected));
        let mut projected = fields(expected);
        // Native omits map/start Foot objects. Match absolute retained bytes
        // wherever this wall history mutates them. Unrelated Foot bytes outside
        // that domain need not match; the whole-plane diff below still catches
        // any unexpected mutation of them. Never reconstruct or reset counts.
        if !neighbor_domain.contains(&at(expected)) {
            actual.as_object_mut().unwrap().remove("neighbors");
            projected.as_object_mut().unwrap().remove("neighbors");
        }
        assert_eq!(actual, projected);
        // Native stops before the later Building-based wall-owner pass. The
        // normal World has run finalize_constructed_scenario and can own these
        // walls. Selected480CB0/480630 never READ +50: retained owners must
        // remain unchanged, and direct480EF6 writes -1. Test those operations
        // below without replacing the World's independently produced owner.
        assert_eq!(expected["wall_owner"], -1);
    }
    fn snapshot(sim: &Simulation) -> Vec<Value> {
        let grid = sim.overlay_grid.as_ref().unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        (0..grid.height())
            .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| terrain.cell(x, y).is_some())
            .map(|at| cell(sim, at))
            .collect()
    }
    fn rng(sim: &Simulation) -> Value {
        json!({"scenario":sim.scenario_rng.native_state_hex(),
            "main":sim.main_rng.native_state_hex(),"mapgen":sim.mapgen_rng.native_state_hex()})
    }

    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.authored-fence.example.json");
    let native = crate::test_fixture::json(native_path);
    assert!(!native["cases"].as_array().unwrap().is_empty());
    for case in native["cases"].as_array().unwrap() {
        let neighbor_domain = case["steps"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|step| {
                step["changed"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|change| at(&change["after"]))
            })
            .collect::<std::collections::BTreeSet<_>>();
        let launch: crate::skirmish_launch::SkirmishLaunchSession =
            serde_json::from_value(profile["launch"].clone()).unwrap();
        let map_name = launch.selected_map_file.as_deref().unwrap().to_owned();
        let selected =
            crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
                .unwrap();
        let map_source = case["input"]["manifest"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| {
                source["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case(&map_name)
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(selected.source).unwrap()["source_sha256"],
            map_source["sha256"]
        );
        let mut scene = crate::headless_scenario::load_session(
            &root,
            &map_name,
            profile["seed"].as_u64().unwrap() as u32,
            launch,
        )
        .unwrap();
        let rules = &scene.runtime.resources.rules;
        let registry = &scene.runtime.resources.overlay_registry;
        if let Some(gdi) = case["input"].get("gdi_type_inputs") {
            let final_types = &gdi["layers"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|layer| layer.get("after").is_some())
                .unwrap()["after"];
            for (key, actual) in [
                ("GDIGateOne", &rules.general.building_types.gdi_gate_one),
                ("GDIGateTwo", &rules.general.building_types.gdi_gate_two),
                ("NodGateOne", &rules.general.building_types.nod_gate_one),
                ("NodGateTwo", &rules.general.building_types.nod_gate_two),
                ("WallTower", &rules.general.building_types.wall_tower),
            ] {
                assert_eq!(actual.as_deref(), final_types[key]["name"].as_str());
            }
            // Original480575..4805B8 compares a living Building's type
            // against these retained identities before its direction gates.
            // The shared query's unported positive-list arm therefore has a
            // proved false result for these ordinary paid-base neighbors.
            for query in case["query_controls"]["rows"].as_array().unwrap() {
                if let Some(buildings) = query["buildings"].as_array() {
                    assert_eq!(query["returned_low_byte"], 0);
                    assert_eq!(query["rng_unchanged"], true);
                    for name in buildings {
                        let name = name.as_str().unwrap();
                        assert!(rules.object(name).is_some());
                        assert!(!rules.general.building_types.stands_on_gdi_wall(name));
                    }
                }
            }
        }
        if direct {
            assert_eq!(
                u64::from(rules.object("MTNK").unwrap().tilts_when_crushes),
                case["input"]["mtnk_tilts"]["value"].as_u64().unwrap()
            );
        }
        let sim = &mut scene.runtime.simulation;
        let wall = registry.id_for_name(wall_name).unwrap();
        if let Some(construction) = case.get("construction") {
            // The original component starts before the later Building/Foot
            // population. Run its wall Mark material through the shared owner;
            // do not copy its resulting data/122/zone fields from the packet.
            sim.acknowledge_radar_terrain_dirty(sim.radar_terrain_dirty_generation);
            for placement in construction["placements"].as_array().unwrap() {
                let coordinate = (
                    placement["cell"][0].as_u64().unwrap() as u16,
                    placement["cell"][1].as_u64().unwrap() as u16,
                );
                assert_eq!(
                    sim.mark_overlay_for_test(rules, registry, coordinate, wall),
                    crate::map::overlay_mark::OverlayMarkOutcome::Marked
                );
                for change in placement["changed"].as_array().unwrap() {
                    assert_cell(sim, &change["after"], &neighbor_domain);
                }
            }
        }
        for expected in case["initial_patch"].as_array().unwrap() {
            assert_cell(sim, expected, &neighbor_domain);
        }
        let expected_type = case["input"]["wall_types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["index"].as_u64() == Some(u64::from(wall)))
            .unwrap();
        let flags = registry.flags(wall).unwrap();
        assert_eq!(expected_type["name"], wall_name);
        assert_eq!(json!(flags.strength), expected_type["strength"]);
        assert_eq!(json!(flags.damage_levels), expected_type["damage_levels"]);
        assert_eq!(
            u64::from(flags.wall),
            expected_type["wall"].as_u64().unwrap()
        );
        let weapon = rules.weapon("105mm").unwrap();
        if !direct {
            assert_eq!(json!(weapon.damage), case["input"]["damage"]);
        }
        assert_eq!(weapon.warhead.as_deref(), Some("AP"));
        let warhead = rules.warhead("AP").unwrap();
        assert!(warhead.wall && warhead.wood && !warhead.wall_absolute_destroyer);
        let ap = sim.intern(&warhead.id);
        let nearby_foot = sim
            .substrate
            .entities
            .values()
            .filter_map(|entity| {
                let source = entity.navigation.neighbor_state.cell();
                ((45..=47).contains(&source.0)
                    && (79..=81).contains(&source.1)
                    && source != (46, 80))
                    .then(|| {
                        (
                            entity.stable_id(),
                            sim.resolve(entity.type_ref()).to_owned(),
                            source,
                        )
                    })
            })
            .collect::<Vec<_>>();
        println!("authored_fence_excluded_foot122_context at46,80={nearby_foot:?}");
        let source = (
            case["input"]["impact_cell"][0].as_u64().unwrap() as u16,
            case["input"]["impact_cell"][1].as_u64().unwrap() as u16,
        );
        println!(
            "authored_fence_native_boundary seed={} source_owner={:?} (native pre-Building owner=-1)",
            case["input"]["seed"],
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .cell(source.0, source.1)
                .wall_owner
                .map(|owner| sim.interner.resolve(owner))
        );
        let center = crate::sim::cell_rect::get_cellclass_fallback(
            sim.resolved_terrain.as_ref(),
            i32::from(source.0),
            i32::from(source.1),
        )
        .center_coordinate()
        .unwrap();
        assert_eq!(
            json!([center.x, center.y, center.z]),
            case["input"]["impact_xyz"]
        );
        // Original native controls reseed all three streams after setup. These
        // explicit component inputs do not claim native whole-load RNG parity.
        sim.process_pending_delete_with(
            Some(rules),
            Some(registry),
            FrameEffects::default(),
            0,
            true,
        );
        for expected in case["initial_patch"].as_array().unwrap() {
            assert_cell(sim, expected, &neighbor_domain);
        }
        let seed = case["input"]["seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        sim.main_rng = SimRng::new(seed).into();
        sim.mapgen_rng = SimRng::new(seed);
        assert_eq!(rng(sim), case["initial"]["rng"]);
        if case.get("construction").is_none() {
            sim.acknowledge_radar_terrain_dirty(sim.radar_terrain_dirty_generation);
        }
        assert_eq!(
            json!(sim.radar_terrain_dirty_cells),
            case["initial"]["radar"]["cells"]
        );
        let cursor = sim.native_identity_cursor();
        let mut expected_patch = case["initial_patch"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (at(row), row.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        for step in case["steps"].as_array().unwrap() {
            assert_eq!(rng(sim), step["before"]["rng"]);
            for change in step["changed"].as_array().unwrap() {
                assert_cell(sim, &change["before"], &neighbor_domain);
            }
            let before = snapshot(sim);
            let owners_before = sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .cells()
                .iter()
                .map(|cell| (cell.overlay_id, cell.wall_owner))
                .collect::<Vec<_>>();
            let tactical_start = sim.tactical_dirty_cells.len();
            let (_, draws) = crate::sim::rng::trace_draws(|| {
                if direct {
                    let at = at(&step["source"]);
                    sim.apply_wall_damage(
                        crate::sim::overlay_grid::WallPointerTarget::Real(at.0, at.1),
                        -1,
                        registry,
                    );
                } else {
                    apply_area_damage(
                        sim,
                        rules,
                        Some(registry),
                        ProjectileCoord::new(center.x, center.y, center.z),
                        weapon.damage,
                        warhead,
                        (crate::sim::combat::RAD_NO_ATTACKER, None, ap),
                        FrameEffects::default(),
                    );
                }
            });
            let events = step["events"].as_array().unwrap();
            let actual_draws = draws
                .iter()
                .map(|row| row["value"].clone())
                .collect::<Vec<_>>();
            let expected_draws = events
                .iter()
                .filter(|row| row["kind"] == "raw_draw")
                .map(|row| row["value"].clone())
                .collect::<Vec<_>>();
            assert_eq!(
                actual_draws, expected_draws,
                "seed{seed} hit{} raw order",
                step["hit"]
            );
            assert_eq!(rng(sim), step["before_drain"]["rng"]);
            assert_eq!(
                sim.native_identity_cursor(),
                cursor,
                "wall Cell removal does not construct an object"
            );
            let after = snapshot(sim);
            // Direct480EF6 clears +50; isolated4808A7/4808BA cleanup leaves
            // it retained. Native's -1 prior cannot itself distinguish them,
            // so use the actual7258D0 receiver at return4808B8. A480630 call
            // visits five Cells; its outer receiver need not be the one erased.
            // Preserve the independently produced World owner on that arm.
            let cleanup_removed: std::collections::BTreeSet<_> = events
                .iter()
                .filter(|event| event["kind"] == "pointer_expiry" && event["caller"] == "004808B8")
                .map(|event| at(&event["before"]))
                .collect();
            let grid_width = usize::from(sim.overlay_grid.as_ref().unwrap().width());
            for (index, (prior, current)) in owners_before
                .iter()
                .zip(sim.overlay_grid.as_ref().unwrap().cells())
                .enumerate()
            {
                let coordinate = ((index % grid_width) as u16, (index / grid_width) as u16);
                if current.overlay_id.is_some()
                    || prior.0.is_none()
                    || cleanup_removed.contains(&coordinate)
                {
                    assert_eq!(
                        current.wall_owner, prior.1,
                        "retained owner is not read or replaced by this AP transaction"
                    );
                } else {
                    assert!(
                        current.wall_owner.is_none(),
                        "direct480EF6 clears the removed wall's existing owner"
                    );
                }
            }
            let actual_changes = before
                .iter()
                .zip(&after)
                .filter(|(a, b)| a != b)
                .map(|(a, b)| json!({"before":a,"after":b}))
                .collect::<Vec<_>>();
            let expected_changes = step["changed"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|change| {
                    let old = fields(&change["before"]);
                    let new = fields(&change["after"]);
                    (old != new).then(|| json!({"before":old,"after":new}))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual_changes, expected_changes,
                "seed{seed} hit{} all represented cell fields",
                step["hit"]
            );
            for change in step["changed"].as_array().unwrap() {
                assert_cell(sim, &change["after"], &neighbor_domain);
                let coordinate = at(&change["after"]);
                let ground = sim
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(coordinate.0, coordinate.1)
                    .unwrap();
                assert!(
                    sim.path_grid
                        .as_ref()
                        .unwrap()
                        .resolved_cell_is_current(ground, false),
                    "wall navigation publication at {coordinate:?}"
                );
                if expected_patch.contains_key(&at(&change["after"])) {
                    expected_patch.insert(at(&change["after"]), change["after"].clone());
                }
            }
            for expected in expected_patch.values() {
                assert_cell(sim, expected, &neighbor_domain);
            }
            assert_cell(sim, &step["source"], &neighbor_domain);
            assert_eq!(
                json!(sim.radar_terrain_dirty_cells),
                step["before_drain"]["radar"]["cells"]
            );
            assert_eq!(
                json!(sim.substrate.pending_delete.len()),
                step["before_drain"]["pending_delete"]
            );
            // Original direct dirty call480E0C retains EDI=its wall receiver;
            // cleanup4807B4 retains ESI and immediately passes that same Cell's
            // coords to6551C0. These are observed native callback operands,
            // not a replay of the cleanup algorithm from expected mutations.
            let mut active_wall = None;
            let mut expected_tactical = Vec::new();
            for (index, event) in events.iter().enumerate() {
                if event["kind"] == "wall_damage" {
                    active_wall = Some(at(&event["before"]));
                }
                if event["kind"] != "tactical_dirty" {
                    continue;
                }
                match event["caller"].as_str().unwrap() {
                    "00480E11" => expected_tactical.push(active_wall.unwrap()),
                    "004807B9" => {
                        let radar = events[index + 1..]
                            .iter()
                            .find(|event| event["kind"] == "radar_dirty")
                            .unwrap();
                        expected_tactical.push((
                            radar["cell"][0].as_u64().unwrap() as u16,
                            radar["cell"][1].as_u64().unwrap() as u16,
                        ));
                    }
                    other => panic!("uncovered native tactical callback {other}"),
                }
            }
            assert_eq!(
                &sim.tactical_dirty_cells[tactical_start..],
                expected_tactical
            );
            sim.process_pending_delete_with(
                Some(rules),
                Some(registry),
                FrameEffects::default(),
                0,
                true,
            );
            assert_eq!(rng(sim), step["after"]["rng"]);
            assert_eq!(
                json!(sim.substrate.pending_delete.len()),
                step["after"]["pending_delete"]
            );
            for expected in expected_patch.values() {
                assert_cell(sim, expected, &neighbor_domain);
            }
            // Zero-listener native expiry entries are not inferred from an
            // empty Rust detach list. Positive target release is tested above.
        }
    }
}

/// Real command/path/paid-point/PerCell production path, using stock actors
/// and authored fences. Crossing a physical cell is distinct from PerCell;
/// no expected native frame number is inferred from this Rust observation.
#[test]
fn retail_mtnk_move_crushes_fences_through_per_cell_with_body_rocking() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::{FrameEffects, TickLane};
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.fence-driveover.example.json");
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map = launch.selected_map_file.clone().unwrap();
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    scene
        .runtime
        .simulation
        .hash_state_every(crate::sim::replay::PLAY_HASH_INTERVAL);
    let owner = scene.sim().session.current_house().unwrap();
    let command: Command =
        serde_json::from_value(profile["commands"][0]["payload"].clone()).unwrap();
    let order = CommandEnvelope::new(owner, 0, command);
    let cells = [(43, 82), (44, 82)];
    let mut clears = [None; 2];
    let mut sounds = 0;
    let mut crossed_before_callback = false;
    let mut tilted = false;
    for step in 1..=profile["ticks"].as_u64().unwrap() {
        let output = scene
            .runtime
            .advance_frame(
                if step == 1 {
                    std::slice::from_ref(&order)
                } else {
                    &[]
                },
                22,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let sim = scene.sim();
        let actor = sim.entities().get(1376).unwrap();
        let grid = sim.overlay_grid.as_ref().unwrap();
        if (actor.position.rx(), actor.position.ry()) == cells[0]
            && grid.cell(cells[0].0, cells[0].1).overlay_id.is_some()
        {
            crossed_before_callback = true;
        }
        if let Some(rocking) = actor.rocking {
            tilted |= rocking.native_bits()[3] & 0x8000_0000 != 0;
        }
        for (index, &(x, y)) in cells.iter().enumerate() {
            if clears[index].is_none() && grid.cell(x, y).overlay_id.is_none() {
                clears[index] = Some(step);
                assert!(sim.radar_terrain_dirty_cells.contains(&(x, y)));
                assert!(sim.tactical_dirty_cells.contains(&(x, y)));
            }
        }
        sounds += output
            .sound_events
            .iter()
            .filter(|event| {
                matches!(event,
                    crate::sim::world::SimSoundEvent::WallCrushed { sound_id, .. }
                        if sound_id == "WallCrushBlack"
                )
            })
            .count();
    }
    assert!(
        crossed_before_callback,
        "physical crossing must not trigger the removed global sweep"
    );
    assert!(
        tilted,
        "native Drive pre-entry negative rocking rate is visible in ordinary movement"
    );
    assert!(clears.iter().all(Option::is_some));
    assert_eq!(
        sounds, 2,
        "one sound at each actual admitted overlay callback"
    );
    let actor = scene.sim().entities().get(1376).unwrap();
    assert_eq!((actor.position.rx(), actor.position.ry()), (45, 81));
    assert_eq!(
        actor.mission.current().known(),
        Some(crate::rules::mission_data::MissionType::Guard)
    );
    println!("authored_fence_driveover clears={clears:?} sounds={sounds}");
}
