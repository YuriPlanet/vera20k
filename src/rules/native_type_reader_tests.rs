//! Per-pass Type reads the ordered Rules processor alone owns: the TechnoType
//! weapon block, ParticleSystemType HoldsWhat, the 25-byte Image that names a
//! Type's fixed ART section, the references that name another Type, from a
//! Type's rules or fixed ART section or from a RulesClass section, and
//! WeaponType Range.
use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ruleset::RuleSet;

/// ReadGeneral66F362..66F3F8 retains the BuildingType list at Rules+850.
#[test]
fn repair_bay_list_retains_resolved_identities_across_layers() {
    let base = IniFile::from_str(
        "[BuildingTypes]\n0=GADEPT\n1=NADEPT\n[General]\nRepairBay=gadept,none,nadept,gadept\n",
    );
    let mut layers = RulesLayerStack::new(base.clone());
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nRepairBay=\n"),
    );
    let retained = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(retained.repair_bay_types, ["GADEPT", "NADEPT", "GADEPT"]);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nRepairBay=nadept\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .repair_bay_types,
        ["NADEPT"]
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nRepairBay=<none>\n"),
    );
    assert!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .repair_bay_types
            .is_empty()
    );
    assert!(
        RuleSet::from_ini(&IniFile::from_str("[General]\n"))
            .unwrap()
            .repair_bay_types
            .is_empty()
    );
}

#[test]
fn retail_repair_bay_list_uses_the_stock_depots() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_eq!(
        retail.rules.repair_bay_types,
        ["GADEPT", "NADEPT", "CAOUTP"]
    );
}

/// TechnoType ReadINI's weapon block (`0x0071284A..0x00712AAD`) runs on every
/// reached pass with the live TurretCount, WeaponCount and ClearAllWeapons as
/// its defaults, and its ClearAllWeapons clear (`0x00712A8F`) follows either
/// branch.
#[test]
fn weapon_block_reads_each_reached_pass_from_the_live_counts() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=SWAP\n1=SHRINK\n2=CLEARED\n\
         [SWAP]\nTurretCount=1\nWeaponCount=2\nWeapon1=A\nWeapon2=B\n\
         [SHRINK]\nTurretCount=1\nWeaponCount=3\nWeapon3=C\n\
         [CLEARED]\nTurretCount=1\nWeaponCount=2\nWeapon1=A\nWeapon2=B\nClearAllWeapons=yes\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[SWAP]\nTurretCount=0\nPrimary=P\n[SHRINK]\nWeaponCount=1\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();

    // The second pass takes the Primary branch; slot 1 keeps Weapon2.
    let swap = rules.object("SWAP").unwrap();
    assert_eq!(swap.turret_count, 0);
    assert_eq!((swap.primary(), swap.secondary()), (Some("P"), Some("B")));
    // A smaller WeaponCount stops reading slot 3 without clearing it.
    let shrink = rules.object("SHRINK").unwrap();
    assert_eq!(shrink.weapon_count, 1);
    assert_eq!(shrink.weapon_at(2), Some("C"));
    // ClearAllWeapons nulls slots 0 and 1 after the Weapon%d loop too.
    let cleared = rules.object("CLEARED").unwrap();
    assert_eq!((cleared.primary(), cleared.secondary()), (None, None));
}

/// ParticleSystemType ReadINI (`0x0064430F..0x00644323`) rebinds +294 on every
/// reached pass through ParticleType's factory (`0x00645430`), which answers
/// -1 only for `<none>`: an omitted key binds an empty-named type and `none`
/// a section-less type named `none`, which RuleSet lists with its defaults.
#[test]
fn holds_what_rebinds_on_every_reached_pass() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[Particles]\n0=Smoke1\n1=Smoke2\n\
         [ParticleSystems]\n0=KEPT\n1=OMITTED\n2=CLEARED\n3=NAMEDNONE\n\
         [Smoke1]\nBehavesLike=Smoke\n[Smoke2]\nBehavesLike=Smoke\n\
         [KEPT]\nHoldsWhat=Smoke1\n[OMITTED]\nHoldsWhat=Smoke1\n\
         [CLEARED]\nHoldsWhat=Smoke1\n[NAMEDNONE]\nHoldsWhat=Smoke1\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[KEPT]\nHoldsWhat=Smoke2\n[OMITTED]\nLifetime=5\n\
             [CLEARED]\nHoldsWhat=<none>\n[NAMEDNONE]\nHoldsWhat=none\n",
        ),
    );
    let processed = layers.process().unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let holds_what = |system: &str| {
        let id = rules.ps_type_id_by_name(system).unwrap();
        rules.particle_system_type(id).holds_what
    };

    assert_eq!(holds_what("KEPT"), rules.p_type_id_by_name("Smoke2"));
    for system in ["OMITTED", "CLEARED"] {
        assert_eq!(holds_what(system), None, "{system}");
    }
    let named_none = rules.p_type_id_by_name("none");
    assert!(named_none.is_some());
    assert_eq!(holds_what("NAMEDNONE"), named_none);
    assert_eq!(
        processed.ini().section("Particles").unwrap().registry_ids(),
        ["Smoke1", "Smoke2", "", "none"]
    );
}

/// AnimType ReadINI reads `SpawnsParticle=` from the Anim's ART section with
/// no default (`0x00428716`), skips an empty read and hands any other name to
/// the same ParticleType factory (`0x00428730` -> `0x00645430`): `none`
/// allocates a ParticleType named `none`; `<none>` allocates nothing.
#[test]
fn spawns_particle_allocates_through_the_particle_factory() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Animations]\n0=NAMED\n1=NULLED\n2=EMPTY\n",
    ))
    .process_with_fixed_art(&IniFile::from_str(
        "[NAMED]\nSpawnsParticle=none\n[NULLED]\nSpawnsParticle=<none>\n\
         [EMPTY]\nSpawnsParticle=\n",
    ))
    .unwrap();
    let particles = processed
        .ini()
        .section("Particles")
        .expect("SpawnsParticle=none allocates a ParticleType");
    assert_eq!(particles.registry_ids(), ["none"]);
}

/// BuildingType's ToOverlay read finds its fixed ART section by ObjectType+1F8
/// (`0x004610DE`), the 25-byte Image buffer: a longer Image names the section
/// of its first 24 bytes, and the type keeps that cut Image.
#[test]
fn fixed_art_reads_use_the_25_byte_image() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=BLD\n[BLD]\nImage=ABCDEFGHIJKLMNOPQRSTUVWXYZ\n",
    ))
    .process_with_fixed_art(&IniFile::from_str(
        "[ABCDEFGHIJKLMNOPQRSTUVWX]\nToOverlay=CUTOVL\n\
         [ABCDEFGHIJKLMNOPQRSTUVWXYZ]\nToOverlay=FULLOVL\n",
    ))
    .unwrap();
    let overlays: Vec<_> = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::OverlayType)
        .map(|event| event.native_stored_id())
        .collect();
    assert_eq!(overlays, ["CUTOVL"]);
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        rules.object("BLD").unwrap().image,
        "ABCDEFGHIJKLMNOPQRSTUVWX"
    );
}

/// Every weapon pointer outside the weapon slots goes through
/// `WeaponTypeClass::FindOrAllocate @ 0x00772FA0`, which answers null for
/// `none` and `<none>` (#1559): TechnoType DeathWeapon (`0x00712308`),
/// InfantryType OccupyWeapon/EliteOccupyWeapon (`0x00524132`, `0x0052416B`),
/// `[CombatDamage] DeathWeapon` (`0x0066C59F`), SuperWeaponType WeaponType
/// (`0x006CEA7D`) and BulletType ShrapnelWeapon (`0x0046C305`). A later pass
/// that says `none` clears the weapon; one that omits the key keeps it.
#[test]
fn weapon_references_read_none_as_null() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[InfantryTypes]\n0=CLEARED\n1=KEPT\n\
         [CLEARED]\nDeathWeapon=Boom\nOccupyWeapon=Gun\nEliteOccupyWeapon=Gun\n\
         [KEPT]\nDeathWeapon=Boom\nOccupyWeapon=Gun\nEliteOccupyWeapon=Gun\n\
         [Boom]\nDamage=1\n[Gun]\nDamage=2\n\
         [CombatDamage]\nDeathWeapon=Boom\n\
         [SuperWeaponTypes]\n0=Nuke\n[Nuke]\nType=MultiMissile\nWeaponType=Boom\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nPrimary=Cannon\n\
         [Cannon]\nProjectile=Shell\n[Shell]\nShrapnelCount=3\nShrapnelWeapon=Gun\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[CLEARED]\nDeathWeapon=none\nOccupyWeapon=<none>\nEliteOccupyWeapon=none\n\
             [KEPT]\nStrength=1\n\
             [CombatDamage]\nDeathWeapon=<none>\n[Nuke]\nWeaponType=none\n\
             [Shell]\nShrapnelWeapon=none\n",
        ),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();

    let weapons = |id: &str| {
        let object = rules.object(id).unwrap();
        (
            object.death_weapon.as_deref(),
            object.occupy_weapon.as_deref(),
            object.elite_occupy_weapon.as_deref(),
        )
    };
    assert_eq!(weapons("CLEARED"), (None, None, None));
    assert_eq!(weapons("KEPT"), (Some("Boom"), Some("Gun"), Some("Gun")));
    assert_eq!(rules.combat_damage.death_weapon, None);
    assert_eq!(rules.super_weapon("Nuke").unwrap().weapon_type, None);
    assert_eq!(rules.projectile("Shell").unwrap().shrapnel_weapon, None);
}

/// TechnoType ReadINI reads `DeploysInto=` (`0x00713279..0x00713297`) through
/// BuildingType's factory (`0x004653C0`), which answers null for `none`: the
/// vehicle then deploys into nothing, where a present name would make it a
/// deployer for every `deploys_into` consumer.
#[test]
fn deploys_into_reads_none_as_null() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=YARD\n[YARD]\nStrength=1000\n\
         [VehicleTypes]\n0=CLEARED\n1=KEPT\n\
         [CLEARED]\nDeploysInto=YARD\n[KEPT]\nDeploysInto=yard\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[CLEARED]\nDeploysInto=none\n[KEPT]\nStrength=1\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();

    assert_eq!(rules.object("CLEARED").unwrap().deploys_into, None);
    // The factory finds the existing type case-insensitively.
    assert_eq!(
        rules.object("KEPT").unwrap().deploys_into.as_deref(),
        Some("YARD")
    );
}

/// Only InfantryType ReadINI reads `OccupyWeapon=` and `EliteOccupyWeapon=`
/// (`0x00524102..0x0052417F`): another TechnoType that names one keeps the
/// constructor's null.
#[test]
fn occupy_weapons_are_infantry_keys() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=GARRISON\n[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=BUNKER\n\
         [GARRISON]\nOccupyWeapon=Gun\nEliteOccupyWeapon=Gun\n\
         [TANK]\nOccupyWeapon=Gun\nEliteOccupyWeapon=Gun\n\
         [BUNKER]\nOccupyWeapon=Gun\nEliteOccupyWeapon=Gun\n[Gun]\nDamage=2\n",
    ))
    .unwrap();

    let weapons = |id: &str| {
        let object = rules.object(id).unwrap();
        (
            object.occupy_weapon.as_deref(),
            object.elite_occupy_weapon.as_deref(),
        )
    };
    assert_eq!(weapons("GARRISON"), (Some("Gun"), Some("Gun")));
    assert_eq!(weapons("TANK"), (None, None));
    assert_eq!(weapons("BUNKER"), (None, None));
}

/// Every retail RULESMD ParticleSystem the processor reads names a registered
/// particle, so none reaches the empty-named type. `SmallGreySmokeSys` is
/// allocated by `DamageParticleSystems=` but has no section, so it is never
/// read and keeps the constructor's -1.
#[test]
fn retail_particle_systems_bind_registered_particles() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let systems = rules.particle_system_type_count();
    assert!(systems > 0);
    let unbound: Vec<_> = (0..systems)
        .map(|index| {
            rules.particle_system_type(crate::rules::particle_system_type::ParticleSystemTypeId(
                index as u32,
            ))
        })
        .filter(|system| system.holds_what.is_none())
        .map(|system| system.name.as_str())
        .collect();
    assert_eq!(unbound, ["SmallGreySmokeSys"]);
}

/// `[CombatDamage] DeathWeapon=` is RulesClass state (+FDC): ReadCombatDamage
/// keeps it on an empty read (`0x0066C57E..0x0066C5A8`), so a continued
/// process whose current stack omits it uses the earlier scenario's weapon,
/// and identical current INIs must not hide that difference.
#[test]
fn continued_process_keeps_the_combat_damage_death_weapon() {
    let tail = RulesLayerStack::new(IniFile::from_str("[CombatDamage]\nMaxDamage=500\n"));
    let continued = |death_weapon: &str| {
        let prior = RulesLayerStack::new(IniFile::from_str(&format!(
            "[VehicleTypes]\n0=TANK\n[TANK]\nPrimary=BoomA\nSecondary=BoomB\n\
             [BoomA]\nDamage=1\n[BoomB]\nDamage=2\n\
             [CombatDamage]\nDeathWeapon={death_weapon}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = prior.into_ini_and_native_type_construction_trace();
        let processed = tail
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        RuleSet::from_processed_rules(&processed).unwrap()
    };
    let first = continued("BoomA");
    let second = continued("BoomB");

    assert_eq!(first.combat_damage.death_weapon.as_deref(), Some("BoomA"));
    assert_eq!(second.combat_damage.death_weapon.as_deref(), Some("BoomB"));
    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}

/// Retail RULESMD sets none of these references to `none` or `<none>`, spells
/// each referenced Type as it is registered and authors `OccupyWeapon=` only
/// on infantry, so the processor publishes exactly what each section authors
/// (gamemd's ReadString of the raw file), as the typed reads did.
#[test]
fn retail_references_publish_their_authored_names() {
    use crate::rules::retail_ini_fixture::{retail_ini, retail_unlayered_rules};
    let (Some(rules), Some(raw)) = (retail_unlayered_rules(), retail_ini("rulesmd.ini")) else {
        return;
    };
    let section = |id: &str| raw.section_or_empty(id);
    let authored = |id: &str, key: &str| {
        Some(section(id).read_string(key, "", 0x80)).filter(|value| !value.is_empty())
    };
    let mut counts = [0; 8];
    for object in rules.all_objects() {
        let id = object.id.as_str();
        let image = section(id).read_string("Image", id, 0x19);
        let references = [
            (&object.death_weapon, "DeathWeapon"),
            (&object.deploys_into, "DeploysInto"),
            (&object.occupy_weapon, "OccupyWeapon"),
            (&object.elite_occupy_weapon, "EliteOccupyWeapon"),
        ];
        assert_eq!(object.image, image, "{id}");
        counts[0] += usize::from(image != id);
        for (slot, (published, key)) in references.into_iter().enumerate() {
            assert_eq!(*published, authored(id, key), "{id} {key}");
            counts[slot + 1] += usize::from(published.is_some());
        }
    }
    for weapon in rules.weapons_iter() {
        let range = section(&weapon.id).read_range("Range", 0);
        assert_eq!(weapon.range_leptons, range, "{}", weapon.id);
        counts[5] += usize::from(range % 256 != 0);
    }
    for projectile in rules.projectiles_iter() {
        let authored = authored(&projectile.id, "ShrapnelWeapon");
        assert_eq!(projectile.shrapnel_weapon, authored, "{}", projectile.id);
        counts[6] += usize::from(authored.is_some());
    }
    for super_weapon in rules.super_weapons.values() {
        let authored = authored(&super_weapon.id, "WeaponType");
        assert_eq!(super_weapon.weapon_type, authored, "{}", super_weapon.id);
        counts[7] += usize::from(authored.is_some());
    }
    assert_eq!(
        rules.combat_damage.death_weapon,
        authored("CombatDamage", "DeathWeapon")
    );
    // Image, DeathWeapon, DeploysInto, OccupyWeapon, EliteOccupyWeapon,
    // fractional Range, ShrapnelWeapon, SuperWeapon WeaponType.
    assert_eq!(counts, [61, 12, 4, 3, 3, 43, 4, 1]);
}

/// Every TechnoType pointer and list [`TechnoTypeReferences`] keeps goes
/// through a FindOrAllocate that answers null for `none` and `<none>`
/// (BuildingType `FreeUnit=` `0x00460540`, `UnloadingClass=` `0x007146E8`,
/// `Enslaves=` through `0x0067BAC0`, the particle systems through
/// `0x00644890`; each list token likewise): a `none` pointer is null and a
/// list drops the token. VERA's typed reads kept a Type of that name.
#[test]
fn techno_references_read_none_as_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=YARD\n[YARD]\nFreeUnit=none\n\
         [VehicleTypes]\n0=TANK\n\
         [TANK]\nUnloadingClass=none\nEnslaves=<none>\nNaturalParticleSystem=none\n\
         RefinerySmokeParticleSystem=<none>\nExplosion=none,BOOM\nDestroyAnim=<none>,BOOM\n\
         DamageParticleSystems=none,SMOKE\nDestroyParticleSystems=none\n\
         DebrisTypes=none,TIRE\nDebrisAnims=<none>,BOOM\nDock=none,YARD\n\
         [InfantryTypes]\n0=GI\n[GI]\nDeadBodies=none,CORPSE\n",
    ))
    .unwrap();

    assert_eq!(rules.object("YARD").unwrap().free_unit, None);
    let tank = rules.object("TANK").unwrap();
    assert_eq!((&tank.unloading_class, &tank.enslaves), (&None, &None));
    assert_eq!(tank.natural_particle_system, None);
    assert_eq!(tank.refinery_smoke_particle_system, None);
    assert_eq!(tank.explosion_anims, ["BOOM"]);
    assert_eq!(tank.destroy_anims, ["BOOM"]);
    assert_eq!(tank.damage_particle_systems, ["SMOKE"]);
    assert!(tank.destroy_particle_systems.is_empty());
    assert_eq!(tank.debris_types, ["TIRE"]);
    assert_eq!(tank.debris_anims, ["BOOM"]);
    assert_eq!(tank.dock, ["YARD"]);
    assert_eq!(rules.object("GI").unwrap().dead_bodies, ["CORPSE"]);
}

/// WarheadType ReadINI's `DebrisTypes=` (`0x0075DAF5`) keeps only the tokens
/// VoxelAnimType's FindOrAllocate (`0x0074B960`) answers, which drops `none`
/// and `<none>` (`0x0075DB43`). The typed read kept them as names.
#[test]
fn warhead_debris_types_read_none_as_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Warheads]\n0=WH\n[WH]\nDebrisTypes=none,TIRE,<none>\n",
    ))
    .unwrap();

    assert_eq!(rules.warhead("WH").unwrap().debris_types, ["TIRE"]);
}

/// WeaponType ReadINI keeps only the `Anim=` tokens the AnimType factory
/// (`0x00428B80`) answers, which drops `none` and `<none>`, and reads
/// `OccupantAnim=` and `OpenToppedAnim=` the same way. `AttachedParticleSystem=`
/// goes through `ParticleSystemTypeClass::FindOrCreate @ 0x00644630`
/// (`0x00772929`): its -1 for `<none>` keeps the field (`0x00772931`), while
/// `none` names a system like any other spelling. VERA's typed reads kept every
/// name, which shifted the damage-indexed `Anim=` pick.
#[test]
fn weapon_anims_read_none_as_null() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nPrimary=GUN\nSecondary=ZAP\n\
         [GUN]\nAnim=none,GUNFIRE\nOccupantAnim=none\nOpenToppedAnim=<none>\n\
         AttachedParticleSystem=SPARKS\n\
         [ZAP]\nAttachedParticleSystem=none\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[GUN]\nAttachedParticleSystem=<none>\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();

    let gun = rules.weapon("GUN").unwrap();
    assert_eq!(gun.anim, ["GUNFIRE"]);
    assert_eq!((&gun.occupant_anim, &gun.open_topped_anim), (&None, &None));
    assert_eq!(gun.attached_particle_system.as_deref(), Some("SPARKS"));
    assert_eq!(
        rules
            .weapon("ZAP")
            .unwrap()
            .attached_particle_system
            .as_deref(),
        Some("none")
    );
}

/// ParticleType `Warhead=` (`0x00645043..0x00645067`) and the RulesClass
/// pointers of [`RulesTypeReferences`] go through factories that answer null
/// for `none` and `<none>`. VERA's typed reads kept those names, so a `none`
/// named a warhead or particle system nothing defines.
#[test]
fn rules_type_references_read_none_as_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=GAS\n[GAS]\nWarhead=none\n\
         [CombatDamage]\nIvanWarhead=none\nDefaultSparkSystem=<none>\nC4Warhead=none\n\
         CrushWarhead=<none>\nIonCannonWarhead=none\n\
         [Radiation]\nRadSiteWarhead=none\n\
         [SpecialWeapons]\nMutateWarhead=none\nMutateExplosionWarhead=<none>\n",
    ))
    .unwrap();

    let gas = rules.p_type_id_by_name("GAS").unwrap();
    assert_eq!(rules.particle_type(gas).warhead, None);
    assert_eq!(rules.combat_damage.ivan_warhead, None);
    assert_eq!(rules.combat_damage.default_spark_system, None);
    let bridge = &rules.bridge_warheads;
    assert_eq!(
        [&bridge.c4_name, &bridge.crush_name, &bridge.ion_cannon_name],
        ["", "", ""]
    );
    assert_eq!(rules.radiation.site_warhead, "");
    assert_eq!(
        [
            &rules.general.mutate_warhead,
            &rules.general.mutate_explosion_warhead
        ],
        ["", ""]
    );
}

/// The RulesClass constructor leaves `C4Warhead=`, `CrushWarhead=`,
/// `IonCannonWarhead=` and `RadSiteWarhead=` null (`0x00666B52`, `0x00666B58`,
/// `0x00666BD5`, `0x00667801`), and a reader keeps the field on an empty read
/// (RadSite: `0x0066D0EA` reloads it, `0x0066D127` stores it back). VERA's
/// typed reads invented `Super`, `Crush`, `IonCannonWH` and `RadSite`.
#[test]
fn rules_class_type_pointers_start_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nMaxDamage=500\nCrushWarhead=\n[Radiation]\nRadDurationMultiple=1\n",
    ))
    .unwrap();

    let bridge = &rules.bridge_warheads;
    assert_eq!(
        [&bridge.c4_name, &bridge.crush_name, &bridge.ion_cannon_name],
        ["", "", ""]
    );
    assert_eq!(rules.radiation.site_warhead, "");
}

/// AnimType ReadINI resolves its fixed-ART references through factories that
/// answer null for `none` and `<none>` (`0x00428045..0x0042867A`), and
/// `SpawnsParticle=` through ParticleType's (`0x00645430`), null only for
/// `<none>`. The ART registry's own reads kept `NONE` and `<NONE>` as names.
#[test]
fn anim_art_references_read_none_as_null() {
    let art = IniFile::from_str(
        "[EXPL]\nNext=none\nTrailerAnim=<none>\nExpireAnim=smoke\nWarhead=none\n\
         SpawnsParticle=<none>\nTiberiumSpawnType=none\n[SMOKE]\nRate=100\n",
    );
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[Animations]\n0=EXPL\n"),
        &art,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));

    let config = rules.art().anim_runtime_config("EXPL").unwrap();
    assert_eq!((&config.next, &config.trailer_anim), (&None, &None));
    assert_eq!(config.expire_anim.as_deref(), Some("SMOKE"));
    assert_eq!((&config.warhead, &config.spawns_particle), (&None, &None));
    assert_eq!(config.tiberium_spawn_type, None);
}

/// VoxelAnimType ReadINI resolves `Spawns=` through `VoxelAnimTypeClass::
/// FindOrCreate @ 0x0074BB20` (`0x0074B219`), which appends a VoxelAnimType for
/// a name [VoxelAnims] does not list. VERA's typed read kept the name with no
/// Type behind it.
#[test]
fn voxel_anim_spawns_allocate_a_voxel_anim_type() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VoxelAnims]\n0=METEOR\n[METEOR]\nSpawns=pebble\nSpawnCount=3\n",
    ))
    .unwrap();

    assert_eq!(rules.voxel_anim_type_count(), 2);
    let pebble = rules.voxel_anim_type_id_by_name("PEBBLE").unwrap();
    let meteor = rules.voxel_anim_type_id_by_name("METEOR").unwrap();
    assert_eq!(
        rules.voxel_anim_type(meteor).spawns.as_deref(),
        Some(rules.voxel_anim_type(pebble).name.as_str())
    );
}

/// The rocket types (`[General] <prefix>Type=`, AircraftType's factory
/// through `0x0067BD30`) and the six `[CombatDamage]` rocket warheads
/// (`0x0075E3B0`) answer null for `none` and `<none>`; an empty read keeps
/// the earlier pass's. VERA's second read kept `none` as a name.
#[test]
fn rocket_references_read_none_as_null() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nV3RocketType=V3ROCKET\nDMislType=DMISL\n\
         [CombatDamage]\nV3Warhead=V3WH\nDMislWarhead=DMISLWH\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[General]\nV3RocketType=none\nDMislType=\n\
             [CombatDamage]\nV3Warhead=<none>\nDMislEliteWarhead=none\n",
        ),
    );
    let missiles = layers.process().unwrap().missile_spawn().clone();

    assert_eq!(missiles.v3.type_name, "");
    assert_eq!(missiles.dmisl.type_name, "DMISL");
    assert_eq!(missiles.family_of("none"), None);
    assert_eq!([&missiles.v3.warhead, &missiles.v3.elite_warhead], ["", ""]);
    assert_eq!(
        [&missiles.dmisl.warhead, &missiles.dmisl.elite_warhead],
        ["DMISLWH", ""]
    );
}

/// BuildingType ReadINI reads `ToOverlay=` from the fixed ART section its
/// Image names (`0x004611F5..0x0046121F`) and nowhere else; the ART
/// projection's read fell back to the section of the type's ID.
#[test]
fn to_overlay_reads_only_the_image_section() {
    let art = IniFile::from_str("[WALL]\nToOverlay=WALLOVL\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str("[BuildingTypes]\n0=WALL\n[WALL]\nImage=MISSING\n"),
        &art,
    )
    .unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));

    assert_eq!(rules.object("WALL").unwrap().to_overlay, None);
}

/// A stack that sets RulesClass Type pointers and lists of every owner, and
/// crate and rocket scalars, for the process-lifetime tests below.
const RULES_CLASS_REFERENCES: &str = "\
[BuildingTypes]
0=GACNST
[GACNST]
Foundation=1x1
[General]
WarpIn=WARPA
DeadBodies=BODYA
MetallicDebris=DEBRISA
V3RocketType=V3R
V3RocketDamage=5
[AI]
BuildConst=GACNST
[CrateRules]
CrateImg=CRATEA
UnitCrateType=UNITA
CrateMaximum=7
[CombatDamage]
IvanWarhead=IvanWH
DefaultSparkSystem=SparkSys
C4Warhead=C4WH
ControlledAnimationType=MINDANIM
DrainAnimationType=DISKRAY
V3Warhead=V3WH
[Radiation]
RadSiteWarhead=RadWH
[SpecialWeapons]
MutateWarhead=Mutate
";

/// A stack that keeps GACNST and names none of the keys above.
const OMITS_RULES_CLASS_REFERENCES: &str = "\
[BuildingTypes]
0=GACNST
[GACNST]
Foundation=1x1
[General]
Unrelated=1
[CombatDamage]
MaxDamage=500
[Radiation]
RadDurationMultiple=1
";

/// Process [`OMITS_RULES_CLASS_REFERENCES`] after [`RULES_CLASS_REFERENCES`],
/// continuing its live state in `context`.
fn after_rules_class_references(context: RulesProcessContext) -> RuleSet {
    let prior = RulesLayerStack::new(IniFile::from_str(RULES_CLASS_REFERENCES))
        .process()
        .unwrap();
    let (_, trace) = prior.into_ini_and_native_type_construction_trace();
    let processed = RulesLayerStack::new(IniFile::from_str(OMITS_RULES_CLASS_REFERENCES))
        .process_with_fixed_art_and_registry_state_recovering(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
            context,
            &crate::rules::sound_ini::EvaRegistry::default(),
        )
        .unwrap();
    RuleSet::from_processed_rules(&processed).unwrap()
}

/// RulesClass is constructed once, by Init_Game (`0x0052BAD1` calls
/// `0x00665650`), and each reader keeps its field on an empty read, so a
/// Process call that continues the live state keeps every Type pointer and
/// list an earlier call read, the crate and rocket ones included. In
/// production only Full_Init's prepass continues the state, and the reset
/// after it clears these pointers before anything reads them (below). The
/// typed reads saw only the current merged INI, and the crate, rocket and
/// General Type lists restarted from the constructor on every call.
#[test]
fn continued_process_keeps_the_rules_class_type_pointers() {
    let rules = after_rules_class_references(RulesProcessContext::Ordinary);

    let damage = &rules.combat_damage;
    assert_eq!(damage.ivan_warhead.as_deref(), Some("IvanWH"));
    assert_eq!(damage.default_spark_system.as_deref(), Some("SparkSys"));
    assert_eq!(damage.drain_animation_type.as_deref(), Some("DISKRAY"));
    assert_eq!(rules.bridge_warheads.c4_name, "C4WH");
    assert_eq!(
        rules.mind_control.controlled_anim.as_deref(),
        Some("MINDANIM")
    );
    assert_eq!(rules.radiation.site_warhead, "RadWH");
    assert_eq!(rules.general.mutate_warhead, "Mutate");
    assert_eq!(rules.general.warp_in.name, "WARPA");
    assert_eq!(rules.general.dead_bodies, ["BODYA"]);
    assert_eq!(rules.general.metallic_debris, ["DEBRISA"]);
    assert_eq!(rules.build_const_types, ["GACNST"]);
    assert_eq!(rules.crate_rules.crate_img.as_deref(), Some("CRATEA"));
    assert_eq!(rules.crate_rules.unit_crate_type.as_deref(), Some("UNITA"));
    assert_eq!(rules.crate_rules.maximum, 7);
    let v3 = &rules.missile_spawn.v3;
    assert_eq!(
        (v3.type_name.as_str(), v3.warhead.as_str()),
        ("V3R", "V3WH")
    );
    assert_eq!(v3.damage, 5);
}

/// Full_Init's reset (`0x006686C0`) deletes the Types and leaves RulesClass
/// alone, so its scalars are the next scenario's defaults while its Type
/// pointers dangle. VERA clears every pointer and list there, the crate and
/// rocket ones with the others, and keeps the crate and rocket scalars, which
/// restarted from the constructor.
#[test]
fn scenario_reset_clears_the_rules_class_type_pointers_and_keeps_the_scalars() {
    let rules = after_rules_class_references(RulesProcessContext::ScenarioReset);

    assert_eq!(rules.combat_damage.ivan_warhead, None);
    assert_eq!(rules.bridge_warheads.c4_name, "");
    assert_eq!(rules.general.warp_in.name, "");
    assert!(rules.general.dead_bodies.is_empty());
    assert!(rules.general.metallic_debris.is_empty());
    assert!(rules.build_const_types.is_empty());
    assert_eq!(rules.crate_rules.crate_img, None);
    assert_eq!(rules.crate_rules.unit_crate_type, None);
    assert_eq!(rules.crate_rules.maximum, 7);
    let v3 = &rules.missile_spawn.v3;
    assert_eq!((v3.type_name.as_str(), v3.warhead.as_str()), ("", ""));
    assert_eq!(v3.damage, 5);
}

/// ReadGeneral's Type pointers (`0x0066D67B..0x0066FD2F`) and lists go
/// through factories that answer null for `none` and `<none>`, and a list
/// drops those tokens; so do ReadAI's lists. VERA's typed reads kept them as
/// names.
#[test]
fn general_type_references_read_none_as_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nOreTwinkle=none\nBarrelParticle=<none>\nNukeTakeOff=none\nWake=none\n\
         DominatorWarhead=none\nChronoBlast=<none>\nWarpIn=none\nIronCurtainInvokeAnim=none\n\
         InfantryExplode=none\nMoveFlash=<none>\nParachute=none\nEngineer=none\n\
         AlliedCrew=<none>\nDamageFireTypes=none,FIRE1\nDeadBodies=<none>,BODY1\n\
         BaseUnit=none,MCV\nPadAircraft=<none>\nShipyard=none\n",
    ))
    .unwrap();

    let general = &rules.general;
    assert_eq!(
        (&general.ore_twinkle, &general.barrel_particle),
        (&None, &None)
    );
    assert_eq!(
        [
            general.nuke_take_off.as_str(),
            &general.wake.name,
            &general.dominator_warhead,
            &general.chrono_blast_anim,
            &general.warp_in.name,
            &general.iron_curtain_invoke_anim,
            &general.move_flash.name,
        ],
        [""; 7]
    );
    assert_eq!(general.infantry_death_anims[3], None);
    assert_eq!(
        (&general.parachute_shp, &general.engineer_infantry),
        (&None, &None)
    );
    assert_eq!(general.allied_crew, None);
    let fire: Vec<&str> = general
        .damage_fire_types
        .iter()
        .map(|anim| anim.name.as_str())
        .collect();
    assert_eq!(fire, ["FIRE1"]);
    assert_eq!(general.dead_bodies, ["BODY1"]);
    assert_eq!(general.base_unit_types, ["MCV"]);
    assert!(general.pad_aircraft_types.is_empty());
    assert!(rules.shipyard_types.is_empty());
}

/// The RulesClass constructor leaves these pointers null and these vectors
/// empty (WarpIn..ChronoSparkle1 `0x00665B02..0x00665B14`, Wake `0x0066572F`,
/// MoveFlash `0x006665CF`, the infantry death anims `0x0066573B..0x0066575F`,
/// BaseUnit `0x00666515`). VERA's typed reads invented `WARPIN`, `WAKE1`,
/// `RING`, `S_BANG34` and the rest of retail's names, and `AMCV,SMCV,PCV`.
#[test]
fn general_type_references_start_null() {
    let rules = RuleSet::from_ini(&IniFile::from_str("[General]\nUnrelated=1\n")).unwrap();

    let general = &rules.general;
    assert_eq!(
        [
            general.warp_in.name.as_str(),
            &general.warp_out.name,
            &general.warp_away.name,
            &general.chrono_sparkle1.name,
            &general.wake.name,
            &general.move_flash.name,
        ],
        [""; 6]
    );
    for slot in [3, 4, 6, 7, 8, 9, 10] {
        assert_eq!(general.infantry_death_anims[slot], None, "{slot}");
    }
    assert!(general.base_unit_types.is_empty());
}

/// Each list entry is the Type its factory found or allocated: a token names
/// the registered Type, whatever its case, and a token longer than 24 bytes
/// names the Type of its first 24 (`AbstractType` `0x00410800`). VERA's typed
/// reads kept the authored token.
#[test]
fn type_lists_name_the_type_the_factory_found() {
    let long = "LONGBUILDINGNAMEOVER24BYTESX";
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[BuildingTypes]\n0=GACNST\n1={long}\n[GACNST]\nFoundation=1x1\n\
         [{long}]\nFoundation=1x1\n\
         [General]\nShipyard=gacnst\n[AI]\nBuildConst=gacnst\nBuildPower={long}\n"
    )))
    .unwrap();

    assert_eq!(rules.shipyard_types, ["GACNST"]);
    assert_eq!(rules.build_const_types, ["GACNST"]);
    assert_eq!(rules.build_power_types, [&long[..24]]);
}

/// Retail RULESMD over ARTMD, alone and under each of the nine mode INIs
/// `mpmodesmd.ini` names, through the production reader: every reference this
/// chain moved to the ordered processor, pinned as a digest of
/// [`published_references`]. It equals the digest of the values VERA's
/// deleted typed reads published, taken once on the base build. Each stack
/// also runs after Full_Init's reset of the previous one's live state, as a
/// later scenario does, and publishes the same references.
#[test]
fn retail_published_references_are_pinned() {
    use crate::rules::retail_ini_fixture::{art_registry, retail_ini, retail_rules_and_art};
    use crate::util::fnv::{FNV1A64_OFFSET_BASIS, fnv1a64_fold_bytes};
    let Some((rulesmd, artmd)) = retail_rules_and_art() else {
        return;
    };
    let modes = [
        None,
        Some("MPBattleMD.ini"),
        Some("MPCoopMD.ini"),
        Some("MPDuelMD.ini"),
        Some("MPFreeForAllMD.ini"),
        Some("MPMWMD.ini"),
        Some("MPMeatMD.ini"),
        Some("MPNavalMD.ini"),
        Some("MPTeamMD.ini"),
        Some("MPUnholyMD.ini"),
    ];
    let publish = |processed: &ProcessedRulesLayers| {
        let mut rules = RuleSet::from_processed_rules(processed).unwrap();
        rules.install_art_data(art_registry(&artmd));
        published_references(&rules, processed)
    };
    let mut digest = FNV1A64_OFFSET_BASIS;
    let mut lines = 0;
    let mut live = None;
    for mode in modes {
        let mut layers = RulesLayerStack::new(rulesmd.clone());
        if let Some(mode) = mode {
            let Some(mode_ini) = retail_ini(mode) else {
                return;
            };
            layers.push(RulesLayerKind::GameMode, mode_ini);
        }
        let processed = layers.process_with_fixed_art(&artmd).unwrap();
        let published = publish(&processed);
        let processed = match live.take() {
            None => processed,
            Some(state) => {
                let reset = layers
                    .process_with_fixed_art_and_registry_state_recovering(
                        &artmd,
                        state,
                        RulesProcessContext::ScenarioReset,
                        &crate::rules::sound_ini::EvaRegistry::default(),
                    )
                    .unwrap();
                assert_eq!(publish(&reset), published, "{mode:?} after a reset");
                reset
            }
        };
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        live = Some(trace.into_registry_state_discarding_events());
        for line in published {
            digest = fnv1a64_fold_bytes(digest, line.as_bytes());
            digest = fnv1a64_fold_bytes(digest, b"\n");
            lines += 1;
        }
    }
    assert_eq!((lines, digest), (15550, 0x4e44_571d_44c6_6b15));
}

/// One line per owner of the references this chain moved to the ordered
/// processor, in a fixed order.
fn published_references(rules: &RuleSet, processed: &ProcessedRulesLayers) -> Vec<String> {
    let mut lines = Vec::new();
    let mut objects: Vec<_> = rules.all_objects().collect();
    objects.sort_by_key(|object| (format!("{:?}", object.category), object.id.clone()));
    for o in objects {
        let pointers = (
            &o.image,
            &o.death_weapon,
            &o.deploys_into,
            &o.occupy_weapon,
            &o.elite_occupy_weapon,
            &o.undeploys_into,
            &o.powers_unit,
            &o.free_unit,
            &o.to_overlay,
            &o.unloading_class,
            &o.deploying_anim,
        );
        let lists = (
            &o.dock,
            &o.enslaves,
            &o.spawns,
            &o.explosion_anims,
            &o.destroy_anims,
            &o.dead_bodies,
            &o.debris_types,
            &o.debris_anims,
        );
        let systems = (
            &o.natural_particle_system,
            &o.refinery_smoke_particle_system,
            &o.damage_particle_systems,
            &o.destroy_particle_systems,
        );
        lines.push(format!(
            "{:?} {} {pointers:?} {lists:?} {systems:?} {}",
            o.category, o.id, o.build_const_eligible
        ));
    }
    let mut weapons: Vec<_> = rules.weapons_iter().collect();
    weapons.sort_by_key(|weapon| weapon.id.clone());
    for w in weapons {
        let references = (
            &w.anim,
            &w.occupant_anim,
            &w.open_topped_anim,
            &w.attached_particle_system,
        );
        lines.push(format!("Weapon {} {references:?}", w.id));
    }
    let mut warheads: Vec<_> = rules.warheads_iter().collect();
    warheads.sort_by_key(|warhead| warhead.id.clone());
    for w in warheads {
        lines.push(format!("Warhead {} {:?}", w.id, w.debris_types));
    }
    for particle in rules.particle_types_iter() {
        lines.push(format!("Particle {} {:?}", particle.name, particle.warhead));
    }
    for index in 0..rules.voxel_anim_type_count() {
        let v = rules.voxel_anim_type(crate::rules::voxel_anim_type::VoxelAnimTypeId(index as u32));
        let references = (
            &v.spawns,
            &v.bounce_anim,
            &v.expire_anim,
            &v.trailer_anim,
            &v.warhead,
            &v.attached_system,
        );
        lines.push(format!("VoxelAnim {} {references:?}", v.name));
    }
    let mut super_weapons: Vec<_> = rules.super_weapons.values().collect();
    super_weapons.sort_by_key(|super_weapon| super_weapon.id.clone());
    for s in super_weapons {
        lines.push(format!("SuperWeapon {} {:?}", s.id, s.aux_building));
    }
    let animations = processed
        .ini()
        .section_or_empty("Animations")
        .registry_ids();
    for name in animations {
        if let Some(c) = rules.art().anim_runtime_config(name) {
            let references = (
                &c.next,
                &c.spawns,
                &c.tiberium_spawn_type,
                &c.bounce_anim,
                &c.expire_anim,
                &c.trailer_anim,
                &c.warhead,
                &c.spawns_particle,
            );
            lines.push(format!("Anim {name} {references:?}"));
        }
    }
    let damage = &rules.combat_damage;
    lines.push(format!(
        "CombatDamage {:?}",
        (
            (
                &rules.bridge_warheads.c4_name,
                &rules.bridge_warheads.crush_name,
                &rules.bridge_warheads.ion_cannon_name,
                &damage.ivan_warhead,
                &damage.drain_animation_type,
                &rules.mind_control.controlled_anim,
                &rules.mind_control.perma_controlled_anim,
            ),
            (
                &damage.default_large_grey_smoke_system,
                &damage.default_small_grey_smoke_system,
                &damage.default_spark_system,
                &damage.default_large_red_smoke_system,
                &damage.default_small_red_smoke_system,
                &damage.default_debris_smoke_system,
                &damage.default_fire_stream_system,
                &damage.default_test_particle_system,
                &damage.default_repair_particle_system,
            ),
        )
    ));
    let general = &rules.general;
    lines.push(format!(
        "SpecialWeapons {:?}",
        (
            &rules.radiation.site_warhead,
            &general.mutate_warhead,
            &general.mutate_explosion_warhead,
        )
    ));
    lines.push(format!("CrateRules {:?}", rules.crate_rules));
    lines.push(format!("Rockets {:?}", rules.missile_spawn));
    lines.push(format!(
        "General {:?}",
        (
            (
                &general.ore_twinkle,
                &general.barrel_particle,
                &general.nuke_take_off,
                &general.wake.name,
                &general.warp_in.name,
                &general.warp_out.name,
                &general.warp_away.name,
                &general.chrono_sparkle1.name,
                &general.move_flash.name,
                &general.parachute_shp,
            ),
            (
                &general.dominator_warhead,
                &general.dominator_first_anim,
                &general.dominator_second_anim,
                &general.chrono_placement_anim,
                &general.chrono_blast_anim,
                &general.chrono_blast_dest_anim,
                &general.iron_curtain_invoke_anim,
                &general.force_shield_invoke_anim,
                &general.infantry_death_anims,
            ),
            (
                &general.engineer_infantry,
                &general.technician,
                &general.allied_crew,
                &general.soviet_crew,
                &general.third_crew,
                &general.infantry_disguises,
            ),
        )
    ));
    let fire: Vec<&str> = general
        .damage_fire_types
        .iter()
        .map(|anim| anim.name.as_str())
        .collect();
    lines.push(format!(
        "General lists {:?}",
        (
            fire,
            &general.dead_bodies,
            &general.base_unit_types,
            &general.pad_aircraft_types,
            &rules.shipyard_types,
            &rules.harvester_unit_types,
        )
    ));
    lines.push(format!(
        "AI {:?}",
        (
            (
                &rules.build_const_types,
                &rules.build_power_types,
                &rules.build_refinery_types,
                &rules.build_barracks_types,
                &rules.build_tech_types,
                &rules.build_weapons_types,
            ),
            (
                &rules.build_radar_types,
                &rules.allied_base_defense_types,
                &rules.soviet_base_defense_types,
                &rules.third_base_defense_types,
                &rules.concrete_wall_types,
            ),
            processed.neutral_tech_buildings(),
        )
    ));
    lines
}
