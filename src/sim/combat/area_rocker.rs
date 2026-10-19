//! AreaDamage489B49..489E87: post-receiver, live-cell rocker producer.
//! The transaction owner retains its original source and pre-receiver layer.
//! Native arithmetic/argument controls: tools/projectile_oracle/area_rocker.json.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::RAD_NO_ATTACKER;
use crate::sim::map::bridge_topology::ListLayer;
use crate::sim::movement::ground_pose;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rocking::{RockerImpulse, apply_rocker_impulse};
use crate::sim::world::Simulation;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Ordering, sqrt_approx_f32,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct AreaRockerInput {
    pub impact: ProjectileCoord,
    pub cell: (u16, u16),
    pub damage: i32,
    pub source_id: u64,
    pub list_layer: ListLayer,
}

//489B49..81: signed damage, original0.01 double, cap4, binary64 spill.
fn force(damage: i32) -> NativeF64Bits {
    let scaled = X::mul(
        X::load_i32(damage),
        X::load_f64(NativeF64Bits::from_bits(0x3f84_7ae1_47ae_147b)).unwrap(),
    );
    let cap = NativeF64Bits::from_bits(0x4010_0000_0000_0000);
    if X::compare(scaled, X::load_f64(cap).unwrap()) == X87Ordering::Less {
        X::store_f64(scaled).expect("finite area rocker force")
    } else {
        cap
    }
}

///489C6F..489DFB: source-relative point only for the impact cell.
fn center_point(position: [i32; 3], source: [i32; 3]) -> [i32; 3] {
    let delta = std::array::from_fn::<_, 3, _>(|axis| {
        X::store_f32(X::load_i32(source[axis].wrapping_sub(position[axis])))
            .expect("finite area rocker component")
    });
    let [x, y, z] = delta.map(|word| X::load_f32(word).unwrap());
    let squared = X::add(X::add(X::mul(x, x), X::mul(y, y)), X::mul(z, z));
    //489CE5 spills a double before the shared Sqrt_Approx4CAC40 call.
    let squared = X::load_f64(X::store_f64(squared).unwrap()).unwrap();
    let distance = X::load_f32(sqrt_approx_f32(squared).unwrap()).unwrap();
    let zero = X::compare(distance, X::load_i32(0)) == X87Ordering::Equal;
    std::array::from_fn(|axis| {
        let component = X::load_f32(delta[axis]).unwrap();
        let normalized = if zero {
            delta[axis]
        } else {
            X::store_f32(X::div(component, distance).unwrap()).unwrap()
        };
        let scaled = X::store_f32(X::mul(
            X::load_f32(normalized).unwrap(),
            X::load_f32(NativeF32Bits::from_bits(0x4120_0000)).unwrap(),
        ))
        .unwrap();
        position[axis].wrapping_add(X::ftol_i32_low_masked(X::load_f32(scaled).unwrap()))
    })
}

pub(super) fn apply(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    input: AreaRockerInput,
) {
    let force = force(input.damage);
    if !warhead.rocker
        || X::compare(
            X::load_f64(force).unwrap(),
            X::load_f64(NativeF64Bits::from_bits(0x3fd3_3333_3333_3333)).unwrap(),
        ) != X87Ordering::Greater
    {
        return;
    }
    let force = X::store_f32(X::load_f64(force).unwrap()).unwrap();
    let layer = match input.list_layer {
        ListLayer::Ground => MovementLayer::Ground,
        ListLayer::Bridge => MovementLayer::Bridge,
    };
    //489BAA..489E7E: signed packed centre, x-major49-cell traversal.
    let center = (
        i32::from(input.cell.0 as i16),
        i32::from(input.cell.1 as i16),
    );
    for x in center.0 - 3..=center.0 + 3 {
        for y in center.1 - 3..=center.1 + 3 {
            let Some(terrain) = world.resolved_terrain.as_ref() else {
                world
                    .effective_shared_cell_dummy()
                    .stamp_coord(x as i16 as i32, y as i16 as i32);
                continue;
            };
            let query = NativeCellQuery::canonical(terrain);
            let cell = query.lookup((x as i16, y as i16));
            if cell == NativeCellIdentity::Dummy {
                continue;
            }
            let (rx, ry) = query.coord(cell);
            let cell = (rx as u16, ry as u16);
            let mut current = world
                .substrate
                .occupancy
                .get(cell.0, cell.1)
                .and_then(|list| list.first_on_layer(layer));
            while let Some(id) = current {
                //40DD70 admits all four Techno classes. Their original+3D8
                //slots share70B280; its loaded-VXL gate decides admission.
                let request = world.substrate.entities.get(id).and_then(|entity| {
                    let handle = world.interner.type_handle(entity.type_ref(), rules)?;
                    let object = rules.object_by_handle(handle);
                    let location =
                        ground_pose::object_location(entity, world.resolved_terrain.as_ref());
                    let position = [location.x, location.y, location.z];
                    let origin = if (x, y) == center && input.source_id != RAD_NO_ATTACKER {
                        //The stack argument survives PointerExpired on the Bullet.
                        //UnInit leaves its Location/storage until the deferred drain.
                        let source = world
                            .substrate
                            .entities
                            .get(input.source_id)
                            .map(|source| {
                                let p = ground_pose::object_location(
                                    source,
                                    world.resolved_terrain.as_ref(),
                                );
                                [p.x, p.y, p.z]
                            })
                            .expect(
                                "AreaDamage source remains represented until deferred destruction",
                            );
                        center_point(position, source)
                    } else {
                        if warhead.cell_spread_f64.partial_cmp(&0.0)
                            != Some(std::cmp::Ordering::Greater)
                        {
                            return None;
                        }
                        [input.impact.x, input.impact.y, input.impact.z]
                    };
                    Some(RockerImpulse {
                        position,
                        origin,
                        body_facing: entity.body_facing_current(world.session.binary_frame()),
                        force,
                        weight: object.weight,
                        no_dampen: false,
                        has_loaded_voxel: rules.object_has_loaded_voxel(handle),
                    })
                });
                if let Some(request) = request {
                    #[cfg(any(test, feature = "test-support"))]
                    let before = words(world, id);
                    apply_rocker_impulse(
                        &mut world.substrate.entities.get_mut(id).unwrap().rocking,
                        request,
                    );
                    #[cfg(any(test, feature = "test-support"))]
                    CALLS.with_borrow_mut(|capture| {
                        let Some(calls) = capture.as_mut() else {
                            return;
                        };
                        calls.push(ObservedCall {
                            id,
                            point: request.origin,
                            force: request.force.bits(),
                            before,
                            after: words(world, id),
                        })
                    });
                }
                //489E44 reads the current object's link after its callback;
                //do not retain a pre-receiver or pre-call candidate snapshot.
                current = world
                    .substrate
                    .occupancy
                    .get(cell.0, cell.1)
                    .and_then(|list| list.next_on_layer(layer, id));
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
struct ObservedCall {
    id: u64,
    point: [i32; 3],
    force: u32,
    before: [u32; 4],
    after: [u32; 4],
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    //Comparison capture only; no observer state in Simulation/save/hash.
    static CALLS: std::cell::RefCell<Option<Vec<ObservedCall>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-support"))]
fn words(world: &Simulation, id: u64) -> [u32; 4] {
    world
        .substrate
        .entities
        .get(id)
        .unwrap()
        .rocking
        .as_ref()
        .map_or([0; 4], |state| state.native_bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::DriveCoord;
    use crate::sim::rng::SimRng;
    use serde_json::{Value, json};

    fn xyz(value: &Value) -> [i32; 3] {
        std::array::from_fn(|axis| value[axis].as_i64().unwrap() as i32)
    }

    #[test]
    fn area_rocker_arguments_and_retained_words_match_original() {
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/area_rocker.json",
        ))
        .unwrap();
        let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut rules = fixture.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert!(rules.object_has_loaded_voxel(rules.type_handle("MTNK").unwrap()));
        for (name, expected) in corpus["initial"]["model_input"]["files"]
            .as_object()
            .unwrap()
        {
            let physical = assets.load_file_from_mix(name).unwrap();
            assert_eq!(json!(physical.bytes.len()), expected["bytes"]);
            assert_eq!(
                crate::util::sha256::sha256_hex(&physical.bytes),
                expected["sha256"].as_str().unwrap()
            );
        }
        assert_eq!(
            rules
                .object("MTNK")
                .unwrap()
                .weight
                .to_num::<f64>()
                .to_bits(),
            u64::from_str_radix(
                corpus["initial"]["type_input"]["weight_bits"]
                    .as_str()
                    .unwrap(),
                16
            )
            .unwrap()
        );
        let patch = crate::rules::ini_parser::IniFile::from_str("[BlimpHE]\nCellSpread=0\n");
        let zero_rules =
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
                .unwrap()
                .rules;
        let zero_spread = zero_rules.warhead("BlimpHE").unwrap().clone();
        assert_eq!(corpus["rows"].as_array().unwrap().len(), 9);
        for row in corpus["rows"].as_array().unwrap() {
            let input = &row["input"];
            let mut world = crate::sim::arena_fixture::rival_arena_with_size(&rules, 31, 64);
            world.intern_rule_type_ids(&rules);
            world.resolve_type_handles(&rules);
            for cell in &mut world.resolved_terrain.as_mut().unwrap().cells {
                cell.level = 6;
            }
            let mut targets = Vec::new();
            for resident in corpus["initial"]["residents"].as_array().unwrap() {
                let cell = &resident["cell"];
                let id = world
                    .spawn_object(
                        "MTNK",
                        "Americans",
                        cell[0].as_u64().unwrap() as u16,
                        cell[1].as_u64().unwrap() as u16,
                        (resident["facing"].as_u64().unwrap() >> 8) as u8,
                        &rules,
                    )
                    .unwrap();
                let entity = world.substrate.entities.get(id).unwrap();
                let p = ground_pose::object_location(entity, world.resolved_terrain.as_ref());
                assert_eq!([p.x, p.y, p.z], xyz(&resident["position"]));
                assert_eq!(
                    u64::from(entity.body_facing_current(world.session.binary_frame())),
                    resident["facing"].as_u64().unwrap()
                );
                targets.push((resident["name"].as_str().unwrap(), id));
            }
            //Explicit raw source Location is the declared component prior,
            //not a claim of ordinary Kirov flight/placement in this comparison.
            let source_id = if input["source"].is_null() {
                RAD_NO_ATTACKER
            } else {
                let source = xyz(&input["source"]);
                let id = world
                    .spawn_object("MTNK", "Americans", 30, 30, 0, &rules)
                    .unwrap();
                //The native source is a retained, unlisted component input.
                //Use the existing Mark owner, not a second occupancy list.
                world.remove_entity_occupancy(id);
                world
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .position
                    .put_location(DriveCoord {
                        x: source[0],
                        y: source[1],
                        z: source[2],
                    });
                id
            };
            let warhead = if input["cell_spread"] == "0" {
                zero_spread.clone()
            } else {
                rules.warhead("BlimpHE").unwrap().clone()
            };
            assert_eq!(
                warhead.rocker,
                row["warhead_after_read"]["rocker"].as_bool().unwrap()
            );
            assert_eq!(
                (warhead.cell_spread_f64 as f32).to_bits(),
                u32::from_str_radix(
                    row["warhead_after_read"]["cell_spread_bits"]
                        .as_str()
                        .unwrap(),
                    16
                )
                .unwrap()
            );
            world.main_rng =
                SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                    .into();
            world.scenario_rng = SimRng::from_native_state_hex_for_test(
                row["rng_before"]["scenario"].as_str().unwrap(),
            );
            world.mapgen_rng = SimRng::from_native_state_hex_for_test(
                row["rng_before"]["mapgen"].as_str().unwrap(),
            );
            CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
            let impact = xyz(&input["impact"]);
            let request = AreaRockerInput {
                impact: ProjectileCoord::new(impact[0], impact[1], impact[2]),
                cell: (10, 20),
                damage: input["damage"].as_i64().unwrap() as i32,
                source_id,
                list_layer: ListLayer::Ground,
            };
            assert_eq!(
                format!("{:016x}", force(request.damage).bits()),
                row["force_f64_bits"].as_str().unwrap()
            );
            apply(&mut world, &rules, &warhead, request);
            let observed = CALLS.with_borrow_mut(Option::take).unwrap();
            let expected = row["calls"].as_array().unwrap();
            assert_eq!(observed.len(), expected.len(), "{}", input["name"]);
            for (actual, expected) in observed.iter().zip(expected) {
                assert_eq!(
                    targets.iter().find(|(_, id)| *id == actual.id).unwrap().0,
                    expected["target"].as_str().unwrap()
                );
                assert_eq!(json!(actual.point), expected["point"], "{}", input["name"]);
                assert_eq!(json!(actual.force), expected["force_bits"]);
                assert_eq!(expected["no_dampen"], 0);
                assert_eq!(json!(actual.before), expected["before_bits"]);
                assert_eq!(
                    json!(actual.after),
                    expected["after_bits"],
                    "{}",
                    input["name"]
                );
            }
            for expected in row["final"].as_array().unwrap() {
                let id = targets
                    .iter()
                    .find(|(name, _)| *name == expected["target"].as_str().unwrap())
                    .unwrap()
                    .1;
                assert_eq!(
                    json!(words(&world, id)),
                    expected["bits"],
                    "{}",
                    input["name"]
                );
            }
            for (name, actual) in [
                ("main", world.main_rng.native_state_hex()),
                ("scenario", world.scenario_rng.native_state_hex()),
                ("mapgen", world.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(actual, row["rng_after"][name].as_str().unwrap());
            }
        }
    }

    fn scene() -> Option<(RuleSet, Simulation)> {
        let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let mut world = crate::sim::arena_fixture::rival_arena_with_size(&rules, 17, 64);
        world.intern_rule_type_ids(&rules);
        world.resolve_type_handles(&rules);
        Some((rules, world))
    }

    fn blast(layer: ListLayer) -> AreaRockerInput {
        AreaRockerInput {
            impact: ProjectileCoord::new(2688, 5248, 0),
            cell: (10, 20),
            damage: 400,
            source_id: RAD_NO_ATTACKER,
            list_layer: layer,
        }
    }

    #[test]
    fn area_rocker_scans_all_radius_three_corners_and_keeps_shared_model_gate() {
        let Some((rules, mut world)) = scene() else {
            return;
        };
        let mut corners = Vec::new();
        for (x, y) in [(7, 17), (7, 23), (13, 17), (13, 23)] {
            corners.push(
                world
                    .spawn_object("MTNK", "Americans", x, y, 0, &rules)
                    .unwrap(),
            );
        }
        let outside = world
            .spawn_object("MTNK", "Americans", 6, 20, 0, &rules)
            .unwrap();
        let infantry = world
            .spawn_object("E1", "Americans", 9, 20, 0, &rules)
            .unwrap();
        assert!(!rules.object_has_loaded_voxel(rules.type_handle("E1").unwrap()));
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(
            &mut world,
            &rules,
            rules.warhead("BlimpHE").unwrap(),
            blast(ListLayer::Ground),
        );
        let calls = CALLS.with_borrow_mut(Option::take).unwrap();
        assert_eq!(
            calls.iter().map(|call| call.id).collect::<Vec<_>>(),
            vec![corners[0], corners[1], infantry, corners[2], corners[3]]
        );
        for id in corners {
            assert!(
                words(&world, id)[2..]
                    .iter()
                    .any(|bits| bits & 0x7fff_ffff != 0)
            );
        }
        assert!(
            world
                .substrate
                .entities
                .get(outside)
                .unwrap()
                .rocking
                .is_none()
        );
        assert!(
            world
                .substrate
                .entities
                .get(infantry)
                .unwrap()
                .rocking
                .is_none()
        );
    }

    #[test]
    fn area_rocker_uses_frozen_layer_and_live_post_removal_membership() {
        let Some((rules, mut world)) = scene() else {
            return;
        };
        let lower = world
            .spawn_object("MTNK", "Americans", 11, 20, 0, &rules)
            .unwrap();
        let upper = world
            .spawn_object("MTNK", "Americans", 12, 20, 0, &rules)
            .unwrap();
        world.remove_entity_occupancy(upper);
        world
            .substrate
            .entities
            .get_mut(upper)
            .unwrap()
            .set_on_bridge(true);
        assert!(world.mark_entity_put(upper, crate::sim::world::UninitContext::with_rules(&rules)));
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(
            &mut world,
            &rules,
            rules.warhead("BlimpHE").unwrap(),
            blast(ListLayer::Bridge),
        );
        let calls = CALLS.with_borrow_mut(Option::take).unwrap();
        assert_eq!(
            calls.iter().map(|call| call.id).collect::<Vec<_>>(),
            vec![upper]
        );
        assert!(
            world
                .substrate
                .entities
                .get(lower)
                .unwrap()
                .rocking
                .is_none()
        );

        let older = world
            .spawn_object("E1", "Americans", 9, 20, 0, &rules)
            .unwrap();
        let head = world
            .spawn_object("E1", "Americans", 9, 20, 0, &rules)
            .unwrap();
        assert_eq!(
            world
                .substrate
                .occupancy
                .get(9, 20)
                .unwrap()
                .first_on_layer(MovementLayer::Ground),
            Some(head)
        );
        //Synchronous receiver cleanup leaves the object resolvable but unlisted.
        world.uninit_with_rules(head, &rules);
        assert!(world.substrate.entities.contains(head));
        assert_eq!(
            world
                .substrate
                .occupancy
                .get(9, 20)
                .unwrap()
                .first_on_layer(MovementLayer::Ground),
            Some(older)
        );
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(
            &mut world,
            &rules,
            rules.warhead("BlimpHE").unwrap(),
            blast(ListLayer::Ground),
        );
        let calls = CALLS.with_borrow_mut(Option::take).unwrap();
        assert_eq!(
            calls.iter().map(|call| call.id).collect::<Vec<_>>(),
            vec![older, lower]
        );
    }

    #[test]
    fn area_rocker_gates_preserve_state_and_expired_source_keeps_location() {
        let Some((rules, mut world)) = scene() else {
            return;
        };
        let target = world
            .spawn_object("MTNK", "Americans", 10, 20, 0, &rules)
            .unwrap();
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(
            &mut world,
            &rules,
            rules.warhead("AP").unwrap(),
            blast(ListLayer::Ground),
        );
        let mut small = blast(ListLayer::Ground);
        small.damage = 30;
        apply(&mut world, &rules, rules.warhead("BlimpHE").unwrap(), small);
        assert!(CALLS.with_borrow_mut(Option::take).unwrap().is_empty());
        assert!(
            world
                .substrate
                .entities
                .get(target)
                .unwrap()
                .rocking
                .is_none()
        );

        let source = world
            .spawn_object("MTNK", "Americans", 30, 30, 0, &rules)
            .unwrap();
        world.uninit_with_rules(source, &rules);
        assert!(world.substrate.entities.contains(source));
        //An intervening coordinate writer is observed after callbacks, not cached
        //at area collection. Use the native coupled-center control's full inputs.
        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/area_rocker.json",
        ))
        .unwrap();
        let row = &corpus["rows"][0];
        let position = xyz(&corpus["initial"]["residents"][0]["position"]);
        let source_position = xyz(&row["input"]["source"]);
        world
            .substrate
            .entities
            .get_mut(target)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: position[0],
                y: position[1],
                z: position[2],
            });
        world
            .substrate
            .entities
            .get_mut(source)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: source_position[0],
                y: source_position[1],
                z: source_position[2],
            });
        let mut request = blast(ListLayer::Ground);
        request.damage = 250;
        request.source_id = source;
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(
            &mut world,
            &rules,
            rules.warhead("BlimpHE").unwrap(),
            request,
        );
        let calls = CALLS.with_borrow_mut(Option::take).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(json!(calls[0].point), row["calls"][0]["point"]);
    }

    #[test]
    fn area_rocker_keeps_fixed_stride_aliases_outside_playable_coordinates() {
        let Some((rules, mut world)) = scene() else {
            return;
        };
        let target = world
            .spawn_object("MTNK", "Americans", 12, 19, 0, &rules)
            .unwrap();
        //5657A0(-500,20) aliases fixed512 slot(12,19). A rectangular
        //playfield clip would skip a real CellClass before model admission.
        let mut input = blast(ListLayer::Ground);
        input.cell = ((-500_i16) as u16, 20);
        input.impact = ProjectileCoord::new(-500 * 256 - 128, 20 * 256 + 128, 0);
        CALLS.with_borrow_mut(|capture| *capture = Some(Vec::new()));
        apply(&mut world, &rules, rules.warhead("BlimpHE").unwrap(), input);
        let calls = CALLS.with_borrow_mut(Option::take).unwrap();
        assert_eq!(
            calls.iter().map(|call| call.id).collect::<Vec<_>>(),
            vec![target]
        );
        //The distant exact impact still reaches the one shared weak-force gate.
        assert!(
            world
                .substrate
                .entities
                .get(target)
                .unwrap()
                .rocking
                .is_none()
        );
    }
}
