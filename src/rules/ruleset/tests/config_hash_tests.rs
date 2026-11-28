//! Tests of what `RuleSet::simulation_config_hash` and
//! `RuleSet::file_inputs_hash` cover.

use super::*;

#[test]
fn simulation_config_hash_covers_registered_slot_names_offsets_and_runtime() {
    let ini = IniFile::from_str("[BuildingTypes]\n0=B\n[B]\nImage=BODY\n[Animations]\n0=N\n1=D\n");
    let make = |slot: &str, runtime: &str| {
        let art_ini = IniFile::from_str(&format!(
            "[BODY]\nActiveAnim=N\n{slot}\n[N]\n{runtime}\n[D]\n"
        ));
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
        let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
        rules.install_art_data(art);
        rules
    };
    let original = make("", "Rate=900");
    for (slot, runtime) in [
        ("ActiveAnimDamaged=D", "Rate=900"),
        ("ActiveAnimX=1", "Rate=900"),
        ("ActiveAnimY=-1", "Rate=900"),
        ("ActiveAnimZAdjust=7", "Rate=900"),
        ("ActiveAnimYSort=2", "Rate=900"),
        ("", "Rate=450"),
        ("", "RandomRate=900,300"),
        ("", "Next=D"),
    ] {
        let changed = make(slot, runtime);
        assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
        assert_ne!(
            original.simulation_config_hash(),
            changed.simulation_config_hash(),
            "{slot}, {runtime}"
        );
    }
    let mut changed = make("", "Rate=900");
    changed.anim_type_names.insert("ADDED".into());
    assert_ne!(
        original.simulation_config_hash(),
        changed.simulation_config_hash()
    );
}

#[test]
fn simulation_config_hash_covers_effective_building_body_art() {
    use crate::rules::art_data::ArtRegistry;
    let ini = IniFile::from_str("[BuildingTypes]\n0=BUILD\n[BUILD]\nImage=BODY\nGate=yes\n");
    let make = |art: &str| {
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art)));
        rules
    };
    let defaults =
        "GateStages=9\nAnimIdle=0,1,0\nAnimActive=0,1,0\nAnimAux1=0,1,0\nAnimAux2=0,1,0\n";
    let original = make(&format!("[BODY]\n{defaults}"));
    let reordered = make(&format!("[UNUSED]\nGateStages=-1\n[BODY]\n{defaults}"));
    assert_eq!(
        original.simulation_config_hash(),
        reordered.simulation_config_hash(),
        "unused art and section insertion order do not alter resolved body inputs"
    );
    assert_eq!(
        make("[UNUSED]\nGateStages=99\n").simulation_config_hash(),
        original.simulation_config_hash(),
        "missing metadata and explicit constructor defaults are equivalent"
    );
    assert_eq!(
        make(&format!(
            "[BODY]\n{defaults}PowerUp1Anim=UNCONSUMED\nPowerUp1LocXX=123\n"
        ))
        .simulation_config_hash(),
        original.simulation_config_hash(),
        "flat PowerUp declarations cannot replace the retained Type inputs"
    );
    for changed in [
        "GateStages=-1\n",
        "AnimIdle=-1,1,0\n",
        "AnimActive=0,2147483647,0\n",
        "AnimAux1=0,1,-2\n",
        "AnimAux2=65536,1,0\n",
    ] {
        let changed = make(&format!("[BODY]\n{changed}"));
        assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
        assert_ne!(
            original.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }
    let mut firestorm = make(&format!("[BODY]\n{defaults}"));
    firestorm.object_list[0].firestorm_wall = true;
    assert_ne!(
        original.simulation_config_hash(),
        firestorm.simulation_config_hash()
    );
}

#[test]
fn simulation_config_hash_changes_with_resolved_effect_frame_count() {
    let ini = IniFile::from_str("[General]\nWarpOut=WARPOUT\n");
    let mut first = RuleSet::from_ini(&ini).expect("first rules");
    let mut second = RuleSet::from_ini(&ini).expect("second rules");

    first.set_effect_frame_count_for_test("WARPOUT", 5, 5);
    second.set_effect_frame_count_for_test("WARPOUT", 6, 6);

    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

#[test]
fn simulation_config_hash_changes_with_terrain_spawner_frame_count() {
    let ini = IniFile::from_str(
        "[TerrainTypes]\n0=TIBTRE01\n\
             [TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n",
    );
    let mut first = RuleSet::from_ini(&ini).expect("first rules");
    let mut second = RuleSet::from_ini(&ini).expect("second rules");

    first.set_terrain_spawner_frame_count_for_test("TIBTRE01", 22);
    second.set_terrain_spawner_frame_count_for_test("TIBTRE01", 24);

    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

#[test]
fn simulation_config_hash_covers_retained_projectile_color() {
    let make = |color: &str| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Colors]\nFirst=0,0,0\nSecond=1,2,3\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nColor={color}\n",
            )))
            .unwrap()
    };
    let first = make("First");
    let mut second = make("Second");
    assert_eq!(first.projectile("SHOT").unwrap().color_scheme(), 1);
    assert_eq!(second.projectile("SHOT").unwrap().color_scheme(), 3);
    // Isolate the effective retained field from the raw-text fingerprint.
    second.source_ini_hash = first.source_ini_hash;
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

/// The ordered processor retains each reference and the weapon range
/// through missing keys and later passes, so equal current text can carry
/// different values. Each one is part of the identity.
#[test]
fn simulation_config_hash_covers_retained_references_and_range() {
    fn unit(rules: &mut RuleSet) -> &mut ObjectType {
        rules
            .object_list
            .iter_mut()
            .find(|object| object.id == "UNIT")
            .unwrap()
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=YARD\n[YARD]\nStrength=100\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
             [GUN]\nRange=5\nProjectile=SHOT\nWarhead=WH\n[SHOT]\n[WH]\n\
             [SuperWeaponTypes]\n0=NUKE\n[NUKE]\nType=MultiMissile\n\
             [Particles]\n0=GAS\n[GAS]\n[VoxelAnims]\n0=PIECE\n[PIECE]\n",
        ))
        .unwrap();
    let base = rules.simulation_config_hash();
    let name = || Some("NAME".to_owned());
    let names = || vec!["NAME".to_owned()];
    let edits: Vec<(&str, fn(&mut RuleSet))> = vec![
        ("Image", |rules| unit(rules).image = "OTHER".into()),
        ("DeathWeapon", |rules| {
            unit(rules).death_weapon = Some("GUN".into());
        }),
        ("DeploysInto", |rules| {
            unit(rules).deploys_into = Some("GUN".into());
        }),
        ("OccupyWeapon", |rules| {
            unit(rules).occupy_weapon = Some("GUN".into());
        }),
        ("EliteOccupyWeapon", |rules| {
            unit(rules).elite_occupy_weapon = Some("GUN".into());
        }),
        ("Range", |rules| {
            rules.weapon_mut_for_test("GUN").unwrap().range_leptons = 1;
        }),
        ("ShrapnelWeapon", |rules| {
            rules.projectile_mut("SHOT").unwrap().shrapnel_weapon = Some("GUN".into());
        }),
        ("WeaponType", |rules| {
            rules.super_weapons.get_mut("NUKE").unwrap().weapon_type = Some("GUN".into());
        }),
    ];
    for (key, edit) in edits {
        let mut changed = rules.clone();
        edit(&mut changed);
        assert_ne!(changed.simulation_config_hash(), base, "{key}");
    }

    // The references the ordered processor alone reads since part 3.
    type Edit = Box<dyn Fn(&mut RuleSet)>;
    let unit_edit = |set: fn(&mut ObjectType)| -> Edit { Box::new(move |rules| set(unit(rules))) };
    let edits: Vec<(&str, Edit)> = vec![
        (
            "UndeploysInto",
            unit_edit(|unit| unit.undeploys_into = Some("NAME".into())),
        ),
        (
            "PowersUnit",
            unit_edit(|unit| unit.powers_unit = Some("NAME".into())),
        ),
        (
            "FreeUnit",
            unit_edit(|unit| unit.free_unit = Some("NAME".into())),
        ),
        (
            "ToOverlay",
            unit_edit(|unit| unit.to_overlay = Some("NAME".into())),
        ),
        ("Dock", unit_edit(|unit| unit.dock = vec!["NAME".into()])),
        (
            "UnloadingClass",
            unit_edit(|unit| unit.unloading_class = Some("NAME".into())),
        ),
        (
            "DeployingAnim",
            unit_edit(|unit| unit.deploying_anim = Some("NAME".into())),
        ),
        (
            "Enslaves",
            unit_edit(|unit| unit.enslaves = Some("NAME".into())),
        ),
        (
            "Spawns",
            unit_edit(|unit| unit.spawns = Some("NAME".into())),
        ),
        (
            "Explosion",
            unit_edit(|unit| unit.explosion_anims = vec!["NAME".into()]),
        ),
        (
            "DestroyAnim",
            unit_edit(|unit| unit.destroy_anims = vec!["NAME".into()]),
        ),
        (
            "DeadBodies",
            unit_edit(|unit| unit.dead_bodies = vec!["NAME".into()]),
        ),
        (
            "NaturalParticleSystem",
            unit_edit(|unit| unit.natural_particle_system = Some("NAME".into())),
        ),
        (
            "RefinerySmokeParticleSystem",
            unit_edit(|unit| unit.refinery_smoke_particle_system = Some("NAME".into())),
        ),
        (
            "DamageParticleSystems",
            unit_edit(|unit| unit.damage_particle_systems = vec!["NAME".into()]),
        ),
        (
            "DestroyParticleSystems",
            unit_edit(|unit| unit.destroy_particle_systems = vec!["NAME".into()]),
        ),
        (
            "DebrisTypes",
            unit_edit(|unit| unit.debris_types = vec!["NAME".into()]),
        ),
        (
            "DebrisAnims",
            unit_edit(|unit| unit.debris_anims = vec!["NAME".into()]),
        ),
        (
            "AuxBuilding",
            Box::new(move |rules| {
                rules.super_weapons.get_mut("NUKE").unwrap().aux_building = name()
            }),
        ),
        (
            "Anim",
            Box::new(move |rules| rules.weapon_mut_for_test("GUN").unwrap().anim = names()),
        ),
        (
            "OccupantAnim",
            Box::new(move |rules| rules.weapon_mut_for_test("GUN").unwrap().occupant_anim = name()),
        ),
        (
            "OpenToppedAnim",
            Box::new(move |rules| {
                rules.weapon_mut_for_test("GUN").unwrap().open_topped_anim = name()
            }),
        ),
        (
            "AttachedParticleSystem",
            Box::new(move |rules| {
                rules
                    .weapon_mut_for_test("GUN")
                    .unwrap()
                    .attached_particle_system = name();
            }),
        ),
        (
            "Warhead DebrisTypes",
            Box::new(move |rules| rules.warhead_mut("WH").unwrap().debris_types = names()),
        ),
        (
            "Particle Warhead",
            Box::new(move |rules| rules.particle_types[0].warhead = name()),
        ),
        (
            "VoxelAnim Spawns",
            Box::new(move |rules| rules.voxel_anim_types[0].spawns = name()),
        ),
        (
            "VoxelAnim BounceAnim",
            Box::new(move |rules| rules.voxel_anim_types[0].bounce_anim = name()),
        ),
        (
            "VoxelAnim ExpireAnim",
            Box::new(move |rules| rules.voxel_anim_types[0].expire_anim = name()),
        ),
        (
            "VoxelAnim TrailerAnim",
            Box::new(move |rules| rules.voxel_anim_types[0].trailer_anim = name()),
        ),
        (
            "VoxelAnim Warhead",
            Box::new(move |rules| rules.voxel_anim_types[0].warhead = name()),
        ),
        (
            "VoxelAnim AttachedSystem",
            Box::new(move |rules| rules.voxel_anim_types[0].attached_system = name()),
        ),
        (
            "AnimType references",
            Box::new(|rules| {
                rules.anim_type_references.push((
                    "ANIM".into(),
                    crate::rules::native_processing::AnimTypeReferences::default(),
                ));
            }),
        ),
        (
            "C4Warhead",
            Box::new(|rules| rules.bridge_warheads.c4_name = "NAME".into()),
        ),
        (
            "CrushWarhead",
            Box::new(|rules| rules.bridge_warheads.crush_name = "NAME".into()),
        ),
        (
            "IonCannonWarhead",
            Box::new(|rules| rules.bridge_warheads.ion_cannon_name = "NAME".into()),
        ),
        (
            "IvanWarhead",
            Box::new(move |rules| rules.combat_damage.ivan_warhead = name()),
        ),
        (
            "DrainAnimationType",
            Box::new(move |rules| rules.combat_damage.drain_animation_type = name()),
        ),
        (
            "ControlledAnimationType",
            Box::new(move |rules| rules.mind_control.controlled_anim = name()),
        ),
        (
            "PermaControlledAnimationType",
            Box::new(move |rules| rules.mind_control.perma_controlled_anim = name()),
        ),
        (
            "RadSiteWarhead",
            Box::new(|rules| rules.radiation.site_warhead = "NAME".into()),
        ),
        (
            "MutateWarhead",
            Box::new(|rules| rules.general.mutate_warhead = "NAME".into()),
        ),
        (
            "MutateExplosionWarhead",
            Box::new(|rules| rules.general.mutate_explosion_warhead = "NAME".into()),
        ),
        (
            "EMPulseWarhead",
            Box::new(|rules| rules.general.empulse_warhead = Some("NAME".into())),
        ),
        (
            "OreTwinkle",
            Box::new(move |rules| rules.general.ore_twinkle = name()),
        ),
        (
            "DominatorWarhead",
            Box::new(|rules| rules.general.dominator_warhead = "NAME".into()),
        ),
        (
            "Engineer",
            Box::new(move |rules| rules.general.engineer_infantry = name()),
        ),
        (
            "DamageFireTypes",
            Box::new(|rules| {
                rules.general.damage_fire_types = vec![AnimRef {
                    name: "NAME".into(),
                }];
            }),
        ),
        (
            "DeadBodies",
            Box::new(move |rules| rules.general.dead_bodies = names()),
        ),
        (
            "BuildConst",
            Box::new(|rules| rules.set_build_const_types(&["YARD".into()])),
        ),
        (
            "ConcreteWalls",
            Box::new(move |rules| rules.concrete_wall_types = names()),
        ),
        (
            "NSGates",
            Box::new(move |rules| rules.ns_gate_types = names()),
        ),
        (
            "EWGates",
            Box::new(move |rules| rules.ew_gate_types = names()),
        ),
        (
            "CrateImg",
            Box::new(move |rules| rules.crate_rules.crate_img = name()),
        ),
        (
            "CrateMaximum",
            Box::new(|rules| rules.crate_rules.maximum = 7),
        ),
        (
            "V3RocketDamage",
            Box::new(|rules| rules.missile_spawn.v3.damage = 7),
        ),
    ];
    for (key, edit) in edits {
        let mut changed = rules.clone();
        edit(&mut changed);
        assert_ne!(changed.simulation_config_hash(), base, "{key}");
    }
    for (key, publish) in crate::rules::rules_type_references::DEFAULT_PARTICLE_SYSTEMS {
        let mut changed = rules.clone();
        publish(&mut changed, "NAME");
        assert_ne!(changed.simulation_config_hash(), base, "{key}");
    }
}

#[test]
fn simulation_config_hash_covers_effective_projectile_launch_art_and_load() {
    use crate::rules::art_data::ArtRegistry;
    use crate::sim::snapshot::{GameSnapshot, SnapshotError};
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=SHOTART\n[BuildingTypes]\n0=BUILD\n[BUILD]\nImage=BUILDART\n",
    );
    let make = |art: &str| {
        let mut rules =
            RuleSet::from_ini_with_fixed_art_for_test(&ini, &IniFile::from_str(art)).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art)));
        rules
    };
    let first = make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=4\n[UNUSED]\nHeight=9\n");
    let reordered = make(
        "[UNUSED]\nHeight=200\nVoxel=yes\nFlat=no\n[BUILDART]\nHeight=4\n[SHOTART]\nVoxel=yes\nFlat=yes\n",
    );
    let voxel_changed = make("[SHOTART]\nVoxel=no\nFlat=yes\n[BUILDART]\nHeight=4\n");
    let height_changed = make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=5\n");
    let flat_changed = make("[SHOTART]\nVoxel=yes\nFlat=no\n[BUILDART]\nHeight=4\n");
    assert!(first.projectile("SHOT").unwrap().voxel);
    assert!(first.projectile("SHOT").unwrap().flat);
    assert_eq!(
        first.building_launch_height(first.object("BUILD").unwrap()),
        4
    );
    assert_eq!(
        first.simulation_config_hash(),
        reordered.simulation_config_hash(),
        "canonical consumed inputs ignore ART section order and unused metadata"
    );
    for changed in [&voxel_changed, &height_changed, &flat_changed] {
        assert_eq!(first.source_ini_hash(), changed.source_ini_hash());
        assert_ne!(
            first.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }
    let absent = make("[UNUSED]\nHeight=999\n");
    let explicit_default = make("[SHOTART]\nVoxel=no\nFlat=no\n[BUILDART]\nHeight=2\n");
    assert_eq!(
        absent.simulation_config_hash(),
        explicit_default.simulation_config_hash(),
        "authored presence alone is not a consumed launch input"
    );
    let mut retained = make("[SHOTART]\nVoxel=yes\nFlat=yes\n[BUILDART]\nHeight=4\n");
    retained.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
        "[BUILDART]\nHeight=4\n",
    )));
    assert_eq!(
        first.simulation_config_hash(),
        retained.simulation_config_hash(),
        "hash the retained effective ProjectileType value after an absent Voxel key"
    );
    let sim = crate::sim::world::Simulation::new();
    let bytes =
        GameSnapshot::save_validated(&sim, 11, first.simulation_config_hash(), "launch ART", 0);
    assert!(
        GameSnapshot::load_validated(
            &bytes,
            11,
            reordered.simulation_config_hash(),
            &sim.session.map_name()
        )
        .is_ok()
    );
    for changed in [&voxel_changed, &height_changed, &flat_changed] {
        assert!(matches!(
            GameSnapshot::load_validated(
                &bytes,
                11,
                changed.simulation_config_hash(),
                &sim.session.map_name()
            ),
            Err(SnapshotError::RulesMismatch { .. })
        ));
    }
}

#[cfg(feature = "phobos")]
#[test]
fn simulation_config_hash_covers_phobos_warhead_animation_rules() {
    let make = |keys: &str, general: &str| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\n{general}\n[Warheads]\n0=WH\n[WH]\nAnimList=A\n{keys}\n\
                 [Animations]\n0=A\n"
        )))
        .unwrap()
    };
    let base = make("", "");
    let mut per_warhead = make("AnimList.CreateAll=yes\n", "");
    let mut global = make("", "Conventional.IgnoreUnits=yes\n");
    per_warhead.source_ini_hash = base.source_ini_hash;
    global.source_ini_hash = base.source_ini_hash;
    assert_ne!(
        base.simulation_config_hash(),
        per_warhead.simulation_config_hash(),
        "a per-warhead animation key changes the simulation config"
    );
    assert_ne!(
        base.simulation_config_hash(),
        global.simulation_config_hash(),
        "a [General] animation default changes the simulation config"
    );
}

#[cfg(feature = "phobos")]
#[test]
fn simulation_config_hash_covers_warhead_jumpjet_overrides() {
    let make = |keys: &str| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Warheads]\n0=LIFT\n[LIFT]\nIsLocomotor=yes\nLocomotor=Jumpjet\n{keys}\n"
        )))
        .unwrap()
    };
    let base = make("");
    let mut changed = make("JumpjetSpeed=33\n");
    changed.source_ini_hash = base.source_ini_hash;
    assert_ne!(
        base.simulation_config_hash(),
        changed.simulation_config_hash(),
        "a warhead Jumpjet override changes the simulation config"
    );
}

#[cfg(feature = "phobos")]
#[test]
fn simulation_config_hash_covers_mind_control_enhancements() {
    let make = |general: &str, unit: &str, warhead: &str| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\n{general}\n[VehicleTypes]\n0=UNIT\n[UNIT]\n{unit}\n\
                 [Warheads]\n0=WH\n[WH]\n{warhead}\n[Animations]\n0=MIND\n"
        )))
        .unwrap()
    };
    let base = make("", "", "");
    let mut size = make("", "MindControlSize=2\n", "");
    let mut globals = make("MindControl.IgnoreSize=no\n", "", "");
    let mut anim = make("", "", "MindControl.Anim=MIND\n");
    size.source_ini_hash = base.source_ini_hash;
    globals.source_ini_hash = base.source_ini_hash;
    anim.source_ini_hash = base.source_ini_hash;
    assert_ne!(
        base.simulation_config_hash(),
        size.simulation_config_hash(),
        "a per-type MindControlSize changes the simulation config"
    );
    assert_ne!(
        base.simulation_config_hash(),
        globals.simulation_config_hash(),
        "a [General] IgnoreSize default changes the simulation config"
    );
    assert_ne!(
        base.simulation_config_hash(),
        anim.simulation_config_hash(),
        "a warhead MindControl.Anim changes the simulation config"
    );
}

#[test]
fn simulation_config_hash_covers_canonical_smudge_anim_dimensions() {
    let ini = IniFile::from_str("[InfantryTypes]\n[VehicleTypes]\n");
    let mut first = RuleSet::from_ini(&ini).expect("first rules");
    let mut reordered = RuleSet::from_ini(&ini).expect("reordered rules");
    let mut changed = RuleSet::from_ini(&ini).expect("changed rules");

    let mut first_art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[BIGCRATER]\nCrater=yes\n[SCORCH]\nScorch=yes\n",
    ));
    let mut reordered_art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[SCORCH]\nScorch=yes\n[BIGCRATER]\nCrater=yes\n",
    ));
    for art in [&mut first_art, &mut reordered_art] {
        let big = art.get_mut("BIGCRATER").expect("big crater art");
        big.frame_width = 61;
        big.frame_height = 51;
    }
    let mut changed_art = first_art.clone();
    let changed_big = changed_art
        .get_mut("BIGCRATER")
        .expect("changed crater art");
    changed_big.frame_width = 60;
    changed_big.frame_height = 50;

    first.install_art_data(first_art);
    reordered.install_art_data(reordered_art);
    changed.install_art_data(changed_art);

    assert_eq!(first.source_ini_hash(), reordered.source_ini_hash());
    assert_eq!(
        first.simulation_config_hash(),
        reordered.simulation_config_hash(),
        "art section insertion order must not affect compatibility"
    );
    assert_ne!(
        first.simulation_config_hash(),
        changed.simulation_config_hash()
    );
}

/// What a network start certifies of a game's picture and model files
/// follows what the simulation reads from them: how long a building unfolds,
/// a resource spawner's frames, whether a building leaves rubble, the length
/// of an animation that deals damage or that a deployment waits on, a loaded
/// model, the weather bolt's height and whether a scorching animation's
/// frame is large. A file that only looks different leaves it alone, so its
/// player starts with the others: another SHP supplying the same rubble
/// frame, an effect image's frame count, the length of an animation that
/// changes nothing but its picture, the height of one no reader measures and
/// a scorching frame redrawn a pixel larger.
#[test]
fn file_inputs_hash_follows_what_the_simulation_reads_from_files() {
    use crate::rules::building_rubble::RubbleArt;
    let ini = IniFile::from_str(
        "[General]\nWarpOut=WARPOUT\nWeatherConBolts=BOLT\n\
         [BuildingTypes]\n0=B\n[B]\nLeaveRubble=yes\n\
         [VehicleTypes]\n0=V\n[V]\nDeployingAnim=DEPLOY\n\
         [TerrainTypes]\n0=TIBTRE01\n[TIBTRE01]\nSpawnsTiberium=yes\nIsAnimated=yes\n\
         [Animations]\n0=FLASH\n1=BOLT\n2=BURN\n3=DEPLOY\n",
    );
    let art_ini = IniFile::from_str(
        "[FLASH]\nRate=900\n[BOLT]\nRate=900\n[BURN]\nDamage=5\nScorch=yes\n[DEPLOY]\nRate=900\n",
    );
    let hash = |change: &dyn Fn(&mut RuleSet)| {
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
        change(&mut rules);
        rules.file_inputs_hash()
    };
    let rubble = |shp: &str, shadow_frame| RubbleArt {
        shp: shp.to_owned(),
        body_frame: crate::rules::building_rubble::RUBBLE_BODY_FRAME,
        shadow_frame,
        picture: None,
    };
    let scorch_frame = |width: u16, height: u16| {
        move |rules: &mut RuleSet| {
            let burn = rules.art_entry_mut_for_test("BURN").unwrap();
            (burn.frame_width, burn.frame_height) = (width, height);
        }
    };
    let base = hash(&|_| {});
    assert_eq!(hash(&|_| {}), base);
    let with_rubble =
        hash(&|rules| rules.set_building_rubble_art_for_test("B", rubble("b.shp", 7)));
    for read in [
        hash(&|rules| rules.set_buildup_control_for_test("B", [0, 20, 1])),
        hash(&|rules| rules.set_terrain_spawner_frame_count_for_test("TIBTRE01", 24)),
        hash(&|rules| rules.bind_anim_frame_count_for_test("BURN", 12)),
        hash(&|rules| rules.bind_anim_frame_count_for_test("DEPLOY", 12)),
        hash(&|rules| rules.supply_loaded_voxel_for_test("V")),
        hash(&|rules| rules.bind_anim_shp_height_for_test("BOLT", 90)),
        hash(&scorch_frame(61, 51)),
        with_rubble,
    ] {
        assert_ne!(read, base);
    }
    assert_eq!(
        hash(&|rules| rules.set_building_rubble_art_for_test("B", rubble("bt.shp", 9))),
        with_rubble
    );
    for (drawn, what) in [
        (
            hash(&|rules| rules.set_effect_frame_count_for_test("WARPOUT", 6, 6)),
            "an effect image's frames",
        ),
        (
            hash(&|rules| rules.bind_anim_frame_count_for_test("FLASH", 12)),
            "a picture-only animation's frames",
        ),
        (
            hash(&|rules| rules.bind_anim_shp_height_for_test("BURN", 90)),
            "an unmeasured animation's height",
        ),
        (hash(&scorch_frame(60, 50)), "a small scorching frame"),
    ] {
        assert_eq!(drawn, base, "{what}");
    }
}

/// On the retail install, through the production binder: a game folder that
/// redraws a power plant's Buildup with the same frames leaves what a start
/// certifies as it is, and one that gives it fewer frames changes it, with
/// the construction control the simulation reads. The rules name the folder
/// file only then, as a refused start says which files change what it
/// certifies; a file that only redraws and the retail install name none.
#[test]
fn a_replaced_buildup_changes_the_file_inputs_only_with_its_frames() {
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    let (Some(mut rules), Some((root, mut retail))) = (
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art(),
        crate::rules::retail_ini_fixture::retail_assets(),
    ) else {
        return;
    };
    crate::map::theater::load_theater(&mut retail, "TEMPERATE").unwrap();
    let name = crate::rules::art_data::building_shp_names("GAPOWRMK", "TEMPERATE")
        .into_iter()
        .find(|name| retail.get_ref(name).is_some())
        .expect("retail GAPOWR buildup");
    let original = retail.get_ref(&name).unwrap().to_vec();
    let mut bound = |assets: &AssetManager| {
        rules.bind_building_buildup_assets(assets, "TEMPERATE");
        (
            (rules.file_inputs_hash(), rules.buildup_control("GAPOWR")),
            rules.file_input_folder_files(),
        )
    };
    let (base, retail_files) = bound(&retail);
    assert!(retail_files.is_empty(), "{retail_files:?}");
    let pack = crate::map::source::test_support::TestDirectory::new("file-inputs-pack");
    let with_pack = |bytes: &[u8]| {
        pack.write(&name, bytes);
        let mut assets =
            AssetManager::with_mod(&root, Some(pack.path()), MediaArchiveMode::STOCK_DIGITAL)
                .unwrap();
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        assets
    };
    let mut redrawn = original.clone();
    *redrawn.last_mut().unwrap() ^= 1;
    let (same, redrawn_files) = bound(&with_pack(&redrawn));
    assert_eq!(same, base);
    assert!(redrawn_files.is_empty(), "{redrawn_files:?}");
    let mut shorter = original;
    let frames = i16::from_le_bytes([shorter[6], shorter[7]]);
    shorter[6..8].copy_from_slice(&(frames - 2).to_le_bytes());
    let (changed, shorter_files) = bound(&with_pack(&shorter));
    assert_ne!(changed.0, base.0);
    assert_ne!(changed.1, base.1);
    assert_eq!(shorter_files, [name]);
}

/// Of two animations only a game folder supplies, the rules name the file of
/// the one whose frames the simulation reads (it deals damage) and not the
/// file of the one that changes nothing but its picture, whose frames no
/// start certifies.
#[test]
fn the_rules_name_the_folder_file_of_an_animation_the_simulation_reads() {
    let ini = IniFile::from_str("[Animations]\n0=BURN\n1=FLASH\n");
    let art_ini = IniFile::from_str("[BURN]\nDamage=5\n[FLASH]\nRate=900\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let folder = crate::map::source::test_support::TestDirectory::new("file-inputs-anims");
    // An SHP header: zero, width, height, then the frame count.
    for name in ["BURN.SHP", "FLASH.SHP"] {
        folder.write(name, &[0, 0, 4, 0, 4, 0, 6, 0]);
    }
    let assets =
        crate::assets::asset_manager::AssetManager::from_loose_root_for_test(folder.path());
    rules.bind_scheduler_anim_assets(
        &["BURN".to_owned(), "FLASH".to_owned()],
        &assets,
        "tem",
        "TEMPERATE",
    );
    let bound = |name| {
        rules
            .art()
            .anim_runtime_config(name)
            .and_then(|config| config.raw_shp_frame_count)
    };
    assert_eq!((bound("BURN"), bound("FLASH")), (Some(6), Some(6)));
    assert_eq!(rules.file_input_folder_files(), ["BURN.SHP"]);
}
