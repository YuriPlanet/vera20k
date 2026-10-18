//! Per-pass Type reads the ordered Rules processor alone owns: the TechnoType
//! weapon block, ParticleSystemType HoldsWhat and the 25-byte Image that names
//! a Type's fixed ART section.
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
/// of its first 24 bytes.
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
