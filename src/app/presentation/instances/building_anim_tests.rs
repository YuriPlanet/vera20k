//! Synthetic atlas markers exercise the production slot emitter and retained
//! Display lowering. Native field/sort goldens are independent of Rust. This
//! is not a retail-pixel golden; depth_gpu_tests separately reads real GPU output.
use super::*;
use crate::app::presentation::render::draw_plan_lowering::{
    lower_ground_object_instances, lower_object_instances,
};
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteEntry, SpriteAtlas};
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{game_entity::GameEntity, world::Simulation};
use std::collections::HashMap;

fn fixture(building_name: &str, damaged: bool) -> (Simulation, RuleSet, u64, SpriteAtlas) {
    fixture_with_anim_keys(building_name, damaged, "")
}

fn fixture_with_anim_keys(
    building_name: &str,
    damaged: bool,
    anim_keys: &str,
) -> (Simulation, RuleSet, u64, SpriteAtlas) {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.display.json",
    ))
    .unwrap();
    let mut art_text = format!("[{building_name}]\n");
    for (key, value) in golden["art"]["physical_sections"][building_name]
        .as_object()
        .unwrap()
    {
        art_text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
    }
    let mut registry = format!(
        "[BuildingTypes]\n0={building_name}\n[{building_name}]\nStrength=100\n[Animations]\n"
    );
    let mut entries = HashMap::new();
    let mut names = Vec::new();
    for row in golden["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["building"] == building_name)
    {
        for variant in ["normal", "damaged"] {
            let name = row["slot"][variant].as_str().unwrap();
            registry.push_str(&format!("{}={name}\n", names.len()));
            names.push(name.to_string());
            // A nonzero Start catches selecting relative AC as an atlas frame.
            // The offsets are atlas frame offsets, independent of slot X/Y.
            art_text.push_str(&format!(
                "[{name}]\nLayer=ground\nLoopCount=-1\nStart=3\n{anim_keys}"
            ));
            entries.insert(
                ShpSpriteKey {
                    palette_context: ShpPaletteContext::SelectedScheme,
                    type_id: name.to_string(),
                    facing: 0,
                    frame: 3,
                    house_color: HouseColorIndex(1),
                },
                ShpSpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [8.0; 2],
                    offset_x: 2.0,
                    offset_y: -4.0,
                    canvas_rect: [0.0, 0.0, 8.0, 8.0],
                    extended: false,
                    page: row["slot"]["slot"].as_u64().unwrap() as u8,
                    source_palette: 1,
                },
            );
            if !anim_keys.is_empty() {
                let mut shadow_key = ShpSpriteKey {
                    palette_context: ShpPaletteContext::SelectedScheme,
                    type_id: name.to_string(),
                    facing: 0,
                    frame: 3,
                    house_color: HouseColorIndex(1),
                };
                let mut shadow = entries[&shadow_key];
                shadow_key.frame = 23;
                shadow.page += 20;
                shadow.offset_x += 9.0;
                entries.insert(shadow_key, shadow);
            }
        }
    }
    let art_ini = IniFile::from_str(&art_text);
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&registry), &art_ini).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for name in names {
        art.bind_anim_frame_count_for_test(&name, 40);
    }
    rules.install_art_data(art);
    let mut sim = Simulation::new();
    let id = sim.substrate_mut_for_test().allocate_stable_id();
    let mut building = GameEntity::test_default_of_category(
        id,
        building_name,
        "A",
        64,
        60,
        EntityCategory::Structure,
    );
    building.set_type_ref_for_test(sim.intern(building_name));
    building.set_owner_for_test(sim.intern("A"));
    building.health.set_for_test(if damaged { 40 } else { 100 });
    sim.entities_mut_for_test().insert(building);
    let slots: &[u8] = if building_name == "NATBNK" {
        &[10, 11]
    } else {
        &[3]
    };
    for &slot in slots {
        sim.set_building_anim_slot(id, slot, damaged, false, 0, &rules)
            .unwrap();
    }
    (sim, rules, id, SpriteAtlas::from_test_entries(entries))
}

fn emit(
    sim: &Simulation,
    rules: &RuleSet,
    building: u64,
    atlas: &SpriteAtlas,
    order: &NativeDisplayOrder,
) -> Vec<PlannedObjectInstance> {
    emit_with_material(
        sim,
        rules,
        building,
        atlas,
        order,
        PaletteLight::default(),
        DrawState::default(),
    )
}

fn emit_with_material(
    sim: &Simulation,
    rules: &RuleSet,
    building: u64,
    atlas: &SpriteAtlas,
    order: &NativeDisplayOrder,
    palette: PaletteLight,
    material: DrawState,
) -> Vec<PlannedObjectInstance> {
    let mut out = Vec::new();
    let building = sim.entities().get(building).unwrap();
    let grid = crate::map::lighting::CellLightGrid::new();
    emit_building_anims(
        &mut out,
        order,
        atlas,
        rules.art(),
        Some(rules),
        sim.interner().resolve(building.type_ref()),
        HouseColorIndex(1),
        [1.0; 3],
        palette,
        &grid,
        (building.position.rx(), building.position.ry()),
        sim,
        building,
        &|_| 0,
        0.0,
        10000.0,
        [-10000.0, -10000.0, 20000.0, 20000.0],
        material,
        2,
    );
    out
}

/// Registered synthetic ART and the real slot allocator feed the retained
/// Display emitter. Only source-atlas metadata and the caller's material are
/// supplied. Forced translucency below is a declared draw-state control, not
/// proof of a CLEG attack or of native pixels.
fn material_fixture(name: &str, keys: &str) -> (Simulation, RuleSet, u64, u64, SpriteAtlas) {
    let rules_ini = IniFile::from_str(&format!(
        "[BuildingTypes]\n0=PLANT\n[PLANT]\nStrength=100\n[Animations]\n0={name}\n"
    ));
    let art_ini = IniFile::from_str(&format!(
        "[PLANT]\nActiveAnim={name}\n[{name}]\nLayer=ground\nEnd=20\nLoopCount=-1\nShouldUseCellDrawer=yes\nUseNormalLight=yes\n{keys}"
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test(name, 20);
    rules.install_art_data(art);
    let mut sim = Simulation::with_seed(31);
    let building = sim.substrate_mut_for_test().allocate_stable_id();
    let mut entity = GameEntity::test_default_of_category(
        building,
        "PLANT",
        "A",
        64,
        60,
        EntityCategory::Structure,
    );
    entity.set_type_ref_for_test(sim.intern("PLANT"));
    entity.set_owner_for_test(sim.intern("A"));
    sim.entities_mut_for_test().insert(entity);
    let anim = sim
        .set_building_anim_slot(building, 3, false, false, 0, &rules)
        .unwrap();
    assert_eq!(sim.anim(anim).unwrap().runtime().current_frame, 0);
    assert!(sim.display_layers().layer_of(anim).is_some());
    let atlas = SpriteAtlas::from_test_entries(HashMap::from([(
        ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            type_id: name.to_string(),
            facing: 0,
            frame: 0,
            house_color: HouseColorIndex(1),
        },
        ShpSpriteEntry {
            uv_origin: [0.25, 0.125],
            uv_size: [0.125, 0.25],
            pixel_size: [8.0, 6.0],
            offset_x: -4.0,
            offset_y: -3.0,
            canvas_rect: [-4.0, -3.0, 8.0, 6.0],
            extended: false,
            page: 3,
            source_palette: 9,
        },
    )]));
    (sim, rules, building, anim, atlas)
}

fn force_slot_translucency(sim: &mut Simulation, anim: u64) {
    sim.anims_mut_for_test()
        .get_mut(anim)
        .unwrap()
        .draw_runtime_mut_for_test()
        .forced_translucent = true;
}

fn assert_slot_replay(
    planned: Vec<PlannedObjectInstance>,
    anim: u64,
    alpha: f32,
    material: DrawState,
    palette: PaletteLight,
) {
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].parent.id, anim);
    assert_eq!(planned[0].pieces.len(), 1);
    let layer = planned[0].parent.layer.0 as usize;
    let piece = &planned[0].pieces[0];
    assert_eq!(piece.target, ObjectTexture::ShpPage(3));
    assert_eq!(piece.instance.source_palette, 9);
    assert_eq!(piece.instance.uv_origin, [0.25, 0.125]);
    assert_eq!(piece.instance.uv_size, [0.125, 0.25]);
    assert_eq!(piece.instance.alpha, alpha);
    assert_eq!(piece.instance.draw_state, material);
    assert_eq!(piece.instance.palette_light, palette);
    let lowered = lower_object_instances(planned);
    assert_eq!(lowered[layer].owners, [anim]);
    assert_eq!(lowered[layer].runs.len(), 1);
    assert_eq!(
        lowered[layer].runs[0].packed_parent,
        (material.native_selector_bits() & 6 != 0).then_some(anim)
    );
    assert_eq!(lowered[layer].instances[0].alpha, alpha);
    assert_eq!(lowered[layer].instances[0].draw_state, material);
}

#[test]
fn indexed_slot_translucency_preserves_selector_remap_and_replay() {
    for (name, keys, forced, selector) in [
        ("ORDINARY", "", true, 4),
        ("THICK", "DoubleThick=yes\n", true, 6),
        // Forced50 and authored25 are ORed, not one overwriting the other.
        ("AUTHORED", "Translucency=25\n", true, 6),
        ("AUTHORED_ONLY", "Translucency=25\n", false, 2),
    ] {
        let (mut sim, rules, building, anim, atlas) = material_fixture(name, keys);
        if forced {
            force_slot_translucency(&mut sim, anim);
        }
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let palette = PaletteLight::color_scheme([900, 1000, 800], 1000);
        assert_eq!(palette.rows(), 53);
        let inherited = DrawState {
            remap_row: 7,
            ..DrawState::default()
        };
        let before = (sim.state_hash(), sim.rng_state());
        let planned =
            emit_with_material(&sim, &rules, building, &atlas, &order, palette, inherited);
        assert_eq!((sim.state_hash(), sim.rng_state()), before, "{name}");
        assert_slot_replay(
            planned,
            anim,
            1.0,
            DrawState::from_native_selector(7, selector),
            palette,
        );
    }
}

#[test]
fn slot_translucency_preserves_unresolved_inherited_materials() {
    use crate::render::draw_state::{CloakDrawInput, DrawStateInput, FX_EMP};

    let cloak = DrawState::resolve(
        DrawStateInput {
            cloak: Some(CloakDrawInput {
                character: 1,
                progress: 0,
                native_offset_words: -3,
                voxel: false,
            }),
            ..DrawStateInput::default()
        },
        0,
        7,
    );
    assert!(cloak.visible);
    assert_eq!(cloak.state.native_selector_bits(), 2);
    // These are explicit caller-material compatibility inputs, not claims
    // that every state is produced by ordinary stock Building slot drawing.
    let base = DrawState {
        remap_row: 7,
        ..DrawState::default()
    };
    for inherited in [
        cloak.state,
        DrawState {
            native_offset_words: 5,
            ..base
        },
        DrawState {
            fx_flags: FX_EMP,
            ..base
        },
        DrawState {
            fx_params: [1.0, 0.0, 1.0, 12.0],
            ..base
        },
    ] {
        let (mut sim, rules, building, anim, atlas) = material_fixture("ORDINARY", "");
        force_slot_translucency(&mut sim, anim);
        let palette = PaletteLight::color_scheme([1000; 3], 1000);
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let planned =
            emit_with_material(&sim, &rules, building, &atlas, &order, palette, inherited);
        // In particular inherited cloak25 must not be replaced or ORed with
        // the slot's forced50 until that native combination is established.
        assert_slot_replay(planned, anim, 0.5, inherited, palette);
    }
}

#[test]
fn special_slot_anims_keep_their_existing_translucent_material() {
    for (name, keys) in [
        ("RING1", ""),
        ("TILED", "Tiled=yes\n"),
        ("BOUNCER", "Bouncer=yes\n"),
        ("METEOR", "IsMeteor=yes\n"),
    ] {
        let (mut sim, rules, building, anim, atlas) = material_fixture(name, keys);
        force_slot_translucency(&mut sim, anim);
        let palette = PaletteLight::color_scheme([1000; 3], 1000);
        let inherited = DrawState {
            remap_row: 7,
            ..DrawState::default()
        };
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let planned =
            emit_with_material(&sim, &rules, building, &atlas, &order, palette, inherited);
        assert_slot_replay(planned, anim, 0.5, inherited, palette);
    }
}

#[test]
fn metadata_free_slot_keeps_alpha_and_compatibility_replay() {
    let (mut sim, rules, building, anim, atlas) = material_fixture("ORDINARY", "");
    force_slot_translucency(&mut sim, anim);
    let inherited = DrawState {
        remap_row: 7,
        ..DrawState::default()
    };
    // UseNormalLight sets the brightness argument even when there is no
    // indexed metadata. Zero rows must not acquire a packed replay path.
    let palette = PaletteLight::default().with_brightness(1000);
    assert_eq!(palette.rows(), 0);
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let planned = emit_with_material(&sim, &rules, building, &atlas, &order, palette, inherited);
    assert_slot_replay(planned, anim, 0.5, inherited, palette);
}

#[test]
fn rgba_slot_with_palette_metadata_keeps_compatibility_replay() {
    let (mut sim, rules, building, anim, indexed_atlas) = material_fixture("ORDINARY", "");
    force_slot_translucency(&mut sim, anim);
    // This declared RGBA-source compatibility input is outside the indexed
    // atlas path even though the caller supplies a full palette descriptor.
    let atlas = SpriteAtlas::from_test_entries(
        indexed_atlas
            .entries_iter()
            .map(|(key, entry)| {
                (
                    key.clone(),
                    ShpSpriteEntry {
                        source_palette: 0,
                        ..*entry
                    },
                )
            })
            .collect(),
    );
    let inherited = DrawState {
        remap_row: 7,
        ..DrawState::default()
    };
    let palette = PaletteLight::color_scheme([1000; 3], 1000);
    assert_eq!(palette.rows(), 53);
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let planned = emit_with_material(&sim, &rules, building, &atlas, &order, palette, inherited);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].parent.id, anim);
    assert_eq!(planned[0].pieces.len(), 1);
    let layer = planned[0].parent.layer.0 as usize;
    let piece = &planned[0].pieces[0];
    assert_eq!(piece.target, ObjectTexture::ShpPage(3));
    assert_eq!(piece.instance.source_palette, 0);
    assert_eq!(piece.instance.palette_light, palette);
    assert_eq!(piece.instance.alpha, 0.5);
    assert_eq!(piece.instance.draw_state, inherited);
    let lowered = lower_object_instances(planned);
    assert_eq!(lowered[layer].owners, [anim]);
    assert_eq!(lowered[layer].runs.len(), 1);
    assert_eq!(lowered[layer].runs[0].packed_parent, None);
    assert_eq!(lowered[layer].instances[0].alpha, 0.5);
    assert_eq!(lowered[layer].instances[0].draw_state, inherited);
}

/// DrawIt's translucency selection (`0x00423061`) reaches slot anims: a
/// building being erased (Anim `+0x119`) draws them at 50%.
#[test]
fn an_erased_buildings_slot_anims_draw_translucent() {
    let (mut sim, rules, building, atlas) = fixture("CAOILD", false);
    let anim = sim
        .entities()
        .get(building)
        .unwrap()
        .building_art
        .anim_slots()[3]
        .unwrap();
    let order = NativeDisplayOrder::new(&[anim]);
    let alpha = |sim: &Simulation| {
        emit(sim, &rules, building, &atlas, &order)[0].pieces[0]
            .instance
            .alpha
    };
    assert_eq!(alpha(&sim), 1.0);
    sim.anims_mut_for_test()
        .get_mut(anim)
        .unwrap()
        .draw_runtime_mut_for_test()
        .forced_translucent = true;
    assert_eq!(alpha(&sim), 0.5);
}

#[test]
fn building_slot_draw_retains_body_shadow_order_and_absolute_start_frame() {
    use crate::render::{tactical_draw_plan::RenderZPolicy, terrain_draw::TerrainPiece};
    let (sim, rules, building, atlas) = fixture_with_anim_keys("CAOILD", false, "Shadow=yes\n");
    let anim = sim
        .entities()
        .get(building)
        .unwrap()
        .building_art
        .anim_slots()[3]
        .unwrap();
    let order = NativeDisplayOrder::new(&[anim]);
    let planned = emit(&sim, &rules, building, &atlas, &order);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].parent.id, anim);
    assert_eq!(planned[0].pieces.len(), 2);
    assert_eq!(planned[0].pieces[0].target, ObjectTexture::ShpPage(3));
    assert_eq!(
        planned[0].pieces[1].target,
        ObjectTexture::SpriteAtlasEdit(23, TerrainPiece::Shadow)
    );
    assert!(
        planned[0]
            .pieces
            .iter()
            .all(|piece| piece.render_z == RenderZPolicy::ReadOnly)
    );
    let pass = lower_ground_object_instances(planned);
    assert_eq!(pass.owners, [anim, anim]);
}

#[test]
fn building_slot_draws_a_nonempty_shadow_when_the_body_has_no_atlas_entry() {
    use crate::render::{tactical_draw_plan::RenderZPolicy, terrain_draw::TerrainPiece};
    let (sim, rules, building, atlas) = fixture_with_anim_keys("CAOILD", false, "Shadow=yes\n");
    let anim = sim
        .entities()
        .get(building)
        .unwrap()
        .building_art
        .anim_slots()[3]
        .unwrap();
    let order = NativeDisplayOrder::new(&[anim]);
    let full = emit(&sim, &rules, building, &atlas, &order);
    let shadow_key = ShpSpriteKey {
        palette_context: ShpPaletteContext::SelectedScheme,
        type_id: sim
            .interner()
            .resolve(sim.anim(anim).unwrap().type_id())
            .to_string(),
        facing: 0,
        frame: 23,
        house_color: HouseColorIndex(1),
    };
    // render_shp_frame omits a zero-W/H body; the physical companion frame
    // still supplies its own nonempty rectangle and palette context.
    let shadow_only = SpriteAtlas::from_test_entries(HashMap::from([(
        shadow_key.clone(),
        *atlas.get(&shadow_key).unwrap(),
    )]));
    let planned = emit(&sim, &rules, building, &shadow_only, &order);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].parent.id, anim);
    assert_eq!(planned[0].pieces.len(), 1);
    assert_eq!(
        planned[0].pieces[0].target,
        ObjectTexture::SpriteAtlasEdit(23, TerrainPiece::Shadow)
    );
    assert_eq!(planned[0].pieces[0].render_z, RenderZPolicy::ReadOnly);
    assert_eq!(
        planned[0].pieces[0].instance.position,
        full[0].pieces[1].instance.position
    );
    assert_eq!(
        planned[0].pieces[0].instance.z_adjust,
        full[0].pieces[1].instance.z_adjust
    );
}

#[test]
fn bunker_slot_emitter_interleaves_front_and_back_around_the_unit() {
    let golden: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.display.json",
    ))
    .unwrap();
    for damaged in [false, true] {
        let (mut sim, rules, building, atlas) = fixture("NATBNK", damaged);
        let slots = sim
            .entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots();
        let back = slots[11].unwrap();
        let front = slots[10].unwrap();
        let unit = sim.substrate_mut_for_test().allocate_stable_id();
        let identities = [back, unit, front];
        let native_order: Vec<_> = golden["orders"][0]["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| identities[v.as_u64().unwrap() as usize])
            .collect();
        // Parent has no Display membership at all. Only the actual Anim ids
        // appear in this native order; parent admission cannot supply it.
        let order = NativeDisplayOrder::new(&native_order);
        let mut planned = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(planned.len(), 2);
        assert!(planned.iter().all(|entry| entry.building_pieces.is_none()));
        assert!(planned.iter().all(|entry| entry.parent.id != building));
        planned.push(PlannedObjectInstance::object(
            order.object_draw(unit, SpriteEncoding::Voxel).unwrap(),
            vec![ObjectPieceInstance {
                target: ObjectTexture::UnitAtlasPage(0),
                render_z: crate::render::tactical_draw_plan::RenderZPolicy::ReadOnly,
                instance: SpriteInstance::default(),
            }],
        ));
        let pass = lower_ground_object_instances(planned);
        assert_eq!(pass.owners, native_order);
        assert_eq!(
            pass.runs.iter().map(|run| run.target).collect::<Vec<_>>(),
            [
                ObjectTexture::ShpPage(11),
                ObjectTexture::UnitAtlasPage(0),
                ObjectTexture::ShpPage(10)
            ]
        );
        let hidden = sim.anims_mut_for_test().get_mut(front).unwrap();
        hidden.draw_runtime_mut_for_test().hidden = true;
        let remaining = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(
            remaining
                .iter()
                .map(|entry| entry.parent.id)
                .collect::<Vec<_>>(),
            [back]
        );
        sim.clear_building_anim_slot(building, 11);
        assert!(sim.anim(back).is_none());
        assert!(sim.display_layers().layer_of(back).is_none());
        assert!(
            emit(
                &sim,
                &rules,
                building,
                &atlas,
                &NativeDisplayOrder::from_display(sim.display_layers())
            )
            .is_empty()
        );
        let replacement = sim
            .set_building_anim_slot(building, 10, damaged, false, 0, &rules)
            .unwrap();
        assert_ne!(replacement, front);
        assert!(sim.anim(front).is_none());
        assert!(sim.display_layers().layer_of(front).is_none());
        let visible = emit(
            &sim,
            &rules,
            building,
            &atlas,
            &NativeDisplayOrder::from_display(sim.display_layers()),
        );
        assert_eq!(
            visible
                .iter()
                .map(|entry| entry.parent.id)
                .collect::<Vec<_>>(),
            [replacement]
        );
    }
}

#[test]
fn bunker_migration_keeps_active_anim_offsets_start_and_independent_membership() {
    for name in ["CAOILD", "GAHPAD"] {
        let (mut sim, rules, building, atlas) = fixture(name, false);
        let anim_id = sim
            .entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots()[3]
            .unwrap();
        let order = NativeDisplayOrder::new(&[anim_id]);
        let before = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(before.len(), 1);
        let coord = sim.anim_absolute_coord(anim_id).unwrap();
        let (x, y, ..) = super::super::overlays::anim_world_render_coords(coord);
        assert_eq!(before[0].pieces[0].instance.position, [x + 2.0, y - 4.0]);
        assert_eq!(before[0].pieces[0].instance.source_palette, 1);
        // The retained Anim coordinate is the owner. A changed body coordinate
        // cannot silently recalculate its child's location in presentation.
        sim.entities_mut_for_test()
            .get_mut(building)
            .unwrap()
            .position
            .set_rx_for_test(2);
        let after = emit(&sim, &rules, building, &atlas, &order);
        assert_eq!(
            after[0].pieces[0].instance.position,
            before[0].pieces[0].instance.position
        );
        assert!(
            emit(
                &sim,
                &rules,
                building,
                &atlas,
                &NativeDisplayOrder::new(&[])
            )
            .is_empty()
        );
    }
}
