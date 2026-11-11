use super::*;
use crate::rules::retail_ini_fixture::{art_registry, retail_assets, retail_rules_and_art};
use crate::rules::tiberium_type::TiberiumTypeId;
use crate::sim::anim_class::{AnimRemap, AnimWorldCoord};
use crate::sim::components::AnimClassSpawnDescriptor;

/// Post_Load_Game6851B1 -> Anims_Refresh_Tiberium_LightConverts4261D0
/// samples stored Location for every flagged Anim, including attached and
/// inactive entries. It neither uses GetCoords nor live AI's -384 offset.
#[test]
fn player_load_recolors_flagged_anims_from_stored_location_only() {
    let Some((mut root, mut art)) = retail_rules_and_art() else {
        return;
    };
    root.merge(&IniFile::from_str(
        "[Animations]\n__restore=RESTOREORE\n__plain=RESTOREPLAIN\n",
    ));
    art.merge(&IniFile::from_str(
        "[RESTOREORE]\nImage=SMKPUFF\nIsAnimatedTiberium=yes\n\
         [RESTOREPLAIN]\nImage=SMKPUFF\nIsAnimatedTiberium=no\n",
    ));
    let processed = crate::rules::native_processing::RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(art_registry(&art));
    let (_, mut assets) = retail_assets().expect("retail SMKPUFF asset");
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    assert_eq!(
        rules.bind_anim_class_assets(
            &["RESTOREORE".into(), "RESTOREPLAIN".into()],
            &assets,
            "TEM",
            "TEMPERATE",
        ),
        0
    );
    let registry = rules.overlay_registry();
    let scheme = |slot| {
        AnimRemap::ColorScheme(crate::rules::house_colors::HouseColorIndex(
            u8::try_from(
                rules
                    .tiberium_types
                    .get(TiberiumTypeId(slot))
                    .unwrap()
                    .color_scheme()
                    / 2,
            )
            .unwrap(),
        ))
    };
    let (expected, prior) = (scheme(0), scheme(1));
    assert_ne!(expected, prior, "retail ore and gem palettes differ");
    let mut saved = load_fixture_simulation(true);
    *saved.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    crate::sim::arena_fixture::flat_arena(&mut saved, &rules);
    let terrain = saved.resolved_terrain().unwrap().clone();
    let house = saved.intern("Americans");
    saved
        .houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            house, 0, None, true, 0, 10,
        ));
    let owner = saved
        .spawn_object("E1", "Americans", 12, 12, 0, &rules)
        .unwrap();
    let spawn = |sim: &mut Simulation, name: &str, cell: (u16, u16), owner| {
        let coord = AnimWorldCoord {
            x: i32::from(cell.0) * 256 + 128,
            y: i32::from(cell.1) * 256 + 128,
            z: 0,
        };
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = AnimClassSpawnDescriptor {
            delay: 2,
            z_adjust: 13,
            owner,
            ..AnimClassSpawnDescriptor::new(sim.intern(name), rx, ry, sub_x, sub_y, z)
        };
        sim.spawn_anim_at_world(&rules, descriptor, coord).unwrap()
    };
    let attached = spawn(&mut saved, "RESTOREORE", (24, 24), Some(owner));
    // Infantry Unlimbo chooses its subcell. Use the resulting retained
    // owner-relative Location, not an assumed centered Infantry coordinate.
    let stored = saved.anim(attached).unwrap().world_coord();
    let stored_cell = (
        u16::try_from(stored.x / 256).unwrap(),
        u16::try_from(stored.y / 256).unwrap(),
    );
    let absolute = saved.anim_absolute_coord(attached).unwrap();
    let virtual_cell = (absolute.x / 256, absolute.y / 256);
    let live_cell = ((absolute.x - 384) / 256, (absolute.y - 384) / 256);
    let stored_xy = (i32::from(stored_cell.0), i32::from(stored_cell.1));
    assert_ne!(stored_xy, virtual_cell);
    assert_ne!(stored_xy, live_cell);
    assert_ne!(virtual_cell, live_cell);
    let grid = saved.overlay_grid_mut_for_test().as_mut().unwrap();
    grid.place_overlay(
        stored_cell.0,
        stored_cell.1,
        registry.id_for_name("TIB01").unwrap(),
        0,
    );
    grid.place_overlay(24, 24, registry.id_for_name("GEM01").unwrap(), 0);
    let nonresource = spawn(&mut saved, "RESTOREORE", (16, 16), None);
    let unflagged = spawn(&mut saved, "RESTOREPLAIN", stored_cell, None);
    for id in [attached, nonresource, unflagged] {
        assert!(saved.anims_mut_for_test().set_anim_remap(id, prior));
    }
    saved
        .anims_mut_for_test()
        .get_mut(attached)
        .unwrap()
        .runtime_mut_for_test()
        .inactive = true;
    let before = saved.anim(attached).unwrap();
    assert_eq!(virtual_cell, (24, 24));
    assert_eq!(live_cell, (23, 23));
    let runtime = before.runtime().clone();

    let restored = PreparedLoad::prepare_candidate(
        &snapshot_bytes(&saved, &rules),
        Some(&saved),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&rules),
        Some(&terrain),
        Some(registry),
    )
    .expect("the ordinary user-load path admits this saved world")
    .simulation;
    let loaded = restored.anim(attached).expect("retained inactive Anim");
    assert_eq!(loaded.owner_entity(), Some(owner));
    assert_eq!(loaded.runtime(), &runtime);
    assert_eq!(loaded.world_coord(), before.world_coord());
    assert_eq!(loaded.z_adjust(), 13);
    assert_eq!(restored.anim(nonresource).unwrap().remap(), Some(prior));
    assert_eq!(restored.anim(unflagged).unwrap().remap(), Some(prior));
    assert_eq!(
        loaded.remap(),
        Some(expected),
        "post-load uses stored Location ore"
    );
}
