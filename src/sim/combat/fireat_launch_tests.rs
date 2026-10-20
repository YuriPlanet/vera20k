//! Production frames for FireAt's launch geometry on retail rules and art:
//! the launch speed (`WeaponTypeClass::GetSpeed @ 0x00773070`) from the
//! barrel's FLH, and the moving-target lead (`0x0070BCB0`). Skipped without
//! the retail `ini/rulesmd.ini` and `ini/artmd.ini`.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::Simulation;

/// Original BuildingType constructor45DEBC and reader460F87, replayed via
/// tools.spatial_oracle.building_prism.laser_type_inputs on each retail yard:
/// GAYARD/YAYARD [300,200,0], NAYARD [256,256,0]. Building4500A0 adds this
/// to GetCoords; FireAt, laser and bolt endpoints use that virtual.
#[test]
fn retail_shipyard_target_offset_reaches_effect_coordinate() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    for (name, offset) in [
        ("GAYARD", [300, 200, 0]),
        ("NAYARD", [256, 256, 0]),
        ("YAYARD", [300, 200, 0]),
        ("GAPOWR", [0, 0, 0]),
    ] {
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object(name, "Russians", 10, 20, 0, &rules)
            .unwrap();
        let target = sim.substrate.entities.get(id).unwrap();
        let centre = crate::sim::movement::ground_pose::object_get_coords(target, None);
        assert_eq!(
            super::fire_coord::effect_target_coordinate(
                &sim,
                &rules,
                super::TargetKind::Entity(id).projectile_target(&sim)
            ),
            Some(ProjectileCoord::new(
                centre.x + offset[0],
                centre.y + offset[1],
                centre.z + offset[2]
            )),
            "{name} target point"
        );
    }
}

fn attack_aim_corpus() -> &'static serde_json::Value {
    crate::test_fixture::json(
        "tools/procedural_drawing_oracle/action_lines_attack_prerequisites.json",
    )
}

fn attack_aim_row<'a>(
    corpus: &'a serde_json::Value,
    group: &str,
    name: &str,
) -> &'a serde_json::Value {
    corpus[group]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("missing native {group} case {name}"))
}

fn attack_aim_coord(row: &serde_json::Value) -> ProjectileCoord {
    ProjectileCoord::new(
        row[0].as_i64().unwrap() as i32,
        row[1].as_i64().unwrap() as i32,
        row[2].as_i64().unwrap() as i32,
    )
}

/// Use the existing retail-rules fixture with the original query's actor
/// coordinates. The represented local terrain is flat level four, matching
/// these recorded cells; this is not a replay of the whole native Scenario.
fn attack_aim_duel(corpus: &serde_json::Value) -> Option<(Duel, u64, u64)> {
    let mut duel = Duel::new()?;
    let initial = attack_aim_row(corpus, "aim_cases", "stationary_target_aim");
    let source = attack_aim_coord(&initial["input"]["source_xyz"]);
    let target = attack_aim_coord(&initial["input"]["target_xyz"]);
    duel.sim.session.set_map_width_for_test(128);
    duel.sim.session.set_map_height_for_test(128);
    duel.sim
        .install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
            128,
            128,
            |rx, ry| {
                let mut cell = crate::map::resolved_terrain::test_clear_cell(rx, ry);
                cell.level = 4;
                cell.speed_costs = crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS;
                cell.base_speed_costs = cell.speed_costs;
                cell
            },
        ));
    assert!(duel.sim.rebuild_dynamic_navigation(&duel.rules));
    duel.grid = duel
        .sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("native aim fixture navigation grid");
    let source_id = duel.spawn(
        "MTNK",
        "Americans",
        (source.x / 256) as u16,
        (source.y / 256) as u16,
        128,
    );
    let target_id = duel.spawn(
        "MTNK",
        "Russians",
        (target.x / 256) as u16,
        (target.y / 256) as u16,
        0,
    );
    duel.sim.resolve_type_handles(&duel.rules);
    assert_eq!(duel.location(source_id), source);
    assert_eq!(duel.location(target_id), target);
    duel.sim
        .assign_target_represented(
            source_id,
            Some(super::TargetKind::Entity(target_id)),
            Some(&duel.rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("original Unit Assign_Target receiver");
    Some((duel, source_id, target_id))
}

fn begin_native_aim_motion(duel: &mut Duel, target_id: u64) {
    // The native history calls Unit741970 to Cell87,53, then SetSpeedFraction
    // 4D3710(1). It does not run Process_Track before querying the aim.
    assert!(duel.sim.set_unit_destination(
        target_id,
        crate::sim::components::NavTargetRef::Cell { rx: 87, ry: 53 },
        &duel.rules,
        true,
        crate::sim::world::FrameEffects::default(),
    ));
    let target = duel.sim.substrate.entities.get_mut(target_id).unwrap();
    target
        .foot_speed
        .set_speed_fraction(crate::util::fixed_math::SIM_ONE);
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(target),
        Some(true)
    );
}

fn assert_read_only_native_aim(duel: &Duel, source_id: u64, row: &serde_json::Value) {
    let hash = duel.sim.state_hash();
    let main_rng = duel.sim.main_rng.logical_state();
    let scenario_rng = duel.sim.scenario_rng.logical_state();
    let tarcom = duel
        .sim
        .substrate
        .entities
        .get(source_id)
        .unwrap()
        .attack_target
        .target();
    let actual = super::aim_coord::led_target_coordinate(&duel.sim, &duel.rules, source_id, tarcom);
    assert_eq!(actual, attack_aim_coord(&row["aim"]), "{}", row["name"]);
    assert_eq!(duel.sim.state_hash(), hash, "{}: query state", row["name"]);
    assert_eq!(
        duel.sim.main_rng.logical_state(),
        main_rng,
        "{}: Main RNG",
        row["name"]
    );
    assert_eq!(
        duel.sim.scenario_rng.logical_state(),
        scenario_rng,
        "{}: Scenario RNG",
        row["name"]
    );
}

/// Original70BCB0 runs actual target virtuals, Drive::Is_Moving, Foot's live
/// speed, GetCurrentWeapon and FacingClass. Keep the same moving actor while
/// its rank changes: the lead reads the promoted speed.
#[test]
fn original_aim_query_reads_live_motion_rank_and_facing_without_mutation() {
    let corpus = attack_aim_corpus();
    let Some((mut duel, source, target)) = attack_aim_duel(&corpus) else {
        return;
    };
    assert_read_only_native_aim(
        &duel,
        source,
        attack_aim_row(&corpus, "aim_cases", "stationary_target_aim"),
    );
    begin_native_aim_motion(&mut duel, target);
    for (facing, name) in [
        (0, "moving_target_aim_0"),
        (0x4000, "moving_target_aim_16384"),
        (0xc000, "moving_target_aim_49152"),
    ] {
        duel.sim
            .substrate
            .entities
            .get_mut(target)
            .unwrap()
            .body_facing
            .snap(facing, duel.sim.session.binary_frame());
        assert_read_only_native_aim(&duel, source, attack_aim_row(&corpus, "aim_cases", name));
    }
    duel.sim
        .substrate
        .entities
        .get_mut(target)
        .unwrap()
        .veterancy
        .set_veteran();
    assert_read_only_native_aim(
        &duel,
        source,
        attack_aim_row(&corpus, "aim_cases", "veteran_target_aim"),
    );
    duel.sim
        .assign_target_represented(
            source,
            None,
            Some(&duel.rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_read_only_native_aim(
        &duel,
        source,
        attack_aim_row(&corpus, "aim_cases", "targetless_aim"),
    );
}

/// Full original6FDD50 uses Cell87,54 for the shot's launch speed while
/// 70BCB0 independently leads a veteran MTNK TarCom at87,52. Checking the
/// emitted bullet catches the old snap.target/argument-coordinate coupling.
#[test]
fn original_direct_fireat_keeps_argument_speed_separate_from_live_tarcom_aim() {
    let corpus = attack_aim_corpus();
    let Some((mut duel, source, target)) = attack_aim_duel(&corpus) else {
        return;
    };
    begin_native_aim_motion(&mut duel, target);
    let actor = duel.sim.substrate.entities.get_mut(target).unwrap();
    actor
        .body_facing
        .snap(0xc000, duel.sim.session.binary_frame());
    actor.veterancy.set_veteran();
    let row = attack_aim_row(
        &corpus,
        "fireat_cases",
        "fireat_argument_cell_distinct_from_moving_tarcom_unit",
    );
    assert_read_only_native_aim(&duel, source, row);
    let argument = attack_aim_coord(&row["input"]["argument_target_xyz"]);
    let argument_target =
        super::TargetKind::Cell((argument.x / 256) as u16, (argument.y / 256) as u16);
    duel.sim.commit_fire_visit(
        super::world_fire::FireVisit::Direct {
            id: source,
            target: argument_target,
            weapon_index: 0,
        },
        &duel.rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let bullets: Vec<_> = duel
        .sim
        .projectiles
        .iter()
        .map(|(_, bullet)| bullet)
        .collect();
    assert_eq!(bullets.len(), 1);
    let bullet = bullets[0];
    assert_eq!(
        bullet.launch_origin(),
        attack_aim_coord(&row["bullet"]["position"])
    );
    assert_eq!(
        bullet.launch_target(),
        argument,
        "Bullet::Fire retains the unled argument"
    );
    assert_eq!(
        i64::from(bullet.speed_leptons_per_frame()),
        row["launch_speed"].as_i64().unwrap()
    );
    let expected_velocity = row["bullet"]["velocity"]["bits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|component| u64::from_str_radix(component.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    // The level Cannon arc takes its pitch from the Acos table (48A9D0).
    assert_eq!(
        bullet
            .velocity()
            .native()
            .map(|component| component.bits())
            .as_slice(),
        expected_velocity
    );

    // A valid argument does not become the fallback for null TarCom. The
    // native second Direct call reaches70BCB0, returns zero, and emits no
    // surviving bullet. This does not certify its full failure cleanup order.
    let null = attack_aim_row(
        &corpus,
        "fireat_cases",
        "fireat_argument_cell_with_null_tarcom",
    );
    duel.sim
        .assign_target_represented(
            source,
            None,
            Some(&duel.rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_read_only_native_aim(&duel, source, null);
    let previous_bullets = duel
        .sim
        .projectiles
        .iter()
        .map(|(&id, _)| id)
        .collect::<Vec<_>>();
    assert!(null["bullet"].is_null());
    duel.sim.commit_fire_visit(
        super::world_fire::FireVisit::Direct {
            id: source,
            target: argument_target,
            weapon_index: 0,
        },
        &duel.rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        duel.sim
            .projectiles
            .iter()
            .map(|(&id, _)| id)
            .collect::<Vec<_>>(),
        previous_bullets
    );
}

/// A non-null stable TarCom must resolve until the existing UnInit expiry
/// owner clears it. Exercise that owner and the deferred destructor before a
/// presentation-style query. The original concrete expiry receipt also pins
/// the passive-scan timer and all three complete RNG states at that boundary.
#[test]
fn target_uninit_clears_tarcom_before_aim_query_and_physical_removal() {
    use crate::sim::rng::SimRng;

    let corpus = attack_aim_corpus();
    let Some((mut duel, source, target)) = attack_aim_duel(&corpus) else {
        return;
    };
    let expiry = attack_aim_row(&corpus, "steps", "supplied_dead_target_expiry_active_scan");
    let assert_rng = |sim: &Simulation, native: &serde_json::Value, boundary: &str| {
        for (name, actual) in [
            ("scenario", sim.scenario_rng.logical_state()),
            ("main", sim.main_rng.logical_state()),
            ("mapgen", sim.mapgen_rng.logical_state()),
        ] {
            let expected: SimRng = serde_json::from_value(native[name].clone()).unwrap();
            assert_eq!(
                actual,
                expected.logical_state(),
                "{boundary}: full {name} table, disabled flag and both indexes"
            );
        }
    };
    let assert_timer = |sim: &Simulation, native: &serde_json::Value| {
        let timer = sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .passive_scan
            .timer();
        // MissionTimer owns the native +180 start/+188 duration. The
        // intervening +184 DWORD is not a represented cadence input.
        assert_eq!(u64::from(timer.start_frame()), native[0].as_u64().unwrap());
        assert_eq!(u64::from(timer.duration()), native[2].as_u64().unwrap());
    };
    duel.sim
        .session
        .set_binary_frame_for_test(expiry["before"]["source"]["frame"].as_u64().unwrap() as u32);
    let before_timer = &expiry["before"]["targeting_timer"];
    duel.sim
        .substrate
        .entities
        .get_mut(source)
        .unwrap()
        .passive_scan
        .timer_mut_for_test()
        .arm(
            before_timer[0].as_u64().unwrap() as u32,
            before_timer[2].as_u64().unwrap() as u32,
        );
    // Match the supplied native boundary, independently checking all 250
    // words and both cursors below; index_a=3 alone would not establish it.
    duel.sim.scenario_rng = SimRng::new(0);
    for _ in 0..3 {
        duel.sim.scenario_rng.next_u32();
    }
    duel.sim.main_rng = SimRng::new(0).into();
    duel.sim.mapgen_rng = SimRng::new(0);
    assert_rng(&duel.sim, &expiry["rng_before"], "before UnInit");
    assert_timer(&duel.sim, before_timer);

    let ((), draws) = crate::sim::rng::trace_draws(|| {
        duel.sim.uninit_with_rules(target, &duel.rules);
    });
    let expected_raw: Vec<_> = expiry["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "raw")
        .map(|event| event["value"].as_u64().unwrap())
        .collect();
    assert_eq!(
        draws
            .iter()
            .map(|draw| draw["value"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        expected_raw,
        "the full UnInit emits only the original expiry draw"
    );
    assert_rng(&duel.sim, &expiry["rng_after"], "after UnInit");
    assert_timer(&duel.sim, &expiry["after"]["targeting_timer"]);
    assert!(
        duel.sim.substrate.entities.get(target).is_some(),
        "UnInit defers physical removal"
    );
    assert!(
        duel.sim
            .substrate
            .entities
            .get(source)
            .unwrap()
            .attack_target
            .target()
            .is_none(),
        "PointerExpired clears the live reference first"
    );
    let expected = attack_aim_row(&corpus, "aim_cases", "expired_target_aim");
    assert_read_only_native_aim(&duel, source, expected);
    duel.sim.process_pending_delete_with(
        Some(&duel.rules),
        None,
        crate::sim::world::FrameEffects::default(),
        1,
        false,
    );
    assert!(duel.sim.substrate.entities.get(target).is_none());
    assert_read_only_native_aim(&duel, source, expected);
    assert_rng(&duel.sim, &expiry["rng_after"], "after deferred removal");
    assert_timer(&duel.sim, &expiry["after"]["targeting_timer"]);
}

#[test]
fn grand_cannon_recoil_state_cannot_change_simulation_hash_or_rng() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let cannon = duel.spawn("GTGCAN", "Americans", 20, 20, 64);
    let hash = duel.sim.state_hash();
    let rng = duel.sim.scenario_rng.logical_state();
    let entity = duel.sim.substrate.entities.get_mut(cannon).unwrap();
    entity.fire_voxel_recoil(true);
    entity.update_voxel_recoil();
    assert!(entity.voxel_recoil().1);
    assert_eq!(duel.sim.state_hash(), hash);
    assert_eq!(duel.sim.scenario_rng.logical_state(), rng);
}

struct Duel {
    sim: Simulation,
    rules: RuleSet,
    grid: crate::sim::pathfinding::PathGrid,
    /// Projectiles alive before the latest tick.
    before: std::collections::BTreeSet<u64>,
}

impl Duel {
    fn new() -> Option<Self> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
        let mut sim = Simulation::new();
        for (name, side, human) in [("Americans", 0, true), ("Russians", 1, true)] {
            let id = sim.interner.intern(name);
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    id, side, None, human, 0, 10,
                ));
        }
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        // Production binds every rule identity before building the derived
        // type table. A table built with only House IDs cannot resolve types
        // interned by later spawns, including destination preflight inputs.
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some(Self {
            sim,
            rules,
            grid,
            before: std::collections::BTreeSet::new(),
        })
    }

    fn spawn(&mut self, kind: &str, owner: &str, rx: u16, ry: u16, facing: u8) -> u64 {
        let id = self
            .sim
            .spawn_object(kind, owner, rx, ry, facing, &self.rules)
            .unwrap_or_else(|| panic!("spawn {kind}"));
        assert!(
            self.sim
                .object_type(self.sim.entities().get(id).unwrap().type_ref(), &self.rules)
                .is_some(),
            "spawned {kind} must have a bound production type identity"
        );
        id
    }

    fn order(&mut self, owner: &str, command: Command) {
        let owner = self.sim.interner.intern(owner);
        self.sim.queue_command(CommandEnvelope::new(
            owner,
            self.sim.session.tick() + 1,
            command,
        ));
    }

    fn tick(&mut self) {
        self.sim.fire_events.clear_for_test();
        self.before = self.sim.projectiles.iter().map(|(&id, _)| id).collect();
        let commands = self.sim.take_due_commands();
        self.sim
            .advance_tick(&commands, Some(&self.rules), Some(&self.grid), None, 67);
    }

    fn location(&self, id: u64) -> ProjectileCoord {
        let entity = self.sim.substrate.entities.get(id).unwrap();
        ProjectileCoord::new(
            i32::from(entity.position.rx()) * 256 + entity.position.sub_x().to_num::<i32>(),
            i32::from(entity.position.ry()) * 256 + entity.position.sub_y().to_num::<i32>(),
            crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                self.sim.resolved_terrain.as_ref(),
            ),
        )
    }

    fn actor_state(&self, id: u64) -> String {
        let Some(actor) = self.sim.substrate.entities.get(id) else {
            return format!("{id}: retired");
        };
        format!(
            "{id}: location={:?}, health={}, mission={:?}, target={:?}, nav={:?}, moving={:?}, fraction={:?}, last_fire={}",
            self.location(id),
            actor.health.current(),
            actor.mission,
            actor.attack_target,
            actor.navigation.nav_com(),
            crate::sim::movement::motion_query::is_moving(actor),
            actor.foot_speed.applied_fraction(),
            actor.last_fire_frame.frame(),
        )
    }

    /// The first shell the firer launched this frame, if any. It has already
    /// taken its first AI step in the frame's tail, so it is recognised as
    /// new rather than by standing on its launch origin.
    fn launched_by(&self, firer: u64) -> Option<&crate::sim::projectile::Projectile> {
        self.sim
            .projectiles
            .iter()
            .map(|(_, projectile)| projectile)
            .find(|projectile| {
                projectile.source_id() == firer && !self.before.contains(&projectile.id())
            })
    }
}

/// A Rhino driving south past the Grizzly is led: the shell is launched at
/// where it will be, south of where it stands at the shot (`0x0070BCB0`'s lead
/// along its facing). Movement runs before combat in a frame, so the Rhino's
/// position after the frame is its position at the shot.
#[test]
fn a_moving_rhino_is_led() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let grizzly = duel.spawn("MTNK", "Americans", 16, 16, 64);
    let rhino = duel.spawn("HTNK", "Russians", 20, 10, 128);
    duel.order(
        "Russians",
        Command::Move {
            entity_id: rhino,
            target_rx: 20,
            target_ry: 28,
        },
    );
    duel.order(
        "Americans",
        Command::Attack {
            attacker_id: grizzly,
            target_id: rhino,
        },
    );
    let mut moving_frames = 0;
    for _ in 0..240 {
        let moving = duel
            .sim
            .substrate
            .entities
            .get(rhino)
            .is_some_and(|entity| {
                crate::sim::movement::motion_query::is_moving(entity) == Some(true)
            });
        moving_frames += u32::from(moving);
        duel.tick();
        let Some(shell) = duel.launched_by(grizzly) else {
            continue;
        };
        if !moving {
            continue;
        }
        let target = duel.location(rhino);
        let [vx, vy, _] = shell
            .velocity()
            .native()
            .map(|bits| f64::from_bits(bits.bits()));
        // Where the launch heading crosses the Rhino's column, relative to
        // the Rhino: the lead, `ftol(d / (GetSpeed * 0.9) * Rhino speed)`.
        let crossing = shell.launch_origin().y as f64
            + vy / vx * (target.x - shell.launch_origin().x) as f64
            - target.y as f64;
        // About 230 here; the unled heading crosses within ~15 of the Rhino.
        assert!(
            crossing > 100.0,
            "the shell aims {crossing:.1} leptons south of the Rhino"
        );
        return;
    }
    panic!(
        "no shell was launched at the moving Rhino ({moving_frames} moving frames); {}; {}",
        duel.actor_state(grizzly),
        duel.actor_state(rhino),
    );
}

/// An `AreaFire=` weapon fires at the cell under its Unit firer, not at the
/// ordered target (`TechnoClass::FireAt 0x006FE140..0x006FE1D8`): a Chaos
/// Drone told to attack a GI two cells east releases its gas (PsychGasCreate,
/// CellSpread 3) around itself. A GI two cells west of the drone goes
/// berserk; one two cells beyond the target does not. The drone keeps the
/// target as its TarCom.
#[test]
fn a_chaos_drone_gasses_its_own_cell() {
    let Some(mut duel) = Duel::new() else {
        return;
    };
    let drone = duel.spawn("CAOS", "Russians", 16, 16, 64);
    let target = duel.spawn("E1", "Americans", 18, 16, 192);
    let near_drone = duel.spawn("E1", "Americans", 14, 16, 64);
    let past_target = duel.spawn("E1", "Americans", 20, 16, 192);
    duel.order(
        "Russians",
        Command::Attack {
            attacker_id: drone,
            target_id: target,
        },
    );
    let berserk = |duel: &Duel, id: u64| {
        duel.sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.berserk.is_active())
    };
    for _ in 0..120 {
        duel.tick();
        let Some(shot) = duel
            .sim
            .fire_events
            .iter()
            .find(|event| event.attacker_id == drone)
        else {
            continue;
        };
        assert_eq!(
            shot.target,
            super::TargetKind::Cell(16, 16),
            "the gas is fired at the drone's own cell"
        );
        let actor = duel.sim.substrate.entities.get(drone).unwrap();
        assert_eq!(
            actor.attack_target.target(),
            Some(super::TargetKind::Entity(target)),
            "TarCom keeps the GI"
        );
        for _ in 0..4 {
            if berserk(&duel, near_drone) {
                break;
            }
            duel.tick();
        }
        assert!(
            berserk(&duel, near_drone),
            "the GI beside the drone is gassed"
        );
        assert!(
            !berserk(&duel, past_target),
            "the GI beyond the target, out of the drone's spread, is not"
        );
        return;
    }
    panic!(
        "the drone never fired; {}; {}",
        duel.actor_state(drone),
        duel.actor_state(target)
    );
}
