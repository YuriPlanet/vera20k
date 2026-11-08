use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::retail_ini_fixture::{art_registry, retail_assets, retail_rules_and_art};
use crate::sim::overlay_grid::OverlayGrid;

/// AnimAI4243BC..424427 checks the shifted virtual coordinate after the
/// first-AI/delay gates, sets inactive on a mismatch, and still runs this
/// visit's frame work. The next visit retires it through ordinary UnInit.
#[test]
fn animated_tiberium_checks_live_cell_identity_after_delay_without_cutting_the_visit() {
    let Some((mut root, mut art)) = retail_rules_and_art() else {
        return;
    };
    root.merge(&IniFile::from_str(
        "[Animations]\n9999=TIBLIVE\n[TIB01]\nCellAnim=TIBLIVE\n",
    ));
    art.merge(&IniFile::from_str(
        "[TIBLIVE]\nImage=SMKPUFF\nIsAnimatedTiberium=yes\nRate=900\n",
    ));
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(art_registry(&art));
    let (_, mut assets) = retail_assets().expect("retail animation shape");
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    assert_eq!(
        rules.bind_anim_class_assets(&["TIBLIVE".into()], &assets, "TEM", "TEMPERATE"),
        0
    );
    assert!(
        rules
            .art()
            .anim_runtime_config("TIBLIVE")
            .unwrap()
            .is_animated_tiberium
    );
    let overlays = rules.overlay_registry().clone();
    let ore = overlays.id_for_name("TIB01").unwrap();
    assert_eq!(
        overlays.flags(ore).unwrap().cell_anim.as_deref(),
        Some("TIBLIVE")
    );

    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    sim.install_overlay_grid_for_load(OverlayGrid::new(32, 32));
    let owner = sim
        .spawn_object_at_height("E1", "Americans", 12, 12, 0, 0, &rules)
        .unwrap();
    let type_name = sim.interner.intern("TIBLIVE");
    let id = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor {
                delay: 1,
                ..AnimClassSpawnDescriptor::new(type_name, 1, 17, SimFixed::ZERO, SimFixed::ZERO, 0)
            },
            // After -384, X is -1: Map565730 truncates it to cell0, not-1.
            AnimWorldCoord {
                x: 383,
                y: 16 * 256 + 384,
                z: 777,
            },
        )
        .unwrap();
    assert!(sim.set_anim_owner_object(id, Some(owner), &rules));
    assert!(
        sim.anim(id).unwrap().world_coord.x < 0,
        "stored and virtual coordinates differ"
    );
    let visit = |sim: &mut Simulation, frame| {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(id, &rules, Some(&overlays), FrameEffects::default())
            .unwrap();
    };
    visit(&mut sim, 0);
    visit(&mut sim, 1);
    assert!(
        !sim.anim(id).unwrap().runtime.inactive,
        "guard/delay skip the missing cell"
    );
    sim.overlay_grid_mut_for_test()
        .as_mut()
        .unwrap()
        .place_overlay(0, 16, ore, 0);
    visit(&mut sim, 2);
    assert!(
        !sim.anim(id).unwrap().runtime.inactive,
        "matching physical AnimType survives"
    );
    let frame = sim.anim(id).unwrap().runtime.current_frame;
    sim.overlay_grid_mut_for_test()
        .as_mut()
        .unwrap()
        .clear_overlay(0, 16);
    visit(&mut sim, 3);
    let anim = sim.anim(id).unwrap();
    assert!(anim.runtime.inactive);
    assert_eq!(
        anim.runtime.current_frame,
        frame + 1,
        "mismatch does not cut this visit"
    );
    assert!(!sim.substrate.pending_delete.contains_object(&id));
    visit(&mut sim, 4);
    assert!(sim.substrate.pending_delete.contains_object(&id));
    assert_eq!(sim.anim(id).unwrap().owner_entity(), None);
}

/// The native inactive gate42435F follows bounce landing, but TrailerAnim
/// has its own alive/inactive guard4242A6. Reuse the last two updates of the
/// original DBRIS1LG flight rather than calculating a new landing golden.
#[test]
fn inactive_tiberium_anim_lands_before_retirement_without_an_ordinary_trailer() {
    use crate::util::native_x87::NativeF32Bits;

    let art_ini = IniFile::from_str(
        "[CHUNK]\nIsAnimatedTiberium=yes\nBouncer=yes\nElasticity=0\n\
         MaxXYVel=.5\nMinZVel=25\nExpireAnim=BOOM\nSpawns=CHILD\nSpawnCount=1\n\
         TrailerAnim=TRAIL\nTrailerSeperation=2\nRate=900\nEnd=30\n\
         [MARK]\nIsAnimatedTiberium=yes\nTrailerAnim=TRAIL\n\
         TrailerSeperation=2\nRate=900\nEnd=30\n\
         [BOOM]\nRate=900\nEnd=2\n[CHILD]\nRate=900\nEnd=2\n\
         [TRAIL]\nRate=900\nEnd=2\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(
            "[General]\nDamageFireTypes=\n[Animations]\n\
             0=CHUNK\n1=MARK\n2=BOOM\n3=CHILD\n4=TRAIL\n",
        ),
        &art_ini,
    )
    .unwrap();
    let mut art = art_registry(&art_ini);
    for name in ["CHUNK", "MARK", "BOOM", "CHILD", "TRAIL"] {
        art.bind_anim_frame_count_for_test(name, 30);
    }
    rules.install_art_fixture(art);
    let overlays = rules.overlay_registry().clone();
    let native = crate::test_fixture::json("tools/spatial_oracle/anim_bouncer_flight.json");
    let flight = native
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"] == serde_json::json!({"seed":1,"terrain":"flat0","type":"DBRIS1LG"})
        })
        .unwrap();
    let ticks = flight["ticks"].as_array().unwrap();
    let before = &ticks[ticks.len() - 3];
    let coord = |tick: &serde_json::Value| AnimWorldCoord {
        x: tick[1].as_i64().unwrap() as i32,
        y: tick[2].as_i64().unwrap() as i32,
        z: tick[3].as_i64().unwrap() as i32,
    };
    let mut sim = super::meteor_tests::formed_sim(1, 0);
    let spawn = |sim: &mut Simulation, name| {
        let type_name = sim.interner.intern(name);
        sim.spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor::new(type_name, 0, 0, SimFixed::ZERO, SimFixed::ZERO, 0),
            coord(before),
        )
        .unwrap()
    };
    let chunk = spawn(&mut sim, "CHUNK");
    let marker = spawn(&mut sim, "MARK");
    let visit = |sim: &mut Simulation, id, frame| {
        sim.session.set_binary_frame_for_test(frame);
        sim.visit_anim(id, &rules, Some(&overlays), FrameEffects::default())
            .unwrap();
    };
    visit(&mut sim, chunk, 1001);
    visit(&mut sim, marker, 1001); // Odd frame: clear first guards without a trailer.
    let body = sim.anim(chunk).unwrap().bounce.unwrap();
    let floats = |offset: usize| -> [NativeF32Bits; 3] {
        std::array::from_fn(|axis| {
            NativeF32Bits::from_bits(before[offset + axis].as_u64().unwrap() as u32)
        })
    };
    let anim = sim.substrate.anims.get_mut(chunk).unwrap();
    anim.bounce = Some(BounceState::from_native_for_test(
        body.elasticity(),
        body.gravity(),
        body.angular_velocity_magnitude(),
        floats(4),
        floats(7),
        body.orientation(),
        body.rotation(),
    ));
    anim.world_coord = coord(before);
    visit(&mut sim, chunk, 1003);
    visit(&mut sim, marker, 1003);
    assert_eq!(
        sim.anim(chunk).unwrap().world_coord(),
        coord(&ticks[ticks.len() - 2])
    );
    for id in [chunk, marker] {
        assert!(sim.anim(id).unwrap().runtime.inactive);
        assert!(!sim.substrate.pending_delete.contains_object(&id));
    }
    let mut expected_rng = sim.scenario_rng.clone();
    visit(&mut sim, marker, 1004); // A nonlanding parent exposes the trailer guard.
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected_rng.native_state_hex()
    );
    visit(&mut sim, chunk, 1004);
    expected_rng.next_range_i32_inclusive(0, 1);
    expected_rng.next_range_i32_inclusive(0, 1);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected_rng.native_state_hex()
    );
    let landed = coord(&ticks[ticks.len() - 1]);
    assert_eq!(sim.anim(chunk).unwrap().world_coord(), landed);
    assert!(sim.anims().any(|(_, anim)| {
        sim.interner.resolve(anim.type_id()) == "BOOM"
            && anim.world_coord() == landed
            && anim.in_logic_vector()
    }));
    assert!(
        sim.anims()
            .all(|(_, anim)| sim.interner.resolve(anim.type_id()) != "TRAIL")
    );
    for id in [chunk, marker] {
        assert!(sim.substrate.pending_delete.contains_object(&id));
        assert!(!sim.anim(id).unwrap().in_logic_vector());
    }
}
