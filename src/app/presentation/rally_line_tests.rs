use super::*;
use crate::app::presentation::frame::PresentationFrame;
use crate::sim::game_entity::GameEntity;
use serde_json::Value;

pub(super) fn rally_native() -> &'static Value {
    static NATIVE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    NATIVE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/procedural_drawing_oracle/rally.json",
        ))
        .unwrap()
    })
}

fn producer_rows() -> impl Iterator<Item = &'static Value> {
    let native = rally_native();
    native["producer_cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native["multi_producer_cases"].as_array().unwrap().iter())
}

fn selected_for_row(row: &Value) -> Vec<u64> {
    row["input"]["selection_order"].as_array().map_or_else(
        || vec![10],
        |order| order.iter().map(|id| 10 + id.as_u64().unwrap()).collect(),
    )
}

pub(super) fn alpha_fixture([x, y]: [i32; 2], mode: &str) -> u8 {
    match mode {
        "clear" => 127,
        "black" => 0,
        _ => [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
    }
}

fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| value[i].as_i64().unwrap() as i32)
}

/// The house colour the native rally rows store, RGB 248,40,8.
const NATIVE_RALLY_WORD: u16 = 0xF941;

/// Store each pass's strokes as the GPU does, keeping or skipping each pixel
/// by its A gate against `a` (`surface_strokes.wgsl`).
fn composite(
    canvas: &mut [u16],
    strokes: &[SurfaceStroke],
    view: TacticalViewport,
    a: &impl Fn([i32; 2]) -> u8,
) {
    for stroke in strokes {
        stroke.for_each_rect(|[x, y, width, height]| {
            let [x, y] = [x - view.camera[0], y - view.camera[1]];
            for y in y..y + height {
                for x in x..x + width {
                    let zero = a([x, y]) == 0;
                    let stored = match stroke.gate() {
                        StoreGate::All => true,
                        StoreGate::ANonzero => !zero,
                        StoreGate::AZero => zero,
                    };
                    if stored {
                        canvas[(y * 160 + x) as usize] = stroke.word();
                    }
                }
            }
        });
    }
}

pub(super) fn assert_native_passes(
    actual: &[Vec<SurfaceStroke>; 2],
    row: &Value,
    view: TacticalViewport,
    a: impl Fn([i32; 2]) -> u8,
) {
    let mut canvas = vec![0x39E7; 160 * 120];
    for (index, pass) in actual.iter().enumerate() {
        composite(&mut canvas, pass, view, &a);
        let pixels: Vec<_> = canvas
            .iter()
            .enumerate()
            .filter(|(_, c)| **c != 0x39E7)
            .map(|(i, c)| [i as i32 % 160, i as i32 / 160, i32::from(*c)])
            .collect();
        let expected: Vec<[i32; 3]> = row["passes"][index]["pixels"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert!(
            pixels == expected,
            "native producer {} pass {index}: actual/expected lengths {}/{}, first differing stores {:?}",
            row["input"],
            pixels.len(),
            expected.len(),
            pixels
                .iter()
                .zip(&expected)
                .find(|(actual, expected)| actual != expected)
        );
    }
}

#[test]
fn native_patterned_leaf_all_octants_ties_endpoints_and_phases() {
    for row in rally_native()["leaf_cases"].as_array().unwrap() {
        let input = &row["input"];
        let mut pixels = Vec::new();
        crate::render::surface_line::patterned_line(
            point(&input["from_point"]),
            point(&input["to_point"]),
            &RALLY_PATTERN,
            input["phase"].as_i64().unwrap() as i32,
            |p| {
                if usize::from(alpha_fixture(p, "mixed") == 0)
                    == input["pass_id"].as_u64().unwrap() as usize
                {
                    pixels.push([p[0], p[1], 0xF941]);
                }
            },
        );
        pixels.sort_by_key(|p| (p[1], p[0]));
        let expected: Vec<[i32; 3]> = row["pixels"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert_eq!(pixels, expected, "native leaf {input}");
    }
}

fn projected_fixture(row: &Value, camera: [i32; 2]) -> ([Vec<SurfaceStroke>; 2], TacticalViewport) {
    let view = TacticalViewport {
        camera,
        clip: point(&row["clip"]),
    };
    let mut actual = [Vec::new(), Vec::new()];
    for rows in row["passes"][0]["clips"].as_array().unwrap().chunks(3) {
        let clip = &rows[0];
        let mut from = point(&clip["from_point"]);
        let mut to = point(&clip["to_point"]);
        from[1] -= 2;
        to[1] -= 2;
        let input = &row["input"];
        emit_rally_line(
            &mut actual,
            from,
            to,
            NATIVE_RALLY_WORD,
            input["frame"].as_u64().unwrap_or(0) as u32,
            view,
        );
    }
    (actual, view)
}

/// The A fixture a producer row names.
fn row_alpha(row: &Value) -> impl Fn([i32; 2]) -> u8 + '_ {
    |p| alpha_fixture(p, row["input"]["alpha"].as_str().unwrap_or("clear"))
}

#[test]
fn native_rally_producer_clipped_rows_phase_and_abuffer_passes() {
    assert_eq!(
        rally_native()["producer_cases"].as_array().unwrap().len(),
        65
    );
    assert_eq!(
        rally_native()["multi_producer_cases"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    for row in producer_rows() {
        let (actual, view) = projected_fixture(row, [0, 0]);
        assert_native_passes(&actual, row, view, row_alpha(row));
    }
}

#[test]
fn rally_crossing_stock_shroud_frontier_matches_the_original_combined_draw() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud.json",
    ))
    .unwrap();
    let scene = corpus["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scene| scene["input"]["name"] == "factory_frontier")
        .unwrap();
    // The production ShroudBuffer rebuild is independently checked against
    // these whole6D3660 bytes. Feed the same original A into the actual rally
    // emitter and compare6DA9D0's stores after each pass.
    let alpha = scene["alpha_hex"].as_str().unwrap();
    let alpha: Vec<_> = (0..alpha.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&alpha[i..i + 2], 16).unwrap())
        .collect();
    let row = serde_json::json!({
        "input": scene["input"], "passes": scene["rally_passes"]
    });
    let view = TacticalViewport {
        camera: [0; 2],
        clip: [0, 0, 160, 120],
    };
    let clip = &row["passes"][0]["clips"][0];
    let mut from: [i32; 2] = point(&clip["from_point"]);
    let mut to: [i32; 2] = point(&clip["to_point"]);
    from[1] -= 2;
    to[1] -= 2;
    let mut actual = [Vec::new(), Vec::new()];
    emit_rally_line(&mut actual, from, to, NATIVE_RALLY_WORD, 0, view);
    assert_native_passes(&actual, &row, view, |[x, y]| alpha[(y * 160 + x) as usize]);
}

fn simulation_fixture(row: &Value) -> (Simulation, RuleSet) {
    let input = &row["input"];
    let factory = match input["factory"].as_i64().unwrap_or(40) {
        40 => "UnitType",
        16 => "InfantryType",
        _ => "none",
    };
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
        "[BuildingTypes]\n0=FACTORY\n[FACTORY]\nFactory={factory}\nUnitRepair={}\nCloning={}\n",
        input["unit_repair"].as_bool().unwrap_or(false),
        input["cloning"].as_bool().unwrap_or(false)
    )))
    .unwrap();
    let foundation = crate::rules::foundation::Foundation::from(format!(
        "{}x{}",
        row["foundation_size"][0], row["foundation_size"][1]
    ));
    (
        simulation_fixture_for_type(row, "FACTORY", &foundation, "Americans", 10),
        rules,
    )
}

fn simulation_fixture_for_type(
    row: &Value,
    type_name: &str,
    foundation: &crate::rules::foundation::Foundation,
    owner: &str,
    first_id: u64,
) -> Simulation {
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::sim::components::DriveCoord;
    let input = &row["input"];
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default_of_category(
        first_id,
        type_name,
        owner,
        10,
        20,
        EntityCategory::Structure,
    );
    entity.set_type_ref_for_test(sim.intern(type_name));
    entity.set_owner_for_test(sim.intern(owner));
    entity
        .selection
        .set_selected_for_test(input["selected"].as_bool().unwrap_or(true));
    entity
        .lifecycle
        .set_object_alive_for_test(input["alive"].as_bool().unwrap_or(true));
    entity
        .type_facts
        .set_foundation_for_test(foundation.clone());
    let [tx, ty] = input.get("target_cell").map(point).unwrap_or([14, 20]);
    if !input["no_target"].as_bool().unwrap_or(false) {
        if let Some(target) = input.get("target_object") {
            let [x, y, z] = point(target);
            let target_id = first_id + 1;
            let mut target =
                GameEntity::test_default(target_id, "MTNK", owner, tx as u16, ty as u16);
            target.position.put_location(DriveCoord { x, y, z });
            target.set_type_ref_for_test(sim.intern("MTNK"));
            target.set_owner_for_test(sim.intern(owner));
            sim.entities_mut_for_test().insert(target);
            entity.set_archive_target(Some(TargetKind::Entity(target_id)));
        } else {
            entity.set_archive_target(Some(TargetKind::Cell(tx as u16, ty as u16)));
        }
    }
    let sources: Vec<_> = input["sources"].as_array().map_or_else(
        || vec![input.get("source").map(point).unwrap_or([2688, 5248, 0])],
        |sources| sources.iter().map(point).collect(),
    );
    for (index, [x, y, z]) in sources.into_iter().enumerate() {
        let mut factory = entity.clone();
        factory.set_stable_id_for_test(first_id + index as u64);
        factory.position.put_location(DriveCoord { x, y, z });
        let coords = crate::sim::movement::ground_pose::object_get_coords(&factory, None);
        let expected = if input.get("sources").is_some() {
            &row["source_coords"][index]
        } else {
            &row["source_coords"]
        };
        assert_eq!([coords.x, coords.y, coords.z], point::<3>(expected));
        sim.entities_mut_for_test().insert(factory);
    }
    let width = u16::try_from(tx + 1).unwrap().max(32);
    let height = u16::try_from(ty + 1).unwrap().max(32);
    let mut cells: Vec<_> = (0..height)
        .flat_map(|y| (0..width).map(move |x| ResolvedTerrainCell::clear_for_test(x, y)))
        .collect();
    let target = cells
        .iter_mut()
        .find(|c| i32::from(c.rx) == tx && i32::from(c.ry) == ty)
        .unwrap();
    target.level = input["level"].as_u64().unwrap_or(0) as u8;
    target.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
    target.has_bridge_deck = input["flags"].as_u64().unwrap_or(0) & 256 != 0;
    *sim.resolved_terrain_mut_for_test() =
        Some(ResolvedTerrainGrid::from_cells(width, height, cells).test_mark_decks_structural());
    sim.session_mut_for_test()
        .set_binary_frame_for_test(input["frame"].as_u64().unwrap_or(0) as u32);
    sim
}

#[test]
fn native_rally_gates_foundation_ground_bridge_and_object_target_through_production_builder() {
    for row in producer_rows() {
        let (sim, rules) = simulation_fixture(row);
        let mut camera = point(&row["camera"]);
        camera[1] += 15; // VERA's shared absolute projection bias, not a rally offset.
        let view = TacticalViewport {
            camera,
            clip: point(&row["clip"]),
        };
        let mut actual = build_factory_rally_line_strokes(
            Some(&sim),
            Some(&PresentationFrame::of_test(&sim, &rules)),
            Some(&rules),
            &selected_for_row(row),
            &HouseColorMap::new(),
            Some(if row["input"]["enemy"].as_bool().unwrap_or(false) {
                "Other"
            } else {
                "Americans"
            }),
            view,
        );
        // House RGB is a supplied native boundary. Its RGB565 conversion is
        // checked by the separate native producer + GPU comparisons.
        for stroke in actual.iter_mut().flatten() {
            if stroke.word() != 0 {
                *stroke = stroke.with_word(NATIVE_RALLY_WORD);
            }
        }
        assert_native_passes(&actual, row, view, row_alpha(row));
    }
}

#[test]
fn retail_gapile_house_color_and_production_crop_match_original_rally_and_no_target() {
    use crate::rules::color_scheme::scheme_entry_for_priority;
    use crate::rules::house_colors::HouseColorIndex;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    // The fixture owner loads physical startup/Battle/map layers and installs
    // fixed ART. No synthetic FACTORY type or replacement tint enters this test.
    let rules = &retail.rules;
    let stock = &rally_native()["stock_type_inputs"];
    let type_name = stock["result"]["type_id"].as_str().unwrap();
    let object = rules.object(type_name).unwrap();
    assert_eq!(
        serde_json::json!(object.factory_rtti()),
        stock["result"]["factory"]
    );
    let selector = match &object.foundation {
        crate::rules::foundation::Foundation::Stock(index) => *index,
        _ => panic!("retail fixture must retain a stock foundation"),
    };
    assert_eq!(serde_json::json!(selector), stock["result"]["foundation"]);
    assert_eq!(
        serde_json::json!(u8::from(object.unit_repair)),
        stock["result"]["unit_repair"]
    );
    assert_eq!(
        serde_json::json!(u8::from(object.cloning)),
        stock["result"]["cloning"]
    );
    assert_eq!(
        serde_json::json!(object.has_rally_line()),
        stock["result"]["has_rally_point"]
    );

    let rows = rally_native()["production_crop_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let mut target_states = std::collections::BTreeSet::new();
    for row in rows {
        let input = &row["input"];
        let owner = input["production_owner"].as_str().unwrap();
        let id = input["production_actor_id"].as_u64().unwrap();
        let no_target = input["no_target"].as_bool().unwrap();
        target_states.insert(no_target);
        assert_eq!(input["factory"], stock["result"]["factory"]);
        assert_eq!(input["foundation"], stock["result"]["foundation"]);
        assert_eq!(input["unit_repair"], serde_json::json!(object.unit_repair));
        assert_eq!(input["cloning"], serde_json::json!(object.cloning));
        let (width, height) = object.foundation.dimensions();
        assert_eq!(
            [i32::from(width), i32::from(height)],
            point(&row["foundation_size"])
        );

        let entry =
            scheme_entry_for_priority(input["production_color_priority"].as_i64().unwrap() as i32);
        assert_eq!(
            rules.color_schemes[entry].name,
            input["color_scheme_name"].as_str().unwrap()
        );
        let color = HouseColorIndex(u8::try_from(entry).unwrap());
        let colors = HouseColorMap::from([(owner.to_owned(), color)]);
        assert_eq!(
            crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, color)
                .map(i32::from),
            point(&input["rgb"])
        );

        // Original6DA9D0 receives prepared Scenario fields recorded beside the
        // capture hashes: entity/target state, frame, camera and clear A. The
        // null Archive row is the observed post-Stop state, not a Stop execution
        // claim; Building455D50's separate destination corpus covers that write.
        let sim = simulation_fixture_for_type(row, type_name, &object.foundation, owner, id);
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(
            [
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry())
            ],
            point(&input["production_cell"])
        );
        assert_eq!(entity.archive_target().is_none(), no_target);
        let crop: [i32; 2] = point(&input["crop_origin"]);
        let production_camera: [i32; 2] = point(&input["production_camera"]);
        let camera = std::array::from_fn(|axis| production_camera[axis] + crop[axis]);
        let mut native_camera: [i32; 2] = point(&row["camera"]);
        native_camera[1] += input["world_y_bias"].as_i64().unwrap() as i32;
        assert_eq!(camera, native_camera);
        assert_eq!(input["production_zoom"], 1);
        assert_eq!(input["alpha"], "clear");
        let view = TacticalViewport {
            camera,
            clip: point(&row["clip"]),
        };
        let actual = build_factory_rally_line_strokes(
            Some(&sim),
            Some(&PresentationFrame::of_test(&sim, rules)),
            Some(rules),
            &[id],
            &colors,
            Some(owner),
            view,
        );
        assert_native_passes(&actual, row, view, row_alpha(row));
        assert_eq!(actual.iter().all(Vec::is_empty), no_target);
    }
    assert_eq!(
        target_states,
        std::collections::BTreeSet::from([false, true])
    );
}

#[test]
fn presentation_target_query_does_not_stamp_simulation_dummy_cell() {
    let row = &rally_native()["producer_cases"][0];
    let (mut sim, rules) = simulation_fixture(row);
    sim.entities_mut_for_test()
        .get_mut(10)
        .unwrap()
        .set_archive_target(Some(TargetKind::Cell(u16::MAX, u16::MAX)));
    let terrain = sim.resolved_terrain().unwrap();
    let canonical = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let before = canonical.dummy().snapshot().coord;
    let _ = build_factory_rally_line_strokes(
        Some(&sim),
        Some(&PresentationFrame::of_test(&sim, &rules)),
        Some(&rules),
        &[10],
        &HouseColorMap::new(),
        Some("Americans"),
        TacticalViewport {
            camera: [0, 0],
            clip: [0, 0, 160, 120],
        },
    );
    assert_eq!(canonical.dummy().snapshot().coord, before);
}

/// One GPU comparison: each pass's strokes and the native pixels after it.
struct GpuCase {
    strokes: [Vec<SurfaceStroke>; 2],
    passes: Vec<(usize, Vec<[i32; 3]>)>,
    alpha: &'static str,
    input: &'static Value,
}

#[test]
#[ignore = "requires GPU; compares original rally and leaf pixels, their A passes and unchanged depth"]
fn production_rally_gpu_matches_native_pixels_and_preserves_depth() {
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, draw_strokes, encoded};
    let gpu = Gpu::new();
    let size = [160, 120];
    let camera_pos = [-17, 73];
    let pixels =
        |value: &Value| -> Vec<[i32; 3]> { value.as_array().unwrap().iter().map(point).collect() };
    // DSurface4C0750's own leaf rows: one pass of one walk over the mixed A,
    // at every octant, tie, endpoint and phase.
    let leaves = rally_native()["leaf_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let input = &row["input"];
            let pass = input["pass_id"].as_u64().unwrap() as usize;
            let mut strokes = [Vec::new(), Vec::new()];
            strokes[pass].push(SurfaceStroke::patterned(
                point(&input["from_point"]),
                point(&input["to_point"]),
                &RALLY_PATTERN,
                input["phase"].as_i64().unwrap() as i32,
                NATIVE_RALLY_WORD,
                [StoreGate::ANonzero, StoreGate::AZero][pass],
                camera_pos,
            ));
            GpuCase {
                strokes,
                passes: vec![(pass, pixels(&row["pixels"]))],
                alpha: "mixed",
                input,
            }
        });
    // The producer rows: both passes, each after the last.
    let producers = producer_rows().map(|row| GpuCase {
        strokes: projected_fixture(row, camera_pos).0,
        passes: (0..2)
            .map(|pass| (pass, pixels(&row["passes"][pass]["pixels"])))
            .collect(),
        alpha: row["input"]["alpha"].as_str().unwrap_or("clear"),
        input: &row["input"],
    });
    let cases: Vec<_> = leaves.chain(producers).collect();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(
            &gpu.queue,
            crate::render::batch::CameraUniform {
                camera_pos: camera_pos.map(|v| v as f32),
                ..camera(size)
            },
        );
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        for case in &cases {
            // The live A under each stored pixel.
            let plane: Vec<u8> = (0..size[1] as i32)
                .flat_map(|y| (0..size[0] as i32).map(move |x| [x, y]))
                .map(|p| alpha_fixture(p, case.alpha))
                .collect();
            let a = crate::render::shroud_buffer::ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                camera_pos.map(|v| v as f32),
                &plane,
            );
            batch.bind_shroud(&gpu.device, Some(&a));
            let background = encoded(0x39E7, format).repeat((size[0] * size[1]) as usize);
            gpu.queue.write_texture(
                color.as_image_copy(),
                &background,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                color.size(),
            );
            for (pass, pixels) in &case.passes {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(&mut encoder, &cv, &dv, 12345, wgpu::LoadOp::Load);
                draw_strokes(&gpu, &batch, &mut encoder, &cv, &dv, &case.strokes[*pass]);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                // A row's pixels are the whole surface after its pass.
                let mut native_words = vec![0x39E7_u16; 160 * 120];
                for &[x, y, word] in pixels {
                    native_words[y as usize * 160 + x as usize] = word as u16;
                }
                let expected: Vec<u8> = native_words
                    .iter()
                    .flat_map(|&word| encoded(word, format))
                    .collect();
                assert!(
                    output[0] == expected,
                    "{format:?}, {} pass{pass}",
                    case.input
                );
                assert!(
                    output[1].as_chunks::<4>().0.iter().all(|bytes| {
                        crate::render::native_z::stored_z(f32::from_le_bytes(*bytes)) == 12345
                    }),
                    "rally changed Z: {format:?}, {} pass{pass}",
                    case.input
                );
            }
        }
    }
}
