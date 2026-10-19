//! Per-pass Type reads the ordered Rules processor alone owns: the TechnoType
//! weapon block, ParticleSystemType HoldsWhat, the 25-byte Image that names a
//! Type's fixed ART section, the references that name another Type and
//! WeaponType Range.
use super::*;
use crate::rules::ruleset::RuleSet;

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
