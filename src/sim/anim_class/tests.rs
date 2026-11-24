use super::ai::{
    AnimBoundary, advance_anim_boundary, native_loop_remaining, trailer_cadence_matches,
};
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;

/// Phobos' `AnimClass_AI_Early @ 0x423B95` replacement of the stock
/// `HideIfNoOre` test: the anim's cell `Get_Tiberium_Value @ 0x00485020`
/// hides it at or below `abs(HideIfNoOre.Threshold=)`, not only at zero.
#[test]
#[cfg(feature = "phobos")]
fn hide_if_no_ore_threshold_moves_the_value_cut_off() {
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::world::Simulation;

    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n{}",
        crate::sim::tiberium::test_support::tiberium_rules_text(),
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("twinkle rules");
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[TWNK1]\nLoopCount=-1\nDetailLevel=2\nHideIfNoOre=true\n\
         HideIfNoOre.Threshold=-60\nRate=450\n",
    ));
    art.bind_anim_frame_count_for_test("TWNK1", 8);
    rules.replace_art_registry_for_test(art);
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);

    let hidden_over = |density: u8| {
        let mut sim = Simulation::new();
        sim.session.set_map_width_for_test(16);
        sim.session.set_map_height_for_test(16);
        let mut grid = OverlayGrid::new(16, 16);
        grid.place_overlay(8, 8, overlays.id_for_name("TIB01").unwrap(), density);
        *sim.overlay_grid_mut_for_test() = Some(grid);
        let type_id = sim.interner.intern("TWNK1");
        let id = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    loop_count: -1,
                    ..runtime_descriptor(type_id, 0)
                },
                AnimWorldCoord {
                    x: 8 * 256 + 128,
                    y: 8 * 256 + 128,
                    z: 0,
                },
            )
            .expect("the twinkle constructs");
        sim.visit_anim(
            id,
            &rules,
            Some(&overlays),
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        sim.anim(id)
            .expect("the looping twinkle survives its visit")
            .draw_runtime
            .hidden
    };

    // `TiberiumClass::Value` times `OverlayData + 1`, so 50 against the
    // threshold's 60 and 125 over it.
    assert!(hidden_over(1), "50 <= abs(-60) hides the twinkle");
    assert!(!hidden_over(4), "125 > 60 leaves it drawing");
}

#[test]
fn damage_fire_references_follow_original_owner_then_anim_expiry() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anim_damage_fire_expiry.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let slot = row["slot"].as_u64().unwrap() as usize;
        let (mut sim, rules, building_id) = damage_fire_fixture(false);
        let owner_coord = sim.anim_owner_coords(building_id).unwrap();
        let type_id = sim.interner.intern("FIRE01");
        let id = sim
            .spawn_anim_at_world(
                &rules,
                runtime_descriptor(type_id, 0),
                AnimWorldCoord {
                    x: owner_coord.x + 128,
                    y: owner_coord.y + 256,
                    z: owner_coord.z + 300,
                },
            )
            .unwrap();
        sim.set_anim_owner_object(id, Some(building_id), &rules);
        sim.substrate
            .entities
            .get_mut(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[slot] = Some(id);
        sim.substrate.anims.get_mut(id).unwrap().damage_fire_slot = Some((building_id, slot as u8));
        sim.detach_all_pointer_expired(
            building_id,
            &rules,
            &mut crate::sim::world::FrameEffects::default(),
        );
        for phase in ["owner_expiry", "anim_expiry"] {
            if phase == "anim_expiry" {
                sim.destroy_anim(id, &rules);
            }
            let expected = &row[phase];
            let anim = sim.anim(id).unwrap();
            assert_eq!(
                anim.owner_entity.is_some(),
                expected["owner"].as_bool().unwrap()
            );
            assert_eq!(
                anim.display.marked_on_map,
                expected["marked"].as_bool().unwrap()
            );
            assert_eq!(
                anim.runtime.inactive,
                expected["expired"].as_bool().unwrap()
            );
            assert_eq!(
                [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
                std::array::from_fn::<_, 3, _>(|i| expected["stored"][i].as_i64().unwrap() as i32)
            );
            let building = sim.substrate.entities.get(building_id).unwrap();
            for (i, id) in building.damage_fires.anim_ids.iter().enumerate() {
                assert_eq!(id.is_some(), expected["slots"][i].as_bool().unwrap());
            }
            assert_eq!(sim.substrate.display.layer_of(id), None);
        }
    }
}

#[test]
fn building_uninit_expires_attached_fires_then_limbo_uninits_them_and_survives_save() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let ids = sim
        .entities()
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids;
    // This is the explicit attached-owner expiry control, as in the
    // native eight-slot corpus. The ordinary fire producer leavesCC0.
    for id in ids.into_iter().flatten() {
        sim.set_anim_owner_object(id, Some(building_id), &rules);
    }
    let coords: Vec<_> = ids
        .iter()
        .flatten()
        .map(|id| (*id, sim.anim(*id).unwrap().world_coord))
        .collect();
    sim.sound_events.clear_for_test();
    sim.uninit_with_rules(building_id, &rules);
    // ObjectUnInit5F6616 expires the owner first (Anim425150: owner
    // cleared, +19B set), then its Limbo call reaches Building Limbo
    // 4458E4, which UnInits each fire (Release4255D5) and nulls its slot
    // before TechnoLimbo6F6C6B releases the empty4A4 handle.
    assert!(matches!(
        sim.sound_events.as_slice().last(),
        Some(SimSoundEvent::GattlingLoopRelease { owner })
            if *owner == crate::sim::combat::gattling::gattling_sound_owner(building_id)
    ));
    assert!(
        sim.entities()
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids
            .iter()
            .all(Option::is_none)
    );
    for (id, coord) in &coords {
        let anim = sim.anim(*id).unwrap();
        assert_eq!(anim.world_coord, *coord);
        assert_eq!(anim.owner_entity, None);
        assert!(anim.runtime.inactive);
        assert_eq!(anim.damage_fire_slot, None);
        assert!(sim.substrate.pending_delete.contains_object(id));
        assert_eq!(sim.substrate.display.layer_of(*id), None);
        assert!(sim.sound_events.iter().any(|event| matches!(event,
            SimSoundEvent::ObjectSoundReleased { owner } if *owner == *id)));
    }
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "building-expiry", 0);
    sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    assert_eq!(sim.state_hash(), hash);
    sim.process_pending_delete();
    assert!(sim.entities().get(building_id).is_none());
    for (id, _) in coords {
        assert!(sim.anim(id).is_none());
    }
}

/// AnimClass::AI's Next= switch (`0x004247F3..0x00424925`) sets Value to
/// the new type's Start (`0x0042491F`) and leaves the step as it was.
#[test]
fn next_takes_the_new_start_as_its_value_and_keeps_the_step() {
    let rules = runtime_rules(
        "[A]\nReverse=yes\nNext=A_NEXT\n[A_NEXT]\nStart=3\n",
        &[("A", 10), ("A_NEXT", 10)],
    );
    let mut sim = Simulation::with_seed(0);
    let type_id = sim.interner.intern("A");
    let id = sim
        .spawn_anim_at_world(
            &rules,
            runtime_descriptor(type_id, 0),
            AnimWorldCoord {
                x: 2816,
                y: 2944,
                z: 0,
            },
        )
        .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime.frame_step, -1);
    sim.switch_anim_type(id, "A_NEXT", &rules, None, &mut FrameEffects::default())
        .unwrap();
    let runtime = &sim.anim(id).unwrap().runtime;
    assert_eq!((runtime.current_frame, runtime.frame_step), (3, -1));
}

#[test]
fn animation_display_owner_histories_match_native_and_survive_save() {
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::display_layers::DisplayLayer;
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/display_anim_owner.json",
    ))
    .unwrap();
    assert_eq!(rows.len(), 24);
    for row in rows {
        let input = &row["input"];
        let layer_name = match input["layer"].as_i64().unwrap() {
            -1 => "None",
            0 => "Underground",
            1 => "Surface",
            2 => "Ground",
            3 => "Air",
            4 => "Top",
            _ => unreachable!(),
        };
        let rules = runtime_rules(
            &format!(
                "[A]\nLayer={layer_name}\nYSortAdjust={}\n[A_NEXT]\nLayer={layer_name}\nYSortAdjust=999\n",
                input["adjust"].as_i64().unwrap()
            ),
            &[("A", 10), ("A_NEXT", 10)],
        );
        let mut sim = Simulation::with_seed(0);
        let owner_id = sim.substrate.allocate_stable_id();
        let owner_name = sim.interner.intern("HOUSE");
        let type_ref = sim.interner.intern("UNIT");
        let mut owner = GameEntity::new_at_frame_zero_for_test(
            owner_id,
            10,
            10,
            0,
            0,
            owner_name,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        owner.position.set_sub_x_for_test(SimFixed::from_num(128));
        owner.position.set_sub_y_for_test(SimFixed::from_num(128));
        owner.position.set_exact_z_leptons_for_test(Some(0));
        sim.substrate.entities.insert(owner);
        let type_id = sim.interner.intern("A");
        let id = sim
            .spawn_anim_at_world(
                &rules,
                runtime_descriptor(type_id, 0),
                AnimWorldCoord {
                    x: 2816,
                    y: 2944,
                    z: 300,
                },
            )
            .unwrap();
        // Native corpus deliberately has type+340=999 and a different
        // retained instance+104. Exercise the real Next type transition.
        sim.switch_anim_type(id, "A_NEXT", &rules, None, &mut FrameEffects::default())
            .unwrap();
        assert_eq!(
            sim.anim(id).unwrap().display.y_sort_adjust,
            input["adjust"].as_i64().unwrap() as i32
        );
        sim.substrate
            .mark_anim_display(id, input["marked"].as_bool().unwrap());
        for observation in row["observations"].as_array().unwrap() {
            match observation["op"].as_str().unwrap() {
                "submit" => sim.submit_anim_display(id, &rules),
                "attach" => {
                    assert!(sim.set_anim_owner_object(id, Some(owner_id), &rules));
                }
                "detach" => {
                    assert!(sim.set_anim_owner_object(id, None, &rules));
                }
                "move_owner" => {
                    let owner = sim.substrate.entities.get_mut(owner_id).unwrap();
                    owner.position.set_rx_for_test(3000 / 256);
                    owner.position.set_ry_for_test(3100 / 256);
                    owner
                        .position
                        .set_sub_x_for_test(SimFixed::from_num(3000 % 256));
                    owner
                        .position
                        .set_sub_y_for_test(SimFixed::from_num(3100 % 256));
                    owner.position.set_exact_z_leptons_for_test(Some(700));
                }
                "expire" => {
                    assert!(sim.anim_pointer_expired(
                        id,
                        owner_id,
                        crate::rules::ruleset::RuleSet::empty_for_test()
                    ));
                }
                other => panic!("unexpected {other}"),
            }
            let expected = &observation["state"];
            let anim = sim.anim(id).unwrap();
            let coord = |key: &str| AnimWorldCoord {
                x: expected[key][0].as_i64().unwrap() as i32,
                y: expected[key][1].as_i64().unwrap() as i32,
                z: expected[key][2].as_i64().unwrap() as i32,
            };
            assert_eq!(
                anim.world_coord,
                coord("stored"),
                "{}: {observation}",
                input["name"]
            );
            assert_eq!(sim.anim_absolute_coord(id), Some(coord("absolute")));
            assert_eq!(
                anim.owner_entity.is_some(),
                expected["owner"].as_bool().unwrap()
            );
            assert_eq!(
                anim.display.marked_on_map,
                expected["marked"].as_bool().unwrap()
            );
            assert_eq!(
                anim.runtime.inactive,
                expected["detached"].as_bool().unwrap()
            );
            assert_eq!(
                anim_display_sort_key(anim, &sim.substrate.entities),
                expected["y_sort"].as_i64().unwrap() as i32
            );
            let queried = expected["queried_layer"].as_i64().unwrap();
            let queried = u8::try_from(queried)
                .ok()
                .and_then(DisplayLayer::from_index);
            assert_eq!(sim.anim_display_layer(id, &rules), queried);
            for layer in 0..5 {
                let expected_ids = if expected["layers"][layer] == 0 {
                    vec![]
                } else {
                    vec![id]
                };
                assert_eq!(
                    sim.substrate
                        .display
                        .members(DisplayLayer::from_index(layer as u8).unwrap()),
                    expected_ids
                );
            }
            let bytes = GameSnapshot::save(&sim, 0, 0, "anim-display", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            restored.restore_after_snapshot_load().unwrap();
            assert_eq!(
                restored.state_hash(),
                sim.state_hash(),
                "{}: save {observation}",
                input["name"]
            );
            sim = restored;
        }
        sim.destroy_anim(id, &rules);
        assert_eq!(sim.substrate.display.layer_of(id), None);
        sim.process_pending_delete();
        assert!(sim.anim(id).is_none());
    }
}

#[test]
fn gsi_13_04_signed_constructor_loop_preserves_negative_one_distinction() {
    assert_eq!(native_loop_remaining(0, 1), 1);
    assert_eq!(native_loop_remaining(1, 1), 1);
    assert_eq!(native_loop_remaining(2, 1), 2);
    assert_eq!(native_loop_remaining(-1, 1), u8::MAX);
    assert_eq!(native_loop_remaining(128, 2), 1);
    assert_eq!(native_loop_remaining(-1, -1), u8::MAX);
    assert_eq!(native_loop_remaining(-1, 255), 1);
}

#[test]
fn trailer_separation_spawns_on_signed_multiples_and_never_divides_by_zero() {
    assert!(!trailer_cadence_matches(0, 0, false));
    assert!(trailer_cadence_matches(7, 1, false));
    assert!(trailer_cadence_matches(6, 3, false));
    assert!(!trailer_cadence_matches(7, 3, false));
    assert!(trailer_cadence_matches(6, -3, false));
    assert!(!trailer_cadence_matches(7, -3, false));
    // Ares spawns no trailer below a separation of 1.
    assert!(!trailer_cadence_matches(6, -3, true));
    assert!(trailer_cadence_matches(6, 3, true));
}

fn damage_fire_fixture(can_be_occupied: bool) -> (Simulation, RuleSet, u64) {
    damage_fire_fixture_with_strength(can_be_occupied, 100)
}

fn damage_fire_fixture_with_strength(
    can_be_occupied: bool,
    strength: i32,
) -> (Simulation, RuleSet, u64) {
    damage_fire_fixture_with_offsets(
        can_be_occupied,
        strength,
        "DamageFireOffset0=-24,-1\nDamageFireOffset1=64,36\n",
    )
}

/// `offsets` are TESTART's `DamageFireOffsetN=` lines.
fn damage_fire_fixture_with_offsets(
    can_be_occupied: bool,
    strength: i32,
    offsets: &str,
) -> (Simulation, RuleSet, u64) {
    let rules_ini = IniFile::from_str(&format!(
        "[BuildingTypes]\n0=TESTBLD\n\n\
         [TESTBLD]\nStrength={strength}\nImage=TESTART\nCanBeOccupied={}\n\n\
         [General]\nDamageFireTypes=FIRE01,FIRE02,FIRE03\n\n\
         [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n",
        if can_be_occupied { "yes" } else { "no" },
    ));
    let art_ini = IniFile::from_str(&format!(
        "[TESTART]\nFoundation=4x4\n{offsets}\n\
         [FIRE01]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireBig\n\n\
         [FIRE02]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireMed\n\n\
         [FIRE03]\nRate=450\nLoopCount=-1\nStartSound=BuildingFireSmall\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
        .expect("damage-fire rules and ART");
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("FIRE01", 30);
    art.bind_anim_frame_count_for_test("FIRE02", 64);
    art.bind_anim_frame_count_for_test("FIRE03", 30);
    rules.install_art_fixture(art);

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("A");
    let type_ref = sim.interner.intern("TESTBLD");
    for name in ["FIRE01", "FIRE02", "FIRE03"] {
        sim.interner.intern(name);
    }
    let id = sim.substrate.allocate_stable_id();
    let mut building = GameEntity::new_at_frame_zero_for_test(
        id,
        10,
        10,
        0,
        0,
        owner,
        Health::new(100),
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building
        .seldom
        .type_facts
        .set_foundation_for_test("4x4".to_string().into());
    sim.substrate.entities.insert(building);
    sim.reveal(id);
    (sim, rules, id)
}

#[test]
fn original_health_ratio_corpus_drives_damage_fire_update() {
    for row in crate::sim::health_ratio_fixture::rows() {
        for occupied in [false, true] {
            let (mut sim, mut rules, id) =
                damage_fire_fixture_with_strength(occupied, row.input.strength);
            rules.general.condition_yellow = row.input.yellow();
            rules.general.condition_red = row.input.red();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .health
                .set_for_test(row.input.current);
            sim.update_building_damage_fire(id, &rules);
            let building = sim.substrate.entities.get(id).unwrap();
            let expected = if occupied {
                row.output.damage_fire_occupied
            } else {
                row.output.damage_fire_ordinary
            };
            assert_eq!(
                building.damage_fires.active, expected,
                "{row:?}, occupied={occupied}"
            );
            assert_eq!(
                building.damage_fires.anim_ids.iter().flatten().count(),
                if expected { 2 } else { 0 },
                "{row:?}"
            );
        }
    }
}

/// 43C124..43C137: an authored (0,0) offset ends the fires, as the
/// reader's end marker does, so later offsets get none.
#[test]
fn a_zero_damage_fire_offset_ends_the_fires() {
    let (mut sim, rules, id) = damage_fire_fixture_with_offsets(
        false,
        100,
        "DamageFireOffset0=-24,-1\nDamageFireOffset1=0,0\nDamageFireOffset2=64,36\n",
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .health
        .set_for_test(10);
    sim.update_building_damage_fire(id, &rules);
    let fires = &sim.substrate.entities.get(id).unwrap().damage_fires;
    assert!(fires.active);
    let slots: Vec<bool> = fires.anim_ids.iter().map(Option::is_some).collect();
    assert_eq!(
        slots,
        [true, false, false, false, false, false, false, false]
    );
}

/// Each `art_text` section is an AnimType, so the ordered Rules processor
/// reads its fixed-ART Type references (`Next=`, `TrailerAnim=`, ...) as
/// in production.
fn runtime_rules(art_text: &str, frame_counts: &[(&str, i32)]) -> RuleSet {
    let art_ini = IniFile::from_str(art_text);
    let animations: String = art_ini
        .section_names()
        .iter()
        .enumerate()
        .map(|(index, name)| format!("{index}={name}\n"))
        .collect();
    // SOUNDMD knows every sound the art names.
    let sounds: Vec<&str> = art_ini
        .section_names()
        .iter()
        .filter_map(|name| art_ini.section(name))
        .flat_map(|section| {
            ["StartSound", "Report", "StopSound"].map(|key| section.get_for_test(key))
        })
        .flatten()
        .collect();
    let mut rules = RuleSet::from_ini_with_fixed_art_and_sounds_for_test(
        &IniFile::from_str(&format!(
            "[General]\nDamageFireTypes=\n\n[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\n\
             [Animations]\n{animations}"
        )),
        &art_ini,
        &sounds,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for &(name, frames) in frame_counts {
        art.bind_anim_frame_count_for_test(name, frames);
    }
    rules.install_art_fixture(art);
    rules
}

fn runtime_descriptor(type_name: InternedId, delay: u16) -> AnimClassSpawnDescriptor {
    AnimClassSpawnDescriptor {
        type_name,
        rx: 0,
        ry: 0,
        sub_x: crate::util::fixed_math::SIM_ZERO,
        sub_y: crate::util::fixed_math::SIM_ZERO,
        z: 0,
        delay,
        loop_count: 1,
        draw_flags: TRAILER_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        use_cell_drawer: false,
        terrain_attached: false,
        draw_runtime: AnimDrawRuntime::default(),
        owner: None,
    }
}

/// A producer's `owner` attaches inside the constructor without Reveal's
/// own Display submit. Destroy follows the shared SetOwner(NULL) detach
/// and conceal sequence, leaving Display as those native calls do.
#[test]
fn an_owned_anim_leaves_display_as_construct_then_attach_does() {
    let rules = runtime_rules("[A]\nLayer=Ground\n", &[("A", 10)]);
    let coord = AnimWorldCoord {
        x: 2816,
        y: 2944,
        z: 0,
    };
    let scene = || {
        let mut sim = Simulation::with_seed(0);
        let house = sim.interner.intern("HOUSE");
        let unit = sim.interner.intern("UNIT");
        let ids: Vec<u64> = [(10, 10), (14, 3), (6, 9), (12, 12), (8, 15)]
            .into_iter()
            .map(|(rx, ry)| {
                let id = sim.substrate.allocate_stable_id();
                sim.substrate
                    .entities
                    .insert(GameEntity::new_at_frame_zero_for_test(
                        id,
                        rx,
                        ry,
                        0,
                        0,
                        house,
                        Health::new(100),
                        unit,
                        EntityCategory::Unit,
                        0,
                        5,
                        false,
                    ));
                sim.resubmit_entity_display_for_test(id);
                id
            })
            .collect();
        let type_id = sim.interner.intern("A");
        (sim, ids[3], type_id)
    };
    let layers = |sim: &Simulation| -> Vec<Vec<u64>> {
        (0..5)
            .map(|index| {
                let layer = crate::sim::world::display_layers::DisplayLayer::from_index(index);
                sim.substrate.display.members(layer.unwrap()).to_vec()
            })
            .collect()
    };

    let (mut native, owner, type_id) = scene();
    let id = native
        .spawn_anim_at_world(&rules, runtime_descriptor(type_id, 0), coord)
        .unwrap();
    assert!(native.set_anim_owner_object(id, Some(owner), &rules));
    let (mut sim, _, _) = scene();
    let descriptor = AnimClassSpawnDescriptor {
        owner: Some(owner),
        ..runtime_descriptor(type_id, 0)
    };
    assert_eq!(
        sim.spawn_anim_at_world(&rules, descriptor, coord).unwrap(),
        id
    );
    assert_eq!(sim.anim(id).unwrap().owner_entity, Some(owner));
    assert_eq!(
        sim.anim(id).unwrap().world_coord,
        native.anim(id).unwrap().world_coord
    );
    assert!(sim.anim(id).unwrap().display.marked_on_map);
    assert_eq!(layers(&sim), layers(&native));

    assert!(native.set_anim_owner_object(id, None, &rules));
    native.destroy_anim(id, &rules);
    sim.destroy_anim(id, &rules);
    assert_eq!(
        sim.anim(id).unwrap().world_coord,
        native.anim(id).unwrap().world_coord
    );
    assert_eq!(sim.anim(id).unwrap().owner_entity, None);
    assert_eq!(layers(&sim), layers(&native));
    assert_eq!(sim.substrate.display.layer_of(id), None);
}

/// Techno710443..71044E clears the matching retained animation pointer,
/// leaving Unit deployment state for the mission's next visit.
#[test]
fn deployment_attachment_expiry_clears_only_the_matching_pointer() {
    for detach_with_coords in [false, true] {
        let rules = runtime_rules("[DEPLOY]\nEnd=3\n", &[("DEPLOY", 3)]);
        let mut sim = Simulation::new();
        let owner_id = 100;
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("UNIT");
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            owner_id,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.set_unit_simple_deploy_for_test(false, true, false);
        sim.substrate.entities.insert(entity);
        let type_id = sim.interner.intern("DEPLOY");
        let other = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        let retained = sim
            .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap();
        for id in [other, retained] {
            sim.set_anim_owner_object(id, Some(owner_id), &rules);
        }
        sim.substrate
            .entities
            .get_mut(owner_id)
            .unwrap()
            .retain_deploy_anim(retained);
        for (id, expected) in [(other, Some(retained)), (retained, None)] {
            if detach_with_coords {
                assert_eq!(sim.detach_anim_from_owner(id, &rules), Some(owner_id));
            } else {
                assert!(sim.anim_pointer_expired(id, owner_id, &rules));
            }
            assert_eq!(sim.anim(id).unwrap().owner_entity, None);
            let entity = sim.substrate.entities.get(owner_id).unwrap();
            assert_eq!(entity.deploy_anim(), expected);
            assert!(entity.unit_deploying());
            assert!(!entity.is_fully_deployed());
        }
    }
}

/// SetOwnerObject424B50, destructor4228E0 and PointerExpired425150 have
/// distinct effects on the retained Object+84 byte and owner+60 links.
#[test]
fn attached_animation_transfer_and_expiry_preserve_shared_owner_state() {
    let rules = runtime_rules("[A]\nEnd=3\n", &[("A", 3)]);
    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    for owner in [100, 101] {
        sim.substrate
            .entities
            .insert(GameEntity::test_default(owner, "UNIT", "Americans", 10, 10));
    }
    let type_id = sim.interner.intern("A");
    let first = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();
    let second = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();
    for anim in [first, second] {
        assert!(sim.set_anim_owner_object(anim, Some(100), &rules));
    }
    // The native scan includes an inactive Anim while its CC still names
    // this owner; filtering to Logic/Display/live entries clears too early.
    sim.substrate
        .anims
        .get_mut(second)
        .unwrap()
        .runtime
        .inactive = true;
    assert!(sim.set_anim_owner_object(first, Some(101), &rules));
    assert!(sim.substrate.entities.get(100).unwrap().is_parachuted());
    assert!(sim.substrate.entities.get(101).unwrap().is_parachuted());

    sim.scalar_delete_anim(second);
    assert!(sim.anim(second).is_none());
    assert!(!sim.substrate.entities.get(100).unwrap().is_parachuted());
    sim.substrate
        .entities
        .hold_parachute_animation(101, first)
        .unwrap();
    assert!(sim.anim_pointer_expired(first, 101, &rules));
    assert_eq!(sim.anim(first).unwrap().owner_entity, None);
    let owner = sim.substrate.entities.get(101).unwrap();
    assert_eq!(
        owner.parachute_animation(),
        None,
        "owner+60 ran before unlink"
    );
    assert!(owner.is_parachuted(), "PointerExpired retains Object+84");
    sim.scalar_delete_anim(first);
    assert!(sim.anim(first).is_none());
    assert!(
        sim.substrate.entities.get(101).unwrap().is_parachuted(),
        "a later destructor cannot reconstruct the already-cleared owner"
    );

    // GameEntity serde is the production snapshot payload. Preserve the
    // orphaned raw flag, then let ObjectAI consume it in a subsequent fall.
    let cap = rules.general.parachute_max_fall_rate;
    let owner = sim.substrate.entities.get_mut(101).unwrap();
    owner.set_fall_rate_for_test(cap);
    let bytes = serde_json::to_vec(owner).unwrap();
    let mut restored: GameEntity = serde_json::from_slice(&bytes).unwrap();
    restored.lifecycle.set_in_limbo_for_test(false);
    restored.lifecycle.set_cell_marked_for_test(false);
    restored.position.put_z(512);
    restored.begin_bomb_fall();
    *sim.substrate.entities.get_mut(101).unwrap() = restored;
    sim.object_ai_fall(101, &rules, None, &mut FrameEffects::default())
        .unwrap();
    let owner = sim.substrate.entities.get(101).unwrap();
    assert!(owner.is_parachuted());
    assert_eq!(owner.parachute_animation(), None);
    assert_eq!(
        owner.fall_rate(),
        cap,
        "restored raw flag still selects canopy cap"
    );
}

/// The owners' counts answer SetOwnerObject424B79's and the destructor
/// 42291A's Anim array scan through each owner write, a load, and a
/// removal of an Anim that still names its owner.
#[test]
fn the_last_anim_naming_an_owner_releases_it_through_writes_loads_and_removals() {
    let rules = runtime_rules("[A]\nEnd=3\n", &[("A", 3)]);
    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    for owner in [100, 101] {
        sim.substrate
            .entities
            .insert(GameEntity::test_default(owner, "UNIT", "Americans", 10, 10));
    }
    let type_id = sim.interner.intern("A");
    let [a, b, c] = [0; 3].map(|_| {
        sim.spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
            .unwrap()
    });
    for (anim, owner) in [(a, 100), (b, 100), (c, 101)] {
        assert!(sim.set_anim_owner_object(anim, Some(owner), &rules));
    }
    let parachuted =
        |sim: &Simulation, owner| sim.substrate.entities.get(owner).unwrap().is_parachuted();
    sim.substrate.anims = crate::util::save_codec::deserialize(
        &crate::util::save_codec::serialize(&sim.substrate.anims).unwrap(),
    )
    .unwrap();
    assert!(sim.set_anim_owner_object(a, Some(101), &rules));
    assert!(parachuted(&sim, 100), "the loaded b still names 100");
    // PointerExpired's bare CC clear leaves b no longer naming 100.
    assert_eq!(sim.substrate.release_anim_owner_reference(b), Some(100));
    assert!(sim.set_anim_owner_object(c, Some(100), &rules));
    assert!(parachuted(&sim, 101), "a still names 101");
    assert!(sim.set_anim_owner_object(c, None, &rules));
    assert!(!parachuted(&sim, 100), "c was the last Anim naming 100");
    assert!(sim.set_anim_owner_object(b, Some(101), &rules));
    assert!(sim.substrate.anims.remove(b).is_some());
    sim.scalar_delete_anim(a);
    assert!(
        !parachuted(&sim, 101),
        "a was the last stored Anim naming 101"
    );
}

#[test]
fn deployment_attachment_retirement_without_rules_keeps_absolute_sound_position() {
    let rules = runtime_rules("[DEPLOY]\nEnd=3\nStopSound=DeployDone\n", &[("DEPLOY", 3)]);
    let mut sim = Simulation::new();
    let owner_id = 100;
    let mut entity = GameEntity::test_default(owner_id, "UNIT", "Americans", 10, 10);
    entity.set_owner_for_test(sim.interner.intern("Americans"));
    entity.set_type_ref_for_test(sim.interner.intern("UNIT"));
    sim.substrate.entities.insert(entity);
    let type_id = sim.interner.intern("DEPLOY");
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();
    sim.set_anim_owner_object(id, Some(owner_id), &rules);
    let entity = sim.substrate.entities.get_mut(owner_id).unwrap();
    entity.retain_deploy_anim(id);
    entity.position.set_rx_for_test(entity.position.rx() + 1);
    let absolute = sim.anim_absolute_coord(id).unwrap();
    assert_ne!(sim.anim(id).unwrap().world_coord, absolute);

    sim.destroy_anim_with_context(id, crate::rules::ruleset::RuleSet::empty_for_test());

    let anim = sim.anim(id).unwrap();
    assert_eq!(anim.world_coord, absolute);
    assert_eq!(anim.owner_entity, None);
    assert_eq!(
        sim.substrate.entities.get(owner_id).unwrap().deploy_anim(),
        None
    );
    assert!(sim.substrate.pending_delete.contains_object(&id));
    assert!(sim.sound_events.iter().any(|event| matches!(event,
        SimSoundEvent::AnimationStopped { anim_id, world, .. }
            if *anim_id == id && *world == absolute
    )));
}

#[test]
fn native_anim_boundary_and_reset_vectors() {
    let golden: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/anim_oracle/boundary.json")).unwrap();
    let rules = runtime_rules("[TEST]\nEnd=64\n", &[("TEST", 64)]);
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("TEST");
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();
    let template = sim.anim(id).unwrap().clone();
    let mut config = rules.art().anim_runtime_config("TEST").unwrap().clone();
    let rows = golden["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 13_312);
    for row in rows {
        let name = row[0].as_str().unwrap();
        let bounds = golden["bounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bounds| bounds[0].as_str() == Some(name))
            .unwrap();
        let mut anim = template.clone();
        config.start = bounds[1].as_i64().unwrap() as i32;
        config.loop_start = bounds[2].as_i64().unwrap() as i32;
        anim.effective_loop_end = bounds[3].as_i64().unwrap() as i32;
        anim.effective_end = bounds[4].as_i64().unwrap() as i32;
        anim.runtime.loop_remaining = row[1].as_u64().unwrap() as u8;
        anim.runtime.current_frame = row[2].as_i64().unwrap() as i32;
        config.shadow = row[3].as_u64().unwrap() != 0;
        config.reverse = row[4].as_u64().unwrap() != 0;
        anim.runtime.constructor_reverse = row[5].as_u64().unwrap() != 0;
        config.ping_pong = row[6].as_u64().unwrap() != 0;
        anim.runtime.frame_step = row[7].as_i64().unwrap() as i32;
        let expected = match row[8].as_str().unwrap() {
            "continue" => AnimBoundary::Continue,
            "bounce" => AnimBoundary::Bounce,
            "boundary_loop" => AnimBoundary::Loop,
            "boundary_terminal" => AnimBoundary::Complete,
            other => panic!("unknown native decision {other}"),
        };
        assert_eq!(advance_anim_boundary(&mut anim, &config), expected, "{row}");
        assert_eq!(
            anim.runtime.frame_step,
            row[9].as_i64().unwrap() as i32,
            "{row}"
        );
        assert_eq!(
            anim.runtime.loop_remaining,
            row[10].as_u64().unwrap() as u8,
            "{row}"
        );
        assert_eq!(
            anim.runtime.current_frame,
            row[11].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn combat_anim_shadow_endpoint_retires_in_runtime_and_survives_restore() {
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::TickLane;

    fn run(restore: bool) -> Vec<u64> {
        // Explicit ART override on the existing combat producer. Bounds
        // match native stock_FDHD rows; Rate=900/Normalized=no selects one
        // frame per visit for this fixture. This is not a claim that retail
        // NAMISL is produced by combat or that its Building host is bound.
        let rules = runtime_rules(
            "[TWLT036]\nStart=16\nLoopStart=0\nLoopEnd=32\nEnd=31\nShadow=yes\nRate=900\nNormalized=no\n\n\
             [NEIGHBOR]\nEnd=64\nLoopCount=-1\nRate=900\nNormalized=no\n",
            &[("TWLT036", 64), ("NEIGHBOR", 64)],
        );
        // Native load resets Scenario RNG to seed zero. Keep this fixture
        // on that stream so endpoint continuation is comparable across load.
        let mut sim = Simulation::with_seed(0);
        sim.session
            .set_map_name_for_test("anim-boundary".to_string());
        let explosion_type = sim.interner.intern("TWLT036");
        let neighbor_type = sim.interner.intern("NEIGHBOR");
        let explosion = sim
            .spawn_combat_explosion_anim(
                &rules,
                explosion_type,
                7,
                9,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
                0,
            )
            .unwrap();
        let neighbor = sim
            .spawn_combat_explosion_anim(
                &rules,
                neighbor_type,
                7,
                9,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
                0,
            )
            .unwrap();
        assert_eq!(sim.live_object_order_snapshot(), vec![explosion, neighbor]);
        assert_eq!(sim.anim(explosion).unwrap().runtime.rate_reload, 1);
        let mut resources = SimResources::empty();
        resources.rules = rules;
        let mut runtime = SimRuntime {
            simulation: sim,
            resources: std::sync::Arc::new(resources),
        };
        let mut hashes = Vec::new();
        for frame in 0..=17 {
            assert_eq!(runtime.simulation.session.binary_frame(), frame);
            if restore && frame == 8 {
                assert_eq!(
                    runtime.simulation.rng_state().scenario,
                    Simulation::with_seed(0).rng_state().scenario,
                    "endpoint fixture has consumed no Scenario RNG"
                );
                let before = runtime.simulation.state_hash();
                let rules_hash = runtime.resources.rules.simulation_config_hash();
                let bytes = GameSnapshot::save_validated(
                    &runtime.simulation,
                    0x1234,
                    rules_hash,
                    "Anim boundary fixture",
                    0,
                );
                let mut restored =
                    GameSnapshot::load_validated(&bytes, 0x1234, rules_hash, "anim-boundary")
                        .expect("valid live Anim snapshot")
                        .sim;
                restored
                    .restore_after_snapshot_load()
                    .expect("live Anim references restore");
                restored.retain_process_state_for_load(
                    crate::sim::world::SnapshotProcessState::Live(&runtime.simulation),
                );
                assert_eq!(restored.rng_state(), runtime.simulation.rng_state());
                runtime.replace_simulation(restored);
                assert_eq!(runtime.simulation.state_hash(), before);
            }
            let output = runtime
                .advance_frame(
                    &[],
                    67,
                    TickLane::Ordinary,
                    &mut crate::sim::world::FrameEffects::default(),
                )
                .expect("fixture frame must complete");
            assert!(output.tick.frame_committed);
            if frame < 16 {
                assert_eq!(
                    runtime
                        .simulation
                        .anim(explosion)
                        .unwrap()
                        .runtime
                        .current_frame,
                    frame as i32
                );
                assert_eq!(
                    runtime.simulation.live_object_order_snapshot(),
                    vec![explosion, neighbor]
                );
            } else {
                // Native golden stock_FDHD, loop1, stage16, Shadow1,
                // reverse0/constructor_reverse0/pingpong0 is terminal.
                // The actual live-vector pass removes the first object;
                // its shifted neighbor is skipped until the next pass.
                assert!(runtime.simulation.anim(explosion).is_none());
                assert_eq!(
                    runtime.simulation.live_object_order_snapshot(),
                    vec![neighbor]
                );
            }
            let neighbor_stage = if frame < 16 { frame } else { frame - 1 };
            assert_eq!(
                runtime
                    .simulation
                    .anim(neighbor)
                    .unwrap()
                    .runtime
                    .current_frame,
                neighbor_stage as i32
            );
            hashes.push(runtime.simulation.state_hash());
        }
        hashes
    }
    assert_eq!(run(false), run(true));
}

#[test]
fn gsi_04_12_anim_make_infantry_ini_preserves_native_default_and_signed_value() {
    let rules = runtime_rules(
        "[DEFAULT]\nRate=900\nEnd=1\n\n[EXPLICIT]\nRate=900\nEnd=1\nMakeInfantry=-2\n",
        &[("DEFAULT", 1), ("EXPLICIT", 1)],
    );

    assert_eq!(
        rules
            .art()
            .anim_runtime_config("DEFAULT")
            .unwrap()
            .make_infantry,
        -1
    );
    assert_eq!(
        rules
            .art()
            .anim_runtime_config("EXPLICIT")
            .unwrap()
            .make_infantry,
        -2
    );
}

/// `AnimClass::AI @ 0x00423AC0`: the loop boundary stores the signed
/// `RandomLoopDelay=` draw (`0x004247B1..0x004247E5`) and the countdown
/// tests `!= 0` (`0x00424386`), so a negative pause holds the anim.
#[test]
fn a_negative_random_loop_delay_holds_the_anim() {
    let rules = runtime_rules(
        "[LOOPER]\nRate=900\nEnd=2\nLoopCount=-1\nRandomLoopDelay=-3,-3\n",
        &[("LOOPER", 2)],
    );
    let mut sim = Simulation::new();
    let type_name = sim.interner.intern("LOOPER");
    let id = sim
        .spawn_anim_object(
            &rules,
            AnimClassSpawnDescriptor {
                loop_count: -1,
                ..runtime_descriptor(type_name, 0)
            },
        )
        .expect("bound art constructs");
    for frame in 1..40 {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(
            id,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    }
    let delay = sim.anim(id).unwrap().runtime.delay_remaining;
    assert!(delay < -3, "the pause counts on down, got {delay}");
}

/// The whole point of routing combat explosions through `AnimStore`: the
/// verified constructor row from `BulletClass::DetonateAtCoord 0x00469C93`,
/// and the `Report=` that only `AnimClass::Start @ 0x00424CE0` can play.
/// `AnimClass::AI @ 0x00423AC0`: a constructor delay keeps `Start` out of
/// the constructor; the AI visit that counts the delay to zero calls
/// `AnimClass::Start @ 0x00424CE0` (`0x004243A1`) and returns, so the
/// `Report=` of a delayed anim plays exactly then, once.
#[test]
fn delayed_anim_plays_its_report_when_the_delay_expires() {
    let rules = runtime_rules(
        "[TWLT036]\nTranslucent=yes\nReport=Explosion06\nEnd=8\n",
        &[("TWLT036", 8)],
    );
    let mut sim = Simulation::new();
    let type_name = sim.interner.intern("TWLT036");
    let descriptor = AnimClassSpawnDescriptor {
        delay: 2,
        ..AnimClassSpawnDescriptor::new(
            type_name,
            7,
            9,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            0,
        )
    };
    let id = sim
        .spawn_anim_object(&rules, descriptor)
        .expect("bound art constructs");
    let starts = |sim: &Simulation| {
        sim.sound_events
            .iter()
            .filter(|event| {
                matches!(event, SimSoundEvent::AnimationStarted { anim_id, .. } if *anim_id == id)
            })
            .count()
    };
    assert_eq!(
        starts(&sim),
        0,
        "a delayed constructor does not reach Start"
    );
    assert!(!sim.anim(id).unwrap().start_sound_active);

    // The first AI visit only clears the first-AI guard.
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 1);
    assert_eq!(starts(&sim), 0);

    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime.delay_remaining, 0);
    assert_eq!(starts(&sim), 1, "the expiring visit runs Start");
    assert_eq!(
        sim.anim(id).unwrap().runtime.current_frame,
        0,
        "and returns before advancing a frame"
    );

    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(starts(&sim), 1, "Start runs once per delay");
}

/// A presentation anim draws where gamemd's Display would hold it as an
/// ordinary anim: on Ground by its GetYSort key (Anim422BC0, inserted
/// by Ground551A90) among the members, not after all of them.
#[test]
fn a_ground_presentation_anim_sorts_among_the_ground_members() {
    use crate::sim::world::display_layers::DisplayLayer;
    let rules = runtime_rules("[PUFF]\nEnd=8\nLayer=Ground\n", &[("PUFF", 8)]);
    let zero = crate::util::fixed_math::SIM_ZERO;
    let mut sim = Simulation::new();
    sim.present_effect_anims();
    let type_id = sim.interner.intern("PUFF");
    let near = sim
        .spawn_combat_explosion_anim(&rules, type_id, 1, 1, zero, zero, 0, 0)
        .unwrap();
    let far = sim
        .spawn_combat_explosion_anim(&rules, type_id, 10, 10, zero, zero, 0, 0)
        .unwrap();
    // X+Y 2000 lies between the kept anims' cells 1,1 and 10,10.
    let point = crate::sim::projectile::ProjectileCoord::new(1000, 1000, 0);
    let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
    crate::sim::world::damage_consequences::admit_explosion_effect(&mut sim, &rules, packet);
    let (puff, layer) = sim.presentation_anims().next().unwrap();
    assert_eq!(layer, DisplayLayer::GROUND);
    let ground: Vec<u64> = sim
        .picture_draw_order(Some(&rules))
        .into_iter()
        .filter(|&(layer, _)| layer == DisplayLayer::GROUND)
        .map(|(_, id)| id)
        .collect();
    assert_eq!(ground, [near, puff, far]);
}

/// The impact packet keeps nothing, so a picture-only impact is a
/// presentation anim; `spawn_combat_explosion_anim`'s callers may keep
/// theirs (the nuke awaits its NUKEBALL), so it makes an ordinary one.
#[test]
fn only_an_unkept_picture_only_impact_is_a_presentation_anim() {
    let rules = runtime_rules("[PUFF]\nEnd=8\n", &[("PUFF", 8)]);
    let point = crate::sim::projectile::ProjectileCoord::new(1000, 1000, 0);
    // A new world makes it in native order: in the Anim array.
    let mut native = Simulation::new();
    let type_id = native.interner.intern("PUFF");
    let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
    crate::sim::world::damage_consequences::admit_explosion_effect(&mut native, &rules, packet);
    assert_eq!(native.anims().count(), 1, "in the Anim array, as gamemd's");
    assert_eq!(native.presentation_anims().count(), 0);

    let mut sim = Simulation::new();
    sim.present_effect_anims();
    let type_id = sim.interner.intern("PUFF");
    let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
    crate::sim::world::damage_consequences::admit_explosion_effect(&mut sim, &rules, packet);
    assert_eq!(sim.anims().count(), 0, "not in the Anim array");
    assert_eq!(sim.presentation_anims().count(), 1);

    let zero = crate::util::fixed_math::SIM_ZERO;
    let kept = sim
        .spawn_combat_explosion_anim(&rules, type_id, 3, 3, zero, zero, 0, 0)
        .unwrap();
    assert!(!sim.anim(kept).unwrap().is_presentation());
    assert_eq!(sim.anims().count(), 1);
}

#[test]
fn signed_impact_packet_keeps_exact_xy_through_explosion_admission() {
    // Sticky= keeps it an ordinary anim (`ArtRegistry::anim_presentation_only`).
    let rules = runtime_rules("[TWLT036]\nEnd=8\nSticky=yes\n", &[("TWLT036", 8)]);
    for (x, y) in [(-257, -511), (-1, 1811), (-128_128, 5248), (2694, 5253)] {
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern("TWLT036");
        let point = crate::sim::projectile::ProjectileCoord::new(x, y, 209);
        let packet = crate::sim::combat::detonation_anim::placed_effect(type_id, point);
        let packed = (packet.rx, packet.ry, packet.sub_x, packet.sub_y);
        crate::sim::world::damage_consequences::admit_explosion_effect(&mut sim, &rules, packet);
        let ids = sim.anims().map(|(id, _)| *id).collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            1,
            "the real constructor admits the impact packet"
        );
        let coord = sim.anim_absolute_coord(ids[0]).unwrap();
        assert_eq!(coord, AnimWorldCoord { x, y, z: 209 });
        let (rx, ry, sub_x, sub_y, _) = coord.to_cell_sub_z();
        assert_eq!((rx, ry, sub_x, sub_y), packed);
    }
}

#[test]
fn combat_explosion_z_adjust_and_owner_reach_the_anim() {
    let rules = runtime_rules("[TWLT036]\nEnd=8\n", &[("TWLT036", 8)]);
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("TWLT036");
    let house = sim.interner.intern("Americans");
    let id = sim
        .spawn_combat_explosion_anim_with_z_adjust(
            &rules,
            type_id,
            1,
            1,
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ZERO,
            0,
            0,
            7,
            Some(house),
            true,
        )
        .unwrap();
    let anim = sim.anim(id).unwrap();
    assert_eq!(anim.z_adjust, 7);
    assert_eq!(anim.owner_house(), Some(house));
}

#[test]
fn combat_explosion_uses_the_verified_constructor_row_and_plays_report() {
    // TWLT036 is the stock AP-shell explosion: Report=Explosion06,
    // Translucent=yes, no Rate= (so one logic frame per image frame).
    let rules = runtime_rules(
        "[TWLT036]\nTranslucent=yes\nReport=Explosion06\nEnd=8\n",
        &[("TWLT036", 8)],
    );
    let mut sim = Simulation::new();
    let type_name = sim.interner.intern("TWLT036");

    let id = sim
        .spawn_combat_explosion_anim(
            &rules,
            type_name,
            7,
            9,
            crate::util::fixed_math::SimFixed::from_num(128),
            crate::util::fixed_math::SimFixed::from_num(64),
            3,
            3 * LEVEL_HEIGHT_LEPTONS,
        )
        .expect("bound explosion art constructs an AnimClass");

    let anim = sim.anim(id).expect("explosion is a registered AnimObject");
    assert_eq!(anim.draw_flags, COMBAT_EXPLOSION_DRAW_FLAGS);
    assert_eq!(anim.z_adjust, COMBAT_EXPLOSION_Z_ADJUST);
    assert_eq!(anim.runtime.delay_remaining, 0);
    assert!(!anim.runtime.constructor_reverse);
    assert_eq!(anim.runtime.current_frame, 0);
    assert_eq!(anim.runtime.loop_remaining, 1);
    assert_eq!(anim.owner_entity, None);

    // `delay == 0` means the constructor itself reached Start, so the
    // sound is already out before the first AI visit.
    assert!(anim.start_sound_active);
    assert!(
        sim.sound_events.iter().any(|event| matches!(
            event,
            SimSoundEvent::AnimationStarted { anim_id, sound_id, .. }
                if *anim_id == id && sound_id.name_in(&rules) == "EXPLOSION06"
        )),
        "explosion must emit its art `Report=`, got {:?}",
        sim.sound_events
    );

    // The level-keyed spawn coordinate decomposes back to the same cell,
    // sub-cell and level.
    let coord = sim.anim_absolute_coord(id).expect("absolute coordinate");
    assert_eq!(
        coord.to_cell_sub_z(),
        (
            7,
            9,
            crate::util::fixed_math::SimFixed::from_num(128),
            crate::util::fixed_math::SimFixed::from_num(64),
            3
        )
    );
}

/// A name with no registered AnimType declines the spawn rather than
/// panic or block the shot. Retail's art-less names are registered: rules
/// processing mints them as native does, and a minted type without an art
/// section (`gtpowexp` and `tstlexp` from the power plants' `Explosion=`)
/// keeps `End=0` because `AnimTypeClass::ReadINI @ 0x00427D00` bails
/// before the image loader, so its anim is constructed and dies unseen
/// (`ArtRegistry::bind_anim_class_assets`).
#[test]
fn combat_explosion_with_unbound_art_declines_instead_of_panicking() {
    let rules = runtime_rules("[TWLT036]\nEnd=8\n", &[("TWLT036", 8)]);
    let mut sim = Simulation::new();
    let missing = sim.interner.intern("MININUKE - ADDED 11/30");
    assert!(
        sim.spawn_combat_explosion_anim(
            &rules,
            missing,
            1,
            1,
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ZERO,
            0,
            0,
        )
        .is_none()
    );
    assert_eq!(sim.substrate.anims.len(), 0);
    assert!(sim.sound_events.is_empty());
}

#[test]
fn gsi_13_04_draw_and_terrain_attachment_state_roundtrip_and_hash() {
    let rules = runtime_rules("[DRAW]\nRate=900\nEnd=1\n", &[("DRAW", 1)]);
    let mut sim = Simulation::new();
    let mut descriptor = runtime_descriptor(sim.interner.intern("DRAW"), 0);
    descriptor.draw_runtime = AnimDrawRuntime {
        hidden: false,
        special_hidden: true,
        translucency_ramp: 6,
        forced_translucent: true,
    };
    descriptor.use_cell_drawer = true;
    descriptor.terrain_attached = true;
    let draw_runtime = descriptor.draw_runtime;
    let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
    assert_eq!(sim.anim(id).unwrap().draw_runtime, draw_runtime);

    let serialized = crate::util::save_codec::serialize(&sim.substrate.anims).unwrap();
    let restored: AnimStore = crate::util::save_codec::deserialize(&serialized).unwrap();
    assert_eq!(restored.get(id).unwrap().draw_runtime, draw_runtime);
    assert!(restored.get(id).unwrap().use_cell_drawer);
    assert!(restored.get(id).unwrap().terrain_attached);

    let before = sim.state_hash();
    sim.substrate.anims.get_mut(id).unwrap().terrain_attached = false;
    assert_ne!(sim.state_hash(), before);
}

/// The store saves as the former derived newtype `AnimStore(map)` (the
/// touch log is never saved), and every mutable hand-out is noted for the
/// pointer-expiry listener index.
#[test]
fn anim_store_saves_as_before_and_notes_every_hand_out() {
    #[derive(Serialize)]
    #[serde(rename = "AnimStore")]
    struct Former<'a>(&'a BTreeMap<AnimId, AnimObject>);
    let rules = runtime_rules("[DRAW]\nRate=900\nEnd=1\n", &[("DRAW", 1)]);
    let mut sim = Simulation::new();
    let descriptor = runtime_descriptor(sim.interner.intern("DRAW"), 0);
    let id = sim.spawn_anim_object(&rules, descriptor).unwrap();
    let store = &mut sim.substrate.anims;
    let former = Former(&store.anims);
    assert_eq!(
        crate::util::save_codec::serialize(&*store).unwrap(),
        crate::util::save_codec::serialize(&former).unwrap()
    );
    assert_eq!(
        serde_json::to_string(&*store).unwrap(),
        serde_json::to_string(&former).unwrap()
    );
    let bytes = crate::util::save_codec::serialize(&*store).unwrap();
    let restored: AnimStore = crate::util::save_codec::deserialize(&bytes).unwrap();
    assert_eq!(
        crate::util::save_codec::serialize(&restored).unwrap(),
        bytes
    );

    // Nothing has been derived from a fresh store yet.
    let keys = AnimTouchReader::ExpiryListeners;
    assert_eq!(store.take_touched(keys), Touched::All);
    assert!(store.get(id).is_some());
    assert_eq!(store.take_touched(keys), Touched::Ids(Vec::new()));
    store.get_mut(id).unwrap().z_adjust += 1;
    let removed = store.remove(id).unwrap();
    store.insert(removed);
    assert_eq!(store.take_touched(keys), Touched::Ids(vec![id]));
    assert_eq!(store.clone().take_touched(keys), Touched::All);
    let _ = store.values_mut().count();
    assert_eq!(store.take_touched(keys), Touched::All);
}

#[test]
fn runtime_anim_retains_native_identity_and_spends_it_before_asset_failure() {
    let rules = runtime_rules("[LIVE]\nRate=900\nEnd=2\n", &[("LIVE", 2)]);
    let mut sim = Simulation::new();
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(1000));
    let live = sim.interner.intern("LIVE");
    let missing = sim.interner.intern("UNBOUND");
    let rng = sim.scenario_rng.logical_state();
    assert!(
        sim.spawn_anim_object(&rules, runtime_descriptor(missing, 0))
            .is_err()
    );
    assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(live, 0))
        .unwrap();
    assert_eq!(sim.anim(id).unwrap().native_unique_id, 1002);
    assert_ne!(id as i32, 1002, "stable handles have an independent owner");
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

/// A local anim skips its constructor stream exactly when the constructor
/// would neither draw nor return a draw.
#[test]
fn only_random_rate_bouncer_and_meteor_constructors_draw() {
    let rules = runtime_rules(
        "[FLASH]\nEnd=2\n[RATED]\nRandomRate=50,150\nEnd=2\n\
         [BOUNCY]\nBouncer=yes\nEnd=2\n[ROCK]\nIsMeteor=yes\nEnd=2\n",
        &[("FLASH", 2), ("RATED", 2), ("BOUNCY", 2), ("ROCK", 2)],
    );
    let coord = AnimWorldCoord {
        x: 128,
        y: 128,
        z: 20,
    };
    for (name, draws) in [
        ("FLASH", false),
        ("RATED", true),
        ("BOUNCY", true),
        ("ROCK", true),
    ] {
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(super::spawn::constructor_draws(config), draws, "{name}");
        let mut rng = crate::sim::rng::SimRng::new(7);
        let before = rng.logical_state();
        let taken = anim_constructor_draws(config, coord, &mut rng).unwrap();
        // Equal RandomRate bounds return a rate without a draw, so only the
        // skipped side must leave the stream as it was.
        assert!(draws || rng.logical_state() == before, "{name}");
        assert_eq!(taken != AnimConstructorDraws::default(), draws, "{name}");
    }
}

#[test]
fn preconsumed_anim_constructor_retains_identity_without_redrawing_or_reassigning() {
    let rules = runtime_rules(
        "[DEBRIS]\nRate=900\nRandomRate=50,150\nEnd=2\n",
        &[("DEBRIS", 2)],
    );
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("DEBRIS");
    let descriptor = runtime_descriptor(type_id, 1);
    let coord = AnimWorldCoord {
        x: 128,
        y: 128,
        z: 20,
    };
    let native_unique_id = sim.next_native_runtime_id();
    let mut draws = anim_constructor_draws(
        rules.art().anim_runtime_config("DEBRIS").unwrap(),
        coord,
        &mut sim.scenario_rng,
    )
    .unwrap();
    draws.native_unique_id = Some(native_unique_id);
    let after_draws = sim.scenario_rng.logical_state();
    let next_constructor = sim.next_native_runtime_id();
    let id = sim
        .spawn_anim_at_world_with_constructor(
            &rules,
            descriptor,
            coord,
            AnimConstructorInput::Preconsumed(draws),
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(sim.anim(id).unwrap().native_unique_id, native_unique_id);
    assert_eq!(
        sim.native_unique_ids.as_ref().unwrap().current_raw() as i32,
        next_constructor
    );
    assert_eq!(sim.scenario_rng.logical_state(), after_draws);
}

#[test]
fn authored_load_anim_retains_native_id_and_final_scalar_delete_rechecks_compaction() {
    let rules = runtime_rules(
        "[LOADTILE]\nRate=900\nEnd=2\nLoopCount=-1\nStartSound=TileStart\nStopSound=TileStop\n\n\
         [KEEP]\nRate=900\nEnd=2\nLoopCount=-1\n",
        &[("LOADTILE", 2), ("KEEP", 2)],
    );
    let mut sim = Simulation::new();
    let load_type = sim.interner.intern("LOADTILE");
    let keep_type = sim.interner.intern("KEEP");
    let world = AnimWorldCoord {
        x: 384,
        y: 640,
        z: 208,
    };
    let mut first = runtime_descriptor(load_type, 0);
    first.terrain_attached = true;
    first.use_cell_drawer = true;
    let first_id = sim
        .spawn_anim_at_world_with_constructor(
            &rules,
            first,
            world,
            AnimConstructorInput::AssignedIdentity(1_010_001),
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("first authored load Anim");
    let keep_id = sim
        .spawn_anim_object(&rules, runtime_descriptor(keep_type, 0))
        .expect("ordinary retained Anim");
    let mut second = runtime_descriptor(load_type, 0);
    second.terrain_attached = true;
    second.use_cell_drawer = true;
    let second_id = sim
        .spawn_anim_at_world_with_constructor(
            &rules,
            second,
            world,
            AnimConstructorInput::AssignedIdentity(1_010_002),
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("second authored load Anim");

    assert_eq!(sim.anim(first_id).unwrap().native_unique_id, 1_010_001);
    assert_eq!(sim.anim(second_id).unwrap().native_unique_id, 1_010_002);
    assert!(sim.anim(first_id).unwrap().start_sound_active);

    assert_eq!(sim.scalar_delete_load_terrain_anims(), 2);
    assert!(sim.anim(first_id).is_none());
    assert!(sim.anim(second_id).is_none());
    assert!(sim.anim(keep_id).is_some());
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
            .count(),
        2,
        "final Init scalar deletion suppresses configured StopSound identity"
    );
}

#[test]
fn gsi_04_12_anim_make_infantry_marks_before_first_ai_and_clears_on_natural_end() {
    let rules = runtime_rules(
        "[GENDEATH]\nRate=900\nEnd=1\nLoopCount=1\nMakeInfantry=0\n",
        &[("GENDEATH", 1)],
    );
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("GENDEATH");
    let mut descriptor = runtime_descriptor(type_id, 0);
    descriptor.rx = 3;
    descriptor.ry = 4;
    descriptor.sub_x = SimFixed::from_num(192);
    descriptor.sub_y = SimFixed::from_num(64);
    let id = sim.spawn_anim_object(&rules, descriptor).unwrap();

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(3, 4),
        0x04,
        "the first AI guard runs after MakeInfantry raw marking"
    );
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);

    sim.session.set_binary_frame_for_test(1);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(3, 4), 0);
    assert!(sim.substrate.pending_delete.contains_object(&id));
}

#[test]
fn gsi_04_12_anim_make_infantry_next_and_early_destroy_leave_destructive_mark_stale() {
    let rules = runtime_rules(
        "[GENDEATH]\nRate=900\nEnd=1\nLoopCount=1\nMakeInfantry=0\nNext=PLAIN\n\n\
         [PLAIN]\nRate=900\nEnd=1\nLoopCount=1\n",
        &[("GENDEATH", 1), ("PLAIN", 1)],
    );
    let mut sim = Simulation::new();
    let gen_type = sim.interner.intern("GENDEATH");
    let plain = sim.interner.intern("PLAIN");
    let mut descriptor = runtime_descriptor(gen_type, 0);
    descriptor.rx = 5;
    descriptor.ry = 6;
    descriptor.sub_x = SimFixed::from_num(64);
    descriptor.sub_y = SimFixed::from_num(192);
    let id = sim.spawn_anim_object(&rules, descriptor).unwrap();

    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(5, 6), 0x08);
    sim.session.set_binary_frame_for_test(1);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();

    assert_eq!(sim.anim(id).unwrap().type_id, plain);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 6),
        0x08,
        "Next takes priority and performs no MakeInfantry clear"
    );
    sim.destroy_anim(id, &rules);
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(5, 6),
        0x08,
        "generic Anim destruction does not repair the raw byte"
    );
}

#[test]
fn gsi_04_12_anim_make_infantry_mark_and_clear_keep_native_bridge_asymmetry() {
    let mut grid = RawCellOccupationGrid::new();

    apply_anim_raw_occupation(
        &mut grid,
        RawCellKey::Real(7, 8),
        0x10,
        416,
        416,
        true,
        AnimOccupationOperation::Mark,
    );
    assert_eq!(grid.ground_bits(7, 8), 0);
    assert_eq!(grid.deck_bits(7, 8), 0x10);
    apply_anim_raw_occupation(
        &mut grid,
        RawCellKey::Real(7, 8),
        0x10,
        416,
        416,
        false,
        AnimOccupationOperation::Clear,
    );
    assert_eq!(grid.deck_bits(7, 8), 0);

    apply_anim_raw_occupation(
        &mut grid,
        RawCellKey::Real(7, 8),
        0x10,
        416,
        416,
        false,
        AnimOccupationOperation::Mark,
    );
    assert_eq!(grid.ground_bits(7, 8), 0x10);
    apply_anim_raw_occupation(
        &mut grid,
        RawCellKey::Real(7, 8),
        0x10,
        416,
        416,
        false,
        AnimOccupationOperation::Clear,
    );
    assert_eq!(
        grid.ground_bits(7, 8),
        0x10,
        "height-only clear targets deck after a nonstructural ground mark"
    );
    assert_eq!(grid.deck_bits(7, 8), 0);
}

#[test]
fn make_infantry_occupation_reads_constructor_side_flags_without_sprite_state() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_constructor.json",
    ))
    .unwrap();
    // Supply original5FC380's completed cell fields at the Anim reader
    // boundary. The constructor's common TMP fixture has physical level6.
    for case in &corpus["cases"].as_array().unwrap()[8..12] {
        let native = case["cells"]
            .as_array()
            .unwrap()
            .iter()
            .find(|cell| cell["flags"].as_u64().unwrap() & 0x100 != 0 && cell["overlay"] == -1)
            .unwrap();
        let x = native["coord"][0].as_u64().unwrap() as u16;
        let y = native["coord"][1].as_u64().unwrap() as u16;
        let mut terrain = crate::map::resolved_terrain::bridge_constructor_terrain();
        let cell = terrain.native_cell_identity((x as i16, y as i16));
        let flags = native["flags"].as_u64().unwrap() as u32;
        terrain.write_native_cell_flags(cell, flags);
        // Native -1 identity with the corpus state byte.
        let mut overlays =
            crate::sim::overlay_grid::OverlayGrid::new(terrain.width(), terrain.height());
        overlays.cell_mut_for_test(x, y).overlay_data = native["state"].as_u64().unwrap() as u8;
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(terrain);
        sim.overlay_grid = Some(overlays);
        assert!(sim.bridge_state.is_none());
        let world = AnimWorldCoord {
            x: i32::from(x) * 256 + 128,
            y: i32::from(y) * 256 + 128,
            z: 1040,
        };
        // Original Infantry5217C0's center (128,128) occupies slot0:
        // walk_head_occupation.json raw[94]/[98], deck8 -> deck9.
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            world,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(x, y), 0);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0x01);

        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .write_native_cell_flags(cell, flags & !0x100);
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            world,
            AnimOccupationOperation::Clear,
        );
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0);
        Simulation::apply_make_infantry_raw_occupation(
            &mut sim.substrate,
            &sim.resolved_terrain,
            world,
            AnimOccupationOperation::Mark,
        );
        assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(x, y), 0x01);
        assert_eq!(sim.substrate.raw_cell_occupation.deck_bits(x, y), 0);
    }
}

/// Map[coord] (`0x00565730`) truncates toward zero and wraps by row. A
/// MakeInfantry anim less than a cell left of the map edge occupies cell
/// 0 of its row; a cell further left it wraps to slot 511 of the row
/// above, off this map, and past slot 511 it wraps to the next row.
#[test]
fn a_make_infantry_anim_left_of_the_map_edge_occupies_cell_zero() {
    const SIZE: u16 = 8;
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            SIZE,
            SIZE,
            (0..SIZE)
                .flat_map(|y| {
                    (0..SIZE).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ),
    );
    // Sub-cell (128, 128) is slot 0.
    let world = AnimWorldCoord {
        x: -128,
        y: 5 * 256 + 128,
        z: 0,
    };
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        world,
        AnimOccupationOperation::Mark,
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0x01);
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        world,
        AnimOccupationOperation::Clear,
    );
    assert_eq!(sim.substrate.raw_cell_occupation.ground_bits(0, 5), 0);

    assert_eq!(
        Simulation::map_coord_cell(&sim.resolved_terrain, -128 - 256, world.y),
        None
    );
    assert_eq!(
        Simulation::map_coord_cell(&sim.resolved_terrain, 512 * 256 + 128, world.y),
        Some((0, 6))
    );
}

/// `AnimClass::MarkCellOccupancy` (`0x00426270`) stores the anim's owner
/// index, -1, beside the bit it sets (`0x004262D0`), and
/// `ClearCellOccupancy` (`0x00426300`) resets it only once `0x1C` is clear.
/// Off the map both write the shared dummy cell's planes.
#[test]
fn a_make_infantry_anim_writes_owner_minus_one_on_a_real_or_dummy_cell() {
    const SIZE: u16 = 8;
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            SIZE,
            SIZE,
            (0..SIZE)
                .flat_map(|y| {
                    (0..SIZE).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ),
    );
    let house = InternedId::from_index(41);
    let ground = MovementLayer::Ground;
    // An infantryman in slot 2 leaves its house beside the ground byte;
    // the anim at the centre marks slot 0.
    sim.substrate
        .raw_cell_occupation
        .mark_ground_infantry(3, 3, 1 << 2, house);
    let on_map = AnimWorldCoord {
        x: 3 * 256 + 128,
        y: 3 * 256 + 128,
        z: 0,
    };
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        on_map,
        AnimOccupationOperation::Mark,
    );
    let raw = &sim.substrate.raw_cell_occupation;
    assert_eq!(raw.ground_bits(3, 3), 0x05);
    assert_eq!(raw.occupant_at(RawCellKey::Real(3, 3), ground).1, None);
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        on_map,
        AnimOccupationOperation::Clear,
    );
    let raw = &sim.substrate.raw_cell_occupation;
    assert_eq!(raw.ground_bits(3, 3), 0x04);
    assert_eq!(raw.occupant_at(RawCellKey::Real(3, 3), ground).1, None);

    // A cell left of column 0 wraps to slot 511 of the row above: the
    // shared dummy on this map.
    sim.substrate.raw_cell_occupation.write_occupant(
        RawCellKey::Dummy,
        ground,
        1 << 3,
        Some(house),
        true,
    );
    let off_map = AnimWorldCoord {
        x: -384,
        y: 3 * 256 + 128,
        z: 0,
    };
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        off_map,
        AnimOccupationOperation::Mark,
    );
    let raw = &sim.substrate.raw_cell_occupation;
    assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x09);
    assert_eq!(raw.occupant_at(RawCellKey::Dummy, ground).1, None);
    Simulation::apply_make_infantry_raw_occupation(
        &mut sim.substrate,
        &sim.resolved_terrain,
        off_map,
        AnimOccupationOperation::Clear,
    );
    let raw = &sim.substrate.raw_cell_occupation;
    assert_eq!(raw.bits_at(RawCellKey::Dummy, ground), 0x08);
    assert_eq!(raw.occupant_at(RawCellKey::Dummy, ground).1, None);
}

/// `AnimClass::AnimClass @ 0x00421EA0`'s `RandomRate=` pick and `Bouncer=`
/// launch (`tools/spatial_oracle/anim_bouncer_launch.py`, the whole
/// constructor executed): the rate draw, the three velocity draws and
/// Init's three, state for state, and the body's position, velocity,
/// elasticity, gravity and clamp bits. `RandomRate=` goes through the
/// production reader; the oracle writes the three doubles into the type
/// directly, so they are set exactly (`ReadDouble` would round them
/// through `%f`; the stock values are integers either way).
#[test]
fn bouncer_launch_matches_the_original() {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anim_bouncer_launch.json",
    ))
    .unwrap();
    let rows = golden["ctor"].as_array().unwrap();
    assert!(rows.len() >= 150);
    let mut assigned_identity_rows = 0;
    for row in rows {
        let input = &row["input"];
        let rate = input["random_rate"]
            .as_array()
            .map(|pair| format!("RandomRate={},{}\n", pair[0], pair[1]))
            .unwrap_or_default();
        let art = ArtRegistry::from_ini(&IniFile::from_str(&format!("[T]\nBouncer=yes\n{rate}")));
        let mut config = art.anim_runtime_config("T").unwrap().clone();
        let exact = |key: &str| NativeF64Bits::from_bits(input[key].as_f64().unwrap().to_bits());
        config.elasticity = exact("elasticity");
        config.max_xy_vel = exact("max_xy");
        config.min_z_vel = exact("min_z");
        let coord = input["coord"].as_array().unwrap();
        let at = |i: usize| coord[i].as_i64().unwrap() as i32;
        let mut rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap());
        assert_eq!(
            rng.native_state_hex(),
            row["rng_before"].as_str().unwrap(),
            "{input}"
        );
        let draws = anim_constructor_draws(
            &config,
            AnimWorldCoord {
                x: at(0),
                y: at(1),
                z: at(2),
            },
            &mut rng,
        )
        .unwrap();
        assert_eq!(
            rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );
        let rate_pick = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["site"] == "0x004221F5")
            .map(|event| event["result"].as_u64().unwrap() as u16);
        assert_eq!(draws.random_rate, rate_pick, "{input}");
        let body = draws.bounce.expect("a Bouncer= type launches");
        let native = &row["bounce"];
        let bits = |key: &str, i: usize| native[key][i].as_u64().unwrap() as u32;
        for axis in 0..3 {
            assert_eq!(
                body.position()[axis].bits(),
                bits("position_bits", axis),
                "{input}"
            );
            assert_eq!(
                body.velocity()[axis].bits(),
                bits("velocity_bits", axis),
                "{input}"
            );
        }
        assert_eq!(
            body.elasticity().bits(),
            native["elasticity_bits"].as_u64().unwrap(),
            "{input}"
        );
        assert_eq!(
            body.gravity().bits(),
            native["gravity_bits"].as_u64().unwrap(),
            "{input}"
        );
        assert_eq!(
            body.angular_velocity_magnitude().bits(),
            native["clamp_bits"].as_u64().unwrap(),
            "{input}"
        );
        // Exercise the assigned-identity constructor through the real ART
        // reader wherever its %f conversion preserves the supplied native
        // doubles. The body above was compared to the executed constructor,
        // so the former fresh-load/live-slot entry must retain that same
        // body and full RNG state, rather than taking only RandomRate.
        if ["elasticity", "max_xy", "min_z"].iter().all(|key| {
            let value = input[*key].as_f64().unwrap();
            f64::from(value as f32) == value
        }) {
            assigned_identity_rows += 1;
            let rules = runtime_rules(
                &format!(
                    "[T]\nBouncer=yes\n{rate}Elasticity={}\nMaxXYVel={}\nMinZVel={}\nEnd=2\n",
                    input["elasticity"], input["max_xy"], input["min_z"],
                ),
                &[("T", 2)],
            );
            let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap());
            let type_id = sim.interner.intern("T");
            let native_id = sim.next_native_runtime_id();
            let id = sim
                .spawn_anim_at_world_with_constructor(
                    &rules,
                    runtime_descriptor(type_id, 0),
                    AnimWorldCoord {
                        x: at(0),
                        y: at(1),
                        z: at(2),
                    },
                    AnimConstructorInput::AssignedIdentity(native_id),
                    &mut crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                row["rng_after"].as_str().unwrap(),
                "{input}: assigned-identity constructor draws"
            );
            let anim = sim.anim(id).unwrap();
            assert_eq!(anim.native_unique_id, native_id);
            assert_eq!(
                sim.native_unique_ids.as_ref().unwrap().current_raw() as i32,
                native_id
            );
            assert_eq!(
                anim.bounce,
                Some(body),
                "{input}: assigned-identity native body"
            );
        }
    }
    assert_eq!(assigned_identity_rows, 134);
}

/// Original full Anim421EA0 constructor controls, with a supplied genuine
/// Random2Class state that makes one of the Z/Y/X draws INT_MIN. Shared
/// abs_before_modulo must retain the native CDQ/XOR/SUB-before-IDIV sign.
/// Frame count 2 is a declared fixture binding; no image/render claim.
#[test]
fn bouncer_launch_int_min_axes_match_the_original() {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anim_bouncer_launch_intmin.json",
    ))
    .unwrap();
    let rows = golden["ctor"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let input = &row["input"];
        let name = input["type"].as_str().unwrap();
        let rate = &input["random_rate"];
        let rules = runtime_rules(
            &format!(
                "[{name}]\nBouncer=yes\nRandomRate={},{}\nElasticity={}\nMaxXYVel={}\nMinZVel={}\nEnd=2\n",
                rate[0], rate[1], input["elasticity"], input["max_xy"], input["min_z"],
            ),
            &[(name, 2)],
        );
        let mut sim = Simulation::with_seed(0);
        // The recorded seed describes native setup, not this constrained
        // boundary. Import every logical RNG field without reseeding it.
        sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
            row["rng_before"].as_str().unwrap(),
        );
        let type_id = sim.interner.intern(name);
        let native_id = sim.next_native_runtime_id();
        let at = |axis: usize| input["coord"][axis].as_i64().unwrap() as i32;
        let (id, raw) = crate::sim::rng::trace_draws(|| {
            sim.spawn_anim_at_world_with_constructor(
                &rules,
                runtime_descriptor(type_id, 0),
                AnimWorldCoord {
                    x: at(0),
                    y: at(1),
                    z: at(2),
                },
                AnimConstructorInput::AssignedIdentity(native_id),
                &mut crate::sim::world::FrameEffects::default(),
            )
            .unwrap()
        });
        let actual_raw: Vec<u32> = raw
            .iter()
            .map(|draw| draw["value"].as_u64().unwrap() as u32)
            .collect();
        let native_raw: Vec<u32> = row["raw_draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| draw.as_u64().unwrap() as u32)
            .collect();
        assert_eq!(actual_raw, native_raw, "{input}: all raw words in order");
        assert_eq!(
            raw.len() as u64,
            row["raw_draw_count"].as_u64().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{input}"
        );

        let velocity_draws: Vec<u32> = row["velocity_draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| draw.as_i64().unwrap() as i32 as u32)
            .collect();
        assert_eq!(
            &actual_raw[..3],
            velocity_draws.as_slice(),
            "{input}: Z,Y,X first"
        );
        let exceptional_draw = match input["name"].as_str().unwrap() {
            "int_min_z" => 0,
            "int_min_y" => 1,
            "int_min_x" => 2,
            name => panic!("unexpected axis control {name}"),
        };
        assert_eq!(actual_raw[exceptional_draw], i32::MIN as u32, "{input}");
        assert_eq!(
            velocity_draws
                .iter()
                .filter(|&&word| word == i32::MIN as u32)
                .count(),
            1
        );
        let request_sites: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| event["site"].as_str())
            .collect();
        assert_eq!(
            request_sites,
            [
                "0x004221F5",
                "0x00422552",
                "0x00422564",
                "0x00422579",
                "0x00439857",
                "0x00439875",
                "0x00439897"
            ],
            "{input}"
        );

        let anim = sim.anim(id).unwrap();
        assert_eq!(anim.native_unique_id, native_id);
        let body = anim.bounce.expect("Bouncer=yes must construct its body");
        // Every stored field, including both quaternions, is compared to
        // the whole original constructor rather than a second launch port.
        let actual = serde_json::json!({
            "elasticity_bits": body.elasticity().bits(),
            "gravity_bits": body.gravity().bits(),
            "clamp_bits": body.angular_velocity_magnitude().bits(),
            "position_bits": body.position().map(|v| v.bits()),
            "velocity_bits": body.velocity().map(|v| v.bits()),
            "orientation_bits": body.orientation().map(|v| v.bits()),
            "rotation_bits": body.rotation().map(|v| v.bits()),
        });
        assert_eq!(actual, row["bounce"], "{input}: whole native Bounce state");
    }
}

/// A stock-shaped debris chunk (`DBRIS1LG` numbers, `MaxXYVel` cut to 0.5
/// so it lands in its own cell) launched over a flat arena beside a tank:
/// it flies its body, then on touching down (`BounceClass::Update`
/// reports Stopped on flat ground) constructs its `ExpireAnim=`, blasts
/// the tank through `Apply_area_damage` with its HE `Damage=`, and is
/// destroyed. Its Location follows the body every tick.
#[test]
fn a_debris_chunk_lands_and_blasts_what_is_under_it() {
    let art_ini = IniFile::from_str(
        "[CHUNK]\nBouncer=yes\nElasticity=0.0\nMaxXYVel=0.5\nMinZVel=25.0\n\
         ExpireAnim=BOOM\nDamage=20\nDamageRadius=80\nWarhead=HE\n\
         LoopEnd=15\nLoopCount=-1\nRandomRate=220,600\n\
         [BOOM]\nRate=900\n",
    );
    // The ordered Rules processor reads CHUNK's `ExpireAnim=` and
    // `Warhead=` with its ART body.
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[General]\nDamageFireTypes=\n\
             [AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
             [InfantryTypes]\n[VehicleTypes]\n0=TANK\n[AircraftTypes]\n[BuildingTypes]\n\
             [Animations]\n0=CHUNK\n1=BOOM\n\
             [TANK]\nStrength=300\nArmor=heavy\nSpeed=4\n\
             [Warheads]\n0=HE\n\
             [HE]\nCellSpread=.5\nPercentAtMax=.5\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ),
        &art_ini,
    )
    .unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("CHUNK", 30);
    art.bind_anim_frame_count_for_test("BOOM", 17);
    rules.install_art_fixture(art);
    let mut sim = Simulation::with_seed(3);
    let americans = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            americans, 0, None, true, 0, 10,
        ));
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let tank = sim
        .spawn_object_at_height("TANK", "Americans", 8, 8, 0, 0, &rules)
        .unwrap();
    let chunk_type = sim.interner.intern("CHUNK");
    let center = crate::util::lepton::CELL_CENTER_LEPTON;
    let chunk = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor {
                delay: 0,
                loop_count: 1,
                draw_flags: 0x600,
                z_adjust: 0,
                reverse: false,
                ..AnimClassSpawnDescriptor::new(chunk_type, 8, 8, center, center, 0)
            },
            AnimWorldCoord {
                x: 8 * 256 + 128,
                y: 8 * 256 + 128,
                z: 20,
            },
        )
        .unwrap();
    let launch = sim
        .anim(chunk)
        .unwrap()
        .bounce
        .expect("a Bouncer= chunk has a body");
    assert_eq!(
        launch.position_leptons().z,
        30,
        "10 leptons above its coordinate"
    );

    let mut landed_at = None;
    for frame in 1..200 {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(
            chunk,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let anim = sim.anim(chunk).unwrap();
        let body = anim.bounce.unwrap().position_leptons();
        assert_eq!(
            [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
            [body.x, body.y, body.z]
        );
        if sim.substrate.pending_delete.contains_object(&chunk) {
            landed_at = Some(body);
            break;
        }
    }
    let landed_at = landed_at.expect("the chunk comes down");
    assert_eq!(landed_at.z, 0, "it rests on the flat ground");
    assert_eq!((landed_at.x >> 8, landed_at.y >> 8), (8, 8));
    let boom = sim.interner.intern("BOOM");
    assert_eq!(
        sim.anims()
            .filter(|(_, anim)| anim.type_id == boom)
            .map(|(_, anim)| [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z])
            .collect::<Vec<_>>(),
        vec![[landed_at.x, landed_at.y, landed_at.z]]
    );
    let health = sim.substrate.entities.get(tank).unwrap().health.current();
    assert!(
        (280..300).contains(&health),
        "HE splash of 20 at most, got {health}"
    );
    assert_eq!(sim.combat_light_requests.len(), 1);
}

/// A world whose flat 8x8 map takes a 1x1 crater and a 1x1 scorch.
fn middle_world(art_text: &str, frames: i32) -> (RuleSet, Simulation) {
    let mut rules = runtime_rules(art_text, &[("MARK", frames)]);
    rules.smudge_types =
        crate::rules::smudge_type::SmudgeTypeRegistry::from_rules_ini(&IniFile::from_str(
            "[SmudgeTypes]\n0=CR1\n1=BURN1\n\
             [CR1]\nCrater=yes\nWidth=1\nHeight=1\n\
             [BURN1]\nBurn=yes\nWidth=1\nHeight=1\n",
        ));
    let mut sim = Simulation::new();
    let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(8);
    for ry in 0..8 {
        for rx in 0..8 {
            terrain.cell_mut_for_test(rx, ry).unwrap().accepts_smudge = true;
        }
    }
    sim.resolved_terrain = Some(terrain);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(8, 8));
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(8, 8));
    (rules, sim)
}

fn spawn_mark(sim: &mut Simulation, rules: &RuleSet, world_z: i32) -> AnimId {
    let name = sim.interner.intern("MARK");
    let center = crate::util::lepton::CELL_CENTER_LEPTON;
    sim.spawn_combat_explosion_anim(rules, name, 4, 4, center, center, 0, world_z)
        .unwrap()
}

fn marks_placed(sim: &Simulation) -> usize {
    sim.smudge_grid.as_ref().unwrap().iter_occupied().count()
}

/// `AnimClass::AI 0x0042465D`: a 13-frame crater anim (S_CLSN22, the
/// AP warhead's large AnimList entry) marks the ground once, on the
/// visit that commits stage 6 (`13 / 2`), not at construction.
#[test]
fn a_crater_anim_marks_the_ground_at_its_middle_frame() {
    let (rules, mut sim) = middle_world("[MARK]\nRate=900\nCrater=yes\n", 13);
    let id = spawn_mark(&mut sim, &rules, 0);
    assert_eq!(
        marks_placed(&sim),
        0,
        "Start leaves a 13-frame anim to its middle frame"
    );
    let mut saw_middle = false;
    for frame in 1..40 {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(
            id,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let Some(anim) = sim.anim(id) else {
            break;
        };
        let stage = anim.runtime.current_frame;
        saw_middle |= stage == 6;
        assert_eq!(marks_placed(&sim), usize::from(stage >= 6), "stage {stage}");
    }
    assert!(saw_middle);
    assert_eq!(marks_placed(&sim), 1);
    assert_eq!(
        sim.smudge_grid.as_ref().unwrap().cell(4, 4).type_id,
        Some(0)
    );
}

/// `AnimClass::Middle 0x00425057`: an anim 30 leptons or more above the
/// ground leaves no mark; 29 still marks.
#[test]
fn an_anim_thirty_leptons_up_leaves_no_mark() {
    for (world_z, expected) in [(30, 0), (29, 1)] {
        let (rules, mut sim) = middle_world("[MARK]\nRate=900\nScorch=yes\n", 13);
        let id = spawn_mark(&mut sim, &rules, world_z);
        for frame in 1..40 {
            sim.session.set_binary_frame_for_test(frame);
            sim.visit_anim(
                id,
                &rules,
                None,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        }
        assert_eq!(marks_placed(&sim), expected, "z {world_z}");
    }
}

/// `AnimClass::Start 0x00424D48`: a type without a middle frame (one raw
/// frame, so `+0x298` is 0) marks at Start, before any AI visit.
#[test]
fn a_single_frame_anim_marks_at_start() {
    let (rules, mut sim) = middle_world("[MARK]\nRate=900\nScorch=yes\n", 1);
    spawn_mark(&mut sim, &rules, 0);
    assert_eq!(marks_placed(&sim), 1);
    assert_eq!(
        sim.smudge_grid.as_ref().unwrap().cell(4, 4).type_id,
        Some(1)
    );
}

#[test]
fn delay_guard_and_rate_use_passive_frame_anchor() {
    let rules = runtime_rules("[TEST]\nRate=450\nEnd=3\nLoopCount=1\n", &[("TEST", 3)]);
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("TEST");
    let rng_before = sim.scenario_rng.logical_state();
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 1))
        .unwrap();

    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // constructor first-AI guard at frame 0
    sim.session.set_binary_frame_for_test(1);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // delay 1 -> 0
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
    sim.session.set_binary_frame_for_test(2);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // constructor-anchored timer is already due

    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // a second visit in the same frame cannot advance again
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

/// An anim waiting on its frame timer ends its visit at the gates without
/// being handed out, so nothing re-derives from it; the visit that
/// advances its frame hands it out.
#[test]
fn a_waiting_anims_visit_hands_it_out_to_no_one() {
    let rules = runtime_rules("[TEST]\nRate=450\nEnd=3\nLoopCount=1\n", &[("TEST", 3)]);
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("TEST");
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 1))
        .unwrap();
    let visit = |sim: &mut Simulation, frame: u32| {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(
            id,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        sim.substrate
            .anims
            .take_touched(AnimTouchReader::ExpiryListeners)
    };
    visit(&mut sim, 0); // constructor first-AI guard
    visit(&mut sim, 1); // delay 1 -> 0
    assert_eq!(visit(&mut sim, 2), Touched::Ids(vec![id]));
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
    // The timer the last visit restarted is not due in the same frame.
    assert_eq!(visit(&mut sim, 2), Touched::Ids(Vec::new()));
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
}

#[test]
fn stock_naweap_a_rate_normalizes_to_six_frames_at_speed_one() {
    let rules = runtime_rules(
        "[NAWEAP_A]\nRate=200\nNormalized=yes\nEnd=12\nLoopCount=-1\n",
        &[("NAWEAP_A", 12)],
    );
    let mut sim = Simulation::new();
    sim.session.game_options_mut_for_test().game_speed = 1;
    let type_id = sim.interner.intern("NAWEAP_A");
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();

    assert_eq!(sim.anim(id).unwrap().runtime.rate_reload, 6);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    sim.session.set_binary_frame_for_test(5);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 0);
    sim.session.set_binary_frame_for_test(6);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.anim(id).unwrap().runtime.current_frame, 1);
}

#[test]
fn next_without_preintern_reuses_identity_runs_middle_and_destroy_is_idempotent() {
    let rules = runtime_rules(
        "[FIRST]\nRate=900\nEnd=2\nLoopCount=1\nNext=SECOND\nStartSound=FirstStart\n\n\
         [SECOND]\nRate=900\nEnd=2\nLoopCount=1\nReport=SecondReport\nStopSound=SecondStop\n",
        &[("FIRST", 2), ("SECOND", 2)],
    );
    let mut sim = Simulation::new();
    let first = sim.interner.intern("FIRST");
    assert!(sim.interner.get("SECOND").is_none());
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(first, 0))
        .unwrap();
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id, .. }] if *anim_id == id
    ));

    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // guard
    sim.session.set_binary_frame_for_test(1);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // frame 1
    sim.session.set_binary_frame_for_test(2);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // frame 2 -> SECOND in place + Middle
    let anim = sim.anim(id).unwrap();
    assert_eq!(sim.interner.resolve(anim.type_id), "SECOND");
    assert_eq!(anim.runtime.current_frame, 0);
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::AnimationStarted { .. }))
            .count(),
        2,
    );

    sim.session.set_binary_frame_for_test(3);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // SECOND frame 1 (Next does not restore guard)
    sim.session.set_binary_frame_for_test(4);
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap(); // SECOND frame 2 -> destroy
    sim.destroy_anim(id, &rules);
    assert!(!sim.anim(id).unwrap().runtime.inactive);
    assert!(sim.substrate.pending_delete.contains_object(&id));
    assert!(!sim.live_object_order_snapshot().contains(&id));
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
            .count(),
        1,
    );
}

#[test]
fn destroy_releases_report_before_optional_stop_sound_once() {
    let rules = runtime_rules(
        "[A]\nReport=StartCue\nStopSound=EndCue\nEnd=2\nLoopCount=1\n",
        &[("A", 2)],
    );
    let mut sim = Simulation::new();
    let type_id = sim.interner.intern("A");
    let id = sim
        .spawn_anim_object(&rules, runtime_descriptor(type_id, 0))
        .unwrap();
    let world = sim.anim_absolute_coord(id).unwrap();
    let stop = sim.interner.intern("ENDCUE");
    sim.sound_events.clear_for_test();
    sim.destroy_anim(id, &rules);
    sim.destroy_anim(id, &rules);
    // Original4255D5 releases +1A0 before425618 plays the optional
    // StopSound. Repeated queued Destroy cannot stop the released cue.
    assert!(matches!(sim.sound_events.as_slice(), [
        SimSoundEvent::ObjectSoundReleased { owner },
        SimSoundEvent::AnimationStopped { anim_id, stop_sound_id: Some(sound), world: at },
    ] if *owner == id && *anim_id == id && *sound == stop && *at == world));
    // UnInit4255B0/UnInit5F65F0 retire the receiver without changing19B.
    // The retained physical identity cannot be revealed before the drain.
    assert!(!sim.anim(id).unwrap().runtime.inactive);
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![id]);
    assert!(!sim.anim(id).unwrap().in_logic_vector);
    assert_eq!(sim.display_layers().layer_of(id), None);
    assert!(!sim.reveal_anim(id, &rules, true));
    sim.process_pending_delete();
    assert!(sim.anim(id).is_none());
}

#[test]
fn trailer_without_preintern_is_visited_and_guarded_in_same_live_walk() {
    let rules = runtime_rules(
        // Sticky= keeps CHILD an ordinary anim (`ArtRegistry::anim_presentation_only`).
        "[PARENT]\nRate=0\nEnd=2\nTrailerAnim=CHILD\nTrailerSeperation=1\n\n\
         [CHILD]\nRate=900\nEnd=2\nLoopCount=1\nSticky=yes\n",
        &[("PARENT", 2), ("CHILD", 2)],
    );
    let mut sim = Simulation::new();
    let parent_type = sim.interner.intern("PARENT");
    assert!(sim.interner.get("CHILD").is_none());
    let parent = sim
        .spawn_anim_object(&rules, runtime_descriptor(parent_type, 0))
        .unwrap();

    sim.for_each_live_object(|sim, id| {
        sim.visit_anim(
            id,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    });

    let order = sim.live_object_order_snapshot();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0], parent);
    let child = sim.anim(order[1]).unwrap();
    assert_eq!(sim.interner.resolve(child.type_id), "CHILD");
    assert!(!child.runtime.first_ai_guard);
    assert_eq!(child.runtime.current_frame, 0);
}

#[test]
fn owner_expiry_marks_anim_inactive_until_its_next_ai_visit() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let anim_id = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids[0]
        .unwrap();
    sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
    assert_eq!(sim.anim(anim_id).unwrap().owner_entity, Some(building_id));

    assert!(sim.anim_pointer_expired(
        anim_id,
        building_id,
        crate::rules::ruleset::RuleSet::empty_for_test()
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0],
        Some(anim_id),
        "native owner expiry retains the slot until the Anim's own expiry"
    );
    assert!(sim.anim(anim_id).unwrap().runtime.inactive);
    assert!(sim.live_object_order_snapshot().contains(&anim_id));
    assert!(!sim.substrate.pending_delete.contains_object(&anim_id));

    sim.visit_anim(
        anim_id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.entities()
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0]
            .is_none()
    );
    assert!(!sim.live_object_order_snapshot().contains(&anim_id));
    assert_eq!(sim.substrate.pending_delete.object_ids(), vec![anim_id]);
}

#[test]
fn building_limbo_uninits_its_damage_fire_anims() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let fires = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids;
    let anim_id = fires[0].expect("slot zero");

    // The building is alive: only Building Limbo 4458E4 retires the fires.
    sim.techno_limbo_with_rules(building_id, &rules);

    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(building.is_object_alive());
    assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
    assert!(
        building.damage_fires.active,
        "Limbo leaves Building+5E8 as it was"
    );
    assert!(!sim.live_object_order_snapshot().contains(&anim_id));
    assert!(sim.substrate.pending_delete.contains_object(&anim_id));
}

#[test]
fn pointer_expiry_runs_owner_and_attached_bullet_arms_independently() {
    let (mut sim, rules, owner) = damage_fire_fixture(false);
    let fire_type = sim.interner.get("FIRE01").unwrap();
    let anim = sim
        .spawn_anim_object(&rules, runtime_descriptor(fire_type, 0))
        .unwrap();
    sim.set_anim_owner_object(anim, Some(owner), &rules);
    sim.substrate.set_anim_attached_bullet(anim, Some(owner));
    // Pointer fields are independently compared at425164 and4251AD;
    // clearing the owner must not skip attached-Bullet UnInit.
    assert!(sim.anim_pointer_expired(anim, owner, &rules));
    let object = sim.anim(anim).unwrap();
    assert_eq!(object.owner_entity, None);
    assert_eq!(object.attached_bullet(), None);
    assert!(object.runtime.inactive);
    assert!(!object.in_logic_vector);
    assert!(sim.substrate.pending_delete.contains_object(&anim));
}

#[test]
fn finalizer_defensively_clears_remaining_owner_slot() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    let fire_type = sim.interner.get("FIRE01").unwrap();
    let anim_id = sim
        .spawn_anim_at_world(
            &rules,
            runtime_descriptor(fire_type, 0),
            AnimWorldCoord { x: 0, y: 0, z: 0 },
        )
        .unwrap();
    sim.substrate
        .anims
        .set_owner_entity_for_test(anim_id, Some(building_id));
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .damage_fires
        .anim_ids[0] = Some(anim_id);
    sim.substrate.rebuild_building_anim_slot_indices();
    // Supply the common post-UnInit retirement state through its existing
    // membership owner;19B remains the independent native expiry byte.
    sim.conceal_anim(anim_id);
    sim.substrate.pending_delete.queue_object(anim_id);

    sim.process_pending_delete();

    assert!(sim.anim(anim_id).is_none());
    assert!(!sim.live_object_order_snapshot().contains(&anim_id));
    assert!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids[0]
            .is_none()
    );
}

#[test]
fn gsi_05_12_attached_anim_follows_a_moving_owner_and_detaches_absolute() {
    // The point of owner-relative storage: `AnimClass::GetCoords @
    // 0x00422BE0` adds the owner's live coordinate, so the anim tracks the
    // owner without anyone rewriting the anim. `SetOwnerObject @
    // 0x00424B50` then writes the resolved absolute back on detach, so the
    // anim stays where it was standing.
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let anim_id = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids[0]
        .expect("slot zero");
    sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
    let before = sim.anim_absolute_coord(anim_id).expect("attached anim");
    let stored_before = sim.anim(anim_id).unwrap().world_coord;

    // Move the owner one cell east and one cell south.
    {
        let owner = sim.substrate.entities.get_mut(building_id).unwrap();
        owner.position.set_rx_for_test(owner.position.rx() + 1);
        owner.position.set_ry_for_test(owner.position.ry() + 1);
    }

    assert_eq!(
        sim.anim(anim_id).unwrap().world_coord,
        stored_before,
        "the stored delta is untouched by owner movement"
    );
    assert_eq!(
        sim.anim_absolute_coord(anim_id).unwrap(),
        AnimWorldCoord {
            x: before.x + LEPTONS_PER_CELL,
            y: before.y + LEPTONS_PER_CELL,
            z: before.z
        },
        "the resolved coordinate follows the owner one cell on each axis"
    );

    let moved = sim.anim_absolute_coord(anim_id).unwrap();
    assert_eq!(
        sim.detach_anim_from_owner(anim_id, &rules),
        Some(building_id)
    );
    assert!(sim.anim(anim_id).unwrap().owner_entity.is_none());
    assert_eq!(
        sim.anim(anim_id).unwrap().world_coord,
        moved,
        "detach writes the resolved absolute back into the stored field"
    );
    assert_eq!(sim.anim_absolute_coord(anim_id).unwrap(), moved);
}

/// One Z frame: a producer's height level is `Level * 104` world leptons
/// (`IMUL [0x00ABDE88]`, the ground height unit), and the level a consumer
/// reads back is the floor over the same unit. The store used to keep a
/// private 128-per-level scale next to producers that wrote 104-frame
/// leptons, so those drew a level low on raised ground.
#[test]
fn anim_coordinates_use_the_native_level_height() {
    let level_three =
        AnimWorldCoord::from_cell_sub_z(4, 5, SimFixed::from_num(128), SimFixed::from_num(64), 3);
    assert_eq!(
        level_three,
        AnimWorldCoord {
            x: 4 * 256 + 128,
            y: 5 * 256 + 64,
            z: 3 * 104
        }
    );
    assert_eq!(level_three.to_cell_sub_z().4, 3);
    // An exact-Z producer's coordinate one lepton under level 1 is still
    // level 0; at 104 it is level 1 (128 would have called both level 0).
    let under = AnimWorldCoord {
        z: 103,
        ..level_three
    };
    let at = AnimWorldCoord {
        z: 104,
        ..level_three
    };
    assert_eq!((under.to_cell_sub_z().4, at.to_cell_sub_z().4), (0, 1));
}

/// `AnimClass::GetCoords @ 0x00422BE0` adds the owner's live coordinate on
/// every axis, so an anim attached to an owner that gains height rises with
/// it. The owner term used to be the coarse level byte only.
#[test]
fn gsi_05_12_attached_anim_follows_the_owners_height() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let anim_id = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids[0]
        .expect("slot zero");
    sim.set_anim_owner_object(anim_id, Some(building_id), &rules);
    let before = sim.anim_absolute_coord(anim_id).expect("attached anim");

    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(before.z + 250));

    assert_eq!(
        sim.anim_absolute_coord(anim_id).unwrap(),
        AnimWorldCoord {
            z: before.z + 250,
            ..before
        },
        "250 leptons is not a whole level: the exact height carries through"
    );
}

#[test]
fn building_damage_fire_uses_exact_threshold_slots_coords_and_depth() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    let mut expected_rng = sim.scenario_rng.clone();
    let start_type = expected_rng.next_range_u32_inclusive(0, 2) as usize;
    let type_names = ["FIRE01", "FIRE02", "FIRE03"];
    let frame_counts = [30_u32, 64, 30];
    let expected_types = [start_type, (start_type + 1) % type_names.len()];
    let expected_frames = expected_types
        .map(|index| expected_rng.next_range_u32_inclusive(0, frame_counts[index] - 1) as i32);
    sim.update_building_damage_fire(building_id, &rules);

    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(building.damage_fires.active);
    let first = building.damage_fires.anim_ids[0].expect("slot zero");
    let second = building.damage_fires.anim_ids[1].expect("slot one");
    assert!(
        building.damage_fires.anim_ids[2..]
            .iter()
            .all(Option::is_none)
    );
    let first_anim = sim.anim(first).unwrap();
    assert_eq!(first_anim.owner_entity, None);
    assert_eq!(
        sim.interner.resolve(first_anim.type_id),
        type_names[expected_types[0]]
    );
    assert_eq!(first_anim.runtime.current_frame, expected_frames[0]);
    // Original43C0D0 stores absolute coordinates and leaves ctor+CC0.
    // Its Building5C8 slot/listener is independent of Object attachment.
    assert_eq!(
        sim.anim_absolute_coord(first).unwrap(),
        AnimWorldCoord {
            x: 2450,
            y: 2653,
            z: 0
        }
    );
    assert_eq!(
        first_anim.world_coord,
        AnimWorldCoord {
            x: 2450,
            y: 2653,
            z: 0
        },
        "damage fires retain absolute coordinates"
    );
    assert_eq!(first_anim.z_adjust, -192);
    let second_anim = sim.anim(second).unwrap();
    assert_eq!(second_anim.owner_entity, None);
    assert_eq!(
        sim.interner.resolve(second_anim.type_id),
        type_names[expected_types[1]]
    );
    assert_eq!(second_anim.runtime.current_frame, expected_frames[1]);
    assert_eq!(
        sim.anim_absolute_coord(second).unwrap(),
        AnimWorldCoord {
            x: 3140,
            y: 2594,
            z: 0
        }
    );
    assert_eq!(
        second_anim.world_coord,
        AnimWorldCoord {
            x: 3140,
            y: 2594,
            z: 0
        },
        "damage fires retain absolute coordinates"
    );
    assert_eq!(second_anim.z_adjust, -136);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
    assert_eq!(
        sim.live_object_order_snapshot(),
        vec![building_id, first, second]
    );
}

#[test]
fn unchanged_damage_fire_cache_consumes_no_rng_and_recovery_clears_slots() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    let rng_after_spawn = sim.scenario_rng.logical_state();
    let ids = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids;

    sim.update_building_damage_fire(building_id, &rules);
    assert_eq!(sim.scenario_rng.logical_state(), rng_after_spawn);
    assert_eq!(
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids,
        ids
    );

    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(51);
    sim.update_building_damage_fire(building_id, &rules);
    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(!building.damage_fires.active);
    assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
    assert_eq!(sim.live_object_order_snapshot(), vec![building_id]);
}

#[test]
fn unspawnable_damage_fire_type_leaves_the_slots_empty() {
    let (mut sim, mut rules, building_id) = damage_fire_fixture(false);
    for fire in &mut rules.general.damage_fire_types {
        fire.name = "NOSUCHFIRE".to_string();
    }
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);

    sim.update_building_damage_fire(building_id, &rules);

    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(building.damage_fires.active);
    assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
    assert_eq!(sim.live_object_order_snapshot(), vec![building_id]);
}

#[test]
fn empty_fire_type_list_sets_cache_without_rng_or_slots() {
    let (mut sim, mut rules, building_id) = damage_fire_fixture(false);
    rules.general.damage_fire_types.clear();
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    let rng_before = sim.scenario_rng.logical_state();

    sim.update_building_damage_fire(building_id, &rules);

    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(building.damage_fires.active);
    assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
    assert_eq!(sim.scenario_rng.logical_state(), rng_before);
}

#[test]
fn occupied_first_slot_stops_after_initial_type_roll() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    let fire_type = sim.interner.get("FIRE01").unwrap();
    let occupied_id = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor {
                type_name: fire_type,
                rx: 0,
                ry: 0,
                sub_x: crate::util::fixed_math::SIM_ZERO,
                sub_y: crate::util::fixed_math::SIM_ZERO,
                z: 0,
                delay: 0,
                loop_count: 1,
                draw_flags: TRAILER_DRAW_FLAGS,
                z_adjust: 0,
                reverse: false,
                use_cell_drawer: false,
                terrain_attached: false,
                draw_runtime: AnimDrawRuntime::default(),
                owner: None,
            },
            AnimWorldCoord { x: 0, y: 0, z: 0 },
        )
        .unwrap();
    let building = sim.substrate.entities.get_mut(building_id).unwrap();
    building.damage_fires.anim_ids[0] = Some(occupied_id);
    building.health.set_for_test(50);
    let mut expected_rng = sim.scenario_rng.clone();
    let _ = expected_rng.next_range_u32_inclusive(0, 2);

    sim.update_building_damage_fire(building_id, &rules);

    let slots = sim
        .substrate
        .entities
        .get(building_id)
        .unwrap()
        .damage_fires
        .anim_ids;
    assert_eq!(slots[0], Some(occupied_id));
    assert!(slots[1..].iter().all(Option::is_none));
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

#[test]
fn repaired_health_clears_owned_anims_and_stop_is_idempotent() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    sim.sound_events.clear_for_test();
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(100);

    sim.update_building_damage_fire(building_id, &rules);
    sim.update_building_damage_fire(building_id, &rules);

    let building = sim.substrate.entities.get(building_id).unwrap();
    assert!(!building.damage_fires.active);
    assert!(building.damage_fires.anim_ids.iter().all(Option::is_none));
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::ObjectSoundReleased { .. }))
            .count(),
        2,
    );
}

#[test]
fn anim_store_slots_scheduler_and_hash_roundtrip() {
    let (mut sim, rules, building_id) = damage_fire_fixture(false);
    sim.substrate
        .entities
        .get_mut(building_id)
        .unwrap()
        .health
        .set_for_test(50);
    sim.update_building_damage_fire(building_id, &rules);
    assert!(sim.substrate.pending_delete.is_empty());
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // AnimStore/scheduler persistence on that same post-load cursor.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();
    let expected_order = sim.live_object_order_snapshot();
    let bytes = crate::util::save_codec::serialize(&sim).expect("serialize sim with AnimStore");
    let mut restored: Simulation =
        crate::util::save_codec::deserialize(&bytes).expect("deserialize AnimStore");
    restored.rebuild_logic_membership();
    assert_eq!(restored.live_object_order_snapshot(), expected_order);
    assert_eq!(restored.state_hash(), expected_hash);
    assert!(restored.sound_events.is_empty());
    assert_eq!(
        restored
            .substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids,
        sim.substrate
            .entities
            .get(building_id)
            .unwrap()
            .damage_fires
            .anim_ids,
    );
}
fn waypoint_cue_flare_scene() -> (Simulation, RuleSet, crate::sim::intern::InternedId) {
    // Custom DropZone binding from the original connected controls. ART and
    // the five-frame SHP metadata are physical ARRWSE inputs; no GPU claim.
    let mut rules=RuleSet::from_ini(&IniFile::from_str(
        "[General]\nDropZoneAnim=ARRWSE\n[AudioVisual]\nDropZoneRadius=4\n[Animations]\n0=ARRWSE\n[BuildingTypes]\n0=BLOCK\n[BLOCK]\nStrength=100\n[Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\n",
    )).unwrap();
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[ARRWSE]\nLoopCount=2\nLoopEnd=4\nRate=450\nDemandLoad=true\nLayer=Top\nZAdjust=-300\nNext=none\n",
    ));
    art.bind_anim_frame_count_for_test("ARRWSE", 5);
    art.set_anim_sounds_for_test("ARRWSE", Some("Explosion06"), Some("ExplosionShard"));
    rules.replace_art_registry_for_test(art);
    let map=crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Waypoints]\n59=10010\n123=6005\n[Triggers]\nCUE=Player,<none>,Cue,0,1,1,1,0\n[Events]\nCUE=1,8,0,0\n[Actions]\nCUE=1,8,0,123,0,0,0,0,BH\n[Tags]\nCUETAG=0,Cue,CUE\n",
    ).unwrap();
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    // MapRevealArea rejects the arena's default Size20 diamond edge at
    // (10,10). Install this scene's Size16 through the normal owner.
    sim.install_playfield_from_map_header(&map.header);
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    let player = sim.interner.intern("Player");
    let other = sim.interner.intern("Other");
    for (id, human) in [(player, true), (other, false)] {
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, human, 0, 10,
            ));
    }
    sim.session.set_current_house_for_test(Some(player));
    sim.session.set_binary_frame_for_test(1000);
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
        None,
    );
    sim.classify_map_triggers(&rules);
    (sim, rules, player)
}

fn waypoint_cue_spring_flare(sim: &mut Simulation, rules: &RuleSet) -> AnimId {
    sim.advance_triggers(rules, None, &mut crate::sim::world::FrameEffects::default());
    let anim = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| anim)
        .next()
        .expect("live Tag8 creates the flare");
    anim.stable_id
}

#[test]
fn waypoint_cue_flare_uses_tail_slot_reveals_current_house_and_keeps_first_start() {
    let (mut sim, rules, player) = waypoint_cue_flare_scene();
    assert!(!sim.fog.is_cell_revealed(player, 10, 10));
    let id = waypoint_cue_spring_flare(&mut sim, &rules);
    let anim = sim.anim(id).unwrap();
    assert_eq!(
        anim.world_coord,
        AnimWorldCoord {
            x: 2688,
            y: 2688,
            z: 0
        }
    );
    assert_eq!(anim.draw_flags, 0x600);
    assert_eq!(anim.z_adjust, -300);
    assert!(anim.no_effects());
    assert!(anim.runtime.first_ai_guard);
    assert!(sim.sound_events.iter().any(|event|matches!(event,SimSoundEvent::AnimationStarted{anim_id,sound_id,..} if *anim_id==id && sound_id.name_in(&rules)=="EXPLOSION06")),"first Start precedes the action's NoEffects store");
    assert!(sim.fog.is_cell_revealed(player, 10, 10));
    assert!(!sim.fog.is_cell_revealed(player, 20, 20));
    let other = sim.interner.get("Other").unwrap();
    assert!(!sim.fog.is_cell_revealed(other, 10, 10));
    sim.sound_events.clear_for_test();
    sim.visit_anim(
        id,
        &rules,
        None,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert!(!sim.anim(id).unwrap().runtime.first_ai_guard);
    assert!(
        sim.sound_events.is_empty(),
        "NoEffects suppresses later Start/Stop work"
    );
}

#[test]
fn waypoint_cue_flare_ground_building_ends_before_first_guard_and_deck_does_not() {
    use crate::sim::movement::locomotor::MovementLayer;
    for layer in [MovementLayer::Ground, MovementLayer::Bridge] {
        let (mut sim, rules, player) = waypoint_cue_flare_scene();
        if layer == MovementLayer::Bridge {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let cell = terrain.native_cell_identity((10, 10));
            terrain.write_native_cell_flags(cell, 0x100);
        }
        let type_ref = sim.interner.intern("BLOCK");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            100,
            10,
            10,
            0,
            0,
            player,
            Health::new(100),
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.health.set(0); //Native Cell47C520 has no Health/Alive admission.
        sim.substrate.entities.insert(building);
        sim.set_next_stable_object_id_for_test(101);
        sim.substrate.occupancy.add(
            10,
            10,
            100,
            layer,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
        let id = waypoint_cue_spring_flare(&mut sim, &rules);
        assert_eq!(
            sim.anim(id).unwrap().world_coord.z,
            if layer == MovementLayer::Bridge {
                416
            } else {
                0
            }
        );
        sim.sound_events.clear_for_test();
        let rng = sim.rng_state();
        sim.visit_anim(
            id,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
        let anim = sim.anim(id).unwrap();
        if layer == MovementLayer::Ground {
            assert!(anim.runtime.inactive);
            assert!(
                anim.runtime.first_ai_guard,
                "424358 ends the flare before consuming19C"
            );
            assert!(sim.substrate.pending_delete.contains_object(&id));
            assert!(!sim.sound_events.iter().any(|event|matches!(event,SimSoundEvent::VocAtCoord{sound_id,..} | SimSoundEvent::AnimationStarted{sound_id,..} if sound_id.name_in(&rules)=="EXPLOSIONSHARD")));
            sim.process_pending_delete();
            assert!(sim.anim(id).is_none());
        } else {
            assert!(!anim.runtime.inactive);
            assert!(!anim.runtime.first_ai_guard);
            assert!(!sim.substrate.pending_delete.contains_object(&id));
        }
        assert_eq!(sim.rng_state(), rng);
    }
}

/// An anim's type finds its ART config through the interner's handles as it
/// does by name, in another case, and still finds one the map's
/// `bind_scheduler_anim_assets` adds after the handles were resolved, which
/// is also the `DropZoneAnim=` the name compare matches.
#[test]
fn anim_config_handles_follow_a_config_bound_after_the_resolve() {
    let mut rules =
        RuleSet::from_ini(&IniFile::from_str("[General]\nDropZoneAnim=late\n")).unwrap();
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
        "[BOOM]\nRate=100\n",
    )));
    let mut sim = crate::sim::world::Simulation::new();
    let boom = sim.interner.intern("Boom");
    let late = sim.interner.intern("Late");
    sim.resolve_type_handles(&rules);
    let by_name = |rules: &RuleSet, name| {
        rules
            .art()
            .anim_runtime_config(name)
            .map(std::ptr::from_ref)
    };
    let by_id = |sim: &crate::sim::world::Simulation, rules: &RuleSet, id| {
        sim.interner
            .anim_type_config(id, rules)
            .map(std::ptr::from_ref)
    };
    assert_eq!(by_id(&sim, &rules, boom), by_name(&rules, "BOOM"));
    assert!(by_id(&sim, &rules, boom).is_some());
    assert_eq!(by_id(&sim, &rules, late), None);

    // A theater root no AnimType holds gains an unread config at map load.
    let root = crate::map::source::test_support::TestDirectory::new("late-anim-handle");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(root.path());
    rules.bind_scheduler_anim_assets(&["LATE".into()], &assets, "TEM", "TEMPERATE");
    for covered in [false, true] {
        if covered {
            sim.interner.cover_types(&rules);
        }
        assert!(by_id(&sim, &rules, late).is_some(), "covered: {covered}");
        assert_eq!(by_id(&sim, &rules, late), by_name(&rules, "LATE"));
        assert_eq!(by_id(&sim, &rules, boom), by_name(&rules, "BOOM"));
        let handle = sim.interner.anim_type_config_handle(late, &rules).unwrap();
        assert!(sim.interner.is_drop_zone_anim(handle, &rules));
        let boom_handle = sim.interner.anim_type_config_handle(boom, &rules).unwrap();
        assert!(!sim.interner.is_drop_zone_anim(boom_handle, &rules));
    }
}

/// A new ART registry gives a name another slot; the handles resolved from
/// the old one are not read against it.
#[test]
fn anim_config_handles_do_not_outlive_a_replaced_art_registry() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str("[General]\n")).unwrap();
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
        "[BOOM]\nRate=100\n",
    )));
    let mut sim = crate::sim::world::Simulation::new();
    let boom = sim.interner.intern("BOOM");
    sim.resolve_type_handles(&rules);
    rules.replace_art_registry_for_test(ArtRegistry::from_ini(&IniFile::from_str(
        "[OTHER]\nRate=1\n[BOOM]\nRate=200\n",
    )));
    let config = sim
        .interner
        .anim_type_config(boom, &rules)
        .expect("BOOM keeps a config");
    assert!(std::ptr::eq(
        config,
        rules.art().anim_runtime_config("BOOM").unwrap()
    ));
}
