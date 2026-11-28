use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TerrainDamageEvent;
use crate::sim::combat::combat_aoe::AreaDamageReceiver;
use crate::sim::terrain_object::TerrainObjectState;
use crate::sim::world::Simulation;

fn fire_rules(sparky: bool) -> RuleSet {
    let rules_ini = IniFile::from_str(&format!(
        "[General]\nTreeStrength=100\nTreeFlammability=1.0\n\
         [AudioVisual]\nTreeFire=FIRE01,FIRE02\n\
         [TerrainTypes]\n0=TREE01\n1=TREE02\n2=TIBTRE01\n\
         [Warheads]\n0=SPARKWH\n\
         [TREE01]\nStrength=100\nArmor=wood\nImmune=no\nIsFlammable=yes\n\
         [TREE02]\nStrength=100\nArmor=wood\nImmune=no\n\
         [TIBTRE01]\nStrength=100\nArmor=wood\nImmune=no\nIsFlammable=yes\nSpawnsTiberium=yes\n\
         [SPARKWH]\nWood=yes\nSparky={}\nCellSpread=.5\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if sparky { "yes" } else { "no" },
    ));
    let art_ini =
        IniFile::from_str("[FIRE01]\nRate=1\nLoopCount=1\n\n[FIRE02]\nRate=1\nLoopCount=1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
        .expect("terrain fire rules and ART");
    let mut art = ArtRegistry::from_ini(&art_ini);
    art.bind_anim_frame_count_for_test("FIRE01", 8);
    art.bind_anim_frame_count_for_test("FIRE02", 8);
    rules.install_art_fixture(art);
    rules
}

fn plant(sim: &mut Simulation, id: u64, type_name: &str, cell: (u16, u16)) {
    let type_ref = sim.interner.intern(type_name);
    sim.terrain_objects.objects_mut_for_test().insert(
        id,
        TerrainObjectState::for_test(id, type_ref, cell.0, cell.1),
    );
    sim.terrain_objects
        .cell_index_mut_for_test()
        .insert(cell, id);
}

fn on_fire(sim: &Simulation, id: u64) -> bool {
    sim.terrain_objects.get(id).unwrap().on_fire()
}

/// Ares TerrainClass_ReceiveDamage_ForestFire (1004F4E0, hook 0x71B99E)
/// and Catch_Fire (0x71C5B0, gate 1004F4B0): a `Sparky=` hit sets an
/// `IsFlammable=` tree alight under one `TreeFire=` anim the tree owns, 80
/// leptons above it; a plain tree refuses. When that anim leaves, Finish_Fire_Death (0x71C6B0,
/// the SetOwnerObject callback 0x424BB0) puts the fire out.
#[test]
fn ares_a_sparky_hit_sets_a_flammable_tree_alight_until_its_fire_anim_ends() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for sparky in [true, false] {
        let rules = fire_rules(sparky);
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        plant(&mut sim, 700, "TREE01", (5, 5));
        plant(&mut sim, 701, "TREE02", (8, 8));
        let warhead_ref = sim.interner.intern("SPARKWH");
        let hit = |stable_id, (rx, ry)| {
            AreaDamageReceiver::Terrain(TerrainDamageEvent {
                damage_writeback: None,
                stable_id,
                rx,
                ry,
                damage: 10,
                distance_leptons: 0,
                warhead_ref,
                near_center_ic_isolation_eligible: false,
            })
        };
        sim.commit_noncombat_aoe_receivers(
            &rules,
            None,
            &[hit(700, (5, 5)), hit(701, (8, 8))],
            &mut crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.terrain_objects.get(700).unwrap().health(), 90);
        assert!(
            !on_fire(&sim, 701),
            "a tree without IsFlammable never burns"
        );
        let fires = sim.substrate.anims.owned_by_terrain(700);
        if !sparky {
            assert!(!on_fire(&sim, 700) && fires.is_empty());
            continue;
        }
        assert!(on_fire(&sim, 700));
        assert_eq!(fires.len(), 1);
        let fire = sim.anim(fires[0]).unwrap();
        assert!(["FIRE01", "FIRE02"].contains(&sim.interner.resolve(fire.type_id())));
        let tree = sim.terrain_objects.get(700).unwrap().world_coord();
        let at = fire.world_coord();
        assert_eq!(
            (at.x, at.y, at.z),
            (tree.x, tree.y, tree.z + 80),
            "Catch_Fire 0x0071C638 raises the fire 80 leptons"
        );
        assert!(sim.substrate.anims.owned_by_terrain(701).is_empty());

        sim.destroy_anim_with_context(fires[0], &rules);
        assert!(
            !on_fire(&sim, 700),
            "the last fire anim's detach ends the fire"
        );
    }
}

/// Phobos `TerrainClass_FireOut_Crumbling` (`0x71C6EE`,
/// `Ext/TerrainType/Hooks.cpp:239-257`): `TerrainClass::Finish_Fire_Death`
/// (`0x0071C6B0`) clears IsOnFire and its zero-Health tail
/// (`0x0071C6CD..0x0071C72B`) returns into `Start_To_Crumble` (`0x71C6F8`) for
/// a `HasCrumblingFrames=` type, instead of the ordinary vanish a type without
/// the key takes. The tree is left at zero Health but still represented, so the
/// crumble keeps it alive and `TerrainClass_AI_Crumbling` (`0x71C812`) runs it
/// out afterwards; the Stage restarts at rate 2 / value 0.
#[test]
fn a_burning_tree_with_crumbling_frames_crumbles_instead_of_vanishing() {
    use crate::sim::terrain_spawn::tick_terrain_object_ai;
    use crate::sim::world::{FrameEffects, SimSoundEvent};

    let mut rules = fire_rules(true);
    let mut tree = rules
        .terrain_object_type_case_insensitive("TREE01")
        .expect("tree type")
        .clone();
    tree.has_crumbling_frames = true;
    tree.crumbling_sound = Some("TreeCrumble".to_string());
    tree.animation_length = Some(4);
    rules.install_map_terrain_type(tree);
    // The sibling burns on the same warhead but carries no crumbling keys;
    // `fire_rules`' stock TREE02 is not flammable.
    let mut sibling = rules
        .terrain_object_type_case_insensitive("TREE02")
        .expect("sibling type")
        .clone();
    sibling.is_flammable = true;
    rules.install_map_terrain_type(sibling);
    // 8 SHP frames: the resting crumble cannot reach the removal midpoint.
    rules.set_terrain_spawner_frame_count_for_test("TREE01", 8);

    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    plant(&mut sim, 700, "TREE01", (5, 5));
    plant(&mut sim, 701, "TREE02", (8, 8));
    let warhead_ref = sim.interner.intern("SPARKWH");
    let hit = |stable_id, (rx, ry)| {
        AreaDamageReceiver::Terrain(TerrainDamageEvent {
            damage_writeback: None,
            stable_id,
            rx,
            ry,
            damage: 10,
            distance_leptons: 0,
            warhead_ref,
            near_center_ic_isolation_eligible: false,
        })
    };
    sim.commit_noncombat_aoe_receivers(
        &rules,
        None,
        &[hit(700, (5, 5)), hit(701, (8, 8))],
        &mut FrameEffects::default(),
    );
    assert!(
        on_fire(&sim, 700) && on_fire(&sim, 701),
        "both trees burn before the fire-out"
    );
    // The fire-out tail only runs on an object already at zero Health, and
    // VERA's on-fire lethal arm (`0x71BADC`) retires such a tree, so this
    // installs the state the native callback sees.
    for id in [700, 701] {
        sim.terrain_objects
            .objects_mut_for_test()
            .get_mut(&id)
            .expect("tree")
            .set_health_for_test(0);
    }
    for id in [700, 701] {
        let fire = sim.substrate.anims.owned_by_terrain(id)[0];
        sim.destroy_anim_with_context(fire, &rules);
    }

    assert!(
        sim.terrain_objects
            .get(700)
            .is_some_and(TerrainObjectState::is_live),
        "the crumbling tree stays for its crumble sequence"
    );
    assert!(sim.terrain_objects.is_crumbling(700));
    assert!(
        sim.terrain_objects.get(700).unwrap().in_logic_vector(),
        "the fire-out crumble joins Logic"
    );
    assert_eq!(
        sim.terrain_objects.animation_current_frame(700),
        Some(0),
        "Start_To_Crumble restarts the Stage at value 0"
    );
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(
                event,
                SimSoundEvent::VocAt { sound_id, .. } if sound_id.name_in(&rules) == "TreeCrumble"
            ))
            .count(),
        1,
        "CrumblingSound plays once, for the tree with the key"
    );

    // Rate 2: the Stage holds through frame 1 and steps once at frame 2.
    sim.session.set_binary_frame_for_test(1);
    tick_terrain_object_ai(&mut sim, 700, &rules, None, &mut FrameEffects::default())
        .expect("terrain AI");
    assert_eq!(sim.terrain_objects.animation_current_frame(700), Some(0));
    sim.session.set_binary_frame_for_test(2);
    tick_terrain_object_ai(&mut sim, 700, &rules, None, &mut FrameEffects::default())
        .expect("terrain AI");
    assert_eq!(
        sim.terrain_objects.animation_current_frame(700),
        Some(1),
        "the crumble Stage advances one frame later"
    );

    assert!(
        sim.terrain_objects
            .get(701)
            .is_some_and(TerrainObjectState::is_live)
            && !sim.terrain_objects.is_crumbling(701),
        "a type without HasCrumblingFrames never starts a crumble at fire-out"
    );
    assert_eq!(
        sim.terrain_objects.animation_current_frame(701),
        None,
        "no crumble Stage is seeded for the sibling"
    );
}

/// Ares TerrainClass_Update_ForestFire (1004F530, hook 0x71C7C2): a burning
/// tree spreads on a 1-in-100 draw to each unburnt neighbour that may burn
/// (with `TreeFlammability=1.0` every roll passes); a `SpawnsTiberium=`
/// neighbour refuses.
#[test]
fn ares_a_burning_tree_spreads_to_its_flammable_neighbours() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = fire_rules(true);
    let mut sim = Simulation::new();
    sim.resolve_type_handles(&rules);
    plant(&mut sim, 700, "TREE01", (5, 5));
    plant(&mut sim, 701, "TREE01", (6, 5));
    plant(&mut sim, 702, "TIBTRE01", (5, 6));
    plant(&mut sim, 703, "TREE01", (7, 5));
    assert!(sim.terrain_catch_fire(700, &rules));
    assert!(
        !sim.terrain_catch_fire(700, &rules),
        "a burning tree refuses"
    );
    let mut visits = 0;
    while !on_fire(&sim, 701) && visits < 10_000 {
        sim.ares_spread_forest_fire(700, &rules);
        visits += 1;
    }
    assert!(on_fire(&sim, 701), "the east neighbour catches fire");
    assert!(!on_fire(&sim, 702), "an ore tree never burns");
    assert!(!on_fire(&sim, 703), "only the eight neighbours are visited");
}
