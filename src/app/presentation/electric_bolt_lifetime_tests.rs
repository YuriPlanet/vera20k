//! Original EBolt manager visits, including repeated tactical draws at the
//! same binary frame, rejected clips, reverse order and scene clearing.

use super::*;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::sim::combat::TargetKind;
use crate::sim::combat::world_fire::FireVisit;
use crate::sim::house_state::HouseState;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::{MainRng, SimRng};
use crate::sim::world::Simulation;
use serde_json::{Value, json};

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}
fn point<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|i| int(&value[i]))
}
fn coord(value: &Value) -> ProjectileCoord {
    let [x, y, z] = point(value);
    ProjectileCoord::new(x, y, z)
}

#[test]
fn native_tactical_visits_own_lifetime_reverse_order_and_clear() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/electric_bolt.json",
    ))
    .unwrap();
    let colors = &corpus["palette"]["selected"];
    let palette = ElectricBoltPalette {
        ordinary: int(&colors["10"]) as u16,
        alternate: int(&colors["5"]) as u16,
        center: int(&colors["15"]) as u16,
    };
    for row in corpus["draw_cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["mixed_cases"].as_array().into_iter().flatten())
    {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut camera: [i32; 2] = point(&input["camera"]);
        camera[1] = camera[1].wrapping_add(15);
        let viewport = SurfaceLineViewport {
            camera,
            clip: input.get("clip").map_or([0, 0, 160, 120], point),
            z_origin_y: 0,
            zoom: 1.0,
        };
        let mut bolts = ElectricBolts::default();
        for birth in row["births"].as_array().unwrap() {
            let birth = &birth["bolt"];
            bolts.create(ElectricBoltBirth {
                frame: 0,
                from: coord(&birth["source"]),
                to: coord(&birth["target"]),
                z_adjust: int(&birth["z_adjust"]),
                phase: int(&birth["phase"]),
                alternate_color: birth["alternate"].as_bool().unwrap(),
                owner: None,
            });
            // These explicit native manager fixtures include retained zero
            // and last-decay values, independent of the normal constructor.
            bolts.live.last_mut().unwrap().decay = int(&birth["decay"]);
        }
        let main: MainRng = SimRng::from_native_state_hex_for_test(
            row["birth_rng_after"]["main"].as_str().unwrap(),
        )
        .into();
        for (ordinal, visit) in row["visits"].as_array().unwrap().iter().enumerate() {
            let at = format!("{name} visit{ordinal}");
            assert_eq!(
                main.native_state_hex(),
                visit["rng_before"]["main"].as_str().unwrap(),
                "{at} before"
            );
            if visit["clear"] == true {
                bolts.clear_on_load();
                assert!(bolts.lines.is_empty(), "{at} retained geometry cleared");
            } else {
                let lines = bolts.composite(viewport, Some(palette), &mut main.draws());
                let expected = visit["segments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|line| line["family"].is_null() || line["family"] == "bolt")
                    .map(|line| crate::render::surface_line::SurfaceLine {
                        from: point(&line["from_point"]),
                        to: point(&line["to_point"]),
                        z_adjust: [int(&line["z_start"]), int(&line["z_end"])],
                        blend: crate::render::surface_line::SurfaceLineBlend::Replace(int(
                            &line["color"],
                        )
                            as u16),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(lines, expected, "{at} reverse geometry");
            }
            assert_eq!(
                main.native_state_hex(),
                visit["rng_after"]["main"].as_str().unwrap(),
                "{at} after"
            );
            let observed = bolts
                .observations()
                .map(|b| {
                    json!({
                        "source":b.from,"target":b.to,"z_adjust":b.z_adjust,
                        "alternate":b.alternate_color,"phase":b.phase,"decay":b.decay,
                    })
                })
                .collect::<Vec<_>>();
            let expected = visit["registered"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    let b = &visit["bolts"][id.as_u64().unwrap() as usize];
                    json!({"source":b["source"],"target":b["target"],"z_adjust":b["z_adjust"],
                    "alternate":b["alternate"],"phase":b["phase"],"decay":b["decay"]})
                })
                .collect::<Vec<_>>();
            assert_eq!(observed, expected, "{at} retained registry");
        }
    }
}

#[test]
fn accepted_bolt_composites_preserve_the_next_actual_bullet_identity() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(rules.object("MTNK").unwrap().primary(), Some("105mm"));
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/electric_bolt.json",
    ))
    .unwrap();
    let row = corpus["draw_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "lifetime_same_frame")
        .unwrap();
    let birth = &row["births"][0]["bolt"];
    let colors = &corpus["palette"]["selected"];
    let palette = ElectricBoltPalette {
        ordinary: int(&colors["10"]) as u16,
        alternate: int(&colors["5"]) as u16,
        center: int(&colors["15"]) as u16,
    };
    let mut camera = point(&row["input"]["camera"]);
    camera[1] += 15;
    let viewport = SurfaceLineViewport {
        camera,
        clip: [0, 0, 160, 120],
        z_origin_y: 0,
        zoom: 1.0,
    };
    let mut outcomes = Vec::new();
    for composite_count in [0, 1, 17] {
        let mut sim = Simulation::with_seed(1);
        crate::sim::arena_fixture::flat_arena(&mut sim, rules);
        for (index, name) in ["Americans", "Russians"].into_iter().enumerate() {
            let owner = sim.intern(name);
            sim.houses_mut_for_test().register(HouseState::new(
                owner,
                index as u8,
                None,
                true,
                0,
                10,
            ));
        }
        let source = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, rules)
            .unwrap();
        let target = sim
            .spawn_object("MTNK", "Russians", 13, 10, 0, rules)
            .unwrap();
        sim.resolve_type_handles(rules);
        // FireAt's explicit argument does not install TarCom. The cannon's
        // launch aim70BCB0 reads that retained target independently.
        assert!(crate::sim::combat::install_entity_attack_target_for_test(
            sim.entities_mut_for_test(),
            source,
            target,
        ));
        let cursor = sim.native_identity_cursor().unwrap();
        let before = sim.rng_state();
        let mut bolts = ElectricBolts::default();
        bolts.create(ElectricBoltBirth {
            frame: sim.session().binary_frame() as i32,
            from: coord(&birth["source"]),
            to: coord(&birth["target"]),
            z_adjust: int(&birth["z_adjust"]),
            phase: int(&birth["phase"]),
            alternate_color: birth["alternate"].as_bool().unwrap(),
            owner: None,
        });
        for visit in row["visits"]
            .as_array()
            .unwrap()
            .iter()
            .take(composite_count)
        {
            assert!(!visit["segments"].as_array().unwrap().is_empty());
            let lines =
                bolts.composite(viewport, Some(palette), &mut sim.presentation_main_draws());
            assert!(!lines.is_empty(), "accepted composite must consume Main");
            let native_delta = (int(&visit["native_id_after"]) as u32)
                .wrapping_sub(int(&visit["native_id_before"]) as u32);
            assert_eq!(
                native_delta, 0,
                "native manager has no Abstract constructor"
            );
            assert_eq!(
                sim.native_identity_cursor(),
                Some(cursor.wrapping_add(native_delta))
            );
        }
        let after = sim.rng_state();
        assert_eq!(
            after.scenario, before.scenario,
            "{composite_count} composites"
        );
        assert_eq!(after.mapgen, before.mapgen, "{composite_count} composites");
        assert_eq!(sim.native_identity_cursor(), Some(cursor));
        assert_eq!(after.main == before.main, composite_count == 0);

        // Actual FireAt6FE55D -> CreateBullet46B050 -> Abstract410230 owns
        // this preincrement. This is a real retail shell, not an ID probe.
        assert!(sim.projectiles().is_empty());
        sim.commit_fire_visit(
            FireVisit::Direct {
                id: source,
                target: TargetKind::Entity(target),
                weapon_index: 0,
            },
            rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.projectiles().len(), 1, "retail MTNK emitted its shell");
        let (_, bullet) = sim.projectiles().iter().next().unwrap();
        assert_eq!(bullet.source_id(), source);
        assert_eq!(bullet.native_unique_id(), cursor.wrapping_add(1) as i32);
        outcomes.push((after.main, bullet.native_unique_id(), bullet.id()));
    }
    for outcome in &outcomes[1..] {
        assert_ne!(outcome.0, outcomes[0].0, "composites advanced shared Main");
        assert_eq!((outcome.1, outcome.2), (outcomes[0].1, outcomes[0].2));
    }
    assert_ne!(outcomes[1].0, outcomes[2].0);
}

/// A Unit's `+0x6DC` holds one owned bolt: a second bolt while the first
/// lives gets no owner (`0x006FD5F2`), and once the first has faded the next
/// one links again. Each pass moves an owned bolt to its owner's FLH, keeps
/// it on the static zero answer, and detaches it at its last source when the
/// Unit is gone (`0x004C2830`, `0x00735789`).
#[test]
fn a_unit_owns_one_bolt_whose_source_follows_it() {
    use crate::sim::combat::electric_bolt::ElectricBoltOwner;
    let owner = ElectricBoltOwner {
        unit: 7,
        weapon_index: 0,
    };
    let birth = |x| ElectricBoltBirth {
        frame: 0,
        from: ProjectileCoord::new(x, 0, 0),
        to: ProjectileCoord::new(0, 0, 0),
        z_adjust: 0,
        phase: 0,
        alternate_color: false,
        owner: Some(owner),
    };
    let mut bolts = ElectricBolts::default();
    bolts.create(birth(10));
    bolts.create(birth(20));
    let owners: Vec<_> = bolts.live.iter().map(|bolt| bolt.birth.owner).collect();
    assert_eq!(owners, [Some(owner), None]);

    bolts.follow_owners(|_| Some(ProjectileCoord::new(99, 1, 2)));
    let sources: Vec<_> = bolts.live.iter().map(|bolt| bolt.birth.from.x).collect();
    assert_eq!(sources, [99, 20], "only the owned bolt follows");
    bolts.follow_owners(|_| Some(ProjectileCoord::new(0, 0, 0)));
    assert_eq!(bolts.live[0].birth.from, ProjectileCoord::new(99, 1, 2));
    bolts.follow_owners(|_| None);
    assert_eq!(bolts.live[0].birth.owner, None, "the gone Unit detaches");
    assert_eq!(bolts.live[0].birth.from, ProjectileCoord::new(99, 1, 2));

    let mut bolts = ElectricBolts::default();
    bolts.create(birth(10));
    bolts.live[0].decay = 0;
    bolts.live.retain(|bolt| bolt.decay != 0);
    bolts.create(birth(30));
    assert_eq!(
        bolts.live[0].birth.owner,
        Some(owner),
        "the slot is free again"
    );
}
