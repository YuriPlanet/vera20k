//! Ordered native processing and constructor-oracle regression tests.

use super::*;

#[test]
fn projectile_flat_matches_original_art_reader_and_retained_defaults() {
    use crate::rules::ruleset::RuleSet;
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/projectile_oracle/flat_art.json")).unwrap();
    let cases = cases.as_array().unwrap();
    assert_eq!(cases.len(), 38);
    for case in cases {
        let prior = case["prior"].as_bool().unwrap();
        let mut art = format!(
            "[PRIOR]\nFlat={}\n[{}]\nImage=UNREAD_REDIRECT\n",
            if prior { "yes" } else { "no" },
            case["art_section"].as_str().unwrap()
        );
        if let Some(flat) = case["flat"].as_str() {
            art.push_str(&format!("Flat={flat}\n"));
        }
        // A redirected ART body must not become the source of Flat.
        art.push_str(&format!(
            "[UNREAD_REDIRECT]\nFlat={}\n",
            if prior { "no" } else { "yes" }
        ));
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
             [GUN]\nProjectile=SHOT\n[SHOT]\nImage=PRIOR\n",
        ));
        let mut pass = format!("[SHOT]\nROT=1\nFlat={}\n", if prior { "no" } else { "yes" });
        if let Some(image) = case["image"].as_str() {
            pass.push_str(&format!("Image={image}\n"));
        }
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&pass));
        let processed = layers
            .process_with_fixed_art(&IniFile::from_str(&art))
            .unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.projectile("SHOT").unwrap().flat,
            case["effective_flat"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn projectile_flat_survives_registry_handoff_and_resets_with_its_owner() {
    use crate::rules::ruleset::RuleSet;
    let art = IniFile::from_str("[YES]\nFlat=yes\n");
    let root = IniFile::from_str(
        "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n\
         [GUN]\nProjectile=SHOT\n[SHOT]\nImage=YES\n",
    );
    let processed = RulesLayerStack::new(root.clone())
        .process_with_fixed_art(&art)
        .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let continued = RulesLayerStack::new(IniFile::from_str("[SHOT]\nROT=1\n"))
        .process_with_fixed_art_and_registry_state(
            &art,
            receipt.into_registry_state_discarding_events(),
        )
        .unwrap();
    assert!(
        RuleSet::from_processed_rules(&continued)
            .unwrap()
            .projectile("SHOT")
            .unwrap()
            .flat
    );
    assert!(
        continued
            .native_type_construction_trace()
            .events()
            .is_empty()
    );
    let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
    let mut no_image_root = root;
    no_image_root.replace_first_section(
        IniFile::from_str("[SHOT]\nROT=1\n")
            .section("SHOT")
            .unwrap()
            .clone(),
    );
    let reset = RulesLayerStack::new(no_image_root)
        .process_with_fixed_art_and_registry_state(
            &art,
            receipt
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert!(
        !RuleSet::from_processed_rules(&reset)
            .unwrap()
            .projectile("SHOT")
            .unwrap()
            .flat
    );
}

#[test]
fn anim_art_read_receipt_distinguishes_live_sweep_from_late_allocation() {
    // ReadTypeData 0x00679A5D..0x00679A82 reloads the live Anim count, then
    // proceeds to Techno readers; AudioVisual runs still later in Process.
    let root = IniFile::from_str(
        "[Animations]\n0=ROOT\n1=MISSING\n2=EMPTY\n\
         [General]\nDamageFireTypes=EARLY\n\
         [VehicleTypes]\n0=UNIT\n[UNIT]\nExplosion=LATE\n\
         [AudioVisual]\nSmoke=AFTER\n",
    );
    let art = IniFile::from_str(
        "[ROOT]\nNext=CHILD\n[CHILD]\nTrailerAnim=TAIL\n\
         [TAIL]\nRate=1\n[EARLY]\nRate=1\n[LATE]\nRate=1\n\
         [AFTER]\nRate=1\n[EMPTY]\n",
    );
    let mut layers = RulesLayerStack::new(root);
    let first = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        first.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("ROOT", true),
            ("MISSING", false),
            ("EMPTY", false),
            ("EARLY", true),
            ("CHILD", true),
            ("TAIL", true),
            ("LATE", false),
            ("AFTER", false),
        ],
    );
    assert!(
        first
            .ini()
            .section("Animations")
            .unwrap()
            .get_values()
            .contains(&"LATE")
    );
    assert!(
        art.section("LATE").is_some(),
        "ART presence is not a read receipt"
    );
    assert!(
        art.section("EMPTY").is_none(),
        "native lexical loading drops empty sections"
    );

    layers.push(RulesLayerKind::Scenario, IniFile::empty());
    let next = layers.process_with_fixed_art(&art).unwrap();
    assert_eq!(
        next.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("ROOT", true),
            ("MISSING", false),
            ("EMPTY", false),
            ("EARLY", true),
            ("CHILD", true),
            ("TAIL", true),
            ("LATE", true),
            ("AFTER", true),
        ],
    );
    assert_eq!(
        first.native_type_construction_trace().events(),
        next.native_type_construction_trace().events(),
        "body reads do not spend new constructor IDs",
    );
}

#[test]
fn anim_art_read_receipt_survives_handoff_and_follows_registry_reset() {
    let root = IniFile::from_str("[Animations]\n0=READ\n1=MISSING\n");
    let art = IniFile::from_str("[READ]\nRate=1\n");
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
    let state = receipt.into_registry_state_discarding_events();
    assert_eq!(
        state.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![("READ", true), ("MISSING", false)],
    );
    let continued = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&art, state)
        .unwrap();
    assert_eq!(
        continued.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![("READ", true), ("MISSING", false)],
    );
    assert!(
        continued
            .native_type_construction_trace()
            .events()
            .is_empty()
    );
    let (_, receipt) = continued.into_ini_and_native_type_construction_trace();
    let reset = receipt
        .into_registry_state_discarding_events()
        .destructive_reset();
    assert_eq!(reset.anim_type_art_read_states().count(), 0);
    let after_reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&art, reset)
        .unwrap();
    assert_eq!(after_reset.anim_type_art_read_states().count(), 0);
}

#[test]
fn anim_art_read_receipt_preserves_stored_identity_and_duplicate_slots() {
    let root = IniFile::from_str(
        "[General]\nDamageFireTypes=FIRST, SPACED,ABCDEFGHIJKLMNOPQRSTUVWXY,ABCDEFGHIJKLMNOPQRSTUVWXY\n",
    );
    let art = IniFile::from_str(
        "[FIRST]\nRate=1\n[ SPACED]\nRate=1\n[ABCDEFGHIJKLMNOPQRSTUVWX]\nRate=1\n",
    );
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    assert_eq!(
        processed.anim_type_art_read_states().collect::<Vec<_>>(),
        vec![
            ("FIRST", true),
            (" SPACED", true),
            ("ABCDEFGHIJKLMNOPQRSTUVWX", true),
            ("ABCDEFGHIJKLMNOPQRSTUVWX", true),
        ],
        "the receipt is ordered per type, not a trimmed or deduplicated root set",
    );
}

fn process_ini_passes(root: IniFile, later: IniFile) -> ProcessedRulesLayers {
    let mut layers = RulesLayerStack::new(root);
    layers.push(RulesLayerKind::Scenario, later);
    layers.process().expect("ordered synthetic Rules passes")
}

/// Native type-registry passes find-or-allocate every listed value from each
/// later rules layer, preserving entry order independently of the key text.
#[test]
fn map_overrides_union_type_registries_by_value() {
    let mut rules = IniFile::from_str("[VehicleTypes]\n0=MTNK\n[Animations]\n0=RING1\n");
    let map = IniFile::from_str("[VehicleTypes]\n0=EVILTANK\n[Animations]\n0=EVILANIM\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("VehicleTypes").unwrap().get_values(),
        vec!["MTNK", "EVILTANK"]
    );
    assert_eq!(
        rules.section("Animations").unwrap().get_values(),
        vec!["RING1", "EVILANIM"]
    );
}

fn process_rules_passes(root: &str, later: &str) -> ProcessedRulesLayers {
    let mut layers = RulesLayerStack::new(IniFile::from_str(root));
    layers.push(RulesLayerKind::Scenario, IniFile::from_str(later));
    layers.process().expect("synthetic Rules passes process")
}

#[test]
fn native_type_construction_trace_preserves_first_new_process_and_lazy_order() {
    let processed = process_rules_passes(
        "[Countries]\n0=Americans\n\
         [Sides]\nGDI=Americans,French\n\
         [SuperWeaponTypes]\n0=SW1\n1=SW2\n\
         [VehicleTypes]\n0=TANK\n\
         [Particles]\n0=SmokeParticle\n\
         [TANK]\nPrimary=Gun\n\
         [Gun]\nProjectile=Shell\n",
        "[Countries]\n0=americans\n1=British\n\
         [Sides]\nGDI=British\nNod=Russians\n\
         [SuperWeaponTypes]\n0=sw1\n2=SW3\n\
         [VehicleTypes]\n0=tank\n1=NEW\n\
         [Particles]\n1=SparkParticle\n\
         [NEW]\nPrimary=gun\n",
    );

    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::HouseType, "Americans"),
            (NativeTypeConstructorFamily::Side, "GDI"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW1"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW2"),
            (NativeTypeConstructorFamily::UnitType, "TANK"),
            (NativeTypeConstructorFamily::WeaponType, "Gun"),
            (NativeTypeConstructorFamily::BulletType, "Shell"),
            (NativeTypeConstructorFamily::HouseType, "British"),
            (NativeTypeConstructorFamily::Side, "Nod"),
            (NativeTypeConstructorFamily::SuperWeaponType, "SW3"),
            (NativeTypeConstructorFamily::UnitType, "NEW"),
        ]
    );
    assert_eq!(
        processed
            .native_type_construction_trace()
            .allocated_super_weapon_type_count(),
        3
    );
    assert_eq!(
        processed.ini().section("Sides").unwrap().get("GDI"),
        Some("British"),
        "Side constructor identity is the key, while the compatibility value remains the membership list"
    );
    assert!(
        actual
            .iter()
            .all(|(_, name)| !name.eq_ignore_ascii_case("SmokeParticle")
                && !name.eq_ignore_ascii_case("SparkParticle")),
        "ParticleType constructors do not call AssignUniqueID"
    );
}

#[test]
fn native_type_registry_compares_full_input_but_stores_and_emits_24_bytes() {
    let prefix = "abcdefghijklmnopqrstuvwx";
    let long = format!("{prefix}y");
    assert_eq!(prefix.len(), 0x18);
    assert_eq!(long.len(), 0x19);
    let rules = format!(
        "[VehicleTypes]\n0={prefix}\n1={}\n2={long}\n3={long}\n",
        prefix.to_ascii_uppercase()
    );
    let processed = RulesLayerStack::new(IniFile::from_str(&rules))
        .process()
        .expect("long native IDs process");

    let unit_events = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::UnitType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();
    assert_eq!(unit_events, vec![prefix, prefix, prefix]);
    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .expect("rebuilt Unit registry")
            .get_values(),
        vec![prefix, prefix, prefix]
    );
}

#[test]
fn side_registry_and_house_side_lookup_do_not_apply_generic_none_sentinels() {
    let explicit = RulesLayerStack::new(IniFile::from_str(
        "[Sides]\nnone=Americans\n<NONE>=British\nNONE=French\n",
    ))
    .process()
    .expect("Side registry processes");
    let explicit_events = explicit
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        explicit_events,
        vec![
            (NativeTypeConstructorFamily::Side, "none"),
            (NativeTypeConstructorFamily::Side, "<NONE>"),
        ]
    );
    assert!(explicit_events
        .iter()
        .all(|(family, _)| *family != NativeTypeConstructorFamily::HouseType));

    let lazy = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=House\n[House]\nSide=<none>\n",
    ))
    .process()
    .expect("House Side lookup processes");
    assert_eq!(
        lazy.native_type_construction_trace()
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::HouseType, "House"),
            (NativeTypeConstructorFamily::Side, "<none>"),
        ]
    );
}

#[test]
fn constructor_lists_collapse_empty_fields_without_trimming_individual_tokens() {
    assert_eq!(
        native_strtok_comma_tokens(
            "FIRST, SECOND ,,FIRST,,,none,<NoNe>, none , THIRD",
        )
        .collect::<Vec<_>>(),
        vec![
            "FIRST",
            " SECOND ",
            "FIRST",
            "none",
            "<NoNe>",
            " none ",
            " THIRD",
        ]
    );
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=FIRST\n\
         [General]\nDamageFireTypes=FIRST, SECOND ,,FIRST,,,none,<NoNe>, none , THIRD\n\
         PrerequisitePower=FIRST,FIRST,,none\n",
    ))
    .process()
    .expect("native list fixture processes");
    let anim_ids = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::AnimType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();

    assert_eq!(anim_ids, vec!["FIRST", " SECOND ", " none ", " THIRD"]);
    assert_eq!(
        processed
            .ini()
            .section("General")
            .expect("projected General section")
            .get("PrerequisitePower"),
        Some("FIRST,FIRST"),
        "native strtok vectors retain repeated resolved pointers",
    );
}

fn native_type_family_oracle_code(family: NativeTypeConstructorFamily) -> u8 {
    match family {
        NativeTypeConstructorFamily::HouseType => 0,
        NativeTypeConstructorFamily::Side => 1,
        NativeTypeConstructorFamily::OverlayType => 2,
        NativeTypeConstructorFamily::SuperWeaponType => 3,
        NativeTypeConstructorFamily::WarheadType => 4,
        NativeTypeConstructorFamily::SmudgeType => 5,
        NativeTypeConstructorFamily::TerrainType => 6,
        NativeTypeConstructorFamily::BuildingType => 7,
        NativeTypeConstructorFamily::UnitType => 8,
        NativeTypeConstructorFamily::AircraftType => 9,
        NativeTypeConstructorFamily::InfantryType => 10,
        NativeTypeConstructorFamily::AnimType => 11,
        NativeTypeConstructorFamily::VoxelAnimType => 12,
        NativeTypeConstructorFamily::ParticleSystemType => 13,
        NativeTypeConstructorFamily::WeaponType => 14,
        NativeTypeConstructorFamily::BulletType => 15,
    }
}

fn extend_native_type_event_oracle_hash(
    mut hash: u64,
    events: &[NativeTypeConstructionEvent],
) -> u64 {
    for event in events {
        for byte in std::iter::once(native_type_family_oracle_code(event.family()))
            .chain(event.native_stored_id().bytes())
            .chain(std::iter::once(0xff))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn native_type_event_oracle_hash(events: &[NativeTypeConstructionEvent]) -> u64 {
    extend_native_type_event_oracle_hash(0xcbf29ce484222325_u64, events)
}

#[test]
fn retail_rulesmd_artmd_constructor_trace_matches_verified_base_oracle() {
    let Some((rules, fixed_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let processed = RulesLayerStack::new(rules)
        .process_with_fixed_art(&fixed_art)
        .expect("stock base Rules pass processes");
    let events = processed.native_type_construction_trace().events();

    assert_eq!(events.len(), 1_975);
    assert_eq!(
        native_type_event_oracle_hash(events),
        0x24516fbd1a096a12
    );

    let explicit_boundary = events[1_699..1_704]
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();
    assert_eq!(
        explicit_boundary,
        vec![
            (NativeTypeConstructorFamily::AnimType, "D"),
            (NativeTypeConstructorFamily::AnimType, "WCLBOLT2"),
            (NativeTypeConstructorFamily::UnitType, "VISC_LRG"),
            (NativeTypeConstructorFamily::UnitType, "VISC_SML"),
            (NativeTypeConstructorFamily::WeaponType, "Vulcan2"),
        ],
        "stock AI adds nothing; General first emits the cap-truncated MetallicDebris token D, then its other four fresh constructors",
    );

    let assert_ordered = |expected: &[(NativeTypeConstructorFamily, &str)]| {
        let mut cursor = 0;
        for &(family, identity) in expected {
            let relative = events[cursor..]
                .iter()
                .position(|event| {
                    event.family() == family && event.native_stored_id() == identity
                })
                .unwrap_or_else(|| {
                    panic!("missing ordered retail constructor {family:?} {identity}")
                });
            cursor += relative + 1;
        }
    };
    assert_ordered(&[
        (NativeTypeConstructorFamily::AnimType, "SMOKEY2"),
        (NativeTypeConstructorFamily::AnimType, "gtpowexp"),
        (NativeTypeConstructorFamily::AnimType, "tstlexp"),
        (NativeTypeConstructorFamily::AnimType, "CAWA15DM"),
        (NativeTypeConstructorFamily::AnimType, "CACH06DM"),
        (NativeTypeConstructorFamily::AnimType, "YURICNTL"),
        (NativeTypeConstructorFamily::AnimType, "APMUZZLE"),
        (NativeTypeConstructorFamily::AnimType, "BBBLELRG"),
        (NativeTypeConstructorFamily::AnimType, "MINDANIMR"),
        (NativeTypeConstructorFamily::AnimType, "xxxx"),
        (NativeTypeConstructorFamily::BulletType, "NukeUp"),
        (NativeTypeConstructorFamily::BulletType, "NukeDown"),
    ]);
    assert_ordered(&[
        (NativeTypeConstructorFamily::WarheadType, "PrismWarhead"),
        (NativeTypeConstructorFamily::WarheadType, "DummyWarhead"),
        (NativeTypeConstructorFamily::WarheadType, "ApocAPE"),
        (NativeTypeConstructorFamily::WarheadType, "RHINAPE"),
        (NativeTypeConstructorFamily::WarheadType, "GRIZAPE"),
        (NativeTypeConstructorFamily::WarheadType, "Special"),
        (NativeTypeConstructorFamily::WarheadType, "KTSTLEXP"),
        (NativeTypeConstructorFamily::WarheadType, "UltraAPE"),
        (NativeTypeConstructorFamily::WarheadType, "Shock"),
        (NativeTypeConstructorFamily::WarheadType, "MagneShakeWH"),
        (NativeTypeConstructorFamily::WarheadType, "HollowPoint4"),
    ]);

    let event_index = |family, identity| {
        events
            .iter()
            .position(|event| event.family() == family && event.native_stored_id() == identity)
            .unwrap_or_else(|| panic!("missing retail constructor {family:?} {identity}"))
    };
    let building_tail = event_index(NativeTypeConstructorFamily::AnimType, "CACH06DM");
    let bullet_art_head = event_index(NativeTypeConstructorFamily::AnimType, "BBBLELRG");
    for weapon_anim in ["APMUZZLE", "YURICNTL"] {
        let index = event_index(NativeTypeConstructorFamily::AnimType, weapon_anim);
        assert!(
            building_tail < index && index < bullet_art_head,
            "Weapon-body Anim {weapon_anim} must be constructed after Building bodies and before Bullet Art",
        );
    }
    let post_reader_head = event_index(NativeTypeConstructorFamily::AnimType, "MINDANIMR");
    for warhead_anim in ["APOCEXP", "MININUKE - added 11/30"] {
        let index = event_index(NativeTypeConstructorFamily::AnimType, warhead_anim);
        assert!(
            bullet_art_head < index && index < post_reader_head,
            "Warhead-body Anim {warhead_anim} must be constructed after Bullet bodies and before post-Type readers",
        );
    }
    assert!(events.iter().all(|event| {
        event.family() != NativeTypeConstructorFamily::AnimType
            || event.native_stored_id() != "DURASMOKE"
    }));
}

#[test]
fn retail_cold_start_and_noncampaign_prepass_match_verified_native_oracles() {
    let Some((rules, fixed_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let (startup, startup_boundaries) = process_native_rules_cold_start_inner(
        NativeRulesRegistryState::default(),
        &rules,
        &fixed_art,
        None,
    )
    .expect("stock cold startup processes");

    assert_eq!(startup_boundaries, [1, 608, 612, 1_014, 1_070]);
    assert_eq!(startup.event_count(), 1_070);
    assert_eq!(
        native_type_event_oracle_hash(startup.events()),
        0x408b802af3a4cfce
    );
    assert_eq!(
        startup.events()[..3]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "xxxx"),
            (NativeTypeConstructorFamily::AnimType, "TWLT100"),
            (NativeTypeConstructorFamily::AnimType, "ELECTRO"),
        ]
    );
    assert_eq!(
        startup.events()[608..612]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "SMOKEY2"),
            (NativeTypeConstructorFamily::WarheadType, "HE"),
            (NativeTypeConstructorFamily::OverlayType, "TIB2_01"),
            (NativeTypeConstructorFamily::WarheadType, "TankOGas"),
        ]
    );
    for (family, expected) in [
        (NativeTypeConstructorFamily::AnimType, 613),
        (NativeTypeConstructorFamily::BuildingType, 402),
        (NativeTypeConstructorFamily::WeaponType, 31),
        (NativeTypeConstructorFamily::OverlayType, 9),
        (NativeTypeConstructorFamily::UnitType, 7),
        (NativeTypeConstructorFamily::ParticleSystemType, 5),
        (NativeTypeConstructorFamily::WarheadType, 2),
        (NativeTypeConstructorFamily::InfantryType, 1),
    ] {
        assert_eq!(startup.registry_state().family_len(family), expected);
    }
    for family in [
        NativeTypeConstructorFamily::HouseType,
        NativeTypeConstructorFamily::Side,
        NativeTypeConstructorFamily::SuperWeaponType,
        NativeTypeConstructorFamily::SmudgeType,
        NativeTypeConstructorFamily::TerrainType,
        NativeTypeConstructorFamily::AircraftType,
        NativeTypeConstructorFamily::VoxelAnimType,
        NativeTypeConstructorFamily::BulletType,
    ] {
        assert_eq!(startup.registry_state().family_len(family), 0);
    }
    assert_eq!(startup.allocated_super_weapon_type_count(), 0);
    assert_eq!(
        startup
            .registry_state()
            .families
            .get(&RulesTypeFamily::Particle)
            .map_or(0, Vec::len),
        1,
        "VirusCloud1 is a retained Particle but spends no native Type ID",
    );

    let startup_hash = native_type_event_oracle_hash(startup.events());
    let startup_state = startup.into_registry_state_discarding_events();
    let (prepass, prepass_boundaries) =
        process_native_noncampaign_rules_prepass_inner(startup_state, &rules);
    assert_eq!(prepass_boundaries, [14, 46, 51]);
    assert_eq!(prepass.event_count(), 51);
    assert_eq!(
        native_type_event_oracle_hash(prepass.events()),
        0x45b8b69cd005937d
    );
    assert_eq!(
        extend_native_type_event_oracle_hash(startup_hash, prepass.events()),
        0x026859d66424f324
    );
    assert_eq!(prepass.allocated_super_weapon_type_count(), 0);
    for (family, expected) in [
        (NativeTypeConstructorFamily::HouseType, 14),
        (NativeTypeConstructorFamily::Side, 5),
        (NativeTypeConstructorFamily::OverlayType, 9),
        (NativeTypeConstructorFamily::SuperWeaponType, 0),
        (NativeTypeConstructorFamily::WarheadType, 4),
        (NativeTypeConstructorFamily::SmudgeType, 0),
        (NativeTypeConstructorFamily::TerrainType, 5),
        (NativeTypeConstructorFamily::BuildingType, 402),
        (NativeTypeConstructorFamily::UnitType, 12),
        (NativeTypeConstructorFamily::AircraftType, 5),
        (NativeTypeConstructorFamily::InfantryType, 11),
        (NativeTypeConstructorFamily::AnimType, 615),
        (NativeTypeConstructorFamily::VoxelAnimType, 3),
        (NativeTypeConstructorFamily::ParticleSystemType, 5),
        (NativeTypeConstructorFamily::WeaponType, 31),
        (NativeTypeConstructorFamily::BulletType, 0),
    ] {
        assert_eq!(prepass.registry_state().family_len(family), expected);
    }
}

#[test]
fn native_startup_prepass_repeat_and_reset_keep_each_phase_on_one_registry_owner() {
    let root = IniFile::from_str(
        "[AudioVisual]\nSmoke=SEED\n\
         [Animations]\n0=A\n\
         [BuildingTypes]\n0=B\n\
         [Countries]\n0=HOUSE\n\
         [General]\nDamageFireTypes=SEED,PREANIM\n\
         [A]\nNext=RULES_ANIM_BODY_IGNORED\n\
         [B]\nPrimary=W\nExplosion=BA\nFreeUnit=U\nSecretBuilding=B2\n\
         [B2]\nPrimary=W2\n\
         [W]\nWarhead=TOO_LATE_WEAPON_BODY\n\
         [U]\nPrimary=TOO_LATE_UNIT_BODY\n\
         [BA]\nNext=TOO_LATE_ANIM_BODY\n\
         [HOUSE]\nVeteranInfantry=INF\nVeteranUnits=HUNIT\nVeteranAircraft=HAIR\nSide=HSIDE\n",
    );
    let fixed_art = IniFile::from_str(
        "[A]\nNext=A2\n\
         [A2]\nWarhead=ANIMWH\n\
         [B]\nToOverlay=BOV\n\
         [B2]\nToOverlay=B2OV\n\
         [BA]\nNext=TOO_LATE_ANIM_BODY\n",
    );
    let (startup, boundaries) = process_native_rules_cold_start_inner(
        NativeRulesRegistryState::default(),
        &root,
        &fixed_art,
        None,
    )
    .expect("synthetic startup processes");
    assert_eq!(boundaries, [1, 2, 4, 5, 12]);
    assert_eq!(
        startup.events()[..4]
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::AnimType, "SEED"),
            (NativeTypeConstructorFamily::AnimType, "A"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::WarheadType, "ANIMWH"),
        ],
        "AudioVisual reads Smoke twice but allocates once; the live fixed-Art Anim loop reaches A2",
    );
    let startup_ids = startup
        .events()
        .iter()
        .map(NativeTypeConstructionEvent::native_stored_id)
        .collect::<Vec<_>>();
    for expected in ["W", "BA", "U", "B2", "BOV", "W2", "B2OV"] {
        assert!(startup_ids.contains(&expected), "missing startup event {expected}");
    }
    for forbidden in [
        "RULES_ANIM_BODY_IGNORED",
        "TOO_LATE_WEAPON_BODY",
        "TOO_LATE_UNIT_BODY",
        "TOO_LATE_ANIM_BODY",
    ] {
        assert!(!startup_ids.contains(&forbidden));
    }

    let startup_state = startup.into_registry_state_discarding_events();
    let (repeat, repeat_boundaries) =
        process_native_rules_cold_start_inner(startup_state, &root, &fixed_art, None)
            .expect("direct startup repeat processes retained state");
    assert_eq!(repeat_boundaries, [0, 0, 1, 1, 1]);
    assert_eq!(
        repeat
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![(
            NativeTypeConstructorFamily::AnimType,
            "TOO_LATE_ANIM_BODY"
        )],
        "the repeat's live Anim sweep must reach BA, which the prior Building sweep allocated after the first Anim sweep",
    );

    let retained_state = repeat.into_registry_state_discarding_events();
    let (prepass, prepass_boundaries) =
        process_native_noncampaign_rules_prepass_inner(retained_state, &root);
    assert_eq!(prepass_boundaries, [1, 2, 6]);
    assert_eq!(
        prepass
            .events()
            .iter()
            .map(|event| (event.family(), event.native_stored_id()))
            .collect::<Vec<_>>(),
        vec![
            (NativeTypeConstructorFamily::HouseType, "HOUSE"),
            (NativeTypeConstructorFamily::AnimType, "PREANIM"),
            (NativeTypeConstructorFamily::InfantryType, "INF"),
            (NativeTypeConstructorFamily::UnitType, "HUNIT"),
            (NativeTypeConstructorFamily::AircraftType, "HAIR"),
            (NativeTypeConstructorFamily::Side, "HSIDE"),
        ],
        "prepass must retain lookup state and read each House body in Infantry/Unit/Aircraft/Side order",
    );

    let reset_state = prepass
        .into_registry_state_discarding_events()
        .destructive_reset();
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art_and_registry_state(&fixed_art, reset_state)
        .expect("post-reset full Process succeeds");
    let post_reset_events = processed.native_type_construction_trace().events();
    assert!(post_reset_events.iter().any(|event| {
        event.family() == NativeTypeConstructorFamily::HouseType
            && event.native_stored_id() == "HOUSE"
    }));
    assert!(post_reset_events.iter().any(|event| {
        event.family() == NativeTypeConstructorFamily::AnimType && event.native_stored_id() == "A"
    }));
}

#[test]
fn fixed_art_drives_constructors_without_entering_rules_content_or_bodies() {
    let rules = IniFile::from_str("[Animations]\n0=ROOT\n[ROOT]\nRate=7\n");
    let without_art = RulesLayerStack::new(rules.clone())
        .process_with_fixed_art(&IniFile::empty())
        .expect("empty fixed Art processes");
    let with_art = RulesLayerStack::new(rules)
        .process_with_fixed_art(&IniFile::from_str("[ROOT]\nNext=TAIL\n"))
        .expect("populated fixed Art processes");

    assert_eq!(without_art.content_hash(), with_art.content_hash());
    assert_eq!(
        with_art.ini().section("ROOT").expect("Rules body").get("Next"),
        None,
        "standalone Art keys must not merge into the Rules body projection",
    );
    assert_eq!(
        without_art
            .native_type_construction_trace()
            .events()
            .iter()
            .map(|event| event.native_stored_id())
            .collect::<Vec<_>>(),
        vec!["ROOT"],
    );
    assert_eq!(
        with_art
            .native_type_construction_trace()
            .events()
            .iter()
            .map(|event| event.native_stored_id())
            .collect::<Vec<_>>(),
        vec!["ROOT", "TAIL"],
    );
}

#[test]
fn fixed_art_and_member_major_type_data_follow_live_native_order() {
    let rules = IniFile::from_str(
        "[BuildingTypes]\n0=BLD\n\
         [VehicleTypes]\n0=UNIT\n\
         [AircraftTypes]\n0=PLANE\n\
         [Animations]\n0=ROOT\n\
         [BLD]\nFreeUnit=FREE\nSecretInfantry=SECINF\nSecretUnit=SECUNIT\nSecretBuilding=SECBLD\nImage=BLDART\n\
         [PLANE]\nImage=PLANEART\n\
         [UNIT]\nPrimary=W1\nSecondary=W2\n\
         [W1]\nAnim=A1,A2\nAssaultAnim=A3\nOccupantAnim=A4\nOpenToppedAnim=A5\nAttachedParticleSystem=PS1\nWarhead=WH1\nProjectile=P1\n\
         [W2]\nAnim=B1,B2\nAssaultAnim=B3\nOccupantAnim=B4\nOpenToppedAnim=B5\nAttachedParticleSystem=PS2\nWarhead=WH2\nProjectile=P2\n\
         [P1]\nImage=P1ART\nAirburstWeapon=LATEW1\nShrapnelWeapon=LATEW2\n\
         [P2]\nImage=P2ART\n",
    );
    let fixed_art = IniFile::from_str(
        "[ROOT]\nNext=ROOT2\n\
         [ROOT2]\nTrailerAnim=ROOT3\n\
         [BLDART]\nToOverlay=BLDOVL\n\
         [PLANEART]\nTrailer=PLANETR\n\
         [PLANETR]\nNext=TOO_LATE\n\
         [P1ART]\nTrailer=P1TR\n\
         [P2ART]\nTrailer=P2TR\n",
    );
    let processed = RulesLayerStack::new(rules)
        .process_with_fixed_art(&fixed_art)
        .expect("fixed-Art TypeData fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::BuildingType, "BLD"),
            (NativeTypeConstructorFamily::UnitType, "UNIT"),
            (NativeTypeConstructorFamily::AircraftType, "PLANE"),
            (NativeTypeConstructorFamily::AnimType, "ROOT"),
            (NativeTypeConstructorFamily::AnimType, "ROOT2"),
            (NativeTypeConstructorFamily::AnimType, "ROOT3"),
            (NativeTypeConstructorFamily::UnitType, "FREE"),
            (NativeTypeConstructorFamily::InfantryType, "SECINF"),
            (NativeTypeConstructorFamily::UnitType, "SECUNIT"),
            (NativeTypeConstructorFamily::BuildingType, "SECBLD"),
            (NativeTypeConstructorFamily::OverlayType, "BLDOVL"),
            (NativeTypeConstructorFamily::AnimType, "PLANETR"),
            (NativeTypeConstructorFamily::WeaponType, "W1"),
            (NativeTypeConstructorFamily::WeaponType, "W2"),
            (NativeTypeConstructorFamily::AnimType, "A1"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::AnimType, "A3"),
            (NativeTypeConstructorFamily::AnimType, "A4"),
            (NativeTypeConstructorFamily::AnimType, "A5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS1"),
            (NativeTypeConstructorFamily::WarheadType, "WH1"),
            (NativeTypeConstructorFamily::BulletType, "P1"),
            (NativeTypeConstructorFamily::AnimType, "B1"),
            (NativeTypeConstructorFamily::AnimType, "B2"),
            (NativeTypeConstructorFamily::AnimType, "B3"),
            (NativeTypeConstructorFamily::AnimType, "B4"),
            (NativeTypeConstructorFamily::AnimType, "B5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS2"),
            (NativeTypeConstructorFamily::WarheadType, "WH2"),
            (NativeTypeConstructorFamily::BulletType, "P2"),
            (NativeTypeConstructorFamily::AnimType, "P1TR"),
            (NativeTypeConstructorFamily::WeaponType, "LATEW1"),
            (NativeTypeConstructorFamily::WeaponType, "LATEW2"),
            (NativeTypeConstructorFamily::AnimType, "P2TR"),
        ]
    );
    assert!(actual.iter().all(|(_, id)| *id != "TOO_LATE"));
}

#[test]
fn techno_weapon_bank_uses_effective_native_gates_and_elite_slot_order() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TURRETS\n1=ORDINARY\n2=CLEARED\n\
         [TURRETS]\nTurretCount=2\nWeaponCount=2\nPrimary=WRONG\nWeapon1=W1\nEliteWeapon1=E1\nWeapon2=W2\nEliteWeapon2=E2\n\
         [ORDINARY]\nPrimary=P\nSecondary=S\nElitePrimary=EP\nEliteSecondary=ES\n\
         [CLEARED]\nTurretCount=0\nClearAllWeapons=yes\nPrimary=ALSO_WRONG\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[TURRETS]\nWeaponCount=1\nWeapon1=MAPW1\nEliteWeapon1=MAPE1\n\
             [ORDINARY]\nTurretCount=1\nWeaponCount=0\nPrimary=MAP_WRONG\n",
        ),
    );
    let processed = layers.process().expect("Techno gate fixture processes");
    let weapon_ids = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .filter(|event| event.family() == NativeTypeConstructorFamily::WeaponType)
        .map(|event| event.native_stored_id())
        .collect::<Vec<_>>();

    assert_eq!(
        weapon_ids,
        vec!["W1", "E1", "W2", "E2", "P", "S", "EP", "ES", "MAPW1", "MAPE1"]
    );
    assert!(weapon_ids
        .iter()
        .all(|id| !matches!(*id, "WRONG" | "ALSO_WRONG" | "MAP_WRONG")));
}

#[test]
fn post_type_readers_preserve_crate_through_tiberium_constructor_order() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nWoodCrateImg=O1\nCrateImg=O2\nWaterCrateImg=O3\nUnitCrateType=U1\n\
         [CombatDamage]\nScorches=S0\nScorches1=S1\nScorches2=S2\nScorches3=S3\nScorches4=S4\nSplashList=A0\n\
         FlameDamage=W0\nFlameDamage2=W1\nC4Warhead=W2\nCrushWarhead=W3\nV3Warhead=W4\nDMislWarhead=W5\nV3EliteWarhead=W6\nDMislEliteWarhead=W7\nCMislWarhead=W8\nCMislEliteWarhead=W9\nIvanWarhead=W10\n\
         DeathWeapon=DW\nDrainAnimationType=A1\nControlledAnimationType=A2\nPermaControlledAnimationType=A3\nIonCannonWarhead=W11\n\
         DefaultLargeGreySmokeSystem=PS0\nDefaultSmallGreySmokeSystem=PS1\nDefaultSparkSystem=PS2\nDefaultLargeRedSmokeSystem=PS3\nDefaultSmallRedSmokeSystem=PS4\nDefaultDebrisSmokeSystem=PS5\nDefaultFireStreamSystem=PS6\nDefaultTestParticleSystem=PS7\nDefaultRepairParticleSystem=PS8\n\
         [Radiation]\nRadSiteWarhead=RW\n\
         [AudioVisual]\nDropPodPuff=AV0\nVeinAttack=AV1\nDig=AV2\nAtmosphereEntry=AV3\nTreeFire=AV4,AV5\nOnFire=AV6,AV7\nSmoke=ABCDEFGHIJKLMNOPQRSTUVWXY\nSmallFire=AV8\nLargeFire=AV9\n\
         [SpecialWeapons]\nNukeWarhead=NW\nNukeProjectile=NP\nNukeDown=ND\nMutateWarhead=MW\nMutateExplosionWarhead=MEW\nEMPulseWarhead=EW\nEMPulseProjectile=EP\n\
         [NW]\nParticle=SPS\nAnimList=SAN\nDebrisTypes=SVX\n\
         [Tiberiums]\n0=TIB\n[TIB]\nDebris=TDA,TDB\n",
    ))
    .process()
    .expect("post-reader fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::OverlayType, "O1"),
            (NativeTypeConstructorFamily::OverlayType, "O2"),
            (NativeTypeConstructorFamily::OverlayType, "O3"),
            (NativeTypeConstructorFamily::UnitType, "U1"),
            (NativeTypeConstructorFamily::SmudgeType, "S0"),
            (NativeTypeConstructorFamily::SmudgeType, "S1"),
            (NativeTypeConstructorFamily::SmudgeType, "S2"),
            (NativeTypeConstructorFamily::SmudgeType, "S3"),
            (NativeTypeConstructorFamily::SmudgeType, "S4"),
            (NativeTypeConstructorFamily::AnimType, "A0"),
            (NativeTypeConstructorFamily::WarheadType, "W0"),
            (NativeTypeConstructorFamily::WarheadType, "W1"),
            (NativeTypeConstructorFamily::WarheadType, "W2"),
            (NativeTypeConstructorFamily::WarheadType, "W3"),
            (NativeTypeConstructorFamily::WarheadType, "W4"),
            (NativeTypeConstructorFamily::WarheadType, "W5"),
            (NativeTypeConstructorFamily::WarheadType, "W6"),
            (NativeTypeConstructorFamily::WarheadType, "W7"),
            (NativeTypeConstructorFamily::WarheadType, "W8"),
            (NativeTypeConstructorFamily::WarheadType, "W9"),
            (NativeTypeConstructorFamily::WarheadType, "W10"),
            (NativeTypeConstructorFamily::WeaponType, "DW"),
            (NativeTypeConstructorFamily::AnimType, "A1"),
            (NativeTypeConstructorFamily::AnimType, "A2"),
            (NativeTypeConstructorFamily::AnimType, "A3"),
            (NativeTypeConstructorFamily::WarheadType, "W11"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS0"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS1"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS2"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS3"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS4"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS5"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS6"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS7"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS8"),
            (NativeTypeConstructorFamily::WarheadType, "RW"),
            (NativeTypeConstructorFamily::AnimType, "AV0"),
            (NativeTypeConstructorFamily::AnimType, "AV1"),
            (NativeTypeConstructorFamily::AnimType, "AV2"),
            (NativeTypeConstructorFamily::AnimType, "AV3"),
            (NativeTypeConstructorFamily::AnimType, "AV4"),
            (NativeTypeConstructorFamily::AnimType, "AV5"),
            (NativeTypeConstructorFamily::AnimType, "AV6"),
            (NativeTypeConstructorFamily::AnimType, "AV7"),
            (
                NativeTypeConstructorFamily::AnimType,
                "ABCDEFGHIJKLMNOPQRSTUVWX",
            ),
            (
                NativeTypeConstructorFamily::AnimType,
                "ABCDEFGHIJKLMNOPQRSTUVWX",
            ),
            (NativeTypeConstructorFamily::AnimType, "AV8"),
            (NativeTypeConstructorFamily::AnimType, "AV9"),
            (NativeTypeConstructorFamily::WarheadType, "NW"),
            (NativeTypeConstructorFamily::BulletType, "NP"),
            (NativeTypeConstructorFamily::BulletType, "ND"),
            (NativeTypeConstructorFamily::WarheadType, "MW"),
            (NativeTypeConstructorFamily::WarheadType, "MEW"),
            (NativeTypeConstructorFamily::WarheadType, "EW"),
            (NativeTypeConstructorFamily::BulletType, "EP"),
            (NativeTypeConstructorFamily::ParticleSystemType, "SPS"),
            (NativeTypeConstructorFamily::AnimType, "SAN"),
            (NativeTypeConstructorFamily::VoxelAnimType, "SVX"),
            (NativeTypeConstructorFamily::AnimType, "TDA"),
            (NativeTypeConstructorFamily::AnimType, "TDB"),
        ]
    );
}

#[test]
fn ai_and_general_constructor_sites_cover_the_verified_20_and_89_order() {
    use std::fmt::Write as _;

    const AI_KEYS: &[&str] = &[
        "BuildConst",
        "BuildPower",
        "BuildRefinery",
        "BuildBarracks",
        "BuildTech",
        "BuildWeapons",
        "AlliedBaseDefenses",
        "SovietBaseDefenses",
        "ThirdBaseDefenses",
        "BuildDefense",
        "BuildPDefense",
        "BuildAA",
        "BuildHelipad",
        "BuildRadar",
        "ConcreteWalls",
        "NSGates",
        "EWGates",
        "BuildNavalYard",
        "BuildDummy",
        "NeutralTechBuildings",
    ];
    const GENERAL_SITES: &[(&str, NativeTypeConstructorFamily)] = &[
        ("DamageFireTypes", NativeTypeConstructorFamily::AnimType),
        ("OreTwinkle", NativeTypeConstructorFamily::AnimType),
        ("BarrelExplode", NativeTypeConstructorFamily::AnimType),
        ("BarrelDebris", NativeTypeConstructorFamily::VoxelAnimType),
        (
            "BarrelParticle",
            NativeTypeConstructorFamily::ParticleSystemType,
        ),
        ("NukeTakeOff", NativeTypeConstructorFamily::AnimType),
        ("Wake", NativeTypeConstructorFamily::AnimType),
        ("DropPod", NativeTypeConstructorFamily::AnimType),
        ("DeadBodies", NativeTypeConstructorFamily::AnimType),
        ("MetallicDebris", NativeTypeConstructorFamily::AnimType),
        ("BridgeExplosions", NativeTypeConstructorFamily::AnimType),
        ("IonBlast", NativeTypeConstructorFamily::AnimType),
        ("IonBeam", NativeTypeConstructorFamily::AnimType),
        ("WeatherConClouds", NativeTypeConstructorFamily::AnimType),
        ("WeatherConBolts", NativeTypeConstructorFamily::AnimType),
        (
            "WeatherConBoltExplosion",
            NativeTypeConstructorFamily::AnimType,
        ),
        (
            "DominatorWarhead",
            NativeTypeConstructorFamily::WarheadType,
        ),
        ("DominatorFirstAnim", NativeTypeConstructorFamily::AnimType),
        ("DominatorSecondAnim", NativeTypeConstructorFamily::AnimType),
        ("ChronoPlacement", NativeTypeConstructorFamily::AnimType),
        ("ChronoBeam", NativeTypeConstructorFamily::AnimType),
        ("ChronoBlast", NativeTypeConstructorFamily::AnimType),
        ("ChronoBlastDest", NativeTypeConstructorFamily::AnimType),
        ("WarpIn", NativeTypeConstructorFamily::AnimType),
        ("WarpOut", NativeTypeConstructorFamily::AnimType),
        ("WarpAway", NativeTypeConstructorFamily::AnimType),
        (
            "IronCurtainInvokeAnim",
            NativeTypeConstructorFamily::AnimType,
        ),
        (
            "ForceShieldInvokeAnim",
            NativeTypeConstructorFamily::AnimType,
        ),
        ("WeaponNullifyAnim", NativeTypeConstructorFamily::AnimType),
        ("ChronoSparkle1", NativeTypeConstructorFamily::AnimType),
        ("InfantryExplode", NativeTypeConstructorFamily::AnimType),
        ("FlamingInfantry", NativeTypeConstructorFamily::AnimType),
        ("InfantryHeadPop", NativeTypeConstructorFamily::AnimType),
        ("InfantryNuked", NativeTypeConstructorFamily::AnimType),
        ("InfantryVirus", NativeTypeConstructorFamily::AnimType),
        ("InfantryBrute", NativeTypeConstructorFamily::AnimType),
        ("InfantryMutate", NativeTypeConstructorFamily::AnimType),
        ("Behind", NativeTypeConstructorFamily::AnimType),
        ("MoveFlash", NativeTypeConstructorFamily::AnimType),
        ("Parachute", NativeTypeConstructorFamily::AnimType),
        ("BombParachute", NativeTypeConstructorFamily::AnimType),
        ("DropZoneAnim", NativeTypeConstructorFamily::AnimType),
        ("EMPulseSparkles", NativeTypeConstructorFamily::AnimType),
        ("LargeVisceroid", NativeTypeConstructorFamily::UnitType),
        ("SmallVisceroid", NativeTypeConstructorFamily::UnitType),
        ("DropPodWeapon", NativeTypeConstructorFamily::WeaponType),
        (
            "ExplosiveVoxelDebris",
            NativeTypeConstructorFamily::VoxelAnimType,
        ),
        ("TireVoxelDebris", NativeTypeConstructorFamily::VoxelAnimType),
        ("ScrapVoxelDebris", NativeTypeConstructorFamily::VoxelAnimType),
        ("RepairBay", NativeTypeConstructorFamily::BuildingType),
        ("GDIGateOne", NativeTypeConstructorFamily::BuildingType),
        ("GDIGateTwo", NativeTypeConstructorFamily::BuildingType),
        ("NodGateOne", NativeTypeConstructorFamily::BuildingType),
        ("NodGateTwo", NativeTypeConstructorFamily::BuildingType),
        ("WallTower", NativeTypeConstructorFamily::BuildingType),
        ("Shipyard", NativeTypeConstructorFamily::BuildingType),
        ("GDIPowerPlant", NativeTypeConstructorFamily::BuildingType),
        ("NodRegularPower", NativeTypeConstructorFamily::BuildingType),
        ("NodAdvancedPower", NativeTypeConstructorFamily::BuildingType),
        ("ThirdPowerPlant", NativeTypeConstructorFamily::BuildingType),
        (
            "PrerequisiteProcAlternate",
            NativeTypeConstructorFamily::UnitType,
        ),
        ("BaseUnit", NativeTypeConstructorFamily::UnitType),
        ("HarvesterUnit", NativeTypeConstructorFamily::UnitType),
        ("PadAircraft", NativeTypeConstructorFamily::AircraftType),
        ("Paratrooper", NativeTypeConstructorFamily::InfantryType),
        ("SecretInfantry", NativeTypeConstructorFamily::InfantryType),
        ("SecretUnits", NativeTypeConstructorFamily::UnitType),
        ("SecretBuildings", NativeTypeConstructorFamily::BuildingType),
        ("AlliedDisguise", NativeTypeConstructorFamily::InfantryType),
        ("SovietDisguise", NativeTypeConstructorFamily::InfantryType),
        ("ThirdDisguise", NativeTypeConstructorFamily::InfantryType),
        ("Engineer", NativeTypeConstructorFamily::InfantryType),
        ("Technician", NativeTypeConstructorFamily::InfantryType),
        ("Pilot", NativeTypeConstructorFamily::InfantryType),
        ("AlliedCrew", NativeTypeConstructorFamily::InfantryType),
        ("SovietCrew", NativeTypeConstructorFamily::InfantryType),
        ("ThirdCrew", NativeTypeConstructorFamily::InfantryType),
        ("AmerParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("AllyParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("SovParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("YuriParaDropInf", NativeTypeConstructorFamily::InfantryType),
        ("AnimToInfantry", NativeTypeConstructorFamily::InfantryType),
        (
            "LightningWarhead",
            NativeTypeConstructorFamily::WarheadType,
        ),
        ("PrismType", NativeTypeConstructorFamily::BuildingType),
        ("V3RocketType", NativeTypeConstructorFamily::AircraftType),
        ("DMislType", NativeTypeConstructorFamily::AircraftType),
        ("CMislType", NativeTypeConstructorFamily::AircraftType),
        ("VeinholeTypeClass", NativeTypeConstructorFamily::TerrainType),
        (
            "DefaultMirageDisguises",
            NativeTypeConstructorFamily::TerrainType,
        ),
    ];

    assert_eq!(AI_KEYS.len(), 20);
    assert_eq!(GENERAL_SITES.len(), 89);
    let mut rules = String::from("[AI]\n");
    let mut expected = Vec::new();
    for (index, key) in AI_KEYS.iter().enumerate() {
        let id = format!("AI{index:02}");
        writeln!(rules, "{key}={id}").unwrap();
        expected.push((NativeTypeConstructorFamily::BuildingType, id));
    }
    rules.push_str("[General]\n");
    for (index, &(key, family)) in GENERAL_SITES.iter().enumerate() {
        let id = format!("G{index:02}");
        writeln!(rules, "{key}={id}").unwrap();
        expected.push((family, id));
    }
    rules.push_str("ParaDropPlane=SHOULD_NOT_EXIST\n");

    let processed = RulesLayerStack::new(IniFile::from_str(&rules))
        .process()
        .expect("all AI/General sites process");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id().to_string()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert!(actual.iter().all(|(_, id)| id != "SHOULD_NOT_EXIST"));
}

#[test]
fn warhead_particle_system_and_voxel_children_obey_passed_family_timing() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Warheads]\n0=WH\n\
         [VoxelAnims]\n0=VX\n\
         [Particles]\n0=PART\n\
         [ParticleSystems]\n0=PS\n\
         [WH]\nParticle=WHPS\nAnimList=WHA\nDebrisTypes=WHVX\n\
         [PART]\nWarhead=PARTWH\n\
         [PARTWH]\nAnimList=TOO_LATE_WARHEAD_BODY\n\
         [PS]\nBehavesLike=Smoke\n\
         [WHPS]\nHoldsWhat=WHOLD\n\
         [VX]\nBounceAnim=XBA\nExpireAnim=XEA\nTrailerAnim=XTA\nWarhead=XWH\nAttachedSystem=XPS\n\
         [WHVX]\nBounceAnim=YBA\nExpireAnim=YEA\nTrailerAnim=YTA\nWarhead=YWH\nAttachedSystem=YPS\n\
         [XPS]\nHoldsWhat=TOO_LATE_PARTICLE_SYSTEM_BODY\n",
    ))
    .process()
    .expect("late-family timing fixture processes");
    let actual = processed
        .native_type_construction_trace()
        .events()
        .iter()
        .map(|event| (event.family(), event.native_stored_id()))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (NativeTypeConstructorFamily::WarheadType, "WH"),
            (NativeTypeConstructorFamily::VoxelAnimType, "VX"),
            (NativeTypeConstructorFamily::ParticleSystemType, "PS"),
            (NativeTypeConstructorFamily::ParticleSystemType, "WHPS"),
            (NativeTypeConstructorFamily::AnimType, "WHA"),
            (NativeTypeConstructorFamily::VoxelAnimType, "WHVX"),
            (NativeTypeConstructorFamily::WarheadType, "PARTWH"),
            (NativeTypeConstructorFamily::AnimType, "XBA"),
            (NativeTypeConstructorFamily::AnimType, "XEA"),
            (NativeTypeConstructorFamily::AnimType, "XTA"),
            (NativeTypeConstructorFamily::WarheadType, "XWH"),
            (NativeTypeConstructorFamily::ParticleSystemType, "XPS"),
            (NativeTypeConstructorFamily::AnimType, "YBA"),
            (NativeTypeConstructorFamily::AnimType, "YEA"),
            (NativeTypeConstructorFamily::AnimType, "YTA"),
            (NativeTypeConstructorFamily::WarheadType, "YWH"),
            (NativeTypeConstructorFamily::ParticleSystemType, "YPS"),
        ]
    );
    assert_eq!(
        processed
            .ini()
            .section("Particles")
            .expect("Particle registry")
            .get_values(),
        vec!["PART", "undefined", "WHOLD"]
    );
    assert!(actual.iter().all(|(_, id)| {
        !matches!(
            *id,
            "TOO_LATE_WARHEAD_BODY" | "TOO_LATE_PARTICLE_SYSTEM_BODY"
        )
    }));
}

#[test]
fn negative_tiberium_slot_is_an_explicit_safe_load_error() {
    let error = RulesLayerStack::new(IniFile::from_str("[Tiberiums]\n-1=BadSlot\n"))
        .process()
        .expect_err("native negative pre-array slot must not be accepted");
    assert!(matches!(
        error,
        RulesError::InvalidValue {
            ref section,
            ref key,
            ..
        } if section == "Tiberiums" && key == "-1"
    ));
}

#[test]
fn crate_rules_constructor_and_stock_fields_keep_native_bits() {
    let defaults = RulesLayerStack::new(IniFile::from_str(""))
        .process()
        .expect("crate rules process");
    assert_eq!(defaults.crate_rules().minimum, 1);
    assert_eq!(defaults.crate_rules().maximum, 255);
    assert_eq!(defaults.crate_rules().regen.bits(), 10.0_f64.to_bits());
    assert_eq!(defaults.crate_rules().wood_crate_img, None);
    assert_eq!(defaults.crate_rules().crate_img, None);
    assert_eq!(defaults.crate_rules().water_crate_img, None);

    let stock = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nCrateMinimum=1\nCrateMaximum=255\nCrateRegen=3\n\
         WoodCrateImg=CRATE\nCrateImg=CRATE\nWaterCrateImg=WCRATE\n",
    ))
    .process()
    .expect("crate rules process");
    assert_eq!(stock.crate_rules().minimum, 1);
    assert_eq!(stock.crate_rules().maximum, 255);
    assert_eq!(stock.crate_rules().regen.bits(), 3.0_f64.to_bits());
    assert_eq!(stock.crate_rules().wood_crate_img.as_deref(), Some("CRATE"));
    assert_eq!(stock.crate_rules().crate_img.as_deref(), Some("CRATE"));
    assert_eq!(
        stock.crate_rules().water_crate_img.as_deref(),
        Some("WCRATE")
    );
}

#[test]
fn crate_rules_retain_per_pass_and_preserve_signed_values() {
    let processed = process_rules_passes(
        "[CrateRules]\nCrateMinimum=-7\nCrateMaximum=-12\nCrateRegen=3\n\
         WoodCrateImg=WOOD\nCrateImg=COMMON\nWaterCrateImg=WATER\n",
        "[CrateRules]\nCrateMaximum=2\nWaterCrateImg=none\n",
    );
    let rules = processed.crate_rules();
    assert_eq!(rules.minimum, -7, "missing later key retains signed value");
    assert_eq!(rules.maximum, 2);
    assert_eq!(rules.regen.bits(), 3.0_f64.to_bits());
    assert_eq!(rules.wood_crate_img.as_deref(), Some("WOOD"));
    assert_eq!(rules.crate_img.as_deref(), Some("COMMON"));
    assert_eq!(rules.water_crate_img, None);

    let absent = process_rules_passes(
        "[CrateRules]\nCrateMinimum=9\nCrateMaximum=3\nCrateImg=FIRST\n",
        "[General]\nBuildSpeed=.7\n",
    );
    assert_eq!(absent.crate_rules().minimum, 9);
    assert_eq!(absent.crate_rules().maximum, 3);
    assert_eq!(absent.crate_rules().crate_img.as_deref(), Some("FIRST"));
}

#[test]
fn crate_rule_images_allocate_and_alias_by_overlay_identity() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CrateRules]\nWoodCrateImg=AliasCrate\nCrateImg=aliascrate\n\
         WaterCrateImg=NewWater\n",
    ))
    .process()
    .expect("crate rules process");
    let overlays = processed
        .ini()
        .section("OverlayTypes")
        .expect("crate references allocate overlays");
    assert_eq!(overlays.get_values(), vec!["AliasCrate", "NewWater"]);

    let registry =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(processed.ini(), None);
    assert_eq!(
        registry.id_for_name("AliasCrate"),
        registry.id_for_name("aliascrate")
    );
    assert_ne!(
        registry.id_for_name("AliasCrate"),
        registry.id_for_name("NewWater")
    );
}

#[test]
fn crate_rule_image_readstring_capacity_owns_retention_and_allocation() {
    let exact = "A".repeat(127);
    let over = "B".repeat(128);
    let processed = RulesLayerStack::new(IniFile::from_str(&format!(
        "[CrateRules]\nWoodCrateImg={exact}\nCrateImg={over}\nWaterCrateImg={over}Z\n"
    )))
    .process()
    .expect("crate rules process");

    assert_eq!(
        processed.crate_rules().wood_crate_img.as_deref(),
        Some(exact.as_str())
    );
    let truncated = "B".repeat(127);
    assert_eq!(
        processed.crate_rules().crate_img.as_deref(),
        Some(truncated.as_str())
    );
    assert_eq!(
        processed.crate_rules().water_crate_img.as_deref(),
        Some(truncated.as_str()),
        "capacity includes the forced NUL, so only 127 ASCII bytes survive"
    );
    // The semantic CrateRules strings keep the 127-byte ReadString capacity,
    // but every Type constructor stores only the native 24-byte ID and Find
    // compares the full input against that stored prefix. A 127-byte name
    // therefore misses every lookup and constructs a duplicate stored ID per
    // reference (see `native_type_registry_compares_full_input_but_stores_and_emits_24_bytes`).
    let stored_a = "A".repeat(0x18);
    let stored_b = "B".repeat(0x18);
    assert_eq!(
        processed
            .ini()
            .section("OverlayTypes")
            .expect("truncated references allocate")
            .get_values(),
        vec![stored_a.as_str(), stored_b.as_str(), stored_b.as_str()],
        "late allocation stores 24-byte IDs while semantic crate strings keep 127 bytes"
    );
}

#[test]
fn crate_rules_direct_and_one_layer_entry_points_agree() {
    let ini = IniFile::from_str(
        "[CrateRules]\nCrateMinimum=-2\nCrateMaximum=6\nCrateRegen=3\n\
         WoodCrateImg=WOOD\nCrateImg=COMMON\nWaterCrateImg=WATER\n",
    );
    let direct = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("direct rules");
    let layered =
        crate::rules::ruleset::RuleSet::from_rules_layers(&RulesLayerStack::new(ini.clone()))
            .expect("layered rules");
    assert_eq!(direct.crate_rules, layered.crate_rules);
    assert_eq!(direct.source_ini_hash(), layered.source_ini_hash());
}

#[test]
fn later_malformed_weapon_bool_preserves_current_field_default() {
    use crate::rules::weapon_type::WeaponType;

    let processed = process_rules_passes(
        "[VehicleTypes]\n0=TANK\n[TANK]\nPrimary=Gun\n[Gun]\nRevealOnFire=yes\n",
        "[Gun]\nRevealOnFire=maybe\n",
    );
    let section = processed.ini().section("Gun").expect("allocated Gun body");

    assert_eq!(section.get("RevealOnFire"), Some("maybe"));
    assert!(WeaponType::from_ini_section("Gun", section).reveal_on_fire);
}

#[test]
fn later_allocated_type_does_not_read_earlier_orphan_body() {
    let processed = process_rules_passes(
        "[LATE]\nStrength=900\nCost=700\n",
        "[VehicleTypes]\n0=LATE\n",
    );
    let late = processed.ini().section("LATE").expect("allocated body");
    assert_eq!(late.get("Strength"), None);
    assert_eq!(late.get("Cost"), None);
}

#[test]
fn existing_type_keeps_prior_fields_and_applies_current_body() {
    let processed = process_rules_passes(
        "[VehicleTypes]\n0=EARLY\n[EARLY]\nStrength=100\nCost=700\n",
        "[EARLY]\nStrength=250\n",
    );
    let early = processed.ini().section("EARLY").expect("EARLY body");
    assert_eq!(early.get("Strength"), Some("250"));
    assert_eq!(early.get("Cost"), Some("700"));
}

#[test]
fn tiberium_pass_reuses_numeric_slot_and_ignores_replacement_identity() {
    let processed = process_rules_passes(
        "[Tiberiums]\n0=Riparius\n[Riparius]\nImage=1\nValue=25\n",
        "[Tiberiums]\n0=Cruentus\n[Riparius]\nImage=4\n[Cruentus]\nImage=2\n",
    );

    assert_eq!(
        processed.ini().section("Tiberiums").unwrap().get_values(),
        vec!["Riparius"]
    );
    let riparius = processed.ini().section("Riparius").unwrap();
    assert_eq!(riparius.get("Image"), Some("4"));
    assert_eq!(riparius.get("Value"), Some("25"));
    assert!(processed.ini().section("Cruentus").is_some());
}

#[test]
fn tiberium_out_of_range_slot_appends_one_live_type() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Tiberiums]\n7=Riparius\n0=Cruentus\n[Riparius]\nImage=1\n[Cruentus]\nImage=2\n",
    ))
    .process()
    .expect("Tiberium pass processes");

    assert_eq!(
        processed.ini().section("Tiberiums").unwrap().get_values(),
        vec!["Riparius"]
    );
    assert_eq!(
        processed
            .ini()
            .section("Riparius")
            .and_then(|section| section.get("Image")),
        Some("1")
    );
}

#[test]
fn new_type_reads_its_same_pass_body() {
    let processed = process_rules_passes(
        "[VehicleTypes]\n0=EARLY\n[EARLY]\nStrength=100\n",
        "[VehicleTypes]\n0=LATE\n[LATE]\nStrength=250\n",
    );
    assert_eq!(
        processed
            .ini()
            .section("LATE")
            .and_then(|section| section.get("Strength")),
        Some("250")
    );
}

#[test]
fn ordered_pass_registry_union_is_case_insensitive_by_value() {
    let processed = process_rules_passes(
        "[VehicleTypes]\nFirst=MTNK\n",
        "[VehicleTypes]\n0=mtnk\nAgain=HTNK\n",
    );
    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .unwrap()
            .get_values(),
        vec!["MTNK", "HTNK"]
    );
}

#[test]
fn gsi_05_01_type_allocation_rejects_native_none_sentinels() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=none\n1=<NONE>\n2= NONE \n3=\t<none>\t\n4=NONE_TANK\n",
    ))
    .process()
    .expect("native none fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .expect("rebuilt vehicle registry")
            .get_values(),
        vec!["NONE_TANK"]
    );

    let mut processor = RulesPassProcessor::default();
    assert_eq!(processor.find_or_allocate(RulesTypeFamily::Vehicle, ""), None);
    assert_eq!(
        processor.find_or_allocate(RulesTypeFamily::Vehicle, "none"),
        None
    );
    assert_eq!(
        processor.find_or_allocate(RulesTypeFamily::Vehicle, "<NONE>"),
        None
    );
}

#[test]
fn later_pass_missing_scalar_key_preserves_live_value() {
    let processed = process_rules_passes(
        "[General]\nBuildSpeed=.7\nFlightLevel=1500\n",
        "[General]\nBuildSpeed=.58\n",
    );
    let general = processed.ini().section("General").unwrap();
    assert_eq!(general.get("BuildSpeed"), Some(".58"));
    assert_eq!(general.get("FlightLevel"), Some("1500"));
}

#[test]
fn later_general_section_without_damage_fire_types_preserves_live_list() {
    let processed = process_rules_passes(
        "[General]\nDamageFireTypes=FIRE01,FIRE02\nBuildSpeed=.7\n",
        "[General]\nBuildSpeed=.58\n",
    );
    let general = processed.ini().section("General").unwrap();
    assert_eq!(general.get("DamageFireTypes"), Some("FIRE01,FIRE02"));
    assert_eq!(general.get("BuildSpeed"), Some(".58"));
}

#[test]
fn general_prerequisite_groups_are_lookup_only() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nPrerequisitePower=MAPPOWR\n[MAPPOWR]\nStrength=750\n",
    ))
    .process()
    .expect("prerequisite lookup fixture processes");

    assert!(
        processed
            .ini()
            .section("BuildingTypes")
            .unwrap()
            .get_values()
            .is_empty()
    );
    assert_eq!(
        processed
            .ini()
            .section("General")
            .unwrap()
            .get("PrerequisitePower"),
        Some("")
    );
}

#[test]
fn general_prerequisite_groups_keep_only_registered_buildings() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=GAPOWR\n[General]\nPrerequisitePower=gapowr,MISSING\n",
    ))
    .process()
    .expect("prerequisite registration fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("General")
            .unwrap()
            .get("PrerequisitePower"),
        Some("GAPOWR")
    );
}

#[test]
fn prerequisite_proc_alternate_allocates_unit_before_same_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nPrerequisiteProcAlternate=SMIN\n[SMIN]\nStrength=2000\n",
    ))
    .process()
    .expect("ProcAlternate fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("VehicleTypes")
            .unwrap()
            .get_values(),
        vec!["SMIN"]
    );
    assert_eq!(
        processed
            .ini()
            .section("SMIN")
            .and_then(|section| section.get("Strength")),
        Some("2000")
    );
}

#[test]
fn barrel_particle_allocates_particle_system_before_same_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[General]\nBarrelParticle=BarrelSys\n[BarrelSys]\nHoldsWhat=SmokePart\n[SmokePart]\nDamage=5\n",
    ))
    .process()
    .expect("particle-system fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("ParticleSystems")
            .unwrap()
            .get_values(),
        vec!["BarrelSys"]
    );
    assert_eq!(
        processed.ini().section("Particles").unwrap().get_values(),
        vec!["SmokePart"]
    );
    assert_eq!(
        processed
            .ini()
            .section("BarrelSys")
            .and_then(|section| section.get("HoldsWhat")),
        Some("SmokePart")
    );
    assert_eq!(
        processed
            .ini()
            .section("SmokePart")
            .and_then(|section| section.get("Damage")),
        None,
        "HoldsWhat allocates the particle after the particle body sweep"
    );
}

#[test]
fn special_weapons_created_warhead_reads_same_pass_body() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[SpecialWeapons]\nMutateWarhead=FreshMutate\n\
         [FreshMutate]\nCellSpread=3\nVerses=100%,80%\n",
    ))
    .process()
    .expect("SpecialWeapons Warhead fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("FreshMutate")
            .and_then(|section| section.get("CellSpread")),
        Some("3")
    );
}

#[test]
fn special_weapons_created_projectile_waits_for_next_pass_body_sweep() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[SpecialWeapons]\nNukeProjectile=FreshNuke\n[FreshNuke]\nImage=NUKE\n",
    ))
    .process()
    .expect("SpecialWeapons Bullet fixture processes");

    assert_eq!(
        processed
            .ini()
            .section("FreshNuke")
            .and_then(|section| section.get("Image")),
        None
    );
}

#[test]
fn combat_damage_allocates_late_smudge_and_animation_references() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[CombatDamage]\n\
         Scorches=BurnA,BurnB\n\
         Scorches1=BurnC\n\
         Scorches2=BurnD\n\
         Scorches3=BurnE\n\
         Scorches4=BurnF\n\
         SplashList=SplashA,SplashB\n\
         DrainAnimationType=DrainAnim\n\
         ControlledAnimationType=MindAnim\n\
         PermaControlledAnimationType=MindAnimR\n\
         [BurnA]\nWidth=3\n",
    ))
    .process()
    .expect("CombatDamage fixture processes");

    assert_eq!(
        processed.ini().section("SmudgeTypes").unwrap().get_values(),
        vec!["BurnA", "BurnB", "BurnC", "BurnD", "BurnE", "BurnF"]
    );
    assert_eq!(
        processed.ini().section("Animations").unwrap().get_values(),
        vec!["SplashA", "SplashB", "DrainAnim", "MindAnim", "MindAnimR"]
    );
    assert_eq!(
        processed
            .ini()
            .section("BurnA")
            .and_then(|section| section.get("Width")),
        None,
        "late CombatDamage allocations wait for the next type-data pass"
    );
}

#[test]
fn rules_hash_preserves_pass_boundaries() {
    let single = RulesLayerStack::new(IniFile::from_str("[General]\nBuildSpeed=.58\n"));
    let mut layered = RulesLayerStack::new(IniFile::from_str("[General]\nBuildSpeed=.7\n"));
    layered.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[General]\nBuildSpeed=.58\n"),
    );
    assert_eq!(
        single
            .process()
            .expect("single Rules pass processes")
            .ini()
            .section("General")
            .unwrap()
            .get("BuildSpeed"),
        layered
            .process()
            .expect("layered Rules passes process")
            .ini()
            .section("General")
            .unwrap()
            .get("BuildSpeed")
    );
    assert_ne!(single.content_hash(), layered.content_hash());
}

#[test]
fn projectiles_and_sides_keep_ordinary_compatibility_projection_overlay() {
    let processed = process_rules_passes(
        "[Projectiles]\n0=KEEP\n[Sides]\nGDI=Americans\n",
        "[Projectiles]\n0=REPLACE\n[Sides]\nGDI=French\n",
    );
    assert_eq!(
        processed.ini().section("Projectiles").unwrap().get("0"),
        Some("REPLACE")
    );
    assert_eq!(
        processed.ini().section("Sides").unwrap().get("GDI"),
        Some("French")
    );
}

/// Sections not proven to be native registries keep ordinary overlay behavior.
#[test]
fn existing_unlisted_numbered_section_receives_ordinary_overlay() {
    let mut rules = IniFile::from_str("[FutureTypes]\n0=KEEP\n");
    let map = IniFile::from_str("[FutureTypes]\n0=EVIL\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(rules.section("FutureTypes").unwrap().get("0"), Some("EVIL"));
}

/// Registry processing uses every entry value, including entries whose keys
/// are not numeric.
#[test]
fn map_registry_pass_uses_every_entry_in_source_order() {
    let mut rules =
        IniFile::from_str("[Particles]\n30=FireStream\n[ParticleSystems]\n10=GasCloudSys\n");
    let map = IniFile::from_str(
        "[Particles]\n30=EvilFire\nName=oops\n[ParticleSystems]\n10=EvilSys\nStray=1\n",
    );
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("Particles").unwrap().get_values(),
        vec!["FireStream", "EvilFire", "oops"]
    );
    assert_eq!(
        rules.section("ParticleSystems").unwrap().get_values(),
        vec!["GasCloudSys", "EvilSys", "1"]
    );
}

/// Empty-value semantics: a map line `BuildSpeed=` with no value is
/// "key absent" in the original (its INI writer gates out empty/NULL values
/// and never stores an entry, so the readers keep the value already on the
/// field). It must NOT clobber the value from an earlier rules pass — clobbering it
/// with `""` would reset the field to the hardcoded Rust default at parse time.
#[test]
fn map_overrides_skip_empty_valued_keys() {
    let mut rules = IniFile::from_str(
        "[General]\nBuildSpeed=.7\nFlightLevel=1500\n[CombatDamage]\nC4Delay=.03\n",
    );
    let map = IniFile::from_str("[General]\nBuildSpeed=\n[CombatDamage]\nC4Delay=.06\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("General").unwrap().get("BuildSpeed"),
        Some(".7"),
        "empty map value must leave the merged value intact"
    );
    assert_eq!(
        rules.section("CombatDamage").unwrap().get("C4Delay"),
        Some(".06")
    );
}

/// `[Colors]` is a find-or-create registry: an existing case-insensitive
/// identity keeps its first HSV value, while a new name allocates.
#[test]
fn map_colors_keep_existing_identity_and_allocate_new_name() {
    let mut rules = IniFile::from_str("[Colors]\nGold=42,252,252\nDarkRed=0,151,239\n");
    let map = IniFile::from_str("[Colors]\ngold=1,2,3\nNeonPink=12,200,255\n");
    let rules = process_ini_passes(rules, map).into_projection_discarding_native_receipt();
    assert_eq!(
        rules.section("Colors").unwrap().get("Gold"),
        Some("42,252,252")
    );
    assert_eq!(
        rules.section("Colors").unwrap().get("NeonPink"),
        Some("12,200,255")
    );
}

