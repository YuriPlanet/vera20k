//! Native supplied-row comparison and connected input fixtures.
use super::super::native_cell_input_test_fixture::{
    bounds, coord, native_fixture, pair, terrain_cell,
};
use super::*;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainClass;
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::intern::InternedId;
use serde_json::{Value, json};

/// 20 Infantry/Walk, 7 Unit/Drive and 2 Unit/Ship receivers of the same
/// 4DE1D0 corpus: the resolver is class-generic, so one Rust owner serves all.
#[test]
fn ordinary_foot_input_matches_native_admission_rows() {
    let rows: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_move_admission.json",
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 29);
    for (index, row) in rows.as_array().unwrap().iter().enumerate() {
        let input = &row["input"];
        let (terrain, zones, raw) = native_fixture(input);
        let map = ClickMap {
            cells: NativeCellQuery::isolated(&terrain),
            zones: &zones,
            raw: &raw,
            overlays: None,
            bounds: bounds(),
            size: (8, 8),
            frame: input["frame"].as_u64().unwrap_or(100) as u32,
        };
        let cells = &map.cells;
        let current = coord(input.get("xyz").unwrap_or(&json!([1344, 1344, 0])));
        let head = coord(input.get("head").unwrap_or(&json!([0, 0, 0])));
        let clicked = pair(input.get("clicked").unwrap_or(&json!([11, 5])));
        let click = FootCellClick {
            clicked: (clicked.0 as i16, clicked.1 as i16),
            action: input["action"].as_u64().unwrap_or(1) as u32,
            move_to_shroud: input["move_to_shroud"].as_bool().unwrap_or(true),
            teleporter: false,
            mover: ClickMover {
                current,
                coordinate: if head == (DriveCoord { x: 0, y: 0, z: 0 }) {
                    current
                } else {
                    head
                },
                marked: input["marked"].as_bool().unwrap_or(true),
                on_bridge: input["on_bridge"].as_bool().unwrap_or(false),
                in_tube: false,
                special_type: false,
                movement_zone: match input["movement_zone"].as_u64().unwrap_or(4) {
                    0 => MovementZone::Normal,
                    1 => MovementZone::Crusher,
                    3 => MovementZone::AmphibiousDestroyer,
                    4 => MovementZone::AmphibiousCrusher,
                    10 => MovementZone::Water,
                    other => panic!("unmapped movement zone {other}"),
                },
                speed_type: match input["receiver"].as_str() {
                    Some("unit") => SpeedType::Track,
                    Some("ship") => SpeedType::Float,
                    _ => SpeedType::Foot,
                },
            },
        };
        let result = resolve_foot_cell_click(&map, &click, |id| {
            Ok(match id {
                NativeCellIdentity::Real(_) => {
                    !input["shrouded"].as_array().into_iter().flatten().any(|p| {
                        let c = cells.coord(id);
                        pair(p) == (c.0 as u16, c.1 as u16)
                    })
                }
                NativeCellIdentity::Dummy => false,
            })
        })
        .unwrap();
        let expected = pair(&row["cell"]);
        assert_eq!(
            result,
            (expected != (0, 0)).then_some(expected),
            "row {index}: {input}"
        );
        assert_eq!(
            cells.coord(NativeCellIdentity::Dummy),
            (pair(&row["dummy"]).0 as i16, pair(&row["dummy"]).1 as i16),
            "Dummy row {index}"
        );
        assert_eq!(
            terrain.dummy_cell_requested_coord(),
            (99, 98),
            "input must not stamp canonical Dummy row {index}"
        );
    }
}

/// Original51AA40's human Doing27 gate returns before NavCom, path or Walk
/// writes (walk_first_path setter row). The incoming Doing is a supplied
/// boundary control, not a claim that stock E1 can enter this deploy action.
/// Construction, map setup and ground-order dispatch use their live owners.
#[test]
fn ground_walk_order_preserves_native_human_deploy_refusal() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_first_path.json",
    ))
    .unwrap();
    let native = &corpus["setter"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["doing"] == 27 && row["input"]["human"] != false)
        .unwrap()["setter"];
    let (mut sim, rules, id) = Simulation::walk_cell_input_test_scene(false);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.mission_leaf.set_infantry_doing_verified(27).unwrap();
    actor.navigation.path_runtime.start_movement(50, 5);
    let navigation_before = serde_json::to_value(&actor.navigation).unwrap();
    let rng_before = sim.rng_state();

    // The native class setter is void: its incidental EAX is not acceptance.
    let _ = sim.assign_destination_represented(
        id,
        Some(crate::sim::components::NavTargetRef::cell(11, 10)),
        Some(&rules),
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let actor = sim.substrate.entities.get(id).unwrap();
    let walk = actor.locomotor.as_ref().unwrap();
    let raw_coord = |value: Option<DriveCoord>| {
        let value = value.unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
        json!([value.x, value.y, value.z])
    };
    assert_eq!(raw_coord(walk.walk_destination()), native["destination"]);
    assert_eq!(raw_coord(walk.step_head()), native["head"]);
    assert_eq!(
        json!(u8::from(walk.walk_is_moving().unwrap())),
        native["moving"]
    );
    let timer = actor.navigation.path_runtime.movement_timer();
    assert_eq!(
        json!({"start_frame": timer.start_frame(), "duration": timer.duration()}),
        native["movement_timer"]
    );
    assert_eq!(
        serde_json::to_value(&actor.navigation).unwrap(),
        navigation_before
    );
    assert_eq!(sim.rng_state(), rng_before);
}

#[test]
fn isolated_cell_queries_preserve_all_modeled_dummy_inputs_and_identity() {
    let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![terrain_cell(0, 0)]);
    let canonical = terrain.shared_cell_dummy();
    canonical.stamp_coord(99, 98);
    canonical.set_level_slope(-3, 2);
    canonical.write_raw_flags(0x10d80);
    canonical.write_native_anchor(Some(NativeCellIdentity::Real(0)));
    canonical.set_overlay_fields(Some(0x18), 7);
    canonical.write_raw_tube_index(23);
    let before = format!("{canonical:?}");
    let cells = NativeCellQuery::isolated(&terrain);
    let copied = cells.dummy();
    assert_eq!(copied.snapshot(), canonical.snapshot());
    assert_eq!(copied.raw_flags(), canonical.raw_flags());
    assert_eq!(copied.native_anchor(), canonical.native_anchor());
    assert_eq!(copied.overlay_fields(), canonical.overlay_fields());
    assert_eq!(copied.raw_tube_index(), canonical.raw_tube_index());
    let retained = cells.lookup((20, 21));
    assert_eq!(retained, NativeCellIdentity::Dummy);
    assert_eq!(cells.lookup_world(22 * 256, 23 * 256), retained);
    assert_eq!(cells.coord(retained), (22, 23));
    assert_eq!(copied.snapshot().coord, (22, 23));
    assert_eq!(cells.flags(retained), 0x10d80);
    assert_eq!(format!("{canonical:?}"), before);
}

/// The walk scene's river map with retail rules: the retail E1 it spawns at
/// (5,5) and `type_name` for the same House at (6,4).
fn retail_scene(type_name: &str) -> Option<(Simulation, RuleSet, InternedId, u64, u64)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (mut sim, rules, gi) = Simulation::walk_cell_input_test_scene_with(rules, false);
    let owner = sim.interner.get("Local").unwrap();
    let id = sim
        .spawn_object(type_name, "Local", 6, 4, 0, &rules)
        .unwrap_or_else(|| panic!("retail {type_name} spawns"));
    sim.resolve_type_handles(&rules);
    for cell in [(6, 4), (9, 5), (10, 5), (11, 5), (12, 5)] {
        assert!(
            cell_is_in_playfield_height_aware_in_query(
                (i32::from(cell.0), i32::from(cell.1)),
                sim.playfield_bounds,
                sim.resolved_terrain.as_ref(),
                None,
            ),
            "{cell:?}"
        );
    }
    Some((sim, rules, owner, gi, id))
}

fn query(mouse_over: bool, shift: bool) -> CellActionQuery {
    CellActionQuery { mouse_over, shift }
}

/// The cursor update asks What_Action with its mouse-over argument
/// (`0x0069300B`), which answers Move before Can_Enter_Cell
/// (`0x00700B51..0x00700B57`); the click asks it, so a retail Grizzly's
/// click over the river answers NoMove and resolves to its own bank. Shift
/// answers Move before the probe (`0x00700B03`).
#[test]
fn only_the_click_asks_can_enter_cell() {
    let Some((sim, rules, owner, _, tank)) = retail_scene("MTNK") else {
        return;
    };
    let ask = |mouse_over, shift| {
        sim.cell_move_action(owner, tank, (9, 5), query(mouse_over, shift), &rules, None)
            .unwrap()
    };
    assert_eq!(ask(true, false), MoveAction::Move);
    assert_eq!(ask(false, false), MoveAction::NoMove);
    assert_eq!(ask(false, true), MoveAction::Move);
    let goal = sim
        .cell_click_destination(owner, tank, (9, 5), false, &rules, None)
        .unwrap()
        .unwrap();
    assert!(goal.0 < 8, "{goal:?}");
}

/// Every receiver takes the Cell resolver, aircraft included, and an
/// Aircraft is special there (`0x004DE325..0x004DE335`): a retail Harrier
/// keeps a clicked river Cell, where the retail GI's click resolves to its
/// own bank.
#[test]
fn an_aircraft_click_keeps_a_river_cell() {
    let Some((sim, rules, owner, gi, harrier)) = retail_scene("ORCA") else {
        return;
    };
    assert_eq!(
        sim.cell_click_destination(owner, harrier, (9, 5), false, &rules, None),
        Ok(Some((9, 5)))
    );
    let goal = sim
        .cell_click_destination(owner, gi, (9, 5), false, &rules, None)
        .unwrap()
        .unwrap();
    assert!(goal.0 < 8, "{goal:?}");
}

/// A Teleporter whose own What_Action answers Move keeps the clicked Cell,
/// even across the river (`0x004DE467..0x004DE47E`). Over the river its
/// Can_Enter_Cell answers NoMove, so the zone test and FNPC send it to its
/// own bank (`0x004DE484..0x004DE528`). Retail CLEG.
#[test]
fn a_teleporter_keeps_a_cell_it_can_enter() {
    let Some((sim, rules, owner, _, legionnaire)) = retail_scene("CLEG") else {
        return;
    };
    assert!(rules.object("CLEG").unwrap().teleporter);
    assert_eq!(
        sim.cell_click_destination(owner, legionnaire, (11, 5), false, &rules, None),
        Ok(Some((11, 5)))
    );
    let goal = sim
        .cell_click_destination(owner, legionnaire, (10, 5), false, &rules, None)
        .unwrap()
        .unwrap();
    assert!(goal.0 < 8, "{goal:?}");
}

/// Foot's shroud rung (`0x004DDE64..0x004DDEBE`): over a shrouded Cell a
/// retail CLEG (MoveToShroud=no) answers NoMove for the cursor and the
/// click alike, and its click resolves nothing (`0x004DE26B..0x004DE284`);
/// a retail Grizzly answers Move and its click resolves a Cell.
#[test]
fn shroud_bars_a_type_without_move_to_shroud() {
    let Some((mut sim, rules, owner, _, legionnaire)) = retail_scene("CLEG") else {
        return;
    };
    let tank = sim.spawn_object("MTNK", "Local", 6, 6, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    assert!(!rules.object("CLEG").unwrap().move_to_shroud);
    assert!(rules.object("MTNK").unwrap().move_to_shroud);
    let shrouded = (12, 5);
    sim.fog = crate::sim::vision::FogState::new(16, 16);
    sim.fog.reveal_cells_for_owner(
        owner,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&cell| cell != shrouded),
    );
    assert!(!sim.fog.is_cell_revealed(owner, shrouded.0, shrouded.1));
    for mouse_over in [true, false] {
        let ask = |id| {
            sim.cell_move_action(owner, id, shrouded, query(mouse_over, false), &rules, None)
                .unwrap()
        };
        assert_eq!(ask(legionnaire), MoveAction::NoMove);
        assert_eq!(ask(tank), MoveAction::Move);
    }
    assert_eq!(
        sim.cell_click_destination(owner, legionnaire, shrouded, false, &rules, None),
        Ok(None)
    );
    assert!(
        sim.cell_click_destination(owner, tank, shrouded, false, &rules, None)
            .unwrap()
            .is_some()
    );
}

/// Foot `0x004D74E0`'s Move arm over an object the retail GI's zone reaches
/// (a retail Grizzly beside it): Move keeps the object; NoMove, and an
/// Infantry under the force-move key (`0x004D7B0F..0x004D7B1D`), search near
/// the object's Cell in the mover's own zone. FNPC skips the Grizzly's own
/// Cell, which its occupation bits bar (`0x004834A0`), and answers (5,4). A
/// Unit under the key keeps the object, and a Unit's NoMove over itself
/// orders nothing (`0x007388D5`).
#[test]
fn an_object_click_keeps_an_object_its_zone_reaches() {
    let Some((sim, rules, owner, gi, tank)) = retail_scene("MTNK") else {
        return;
    };
    let click = |id, target, action, force_move_key| {
        sim.object_move_click(owner, id, target, action, force_move_key, &rules)
            .unwrap()
    };
    let onto_tank = Some(TargetKind::Entity(tank));
    let near_tank = Some(TargetKind::Cell(5, 4));
    assert_eq!(click(gi, tank, MoveAction::Move, false), onto_tank);
    assert_eq!(click(gi, tank, MoveAction::NoMove, false), near_tank);
    assert_eq!(click(gi, tank, MoveAction::Move, true), near_tank);
    assert_eq!(
        click(tank, gi, MoveAction::Move, true),
        Some(TargetKind::Entity(gi))
    );
    assert_eq!(click(tank, tank, MoveAction::NoMove, false), None);
    // The class tails turn a player's controllable 0 into NoMove (Infantry
    // `0x0051F05E..0x0051F08B`, Unit `0x007403EB..0x00740414`).
    assert_eq!(click(gi, tank, MoveAction::None, false), near_tank);
    assert_eq!(
        click(tank, gi, MoveAction::None, false),
        click(tank, gi, MoveAction::NoMove, false)
    );
}

/// An object across the river: the retail GI's zone cannot reach its Cell,
/// so FNPC sends the GI to its own bank (`0x004D7B5E..0x004D7BF3`). An
/// Aircraft is special and keeps the object (`0x004D7AFB`), but its wrapper
/// refuses NoMove (`0x00417C12`).
#[test]
fn an_object_across_the_river_sends_the_mover_to_its_own_bank() {
    let Some((mut sim, rules, owner, gi, harrier)) = retail_scene("ORCA") else {
        return;
    };
    let far = sim.spawn_object("MTNK", "Local", 11, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    let click = |id, action| {
        sim.object_move_click(owner, id, far, action, false, &rules)
            .unwrap()
    };
    let Some(TargetKind::Cell(x, _)) = click(gi, MoveAction::Move) else {
        panic!("the GI's click must search its own zone");
    };
    assert!(x < 8, "{x}");
    assert_eq!(
        click(harrier, MoveAction::Move),
        Some(TargetKind::Entity(far))
    );
    assert_eq!(click(harrier, MoveAction::NoMove), None);
    assert_eq!(click(harrier, MoveAction::None), None, "no Aircraft tail");
}

/// NoMove's own shroud test (`0x004D77BD..0x004D77EA`): over an object whose
/// coordinate is shrouded, a retail CLEG (MoveToShroud=no) orders nothing,
/// while a retail Grizzly goes on to the Move arm, where the shrouded Cell
/// sends it to FNPC (`0x004D7B01`).
#[test]
fn no_move_over_a_shrouded_object_needs_move_to_shroud() {
    let Some((mut sim, rules, owner, _, legionnaire)) = retail_scene("CLEG") else {
        return;
    };
    let tank = sim.spawn_object("MTNK", "Local", 6, 6, 0, &rules).unwrap();
    let hidden = sim.spawn_object("MTNK", "Local", 12, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    sim.fog = crate::sim::vision::FogState::new(16, 16);
    sim.fog.reveal_cells_for_owner(
        owner,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&cell| cell != (12, 5)),
    );
    let click = |id| {
        sim.object_move_click(owner, id, hidden, MoveAction::NoMove, false, &rules)
            .unwrap()
    };
    assert_eq!(click(legionnaire), None);
    assert!(
        matches!(click(tank), Some(TargetKind::Cell(..))),
        "{:?}",
        click(tank)
    );
}

/// A JumpJet infantryman (Type+D94) is special on the ground: Move keeps an
/// object across the river (`0x004D7AFB`), and NoMove searches near it with
/// zone -1 (`0x004D7A9B`), where the retail GI's NoMove keeps to its own bank.
#[test]
fn a_jumpjet_infantry_no_move_searches_near_the_object_in_any_zone() {
    let Some((mut sim, rules, owner, gi, rocketeer)) = retail_scene("JUMPJET") else {
        return;
    };
    assert!(rules.object("JUMPJET").unwrap().jumpjet);
    let far = sim.spawn_object("MTNK", "Local", 11, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    let click = |id, action| {
        sim.object_move_click(owner, id, far, action, false, &rules)
            .unwrap()
    };
    assert_eq!(
        click(rocketeer, MoveAction::Move),
        Some(TargetKind::Entity(far))
    );
    let Some(TargetKind::Cell(x, _)) = click(rocketeer, MoveAction::NoMove) else {
        panic!("a special NoMove searches near the object");
    };
    assert!(x > 8, "{x}");
    let Some(TargetKind::Cell(x, _)) = click(gi, MoveAction::NoMove) else {
        panic!("the GI's NoMove searches its own zone");
    };
    assert!(x < 8, "{x}");
}

/// An Aircraft orders nothing over an object whose coordinate, raised to its
/// ground, is shrouded (`0x004D7809..0x004D7896`); the retail GI's Move over
/// it goes to FNPC on its own bank, as the shrouded Cell asks (`0x004D7B01`).
#[test]
fn an_aircraft_orders_nothing_over_a_shrouded_object() {
    let Some((mut sim, rules, owner, gi, harrier)) = retail_scene("ORCA") else {
        return;
    };
    let hidden = sim.spawn_object("MTNK", "Local", 12, 5, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    sim.fog = crate::sim::vision::FogState::new(16, 16);
    sim.fog.reveal_cells_for_owner(
        owner,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .filter(|&cell| cell != (12, 5)),
    );
    let click = |id| {
        sim.object_move_click(owner, id, hidden, MoveAction::Move, false, &rules)
            .unwrap()
    };
    assert_eq!(click(harrier), None);
    let Some(TargetKind::Cell(x, _)) = click(gi) else {
        panic!("the GI's click searches its own zone");
    };
    assert!(x < 8, "{x}");
}

/// Outside the playfield (`0x00578460(cell, 1)`) every mover, special or
/// not, searches its own zone near the object's Cell (`0x004D7AF5..0x004D7AF7`
/// to `0x004D7B5E`), so a Harrier that keeps an object inside it gets a Cell
/// here.
#[test]
fn an_object_outside_the_playfield_sends_every_mover_to_a_cell() {
    let Some((mut sim, rules, owner, gi, harrier)) = retail_scene("ORCA") else {
        return;
    };
    let outside = sim.spawn_object("MTNK", "Local", 3, 3, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    assert!(!cell_is_in_playfield_height_aware_in_query(
        (3, 3),
        sim.playfield_bounds,
        sim.resolved_terrain.as_ref(),
        None,
    ));
    for id in [gi, harrier] {
        let Some(TargetKind::Cell(x, y)) = sim
            .object_move_click(owner, id, outside, MoveAction::Move, false, &rules)
            .unwrap()
        else {
            panic!("{id} must search near the object");
        };
        assert!(
            cell_is_in_playfield_height_aware_in_query(
                (i32::from(x), i32::from(y)),
                sim.playfield_bounds,
                sim.resolved_terrain.as_ref(),
                None,
            ),
            "{id}: ({x}, {y})"
        );
    }
}

/// A Move onto an object runs through the frame host as a Move mission toward
/// the object. The retail GI cannot enter the tank's Cell, so Find_Path's
/// code-6 arm (`0x004D3A92..0x004D3E0A`) retargets it to a Cell FNPC finds
/// beside the tank, and the GI walks there.
#[test]
fn a_move_onto_a_tank_walks_the_gi_beside_it() {
    let Some((mut sim, rules, owner, gi, _)) = retail_scene("MTNK") else {
        return;
    };
    let target = sim.spawn_object("MTNK", "Local", 2, 9, 0, &rules).unwrap();
    sim.resolve_type_handles(&rules);
    let distance = |sim: &Simulation| {
        let a = sim.entities().get(gi).unwrap().position;
        let b = sim.entities().get(target).unwrap().position;
        (i32::from(a.rx()) - i32::from(b.rx())).abs()
            + (i32::from(a.ry()) - i32::from(b.ry())).abs()
    };
    let start = distance(&sim);
    sim.queue_command(crate::sim::command::CommandEnvelope::new(
        owner,
        sim.session.tick,
        crate::sim::command::Command::MoveToObject {
            entity_id: gi,
            target_id: target,
        },
    ));
    for _ in 0..60 {
        let due = sim.take_due_commands();
        let grid = sim.path_grid_snapshot();
        let result = sim.advance_tick(&due, Some(&rules), grid.as_deref(), None, 67);
        assert!(result.frame_committed, "{result:?}");
    }
    let tank = sim.entities().get(target).unwrap().position;
    let Some(NavTargetRef::Cell { rx, ry }) = sim.entities().get(gi).unwrap().navigation.nav_com()
    else {
        panic!("Find_Path retargets the GI to a Cell");
    };
    assert!(
        rx.abs_diff(tank.rx()) <= 1
            && ry.abs_diff(tank.ry()) <= 1
            && (rx, ry) != (tank.rx(), tank.ry()),
        "({rx}, {ry})"
    );
    assert!(distance(&sim) < start, "{} -> {}", start, distance(&sim));
}

impl Simulation {
    /// Synthetic incoming terrain/bridge records; movement, input, codec and
    /// scheduled-frame owners remain production. Native flood/path goldens
    /// are deliberately not inferred from this geometry regression.
    pub fn walk_cell_input_test_scene(bridge: bool) -> (Self, RuleSet, u64) {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [E1]\nStrength=100\nSpeed=4\nSight=8\nMovementZone=Infantry\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [Clear]\nFoot=100%\nTrack=100%\nFloat=0%\n\
             [Water]\nFoot=0%\nTrack=0%\nFloat=100%\n",
        ))
        .unwrap();
        Self::walk_cell_input_test_scene_with(rules, bridge)
    }

    /// [`Self::walk_cell_input_test_scene`] with the given rules: their
    /// `[Clear]` and `[Water]` rows price the cells, and their E1 is spawned.
    pub fn walk_cell_input_test_scene_with(rules: RuleSet, bridge: bool) -> (Self, RuleSet, u64) {
        let mut sim = Self::with_seed(0x4de1d0);
        sim.intern_rule_type_ids(&rules);
        let owner = sim.interner.intern("Local");
        sim.houses.insert(
            owner,
            crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
        );
        sim.session.house_order.push(owner);
        sim.session.current_house = Some(owner);
        sim.session.binary_frame = 100;
        // Infantry-only contenders need Long Game: Short Game correctly
        // defeats both Houses without a building or BaseUnit.
        sim.session.game_options.short_game = false;
        sim.session.map_width = 16;
        sim.session.map_height = 16;
        // Native Unlimbo now publishes sight synchronously. Allocate the map
        // domain before either source can create a viewer plane/cache.
        sim.fog = crate::sim::vision::FogState::new(16, 16);
        sim.playfield_bounds = Some(bounds());
        sim.playfield_size_height = Some(8);
        let clear = rules
            .terrain_rules
            .semantics_by_name("Clear")
            .unwrap()
            .speed_costs;
        let water = rules
            .terrain_rules
            .semantics_by_name("Water")
            .unwrap()
            .speed_costs;
        let cells = (0..16)
            .flat_map(|y| {
                (0..16).map(move |x| {
                    let mut cell = terrain_cell(x, y);
                    let river = (8..=10).contains(&x);
                    cell.level = if bridge && !river { 4 } else { 0 };
                    cell.speed_costs = if river { water } else { clear };
                    cell.base_speed_costs = cell.speed_costs;
                    if river {
                        cell.is_water = true;
                        cell.terrain_class = TerrainClass::Water;
                        cell.base_terrain_class = TerrainClass::Water;
                        cell.land_type = 2;
                        cell.base_land_type = 2;
                        cell.zone_type = crate::map::resolved_terrain::zone_class::WATER;
                        cell.ground_walk_blocked = true;
                        cell.base_ground_walk_blocked = true;
                    }
                    if bridge && river && y == 6 {
                        // 583209's direction bit preserves the lane offset for
                        // the x-directed (7,6)..(11,6) record. The traversable
                        // lane carries 0x200 throughout: 4D9D9A requires it on
                        // entry and 4D9E08 for deck→deck at raw level 0.
                        // It distinguishes stamped lanes, not span endpoints.
                        cell.bridge_facts.raw_flags = 0x100 | 0x800 | 0x200;
                        cell.has_bridge_deck = cell.bridge_facts.has_structural_bridge();
                        cell.bridge_walkable = true;
                        cell.bridge_transition = cell.bridge_facts.raw_flags & 0x200 != 0;
                        cell.bridge_deck_level = 4;
                    }
                    cell
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(16, 16, cells));
        assert!(sim.rebuild_dynamic_navigation(&rules));
        if bridge {
            let records = [BridgeEndpointRecord {
                endpoint_a: (7, 6),
                endpoint_b: (11, 6),
                active: true,
                bridge_kind: BridgeRecordKind::High,
            }];
            sim.zone_grid = Some(ZoneGrid::build_with_native_bridge_geometry(
                sim.path_grid().unwrap(),
                sim.resolved_terrain.as_ref().unwrap(),
                &records,
                16,
                16,
                Some((8, 8)),
            ));
        }
        let start = if bridge { (7, 6) } else { (5, 5) };
        let actor = sim
            .spawn_object("E1", "Local", start.0, start.1, 0, &rules)
            .unwrap();
        // Keep two live Long Game contenders while the real frame host runs.
        let enemy = sim.interner.intern("Enemy");
        sim.houses.insert(
            enemy,
            crate::sim::house_state::HouseState::new(enemy, 1, None, false, 0, 10),
        );
        sim.session.house_order.push(enemy);
        sim.spawn_object("E1", "Enemy", 12, 11, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        sim.fog.reveal_all_for_owner(owner);
        assert!(sim.entities().get(actor).unwrap().lifecycle.cell_marked());
        (sim, rules, actor)
    }

    pub fn close_walk_input_test_bridge(&mut self, rules: &RuleSet) {
        let terrain = self.resolved_terrain.as_mut().unwrap();
        for x in 8..=10 {
            let cell = terrain
                .cells
                .iter_mut()
                .find(|c| c.rx == x && c.ry == 6)
                .unwrap();
            cell.bridge_facts.raw_flags = 0;
            cell.has_bridge_deck = false;
            cell.bridge_walkable = false;
            cell.bridge_transition = false;
        }
        self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        assert!(self.rebuild_dynamic_navigation(rules));
    }
}

/// Techno What_Action on a Cell's unforced Attack rung over retail data: the
/// Grizzly's 105mm warhead AP has `Wall=yes` and the GI's SA does not, and
/// GAWALL keeps the ObjectType constructor's LegalTarget. Only a wall whose
/// owner is not allied with the asker's House admits it.
#[test]
fn a_wall_warhead_reaches_the_attack_rung_over_an_enemy_wall() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let registry =
        OverlayTypeRegistry::from_ini(&fixture.processed_rules, Some(fixture.fixed_art.as_ref()));
    let wall = registry.id_for_name("GAWALL").expect("retail GAWALL");
    assert!(
        registry
            .flags(wall)
            .is_some_and(|flags| flags.legal_target && flags.wall)
    );
    let (mut sim, rules, gi) = Simulation::walk_cell_input_test_scene_with(fixture.rules, false);
    let owner = sim.interner.get("Local").unwrap();
    let tank = sim
        .spawn_object("MTNK", "Local", 6, 4, 0, &rules)
        .expect("retail MTNK spawns");
    sim.resolve_type_handles(&rules);
    let enemy = sim.interner.intern("Russians");
    let (width, height) = {
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        (terrain.width(), terrain.height())
    };
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(width, height);
    grid.place_owned_wall(8, 4, wall, 0, enemy);
    grid.place_owned_wall(6, 6, wall, 0, owner);
    sim.overlay_grid = Some(grid);
    let admits = |id, cell| sim.cell_attack_rung_admits(id, cell, &rules, Some(&registry));
    assert!(admits(tank, (8, 4)));
    assert!(!admits(gi, (8, 4)), "SA has no Wall=");
    assert!(!admits(tank, (6, 6)), "an own wall");
    assert!(!admits(tank, (10, 4)), "no overlay and no cliff");
}
