//! Original Foot4DC060/7049C0 comparisons through the production line builder.
//! The JSON contains native execution, never expected pixels made by Rust.

use super::*;
use crate::sim::components::DriveCoord;
use serde_json::Value;

fn native() -> &'static Value {
    static CORPUS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/action_lines.json",
        ))
        .unwrap()
    })
}

fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| value[i].as_i64().unwrap() as i32)
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn palette_bytes() -> Vec<u8> {
    hex_bytes(
        native()["physical_inputs"]["palettes"]["PALETTE.PAL"]["hex"]
            .as_str()
            .unwrap(),
    )
}

fn baseline_rows() -> impl Iterator<Item = &'static Value> {
    native()["cases"].as_array().unwrap().iter().filter(|row| {
        matches!(
            row["input"]["family"].as_str(),
            Some("geometry" | "timer" | "original_control")
        )
    })
}

fn drawing_rows() -> impl Iterator<Item = &'static Value> {
    native()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(
            native()["tactical_gate_cases"]
                .as_array()
                .unwrap()
                .iter()
                // Planning input/lifecycle is a separate required chain. These
                // comparisons cover every executed ordinary (planning-off) gate.
                .filter(|row| row["input"]["planning"] == false),
        )
        .chain(native()["batch_cases"].as_array().unwrap().iter())
}

fn attack_rows() -> impl Iterator<Item = &'static Value> {
    native()["attack_cases"].as_array().unwrap().iter()
}

fn viewport(input: &Value, zoom: f32) -> TacticalViewport {
    let mut camera = point(&input["camera"]);
    camera[1] += 15; // Shared VERA world projection bias, not a line adjustment.
    TacticalViewport {
        camera,
        clip: input.get("clip").map_or([0, 0, 160, 120], point),
        zoom,
    }
}

fn fixture(row: &Value) -> (Simulation, TargetLineState, TacticalViewport) {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    let input = &row["input"];
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let mut cells: Vec<_> = (0..32)
        .flat_map(|y| (0..32).map(move |x| ResolvedTerrainCell::clear_for_test(x, y)))
        .collect();
    let actors: Vec<_> = input["actors"]
        .as_array()
        .map_or_else(|| vec![input], |actors| actors.iter().collect());
    let order: Vec<_> = input["techno_order"].as_array().map_or_else(
        || (0..actors.len()).collect(),
        |order| order.iter().map(|v| v.as_u64().unwrap() as usize).collect(),
    );
    for (construction_index, actor_index) in order.into_iter().enumerate() {
        let actor_input = actors[actor_index];
        let mut actor =
            GameEntity::test_default(construction_index as u64 + 1, "MTNK", "Americans", 20, 20);
        actor.owner = owner;
        actor.type_ref = sim.interner.intern("MTNK");
        actor.selected = actor_input["selected"].as_bool().unwrap_or(true);
        actor.on_bridge = actor_input["on_bridge"].as_bool().unwrap_or(false);
        let [x, y, z] = point(&actor_input["source"]);
        crate::sim::movement::ground_pose::put_location(
            &mut actor.position,
            DriveCoord { x, y, z },
        );
        let [tx, ty] = point(&actor_input["target_cell"]);
        actor.navigation.nav_com = (!actor_input["nav_null"].as_bool().unwrap_or(false))
            .then_some(NavTargetRef::cell(tx as u16, ty as u16));
        let mut target_cells = vec![[tx, ty]];
        if let Some(queue) = actor_input["queue_cells"].as_array() {
            for target in queue {
                let [x, y] = point(target);
                actor
                    .navigation
                    .nav_queue
                    .push(NavTargetRef::cell(x as u16, y as u16));
                target_cells.push([x, y]);
            }
        }
        for target in cells
            .iter_mut()
            .filter(|c| target_cells.contains(&[i32::from(c.rx), i32::from(c.ry)]))
        {
            target.level = actor_input["level"].as_u64().unwrap_or(0) as u8;
            target.slope_type = actor_input["slope"].as_u64().unwrap_or(0) as u8;
            target.has_bridge_deck = actor_input["flags"].as_u64().unwrap_or(0) & 0x100 != 0;
        }
        sim.entities_mut().insert(actor);
    }
    sim.session.game_mode_nonzero = input["mode"].as_u64().unwrap_or(1) != 0;
    sim.session.current_house = input["local"].as_bool().unwrap_or(true).then_some(owner);
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 0);
    if let Some(flags) = input["campaign_flags"].as_array() {
        house.is_human = flags[0].as_bool().unwrap();
        house.player_control = flags[1].as_bool().unwrap();
    }
    sim.houses.insert(owner, house);
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(100) as u32;
    // The timer must consume the binary frame, independently of VERA's tick.
    sim.session.tick = 999_999;
    sim.resolved_terrain =
        Some(ResolvedTerrainGrid::from_cells(32, 32, cells).test_mark_decks_structural());
    {
        let size = input
            .get("map_size")
            .unwrap_or(&native()["prepared_common"]["map_size"]);
        let [width, height] = point(size);
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: width as u32,
            height: height as u32,
            local_left: 0,
            local_top: 0,
            local_width: width as u32,
            local_height: height as u32,
        });
    }
    let mut state = TargetLineState::default();
    state.start_timer(input["timer_start"].as_u64().unwrap_or(100) as u32);
    state.set_unit_action_lines_enabled(input["option"].as_bool().unwrap_or(true));
    (sim, state, viewport(input, 1.))
}

/// Prepare the original raster inputs through existing rules/state owners.
/// These supplied poses supplement the separately executed retail constructor,
/// command and live-getter histories; they do not stand in for those histories.
fn drawing_fixture(
    row: &Value,
) -> (
    Simulation,
    Option<RuleSet>,
    TargetLineState,
    TacticalViewport,
) {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::sim::combat::{AttackTarget, veterancy};
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::track_process::TrackFamily;
    use crate::util::native_x87::NativeF64Bits;

    let (mut sim, state, view) = fixture(row);
    let input = &row["input"];
    if input["family"] != "attack" {
        return (sim, None, state, view);
    }
    let speed = &input["target_speed_inputs"];
    let weapon = &input["weapon_inputs"];
    let ini = IniFile::from_str(&format!(
        "[AudioVisual]\nGravity={}\n[General]\nVeteranSpeed={}\n\
         [Countries]\n0=Americans\n[Americans]\nSpeedUnitsMult={}\n\
         [VehicleTypes]\n0=MTNK\n[MTNK]\nImage=GTNK\nSpeed={}\nTurret=yes\n\
         Primary={}\nVeteranAbilities={}\nEliteAbilities={}\n\
         [ACTION]\nProjectile=ACTIONP\nSpeed={}\nRange={}\n\
         [ACTIONP]\nROT={}\nFloater={}\n",
        weapon["gravity"].as_f64().unwrap(),
        speed["veteran"].as_f64().unwrap(),
        speed["house"].as_f64().unwrap(),
        speed["speed_percent"].as_i64().unwrap(),
        if weapon["present"].as_bool().unwrap() {
            "ACTION"
        } else {
            ""
        },
        if speed["faster"].as_bool().unwrap() {
            "FASTER"
        } else {
            ""
        },
        if speed["elite_faster"].as_bool().unwrap() {
            "FASTER"
        } else {
            ""
        },
        weapon["speed_percent"].as_i64().unwrap(),
        weapon["range_cells"].as_i64().unwrap(),
        weapon["rot"].as_i64().unwrap(),
        if weapon["floater"].as_bool().unwrap() {
            "yes"
        } else {
            "no"
        },
    ));
    let art = IniFile::from_str(&format!(
        "[GTNK]\nTurretOffset={}\nPrimaryFireFLH=150,0,100\n",
        input["turret_offset"].as_i64().unwrap(),
    ));
    // ReadTypeData runs before AudioVisual in each Process pass. The first
    // weapon postpass therefore sees constructor Gravity3; the later mode
    // and map passes see the loaded Gravity6 and produce the retained95.
    // Native chronology: rules_oracle/weapon_speed_order and the physical
    // action_lines_attack_prerequisites packet. Do not flatten these passes.
    let mut layers = RulesLayerStack::new(ini);
    layers.push(RulesLayerKind::GameMode, IniFile::empty());
    layers.push(RulesLayerKind::Scenario, IniFile::empty());
    let mut rules =
        RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    if weapon["present"].as_bool().unwrap() {
        assert_eq!(
            serde_json::json!(rules.weapon("ACTION").unwrap().speed),
            weapon["speed"]
        );
    }
    assert_eq!(
        crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
            rules.object("MTNK").unwrap().speed
        ),
        speed["raw"].as_i64().unwrap() as i32,
        "prepared raw speed is bound to the original type reader's authored input"
    );
    assert_eq!(speed["flag_owner"], -1, "retail CTF is dormant");
    for house in sim.houses.values_mut() {
        house.project_country_mults(&rules, &sim.interner);
    }
    let frame = sim.session.binary_frame;
    let actor = sim.entities_mut().get_mut(1).unwrap();
    actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    actor
        .body_facing
        .snap(input["body_facing"].as_u64().unwrap() as u16, frame);
    let mut turret = crate::sim::movement::FacingClass::new(0, 0);
    turret.snap(input["turret_facing"].as_u64().unwrap() as u16, frame);
    actor.barrel_facing = Some(turret);
    actor.attack_target = input["target_present"]
        .as_bool()
        .unwrap()
        .then(|| AttackTarget::new(2));
    let owner = actor.owner;
    let type_ref = actor.type_ref;
    let mut target = GameEntity::test_default(2, "MTNK", "Americans", 20, 20);
    target.owner = owner;
    target.type_ref = type_ref;
    target.selected = false;
    let [x, y, z] = point(&input["target_xyz"]);
    crate::sim::movement::ground_pose::put_location(&mut target.position, DriveCoord { x, y, z });
    let mut locomotor = LocomotorState::for_test_kind(LocomotorKind::Drive);
    if input["target_moving"].as_bool().unwrap() {
        locomotor.ensure_installed_track_state();
        assert!(
            locomotor.store_track_destination(TrackFamily::Drive, Some(DriveCoord { x, y, z }))
        );
    }
    target.locomotor = Some(locomotor);
    target
        .body_facing
        .snap(input["target_facing"].as_u64().unwrap() as u16, frame);
    match speed["rank"].as_f64().unwrap() {
        0. => {}
        1. => veterancy::set_veteran(&mut target),
        2. => veterancy::set_elite(&mut target),
        other => panic!("unsupported prepared rank {other}"),
    }
    assert!(
        target
            .foot_speed
            .accept_speed_crate(NativeF64Bits::from_bits(
                speed["crate"].as_f64().unwrap().to_bits(),
            ))
    );
    target
        .foot_speed
        .set_speed_fraction_native_bits(speed["applied"].as_f64().unwrap().to_bits());
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(&target),
        input["target_moving"].as_bool()
    );
    sim.entities_mut().insert(target);
    (sim, Some(rules), state, view)
}

fn built(row: &Value, zoom: f32) -> (Vec<SpriteInstance>, TacticalViewport) {
    if let Some(primitives) = row["input"]["primitives"].as_array() {
        let view = viewport(&row["input"], zoom);
        let project = |value: &Value| {
            let [x, y, z] = point(value);
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
            [
                x as i32 - view.camera[0],
                y as i32 - view.camera[1] + view.clip[1],
            ]
        };
        let order = row["input"]
            .get("primitive_order")
            .unwrap_or(&row["primitive_order"])
            .as_array()
            .unwrap();
        let lines: Vec<_> = order
            .iter()
            .map(|index| {
                let input = &primitives[index.as_u64().unwrap() as usize];
                ProjectedActionLine {
                    start: project(&input["source"]),
                    end: project(&input["target"]),
                    kind: match input["color_index"].as_u64().unwrap() {
                        3 => SelectedLineKind::Move,
                        8 => SelectedLineKind::Attack,
                        _ => panic!("unsupported primitive color"),
                    },
                }
            })
            .collect();
        let palette = crate::assets::pal_file::Palette::from_bytes(&palette_bytes()).unwrap();
        // Original7049C0 primitive color/order, not an Attack coordinate claim.
        return (
            build_selected_action_line_instances(
                &lines,
                [
                    action_line_color(&palette, 3),
                    action_line_color(&palette, 8),
                ],
                view,
            ),
            view,
        );
    }
    let (sim, rules, state, mut view) = drawing_fixture(row);
    view.zoom = zoom;
    let instances = build_target_line_instances(
        &state,
        Some(&sim),
        rules.as_ref(),
        Some(&palette_bytes()),
        view,
    );
    (instances, view)
}

/// Prepared native input for the existing shared CPU/GPU workload harness.
/// The harness disperses raw source locations; it never generates goldens.
pub(super) fn workload_fixture(
    kind: SelectedLineKind,
) -> (
    Simulation,
    Option<RuleSet>,
    TargetLineState,
    TacticalViewport,
    Vec<u8>,
) {
    let row = match kind {
        SelectedLineKind::Move => baseline_rows().find(|row| {
            row["input"]["source"] == serde_json::json!([2688, 5248, 0])
                && row["input"]["target_cell"] == serde_json::json!([14, 20])
                && row["input"]["timer_start"] == 100
                && row["input"]["frame"] == 100
        }),
        SelectedLineKind::Attack => attack_rows().find(|row| {
            row["input"]["target_present"] == true
                && row["input"]["target_moving"] == true
                && row["input"]["turret_offset"] == 0
                && row["input"]["frame"] == 100
        }),
    }
    .unwrap();
    let (mut sim, rules, state, view) = drawing_fixture(row);
    if kind == SelectedLineKind::Attack {
        // Keep the one unselected moving target outside the20k source IDs.
        // This is synthetic load setup, before measurement; no gameplay tick
        // or identity-lifecycle equivalence is claimed for it.
        let mut target = sim.entities_mut().remove(2).unwrap();
        target.stable_id = 20_001;
        sim.entities_mut().insert(target);
        sim.entities_mut().get_mut(1).unwrap().attack_target =
            Some(crate::sim::combat::AttackTarget::new(20_001));
    }
    (sim, rules, state, view, palette_bytes())
}

fn words(row: &Value) -> Vec<u16> {
    let mut result = vec![0x39E7; 160 * 120];
    for pixel in row["pixels"].as_array().unwrap() {
        let [x, y, word] = point(pixel);
        result[(y * 160 + x) as usize] = word as u16;
    }
    result
}

fn has_clip_rounding_residual(row: &Value) -> bool {
    // Saved original clip endpoint [89,119] versus shared f64 [90,119].
    // This is the existing surface_line chop-rounding residual, not a new
    // tolerance for coordinates, colors, timers, uncut lines or other rows.
    matches!(
        row["input"]["name"].as_str(),
        Some(
            "height_2_bridge_1"
                | "slope_5_bridge_1"
                | "slope_6_bridge_1"
                | "slope_7_bridge_1"
                | "slope_8_bridge_1"
        )
    )
}

fn assert_native_pixels<T: PartialEq>(
    actual: &[T],
    expected: &[T],
    size: [usize; 2],
    radius: usize,
    row: &Value,
) {
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual.len(), size[0] * size[1]);
    for (index, (a, b)) in actual.iter().zip(expected).enumerate() {
        if a == b {
            continue;
        }
        assert!(
            radius > 0 && has_clip_rounding_residual(row),
            "{}: differing pixel {index}",
            row["input"]
        );
        let x = index % size[0];
        let y = index / size[0];
        let nearby = |pixels: &[T], color: &T| {
            (y.saturating_sub(radius)..=(y + radius).min(size[1] - 1)).any(|ny| {
                (x.saturating_sub(radius)..=(x + radius).min(size[0] - 1))
                    .any(|nx| &pixels[ny * size[0] + nx] == color)
            })
        };
        assert!(
            nearby(expected, a) && nearby(actual, b),
            "{}: clip residual exceeds {radius} pixels at {index}",
            row["input"]
        );
    }
}

fn raster(instances: &[SpriteInstance], view: TacticalViewport) -> Vec<u16> {
    let mut result = vec![0x39E7; 160 * 120];
    for instance in instances {
        let [r, g, b] = instance.tint.map(|c| (c * 255.).round() as u16);
        let word = (r >> 3) << 11 | (g >> 2) << 5 | (b >> 3);
        let x = instance.position[0] as i32 - view.camera[0];
        let y = instance.position[1] as i32 - view.camera[1];
        for dy in 0..instance.size[1] as i32 {
            for dx in 0..instance.size[0] as i32 {
                assert!((0..160).contains(&(x + dx)) && (0..120).contains(&(y + dy)));
                result[((y + dy) * 160 + x + dx) as usize] = word;
            }
        }
    }
    result
}

#[test]
fn original_foot_move_line_pixels_through_production_builder() {
    let mut count = 0;
    for row in drawing_rows() {
        let (instances, view) = built(row, 1.);
        let actual = raster(&instances, view);
        let expected = words(row);
        assert_native_pixels(
            &actual,
            &expected,
            [160, 120],
            usize::from(has_clip_rounding_residual(row)),
            row,
        );
        count += 1;
    }
    assert_eq!(count, 120);
}

#[test]
fn original_unit_attack_line_pixels_and_read_only_coordinate_queries() {
    let rows: Vec<_> = attack_rows().collect();
    assert!(rows.len() >= 9);
    for row in rows {
        let (sim, rules, state, view) = drawing_fixture(row);
        let hash = sim.state_hash();
        let rng = (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
        );
        let query = NativeCellQuery::canonical(sim.resolved_terrain.as_ref().unwrap());
        let dummy = query.dummy().snapshot();
        let instances = build_target_line_instances(
            &state,
            Some(&sim),
            rules.as_ref(),
            Some(&palette_bytes()),
            view,
        );
        assert_native_pixels(&raster(&instances, view), &words(row), [160, 120], 0, row);
        assert_eq!(sim.state_hash(), hash, "{}", row["input"]);
        assert_eq!(
            (
                sim.main_rng.logical_state(),
                sim.scenario_rng.logical_state()
            ),
            rng
        );
        assert_eq!(query.dummy().snapshot(), dummy);
    }
}

#[test]
fn retail_tank_attack_inputs_match_original_readers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let original: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/action_lines_attack_prerequisites.json",
    ))
    .unwrap();
    let input = &original["type_inputs"];
    let rules = &retail.rules;
    let object = rules.object("MTNK").unwrap();
    let art = crate::sim::combat::fire_coord::firer_art(rules, object).unwrap();
    let weapon = rules.weapon(object.primary.as_deref().unwrap()).unwrap();
    let projectile = rules
        .projectile(weapon.projectile.as_deref().unwrap())
        .unwrap();
    let warhead = rules.warhead(weapon.warhead.as_deref().unwrap()).unwrap();
    assert_eq!(object.image, input["resolved_image"].as_str().unwrap());
    assert_eq!(
        object.primary.as_deref().unwrap(),
        input["primary_name"].as_str().unwrap()
    );
    assert_eq!(serde_json::json!(art.turret_offset), input["turret_offset"]);
    assert_eq!(
        [
            art.primary_fire_flh.forward,
            art.primary_fire_flh.lateral,
            art.primary_fire_flh.height,
        ],
        point::<3>(&input["primary_flh"])
    );
    assert_eq!(
        serde_json::json!(crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
            object.speed
        )),
        input["speed"],
    );
    assert_eq!(
        rules.general.veteran_speed.to_bits(),
        u64::from_le_bytes(
            hex_bytes(input["veteran_speed_bits"].as_str().unwrap())
                .try_into()
                .unwrap(),
        )
    );
    assert_eq!(
        rules.country_speed_mults("Americans")[1].bits(),
        u32::from_le_bytes(
            hex_bytes(input["country_speed_bits"].as_str().unwrap())
                .try_into()
                .unwrap(),
        )
    );
    assert_eq!(serde_json::json!(weapon.speed), input["weapon_speed"]);
    assert_eq!(serde_json::json!(projectile.rot), input["projectile_rot"]);
    assert_eq!(
        serde_json::json!(u8::from(projectile.floater)),
        input["projectile_floater"]
    );
    assert_eq!(serde_json::json!(rules.general.gravity), input["gravity"]);
    assert_eq!(
        weapon.warhead.as_deref().unwrap(),
        input["warhead"].as_str().unwrap()
    );
    assert_eq!(serde_json::json!(warhead.rocker), input["rocker"]);
    assert_eq!(
        serde_json::json!(warhead.direct_rocker),
        input["direct_rocker"]
    );
}

#[test]
fn original_opaque_overlap_order_across_batch_cutover() {
    let rows = native()["compositing_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let (instances, view) = built(row, 1.);
        assert_native_pixels(&raster(&instances, view), &words(row), [160, 120], 0, row);
    }
}

#[test]
fn original_source_and_target_projection_before_surface_clipping() {
    for row in native()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(attack_rows())
    {
        let Some(clip) = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["name"] == "clip_line")
        else {
            continue;
        };
        let (sim, rules, _, view) = drawing_fixture(row);
        let cells = sim.resolved_terrain.as_ref().map(NativeCellQuery::isolated);
        let line = selected_action_line_for_entity(
            sim.entities().get(1).unwrap(),
            &sim,
            rules.as_ref(),
            cells.as_ref(),
        )
        .unwrap();
        let project = |p: ScreenPoint| {
            [
                p.x as i32 - view.camera[0],
                p.y as i32 - view.camera[1] + view.clip[1],
            ]
        };
        // Original7049C0's first7BC2B0 input is observed before rejection or
        // clipping. Thus an empty high-ground surface cannot hide a wrong
        // source Location, target+48 or bridge/slope projection.
        assert_eq!(
            [project(line.start), project(line.end)],
            [
                point::<2>(&clip["points_before"][0]),
                point::<2>(&clip["points_before"][1])
            ],
            "{}",
            row["input"]
        );
    }
}

#[test]
fn original_solid_leaf_and_endpoint_intersection() {
    use crate::render::surface_line::{clip_line, solid_line};
    for row in native()["leaf_cases"].as_array().unwrap() {
        let input = &row["input"];
        let mut from = point(&input["from_point"]);
        let mut to = point(&input["to_point"]);
        let clip = crate::util::rect::clip_rect([0, 0, 160, 120], point(&input["clip"]));
        // XSurface7BA610 translates relative endpoints by the requested
        // clipping origin before7BC2B0. The shared leaf accepts absolute,
        // already clipped pixels; ordinary7049C0 calls it with origin zero.
        for point in [&mut from, &mut to] {
            point[0] += input["clip"][0].as_i64().unwrap() as i32;
            point[1] += input["clip"][1].as_i64().unwrap() as i32;
        }
        let mut actual = vec![0x39E7; 160 * 120];
        if clip_line(&mut from, &mut to, clip) {
            solid_line(from, to, |[x, y, width, height]| {
                for y in y..y + height {
                    for x in x..x + width {
                        actual[(y * 160 + x) as usize] = 0x0540;
                    }
                }
            });
        }
        assert!(actual == words(row), "{input}");
    }
    for row in native()["clip_rect_cases"].as_array().unwrap() {
        // Original optional offset pointers are observed separately; no current
        // Rust consumer asks for them. They cannot change the output rectangle.
        assert_eq!(
            crate::util::rect::clip_rect(
                point(&row["input"]["rectangle"]),
                point(&row["input"]["clip"])
            ),
            point::<4>(&row["rectangle"]),
            "{}",
            row["input"]
        );
    }
}

#[test]
fn original_timer_default_and_successful_load_reanchor() {
    for row in native()["timer_prerequisites"].as_array().unwrap() {
        let input = &row["input"];
        let mut state = TargetLineState::default();
        let frame = input["frame"].as_u64().unwrap() as u32;
        if let Some(before) = input.get("timer_before") {
            state.timer = CdTimer::from_raw(
                before[0].as_u64().unwrap() as i32,
                before[2].as_u64().unwrap() as i32,
            );
            state.reanchor_after_load(frame);
            assert_eq!(
                state.timer.start_frame(),
                row["native_timer"][0].as_u64().unwrap() as i32
            );
        }
        assert_eq!(
            state.timer.duration(),
            row["native_timer"][2].as_u64().unwrap() as i32,
            "{input}"
        );
        assert_eq!(
            state.is_selected_action_active(frame),
            row["pixel_count"].as_u64().unwrap() > 0,
            "{input}"
        );
    }
}

#[test]
fn physical_palette_matches_original_convert_middle_row() {
    let palette = crate::assets::pal_file::Palette::from_bytes(&palette_bytes()).unwrap();
    let expected = hex_bytes(
        native()["native_convert"]["middle_row_hex"]
            .as_str()
            .unwrap(),
    );
    let convert = crate::render::palette_light::PaletteLight::plain(53, 1000);
    let actual: Vec<_> = palette
        .colors
        .iter()
        .enumerate()
        .flat_map(|(index, color)| {
            convert
                .rgb565([color.r, color.g, color.b], index as u8, 127)
                .to_le_bytes()
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn original_action_timer_wrap_sentinel_and_positive_remainder() {
    let mut count = 0;
    for row in baseline_rows().filter(|r| r["input"]["family"] == "timer") {
        let input = &row["input"];
        let mut state = TargetLineState::default();
        state.start_timer(input["timer_start"].as_u64().unwrap() as u32);
        let active = state.is_selected_action_active(input["frame"].as_u64().unwrap() as u32);
        assert_eq!(active, row["pixel_count"].as_u64().unwrap() != 0, "{input}");
        count += 1;
    }
    assert_eq!(count, 25);
}

#[test]
fn move_line_query_leaves_the_simulation_dummy_unchanged() {
    let (mut sim, state, view) = fixture(baseline_rows().next().unwrap());
    sim.entities_mut().get_mut(1).unwrap().navigation.nav_com =
        Some(NavTargetRef::cell(u16::MAX, u16::MAX));
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let query = NativeCellQuery::canonical(terrain);
    let before = query.dummy().snapshot().coord;
    let _ = build_target_line_instances(&state, Some(&sim), None, Some(&palette_bytes()), view);
    assert_eq!(query.dummy().snapshot().coord, before);
}

#[test]
#[ignore = "requires GPU; production action-line pixels, zoom and unchanged depth"]
fn production_action_line_gpu_matches_original_surface() {
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
    use wgpu::util::DeviceExt;
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let white = batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, None);
        for zoom in [0.75, 1., 1.5, 2.] {
            let size = [(160. * zoom) as u32, (120. * zoom) as u32];
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            for row in drawing_rows()
                .chain(attack_rows())
                .chain(native()["compositing_cases"].as_array().unwrap().iter())
            {
                let (mut instances, view) = built(row, zoom);
                batch.write_camera(
                    &gpu.queue,
                    crate::render::batch::CameraUniform {
                        camera_pos: view.camera.map(|v| v as f32),
                        zoom,
                        ..camera(size)
                    },
                );
                for instance in &mut instances {
                    instance.depth = 0.9;
                }
                gpu.queue.write_texture(
                    color.as_image_copy(),
                    &encoded(0x39E7, format).repeat((size[0] * size[1]) as usize),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    color.size(),
                );
                let buffer = (!instances.is_empty()).then(|| {
                    gpu.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Native action-line rectangles"),
                            contents: bytemuck::cast_slice(&instances),
                            usage: wgpu::BufferUsages::VERTEX,
                        })
                });
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(&mut encoder, &cv, &dv, 12345, wgpu::LoadOp::Load);
                if let Some(buffer) = &buffer {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Production action-line comparison"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &cv,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &dv,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    batch.draw_with_buffer_ui_passthrough(
                        &mut pass,
                        &white,
                        buffer,
                        instances.len() as u32,
                    );
                }
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                // Five saved clip controls retain the documented1px CPU
                // rounding residual. For those only, the earlier CPU test
                // bounds the native difference, and this GPU check verifies
                // exact nearest presentation of that CPU raster. It is not
                // an exact native pixel claim for those controls. All other
                //other inputs take their expected pixels directly from native.
                let native_words = if has_clip_rounding_residual(row) {
                    let (logical, logical_view) = built(row, 1.);
                    raster(&logical, logical_view)
                } else {
                    words(row)
                };
                let expected: Vec<_> = (0..size[1])
                    .flat_map(|y| {
                        let native_words = &native_words;
                        (0..size[0]).flat_map(move |x| {
                            let sx = ((x as f32 + 0.5) / zoom).floor() as usize;
                            let sy = ((y as f32 + 0.5) / zoom).floor() as usize;
                            encoded(native_words[sy * 160 + sx], format)
                        })
                    })
                    .collect();
                let actual: Vec<[u8; 4]> = output[0]
                    .chunks_exact(4)
                    .map(|p| p.try_into().unwrap())
                    .collect();
                let expected: Vec<[u8; 4]> = expected
                    .chunks_exact(4)
                    .map(|p| p.try_into().unwrap())
                    .collect();
                assert_native_pixels(&actual, &expected, size.map(|n| n as usize), 0, row);
                assert!(
                    output[1]
                        .chunks_exact(4)
                        .all(
                            |bytes| crate::render::native_z::stored_z(f32::from_le_bytes(
                                bytes.try_into().unwrap()
                            )) == 12345
                        )
                );
            }
        }
    }
}
