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
        let main = crate::sim::rng::MainRng::new(1).draws();
        let mut beams = RadiationBeams::default();
        for &birth in &births {
            beams.create(birth, |_, _| [0; 3]);
        }
        let mut viewport = native::viewport(&native::corpus()["geometry_rows"][0]);
        if row["name"] == "clipped_lifetime" {
            viewport.camera = viewport.camera.map(|v| v + 3000);
        }
        for visit in row["visits"].as_array().unwrap() {
            beams.composite(30, viewport, &main, |_| true);
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

/// Kind2 in 6591B0/659CA0: at the requested rate nothing is culled, yet each
/// active eruption spends one Main 0..99 draw and draws ftol(length / 20)
/// segments. At Logic rate 0 the rate cube is 0, so the draw culls it; a
/// culled eruption then ages out without lines or Main draws.
#[test]
fn eruptions_cull_below_the_requested_rate_and_draw_twenty_lepton_segments() {
    use crate::sim::projectile::ProjectileCoord;
    let birth = RadiationBeamBirth {
        frame: 0,
        from: ProjectileCoord::new(2048, 2048, 0),
        to: ProjectileCoord::new(3048, 2048, 0),
        z_adjust: 0,
        color: [0, 255, 0],
        house: None,
        house_color: crate::sim::combat::radiation_beam::BeamHouseColor::None,
        duration: 10,
        amplitude: crate::util::native_x87::NativeF64Bits::from_bits(300.0_f64.to_bits()),
        eruption: true,
    };
    let viewport = SurfaceLineViewport {
        camera: [0, 0],
        clip: [-8192, -8192, 16384, 16384],
        z_origin_y: 0,
        zoom: 1.0,
    };
    let length = f64::from_bits(BeamGeometry::new(birth.from, birth.to).length().bits());
    let main = crate::sim::rng::MainRng::new(5);
    let mut reference = crate::sim::rng::SimRng::new(5);
    let mut beams = RadiationBeams::default();
    beams.create(birth, |_, _| [0; 3]);
    beams.composite(30, viewport, &main.draws(), |_| true);
    reference.next_range_i32_inclusive(0, 99);
    assert_eq!(main.logical_state(), reference.logical_state());
    assert_eq!(beams.draws().len(), (length / 20.0) as usize);
    beams.composite(0, viewport, &main.draws(), |_| true);
    reference.next_range_i32_inclusive(0, 99);
    assert!(beams.draws().is_empty());
    for _ in 2..birth.duration {
        beams.composite(0, viewport, &main.draws(), |_| true);
    }
    assert_eq!(main.logical_state(), reference.logical_state());
    assert!(beams.live.is_empty());
}
