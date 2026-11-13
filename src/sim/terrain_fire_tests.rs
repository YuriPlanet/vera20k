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
/// `IsFlammable=` tree alight under one `TreeFire=` anim the tree owns; a
/// plain tree refuses. When that anim leaves, Finish_Fire_Death (0x71C6B0,
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
            crate::sim::world::FrameEffects::default(),
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
        assert!(sim.substrate.anims.owned_by_terrain(701).is_empty());

        sim.destroy_anim_with_context(fires[0], &rules);
        assert!(
            !on_fire(&sim, 700),
            "the last fire anim's detach ends the fire"
        );
    }
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
