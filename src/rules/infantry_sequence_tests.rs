//! Tests for infantry animation sequence parsing from art.ini.

use super::*;
use crate::rules::ini_parser::IniFile;

#[derive(serde::Deserialize)]
struct NativeSequenceCorpus {
    names: Vec<String>,
    cases: Vec<NativeSequenceRow>,
}

#[derive(serde::Deserialize)]
struct NativeSequenceRow {
    sequence: Option<String>,
    before: Vec<[i32; 5]>,
    after: Vec<[i32; 5]>,
    layers: Vec<std::collections::BTreeMap<String, Option<String>>>,
}

fn native_sequence_entry(record: [i32; 5]) -> InfantrySequenceEntry {
    let hints = [
        FacingHint::N,
        FacingHint::NE,
        FacingHint::E,
        FacingHint::SE,
        FacingHint::S,
        FacingHint::SW,
        FacingHint::W,
        FacingHint::NW,
    ];
    InfantrySequenceEntry {
        start_frame: record[0],
        frames_per_facing: record[1],
        facings: record[2],
        facing_hint: usize::try_from(record[3])
            .ok()
            .and_then(|i| hints.get(i).copied()),
    }
}

#[test]
fn signed_action_records_match_original_partial_reader_corpus() {
    let corpus: NativeSequenceCorpus = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/infantry_sequence_rules.json"
    ))
    .unwrap();
    assert_eq!(corpus.names, NATIVE_SEQUENCE_NAMES);
    assert_eq!(corpus.cases.len(), 76);
    for (case, row) in corpus.cases.into_iter().enumerate() {
        let mut records: Vec<_> = row.before.into_iter().map(native_sequence_entry).collect();
        if row.sequence.is_some() {
            for layer in &row.layers {
                for (index, name) in NATIVE_SEQUENCE_NAMES.iter().enumerate() {
                    if let Some(Some(value)) = layer.get(*name) {
                        read_sequence_value(value, &mut records[index]);
                    }
                }
            }
        }
        let expected: Vec<_> = row.after.into_iter().map(native_sequence_entry).collect();
        assert_eq!(
            records, expected,
            "original523D00 case {case}: {:?}",
            row.layers
        );
    }
}

#[test]
fn catalog_preserves_signed_actions_and_distinct_ready_guard() {
    use crate::rules::animation_sequence::build_animation_sequence_catalog;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ruleset::RuleSet;

    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=TEST\n[TEST]\nImage=TEST\n",
    ))
    .unwrap();
    let art = IniFile::from_str(
        "[TEST]\nSequence=ArbitraryActions\n[ArbitraryActions]\nReady=7,1,1\nGuard=91,2,2\nDeploy=-8,65536,-3,S\n",
    );
    rules.art_registry = ArtRegistry::from_ini(&art);
    let registry = parse_infantry_sequence_registry(&art);
    let catalog = build_animation_sequence_catalog(&rules, Some(&registry));
    let set = &catalog["TEST"];
    assert_eq!(set.infantry_action(0).unwrap().start_frame, 7);
    assert_eq!(set.infantry_action(1).unwrap().start_frame, 91);
    assert_eq!(
        *set.infantry_action(27).unwrap(),
        InfantrySequenceEntry {
            start_frame: -8,
            frames_per_facing: 65536,
            facings: -3,
            facing_hint: Some(FacingHint::S),
        }
    );
    assert_eq!(
        *set.infantry_action(31).unwrap(),
        InfantrySequenceEntry::default()
    );

    // No u16 draw entry can represent this bank. It still owns action data.
    let signed_only = IniFile::from_str("[ArbitraryActions]\nDeploy=-8,65536,-3\n");
    let registry = parse_infantry_sequence_registry(&signed_only);
    let catalog = build_animation_sequence_catalog(&rules, Some(&registry));
    assert_eq!(
        catalog["TEST"]
            .infantry_action(27)
            .unwrap()
            .frames_per_facing,
        65536
    );
    let missing = build_animation_sequence_catalog(&rules, None);
    for action in 0..42 {
        assert_eq!(
            *missing["TEST"].infantry_action(action).unwrap(),
            InfantrySequenceEntry::default()
        );
    }
}

#[test]
fn projected_partial_sequence_reads_retain_prior_fields() {
    let mut ini = IniFile::from_str("");
    ini.merge_rules_projection(&IniFile::from_str("[Actions]\nDeploy=1,2,3,S\n"));
    ini.merge_rules_projection(&IniFile::from_str("[Actions]\nDeploy=9\n"));
    let registry = parse_infantry_sequence_registry(&ini);
    let set = build_sequence_set(&registry["ACTIONS"]);
    assert_eq!(
        *set.infantry_action(27).unwrap(),
        InfantrySequenceEntry {
            start_frame: 9,
            frames_per_facing: 2,
            facings: 3,
            facing_hint: Some(FacingHint::S),
        }
    );
}

#[test]
fn auto_deploy_difficulty_vector_matches_original_retained_reader() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    #[derive(serde::Deserialize)]
    struct Row {
        prior: Vec<i32>,
        raw: Option<String>,
        output: Vec<i32>,
    }
    let rows: Vec<Row> = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/infantry_deploy_rules.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 42);
    for row in rows {
        let mut projected = IniFile::from_str("");
        if !row.prior.is_empty() {
            let prior = row
                .prior
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            projected.merge_rules_projection(&IniFile::from_str(&format!(
                "[General]\nAIAutoDeployFrameDelay={prior}\n"
            )));
        }
        let mut patch = IniFile::from_str("[General]\nFixture=1\n");
        if let Some(raw) = &row.raw {
            // Supply the exact ReadString payload, including whitespace that
            // the physical INI loader discards before this caller boundary.
            patch
                .projection_section_mut("General")
                .set("AIAutoDeployFrameDelay", raw);
        }
        let mut layers = RulesLayerStack::new(projected);
        layers.push(RulesLayerKind::Scenario, patch);
        let rules = crate::rules::ruleset::RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.general.ai_auto_deploy_frame_delay, row.output,
            "prior={:?}, raw={:?}",
            row.prior, row.raw
        );
    }
    let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
        "[General]\nAIAutoDeployFrameDelay=-1,65536,2147483648\n",
    ))
    .unwrap();
    assert_eq!(
        rules.general.ai_auto_deploy_frame_delay,
        [-1, 65536, i32::MIN]
    );
}

#[test]
fn test_parse_sequence_value_basic() {
    let entry: InfantrySequenceEntry =
        parse_sequence_value("8,6,6").expect("Should parse Walk=8,6,6");
    assert_eq!(entry.start_frame, 8);
    assert_eq!(entry.frames_per_facing, 6);
    assert_eq!(entry.facings, 6);
    assert_eq!(entry.facing_hint, None);
}

#[test]
fn test_parse_sequence_value_with_facing_hint() {
    let entry: InfantrySequenceEntry =
        parse_sequence_value("56,15,0,S").expect("Should parse Idle1=56,15,0,S");
    assert_eq!(entry.start_frame, 56);
    assert_eq!(entry.frames_per_facing, 15);
    assert_eq!(entry.facings, 0);
    assert_eq!(entry.facing_hint, Some(FacingHint::S));
}

#[test]
fn test_parse_sequence_value_facing_hint_ne() {
    let entry: InfantrySequenceEntry =
        parse_sequence_value("71,15,0,NE").expect("Should parse with NE hint");
    assert_eq!(entry.facing_hint, Some(FacingHint::NE));
}

#[test]
fn gsi_05_07_all_completion_facing_tokens_map_to_dir_bytes() {
    let expected = [
        ("N", 0),
        ("NE", 32),
        ("E", 64),
        ("SE", 96),
        ("S", 128),
        ("SW", 160),
        ("W", 192),
        ("NW", 224),
    ];
    for (token, facing) in expected {
        let entry =
            parse_sequence_value(&format!("0,1,0,{token}")).expect("valid completion-facing token");
        assert_eq!(
            completion_facing(entry.facing_hint),
            Some(facing),
            "{token}"
        );
    }
    assert_eq!(completion_facing(None), None);
}

#[test]
fn test_parse_sequence_value_single_frame() {
    let entry: InfantrySequenceEntry =
        parse_sequence_value("0,1,1").expect("Should parse Ready=0,1,1");
    assert_eq!(entry.start_frame, 0);
    assert_eq!(entry.frames_per_facing, 1);
    assert_eq!(entry.facings, 1);
    assert_eq!(entry.facing_hint, None);
}

#[test]
fn test_parse_sequence_value_zero_frames() {
    let entry: InfantrySequenceEntry =
        parse_sequence_value("0,0,0").expect("Should parse zero-frame entry");
    assert_eq!(entry.start_frame, 0);
    assert_eq!(entry.frames_per_facing, 0);
    assert_eq!(entry.facings, 0);
}

#[test]
fn partial_sequence_reads_keep_unconverted_constructor_fields() {
    assert_eq!(
        parse_sequence_value("8,6").unwrap(),
        InfantrySequenceEntry {
            start_frame: 8,
            frames_per_facing: 6,
            ..InfantrySequenceEntry::default()
        }
    );
    assert_eq!(
        parse_sequence_value("8").unwrap(),
        InfantrySequenceEntry {
            start_frame: 8,
            ..InfantrySequenceEntry::default()
        }
    );
    assert!(parse_sequence_value("").is_none());
}

#[test]
fn test_parse_sequence_value_invalid_number() {
    assert_eq!(
        parse_sequence_value("abc,6,6").unwrap(),
        InfantrySequenceEntry::default()
    );
    assert_eq!(
        parse_sequence_value("8,xyz,6").unwrap(),
        InfantrySequenceEntry {
            start_frame: 8,
            ..InfantrySequenceEntry::default()
        }
    );
}

#[test]
fn test_sequence_kind_from_ini_key_core_mappings() {
    assert_eq!(
        sequence_kind_from_ini_key("Ready"),
        Some(SequenceKind::Stand)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Guard"),
        Some(SequenceKind::Stand)
    );
    assert_eq!(sequence_kind_from_ini_key("Walk"), Some(SequenceKind::Walk));
    assert_eq!(
        sequence_kind_from_ini_key("FireUp"),
        Some(SequenceKind::Attack)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Idle1"),
        Some(SequenceKind::Idle1)
    );
    assert_eq!(sequence_kind_from_ini_key("Die1"), Some(SequenceKind::Die1));
    assert_eq!(sequence_kind_from_ini_key("Die5"), Some(SequenceKind::Die5));
    assert_eq!(
        sequence_kind_from_ini_key("Prone"),
        Some(SequenceKind::Prone)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Crawl"),
        Some(SequenceKind::Crawl)
    );
    assert_eq!(
        sequence_kind_from_ini_key("FireProne"),
        Some(SequenceKind::FireProne)
    );
    assert_eq!(sequence_kind_from_ini_key("Down"), Some(SequenceKind::Down));
    assert_eq!(sequence_kind_from_ini_key("Up"), Some(SequenceKind::Up));
    assert_eq!(
        sequence_kind_from_ini_key("Cheer"),
        Some(SequenceKind::Cheer)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Paradrop"),
        Some(SequenceKind::Paradrop)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Panic"),
        Some(SequenceKind::Panic)
    );
}

#[test]
fn test_sequence_kind_from_ini_key_case_insensitive() {
    assert_eq!(sequence_kind_from_ini_key("walk"), Some(SequenceKind::Walk));
    assert_eq!(sequence_kind_from_ini_key("WALK"), Some(SequenceKind::Walk));
    assert_eq!(sequence_kind_from_ini_key("Walk"), Some(SequenceKind::Walk));
}

#[test]
fn test_sequence_kind_from_ini_key_unknown_returns_none() {
    assert_eq!(sequence_kind_from_ini_key("Tumble"), None);
    assert_eq!(sequence_kind_from_ini_key("Carry"), None);
    assert_eq!(sequence_kind_from_ini_key("Shovel"), None);
    assert_eq!(sequence_kind_from_ini_key("Bogus"), None);
}

#[test]
fn test_sequence_kind_from_ini_key_deploy_variants() {
    assert_eq!(
        sequence_kind_from_ini_key("Deploy"),
        Some(SequenceKind::Deploy)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Undeploy"),
        Some(SequenceKind::Undeploy)
    );
    assert_eq!(
        sequence_kind_from_ini_key("Deployed"),
        Some(SequenceKind::Deployed)
    );
    assert_eq!(
        sequence_kind_from_ini_key("DeployedFire"),
        Some(SequenceKind::DeployedFire)
    );
    assert_eq!(
        sequence_kind_from_ini_key("DeployedIdle"),
        Some(SequenceKind::DeployedIdle)
    );
    assert_eq!(sequence_kind_from_ini_key("Swim"), Some(SequenceKind::Swim));
    assert_eq!(sequence_kind_from_ini_key("Fly"), Some(SequenceKind::Fly));
    assert_eq!(
        sequence_kind_from_ini_key("WetAttack"),
        Some(SequenceKind::WetAttack)
    );
}

#[test]
fn test_parse_con_sequence_from_ini() {
    let ini_text: &str = "\
[ConSequence]
Ready=0,1,1
Guard=0,1,1
Walk=8,6,6
FireUp=164,6,6
Idle1=56,15,0,S
Idle2=71,15,0,E
Die1=134,15,0
Die2=149,15,0
Prone=86,1,6
Crawl=86,6,6
Down=260,2,2
Up=276,2,2
FireProne=212,6,6
Cheer=293,8,0,E
Paradrop=292,1,0
Panic=8,6,6
";
    let ini: IniFile = IniFile::from_str(ini_text);
    let registry: InfantrySequenceRegistry = parse_infantry_sequence_registry(&ini);

    assert_eq!(registry.len(), 1, "Should parse exactly one sequence def");
    let def: &InfantrySequenceDef = registry
        .get("CONSEQUENCE")
        .expect("Should find ConSequence");

    // Walk: start=8, 6 frames, 6 facings.
    let walk: &InfantrySequenceEntry = def.entries.get("WALK").expect("Should have Walk");
    assert_eq!(walk.start_frame, 8);
    assert_eq!(walk.frames_per_facing, 6);
    assert_eq!(walk.facings, 6);

    // Idle1: non-directional with facing hint S.
    let idle1: &InfantrySequenceEntry = def.entries.get("IDLE1").expect("Should have Idle1");
    assert_eq!(idle1.start_frame, 56);
    assert_eq!(idle1.frames_per_facing, 15);
    assert_eq!(idle1.facings, 0);
    assert_eq!(idle1.facing_hint, Some(FacingHint::S));

    // FireUp: start=164, 6 frames, 6 facings.
    let fire: &InfantrySequenceEntry = def.entries.get("FIREUP").expect("Should have FireUp");
    assert_eq!(fire.start_frame, 164);
    assert_eq!(fire.frames_per_facing, 6);
    assert_eq!(fire.facings, 6);
}

#[test]
fn test_build_sequence_set_from_con_sequence() {
    let ini_text: &str = "\
[ConSequence]
Ready=0,1,1
Guard=0,1,1
Walk=8,6,6
FireUp=164,6,6
Idle1=56,15,0,S
Idle2=71,15,0,E
Die1=134,15,0
Die2=149,15,0
Prone=86,1,6
Crawl=86,6,6
Down=260,2,2
Up=276,2,2
FireProne=212,6,6
Cheer=293,8,0,E
Paradrop=292,1,0
Panic=8,6,6
";
    let ini: IniFile = IniFile::from_str(ini_text);
    let registry: InfantrySequenceRegistry = parse_infantry_sequence_registry(&ini);
    let def: &InfantrySequenceDef = registry
        .get("CONSEQUENCE")
        .expect("Should find ConSequence");
    let set: SequenceSet = build_sequence_set(def);

    // Stand (from Ready=0,1,1): INI multiplier=1 → facings=8, facing_multiplier=1.
    let stand: &SequenceDef = set.get(&SequenceKind::Stand).expect("Should have Stand");
    assert_eq!(stand.start_frame, 0);
    assert_eq!(stand.frame_count, 1);
    assert_eq!(stand.facings, 8);
    assert_eq!(stand.facing_multiplier, 1);

    // Walk (from Walk=8,6,6): facings=8, facing_multiplier=6.
    let walk: &SequenceDef = set.get(&SequenceKind::Walk).expect("Should have Walk");
    assert_eq!(walk.start_frame, 8);
    assert_eq!(walk.frame_count, 6);
    assert_eq!(walk.facings, 8);
    assert_eq!(walk.facing_multiplier, 6);

    // Attack (from FireUp=164,6,6): facings=8, facing_multiplier=6.
    let attack: &SequenceDef = set.get(&SequenceKind::Attack).expect("Should have Attack");
    assert_eq!(attack.start_frame, 164);
    assert_eq!(attack.frame_count, 6);
    assert_eq!(attack.facings, 8);
    assert_eq!(attack.facing_multiplier, 6);
    assert!(matches!(
        attack.loop_mode,
        LoopMode::TransitionTo(SequenceKind::Stand)
    ));

    // Idle1: facings=0 → engine facings=1 (non-directional).
    let idle1: &SequenceDef = set.get(&SequenceKind::Idle1).expect("Should have Idle1");
    assert_eq!(idle1.start_frame, 56);
    assert_eq!(idle1.frame_count, 15);
    assert_eq!(idle1.facings, 1);

    // Die1: start=134, holds last frame.
    let die1: &SequenceDef = set.get(&SequenceKind::Die1).expect("Should have Die1");
    assert_eq!(die1.start_frame, 134);
    assert_eq!(die1.frame_count, 15);
    assert!(matches!(die1.loop_mode, LoopMode::HoldLast));

    // Down: transition to Prone.
    let down: &SequenceDef = set.get(&SequenceKind::Down).expect("Should have Down");
    assert_eq!(down.start_frame, 260);
    assert!(matches!(
        down.loop_mode,
        LoopMode::TransitionTo(SequenceKind::Prone)
    ));

    // Verify total count: Ready+Guard map to Stand (deduped), so 16 INI keys → 15 SequenceKinds.
    assert!(
        set.len() >= 14,
        "Should have at least 14 sequences, got {}",
        set.len()
    );
}

#[test]
fn test_build_sequence_set_facings_zero_becomes_one() {
    let ini_text: &str = "\
[TestSequence]
Idle1=56,15,0,S
";
    let ini: IniFile = IniFile::from_str(ini_text);
    let registry: InfantrySequenceRegistry = parse_infantry_sequence_registry(&ini);
    let def: &InfantrySequenceDef = registry.get("TESTSEQUENCE").expect("def");
    let set: SequenceSet = build_sequence_set(def);

    let idle: &SequenceDef = set.get(&SequenceKind::Idle1).expect("Idle1");
    assert_eq!(
        idle.facings, 1,
        "facings=0 in INI should become 1 in engine"
    );
}

#[test]
fn ready_guard_precedence_is_stable_across_insertion_order() {
    let ready = InfantrySequenceEntry {
        start_frame: 7,
        frames_per_facing: 1,
        facings: 1,
        facing_hint: None,
    };
    let guard = InfantrySequenceEntry {
        start_frame: 91,
        frames_per_facing: 2,
        facings: 2,
        facing_hint: None,
    };

    let forward = InfantrySequenceDef {
        entries: std::collections::HashMap::from([
            ("READY".to_string(), ready.clone()),
            ("GUARD".to_string(), guard.clone()),
        ]),
    };
    let reverse = InfantrySequenceDef {
        entries: std::collections::HashMap::from([
            ("GUARD".to_string(), guard),
            ("READY".to_string(), ready),
        ]),
    };

    let first = build_sequence_set(&forward);
    let second = build_sequence_set(&reverse);
    assert_eq!(first, second);
    assert_eq!(
        first.get(&SequenceKind::Stand).expect("stand").start_frame,
        7,
        "READY has explicit precedence over its GUARD alias",
    );
}

#[test]
fn gsi_05_07_sequence_build_preserves_completion_facing_and_native_none() {
    let ini = IniFile::from_str("[TestSequence]\nIdle1=56,15,0,S\nIdle2=71,15,0,E\nReady=0,1,1\n");
    let registry = parse_infantry_sequence_registry(&ini);
    let set = build_sequence_set(registry.get("TESTSEQUENCE").expect("sequence"));

    assert_eq!(
        set.get(&SequenceKind::Idle1)
            .expect("Idle1")
            .completion_facing,
        Some(128)
    );
    assert_eq!(
        set.get(&SequenceKind::Idle2)
            .expect("Idle2")
            .completion_facing,
        Some(64)
    );
    assert_eq!(
        set.get(&SequenceKind::Stand)
            .expect("Stand")
            .completion_facing,
        None
    );
}

#[test]
fn test_multiple_sequences_in_registry() {
    let ini_text: &str = "\
[ConSequence]
Ready=0,1,1
Walk=8,6,6

[BruteSequence]
Ready=0,1,1
Walk=8,6,6
FireUp=116,10,10
";
    let ini: IniFile = IniFile::from_str(ini_text);
    let registry: InfantrySequenceRegistry = parse_infantry_sequence_registry(&ini);

    assert_eq!(registry.len(), 2);
    assert!(registry.contains_key("CONSEQUENCE"));
    assert!(registry.contains_key("BRUTESEQUENCE"));

    // BruteSequence FireUp has 10 facings (unusual).
    let brute: &InfantrySequenceDef = registry.get("BRUTESEQUENCE").expect("brute");
    let fire: &InfantrySequenceEntry = brute.entries.get("FIREUP").expect("fireup");
    assert_eq!(fire.facings, 10);
}

#[test]
fn test_non_sequence_sections_ignored() {
    let ini_text: &str = "\
[CONS]
Cameo=E2ICON
Voxel=yes

[ConSequence]
Walk=8,6,6
";
    let ini: IniFile = IniFile::from_str(ini_text);
    let registry: InfantrySequenceRegistry = parse_infantry_sequence_registry(&ini);

    // Only ConSequence should be parsed, not [CONS].
    assert_eq!(registry.len(), 1);
    assert!(registry.contains_key("CONSEQUENCE"));
}

/// Every action id, against the engine's own 42-slot sequence-name array.
///
/// Kept as one exhaustive table rather than a handful of spot checks: the ids
/// are only ever used to index the delay table, so a single wrong row is silent
/// until someone notices an animation running at the wrong speed in game.
#[test]
fn action_ids_match_the_native_sequence_name_table() {
    let expected: &[(SequenceKind, u8)] = &[
        (SequenceKind::Stand, 0),
        (SequenceKind::Prone, 2),
        (SequenceKind::Walk, 3),
        (SequenceKind::Attack, 4),
        (SequenceKind::Down, 5),
        (SequenceKind::Crawl, 6),
        (SequenceKind::Up, 7),
        (SequenceKind::FireProne, 8),
        (SequenceKind::Idle1, 9),
        (SequenceKind::Idle2, 10),
        (SequenceKind::Die1, 11),
        (SequenceKind::Die2, 12),
        (SequenceKind::Die3, 13),
        (SequenceKind::Die4, 14),
        (SequenceKind::Die5, 15),
        (SequenceKind::Tread, 16),
        (SequenceKind::Swim, 17),
        (SequenceKind::WetIdle1, 18),
        (SequenceKind::WetIdle2, 19),
        (SequenceKind::WetAttack, 22),
        (SequenceKind::Hover, 23),
        (SequenceKind::Fly, 24),
        (SequenceKind::FireFly, 26),
        (SequenceKind::Deploy, 27),
        (SequenceKind::Deployed, 28),
        (SequenceKind::DeployedFire, 29),
        (SequenceKind::DeployedIdle, 30),
        (SequenceKind::Undeploy, 31),
        (SequenceKind::Cheer, 32),
        (SequenceKind::Paradrop, 33),
        (SequenceKind::Panic, 37),
        (SequenceKind::SecondaryFire, 40),
        (SequenceKind::SecondaryProne, 41),
    ];
    for &(kind, id) in expected {
        assert_eq!(action_id(kind), id, "{kind:?} action id");
    }
}

/// The playback speeds those ids actually buy, for the rows that were wrong.
///
/// `SecondaryFire` is the one with teeth: the shot leaves the weapon on a named
/// frame of the sequence, so a delay of 3 instead of 1 tripled the time a Brute
/// needed to land each building smash.
#[test]
fn corrected_action_ids_give_the_native_frame_delays() {
    let expected: &[(SequenceKind, u16, bool)] = &[
        (SequenceKind::SecondaryFire, 1, false),
        (SequenceKind::SecondaryProne, 1, false),
        (SequenceKind::WetAttack, 1, false),
        (SequenceKind::WetIdle1, 3, true),
        (SequenceKind::WetIdle2, 3, true),
        (SequenceKind::Hover, 2, true),
        (SequenceKind::Fly, 1, false),
        (SequenceKind::FireFly, 1, false),
        (SequenceKind::Cheer, 3, true),
        (SequenceKind::Paradrop, 1, false),
        (SequenceKind::Panic, 4, false),
        // Unmoved rows, to catch a renumbering that overshoots.
        (SequenceKind::Walk, 3, false),
        (SequenceKind::Attack, 1, false),
        (SequenceKind::Idle1, 3, true),
        (SequenceKind::Idle2, 3, true),
        (SequenceKind::Prone, 6, false),
    ];
    for &(kind, delay, normalized) in expected {
        assert_eq!(action_timing(kind), (delay, normalized), "{kind:?} timing");
    }
}
