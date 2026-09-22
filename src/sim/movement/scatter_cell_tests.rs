use super::*;
use crate::map::resolved_terrain::test_flat_cell;
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
use crate::sim::rng::SimRng;

#[test]
fn source_selection_matches_original_execution() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/infantry_source_scatter.json"
    ))
    .unwrap();
    let mut checked = 0;
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let mut terrain = ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| (0..32).map(move |x| test_flat_cell(x, y)))
                .collect(),
        );
        if let Some(cells) = input["cells"].as_array() {
            for cell in cells {
                let out = terrain
                    .cell_mut(
                        cell[0].as_u64().unwrap() as u16,
                        cell[1].as_u64().unwrap() as u16,
                    )
                    .unwrap();
                out.level = cell[2].as_i64().unwrap() as u8;
                out.bridge_facts.raw_flags = cell[3].as_u64().unwrap() as u32;
            }
        }
        let coord = |name: &str, default: [i32; 3]| -> [i32; 3] {
            input[name].as_array().map_or(default, |v| {
                std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
            })
        };
        let actor = coord("actor", [2688, 2688, 0]);
        let source = coord("source", [1000, 2688, 0]);
        let head = coord("head", [0, 0, 0]);
        let navigation = if head == [0, 0, 0] { actor } else { head };
        let seed = ((navigation[0] / 256) as i16, (navigation[1] / 256) as i16);
        let bounds = input["bounds"]
            .as_array()
            .map_or([16, -16, -16, 64, 64], |v| {
                std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
            });
        let bounds = PlayfieldBounds::from_normalized_local_size(
            bounds[0], bounds[1], bounds[2], bounds[3], bounds[4],
        );
        let mut rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
        let doing = input["doing"].as_i64().unwrap_or(-1) as i32;
        let mut checks = Vec::new();
        let mut events = Vec::new();
        let destination =
            if crate::rules::infantry_sequence::scatter_allowed_by_doing(doing).unwrap() {
                let start =
                    source_start_direction((actor[0], actor[1]), (source[0], source[1]), &mut rng);
                assert_eq!(
                    start & 7,
                    row["start_direction"].as_i64().unwrap() as i32,
                    "{input}"
                );
                events.push("random");
                select_neighbor::<std::convert::Infallible>(seed, start, |candidate, direction| {
                    let cells = NativeCellQuery::canonical(&terrain);
                    let cell = cells.lookup(candidate);
                    if !cell_is_in_playfield_height_aware(
                        (i32::from(candidate.0), i32::from(candidate.1)),
                        Some(bounds),
                        Some(&terrain),
                    ) {
                        return Ok(None);
                    }
                    let code = input["answers"][direction as usize].as_i64().unwrap_or(0);
                    checks.push((cells.coord(cell), direction, code));
                    events.push("entry");
                    Ok((code == 0).then(|| preferred_surface(&terrain, candidate)))
                })
                .unwrap()
            } else {
                None
            };
        if destination.is_some() {
            events.extend(["queue_move", "destination"]);
        }
        let expected_checks: Vec<_> = row["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    (
                        c[0][0].as_i64().unwrap() as i16,
                        c[0][1].as_i64().unwrap() as i16,
                    ),
                    c[1].as_i64().unwrap() as i32,
                    c[3].as_i64().unwrap(),
                )
            })
            .collect();
        assert_eq!(checks, expected_checks, "candidate order: {input}");
        assert_eq!(
            serde_json::json!(destination),
            row["destination"],
            "{input}"
        );
        assert_eq!(serde_json::json!(events), row["events"], "{input}");
        let state = rng.logical_state();
        assert_eq!(
            serde_json::json!([state.index_a, state.index_b]),
            row["random_indices"],
            "{input}"
        );
        checked += 1;
    }
    assert_eq!(checked, 56);
}
