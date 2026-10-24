//! Native6591B0 histories exercise this production registry, including
//! reverse traversal, stable compaction, same-frame and clipped retirement.
use super::*;
use crate::render::radiation_beam::tests as native;

#[test]
fn original_manager_histories_match_without_logic_ticks_or_rng() {
    let sim = crate::sim::world::Simulation::new();
    let rng = sim.rng_state();
    let hash = sim.state_hash();
    for row in native::corpus()["lifetime_rows"].as_array().unwrap() {
        let births = row["births"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| native::birth(&v["birth"]))
            .collect::<Vec<_>>();
        let mut beams = RadiationBeams::default();
        for &birth in &births {
            beams.create(birth);
        }
        let mut viewport = native::viewport(&native::corpus()["geometry_rows"][0]);
        if row["name"] == "clipped_lifetime" {
            viewport.camera = viewport.camera.map(|v| v + 3000);
        }
        for visit in row["visits"].as_array().unwrap() {
            beams.composite(30, viewport, |_| true);
            let expected = visit["registered_after"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| births[id.as_u64().unwrap() as usize].duration)
                .collect::<Vec<_>>();
            assert_eq!(
                beams
                    .live
                    .iter()
                    .map(|b| b.birth.duration)
                    .collect::<Vec<_>>(),
                expected,
                "{} membership",
                row["name"]
            );
            for (beam, expected) in beams.live.iter().zip(visit["states"].as_array().unwrap()) {
                let state = &expected["state"];
                assert_eq!(beam.progress, native::integer(&state["progress"]));
                assert_eq!(
                    (beam.spacing as f64).to_bits(),
                    native::bits(&state["spacing_f64_le_hex"])
                );
                assert_eq!(
                    beam.geometry.unwrap().length().bits(),
                    native::bits(&state["length_f64_le_hex"])
                );
            }
            for (actual, expected) in beams
                .composites
                .iter()
                .zip(visit["returns"].as_array().unwrap())
            {
                let id = expected["beam_index"].as_u64().unwrap() as usize;
                assert_eq!(actual.expired, expected["expired"].as_bool().unwrap());
                assert_eq!(actual.expired, actual.progress_after >= births[id].duration);
                assert_eq!(actual.progress_before + 1, actual.progress_after);
            }
            assert_eq!(
                beams.composites.len(),
                visit["draw_order"].as_array().unwrap().len()
            );
            assert_eq!(
                beams.draws.len(),
                visit["raster_call_count"].as_u64().unwrap() as usize,
                "{} raster calls",
                row["name"]
            );
        }
        assert!(beams.live.is_empty());
    }
    assert_eq!(sim.rng_state(), rng);
    assert_eq!(sim.state_hash(), hash);
}
