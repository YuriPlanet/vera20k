//! Tests of the global rules readers (`[General]`, `[AudioVisual]`, `[AI]`,
//! `[CombatDamage]` and the smaller blocks) and their retail values.

use super::*;

#[test]
fn retail_shell_slide_cues_come_from_audio_visual() {
    // RulesClass+0x19C GUIMoveOutSound (0x006694C7) and +0x1A0
    // GUIMoveInSound (0x00669509), read from [AudioVisual].
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.gui_move_out_sound.as_deref(), Some("MenuSlideOut"));
    assert_eq!(general.gui_move_in_sound.as_deref(), Some("MenuSlideIn"));
    // +0x750 ShellButtonSlideSound (0x0066AF8D) is authored empty, so it
    // keeps the constructor's -1, also after Battle's mode and map passes.
    assert_eq!(general.shell_button_slide_sound, None);
    if let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() {
        assert_eq!(battle.rules.general.shell_button_slide_sound, None);
    }
}

#[test]
fn retail_battle_hills_retains_empulse_warhead_from_rulesmd() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Hills.mmx")
    else {
        return;
    };
    assert_eq!(
        retail.rules.general.empulse_warhead.as_deref(),
        Some("EMPuls"),
        "RulesMD EMPulseWarhead survives Battle and map passes without an override"
    );
}

#[test]
fn empulse_warhead_pointer_keeps_native_empty_reads_and_clears_none() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    assert_eq!(
        RuleSet::from_ini(&IniFile::empty())
            .unwrap()
            .general
            .empulse_warhead,
        None,
        "native constructor initializes Rules+0xFA0 to null"
    );
    let root = IniFile::from_str(
        "[Warheads]\n0=EMPuls\n[EMPuls]\n\
             Verses=100%,90%,80%,70%,60%,50%,40%,30%,20%,10%,0%\n\
             [SpecialWeapons]\nEMPulseWarhead=EMPuls\n",
    );
    let mut layers = RulesLayerStack::new(root);
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[SpecialWeapons]\nEMPulseWarhead=\n"),
    );
    layers.push(RulesLayerKind::Scenario, IniFile::from_str("[General]\n"));
    let retained = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    assert_eq!(retained.general.empulse_warhead.as_deref(), Some("EMPuls"));

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[SpecialWeapons]\nEMPulseWarhead=none\n"),
    );
    let cleared = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    assert_eq!(cleared.general.empulse_warhead, None);
}

#[test]
fn retail_production_click_keys_come_from_rulesmd() {
    // Rules+0xF0 MaximumQueuedObjects ([General], 0x00671DA7), +0x18C
    // GUIBuildSound, +0x6EC BuildingSlam and +0x700 ScoldSound
    // ([AudioVisual], 0x006693C1, 0x0066AAA7 and 0x0066ABE8).
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let general = &retail.rules.general;
    assert_eq!(general.maximum_queued_objects, 29);
    assert_eq!(general.gui_build_sound.as_deref(), Some("MenuClick"));
    assert_eq!(general.building_slam.as_deref(), Some("PlaceBuilding"));
    assert_eq!(
        retail.rules.general.scold_sound.as_deref(),
        Some("MenuScold")
    );
}

/// RulesClass constructor @ 0x00667588 stores 0x280 leptons; ReadGeneral
/// @ 0x00670EDD calls ReadRange @ 0x00474620 for [General] CloseEnough.
/// This pins the Rust reader against the absent key and physical retail
/// input; the arithmetic claim is still instruction-level native evidence.
#[test]
fn close_enough_keeps_constructor_default_and_reads_retail_range() {
    let absent = RuleSet::from_ini(&IniFile::from_str("[General]\n"))
        .expect("rules with absent CloseEnough parse");
    assert_eq!(absent.general.close_enough, 0x280);

    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    assert!(
        ini.section("General")
            .and_then(|section| section.get_for_test("CloseEnough"))
            .is_some(),
        "retail General CloseEnough key is present"
    );
    let retail = RuleSet::from_ini(&ini).expect("retail rules parse");
    assert_eq!(retail.general.close_enough, 576);
}

/// `[CombatDamage] PsychicRevealRadius=` (ReadInteger at `0x0066C665`)
/// and `[AudioVisual] AllyReveal=` (ReadBool at `0x0066B318`) keep the
/// constructor's 3 and 1 (`0x00666BCA`, `0x0066773F`) when absent;
/// retail RULESMD.INI sets 15 and yes.
#[test]
fn psychic_reveal_radius_and_ally_reveal_read_over_constructor_defaults() {
    let absent = GeneralRules::from_ini(&IniFile::from_str("[General]\nFlightLevel=500\n"));
    assert_eq!(absent.psychic_reveal_radius, 3);
    assert!(absent.ally_reveal);
    let off = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nAllyReveal=no\n",
    ));
    assert!(!off.ally_reveal);
    // Both readers run whether or not [General] exists (`0x00668F36`,
    // `0x00668F56`), and so does the case's activate sound.
    let alone = RuleSet::from_ini_with_sounds_for_test(
        &IniFile::from_str(
            "[CombatDamage]\nPsychicRevealRadius=7\n[AudioVisual]\nAllyReveal=no\n\
             PsychicRevealActivateSound=PsychicRevealActivate\n",
        ),
        &["PsychicRevealActivate"],
    )
    .unwrap()
    .general;
    assert_eq!((alone.psychic_reveal_radius, alone.ally_reveal), (7, false));
    assert_eq!(
        alone.psychic_reveal_activate_sound.as_deref(),
        Some("PsychicRevealActivate")
    );
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.psychic_reveal_radius, 15);
    assert!(general.ally_reveal);
}

/// `[MultiplayerDialogSettings] AlliesAllowed=` (`0x0067215B`) and `[AI]
/// Paranoid=` (`0x00673D43`) keep the constructor's 0 and 1
/// (`0x006672D5`, `0x00667715`) when absent, and both readers run whether
/// or not [General] exists (`0x00668EC0`, `0x00668EC8`). The Battle mode
/// layers MPBATTLEMD's `AlliesAllowed=yes` over RULESMD's `no`;
/// FreeForAll's MPFREEFORALLMD says `no`.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn allies_allowed_and_paranoid_read_over_constructor_defaults() {
    let absent = GeneralRules::from_ini(&IniFile::from_str("[General]\nFlightLevel=500\n"));
    assert!(!absent.allies_allowed);
    assert!(absent.paranoid);
    let alone = GeneralRules::from_ini(&IniFile::from_str(
        "[MultiplayerDialogSettings]\nAlliesAllowed=yes\n[AI]\nParanoid=no\n",
    ));
    assert!(alone.allies_allowed);
    assert!(!alone.paranoid);
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let general = GeneralRules::from_ini(&ini);
    assert!(!general.allies_allowed);
    assert!(general.paranoid);
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(battle.rules.general.allies_allowed);
    assert!(battle.rules.general.paranoid);
    let free_for_all = crate::rules::retail_ini_fixture::retail_mode_rules_for_map(2, "Hills.mmx")
        .expect("retail FreeForAll rules beside the Battle ones");
    assert!(!free_for_all.rules.general.allies_allowed);
    assert!(free_for_all.rules.general.paranoid);
}

#[test]
fn cloak_global_defaults_and_native_minute_conversion_parse() {
    let defaults = GeneralRules::default();
    assert_eq!(defaults.cloaking_stages, 9);
    assert_eq!(defaults.cloak_delay_frames, 0);
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str("[General]\n")).cloak_delay_frames,
        defaults.cloak_delay_frames,
    );
    assert_eq!(defaults.cloak_sound, None);
    assert_eq!(defaults.leave_bio_reactor_sound, None);
    assert_eq!(defaults.enter_bio_reactor_sound, None);
    let parsed = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nCloakingStages=13\nCloakDelay=.031\n\
             [AudioVisual]\nCloakSound=NavalUnitEmerge\n",
    ));
    assert_eq!(parsed.cloaking_stages, 13);
    assert_eq!(parsed.cloak_delay_frames, 27);
    assert_eq!(parsed.cloak_sound.as_deref(), Some("NavalUnitEmerge"));
    assert_eq!(parsed.leave_bio_reactor_sound, None);
    assert_eq!(parsed.enter_bio_reactor_sound, None);
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str("[General]\nCloakingStages=9\n")).cloak_sound,
        None,
        "missing CloakSound preserves the native invalid-index default"
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nCloakingStages=9\n[AudioVisual]\nCloakSound=\n",
        ))
        .cloak_sound,
        None,
        "empty CloakSound preserves the native invalid-index default"
    );
}

#[test]
fn unit_delivery_activation_sound_binding_keeps_known_values_across_unknown_layers() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=DeliveryCue\n",
    ));
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[SuperWeaponTypes]\n0=Delivery\n[Delivery]\nType=UnitDelivery\nSW.ActivationSound=deliverycue",
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[Delivery]\nSW.ActivationSound=Unknown"),
    );
    let inspect = |layers: &RulesLayerStack| {
        let processed = layers.process_with_fixed_art(&IniFile::empty()).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        rules
            .super_weapon("Delivery")
            .unwrap()
            .unit_delivery
            .as_ref()
            .unwrap()
            .activation_sound
            .clone()
    };
    assert_eq!(inspect(&layers).as_deref(), Some("DeliveryCue"));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[Delivery]\nSW.ActivationSound=none"),
    );
    assert_eq!(inspect(&layers), None);
}

#[test]
fn bio_reactor_sounds_resolve_only_registered_vocs() {
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=BioReactorEnter\n1=BioReactorExit\n",
    ));
    let ini = IniFile::from_str(
        "[General]\n[AudioVisual]\nEnterBioReactorSound=BioReactorEnter\n\
             LeaveBioReactorSound=BioReactorExit\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("rules");
    assert_eq!(rules.general.leave_bio_reactor_sound, None);
    assert_eq!(rules.general.enter_bio_reactor_sound, None);
    rules.bind_type_sound_references(&ini, &sounds);
    assert_eq!(
        rules.general.leave_bio_reactor_sound.as_deref(),
        Some("BioReactorExit")
    );
    assert_eq!(
        rules.general.enter_bio_reactor_sound.as_deref(),
        Some("BioReactorEnter")
    );

    let unknown_ini = IniFile::from_str(
        "[General]\n[AudioVisual]\nEnterBioReactorSound=NotRegistered\n\
             LeaveBioReactorSound=NotRegistered\n",
    );
    let mut unknown_rules = RuleSet::from_ini(&unknown_ini).expect("rules");
    unknown_rules.bind_type_sound_references(&unknown_ini, &sounds);
    assert_eq!(
        unknown_rules.general.leave_bio_reactor_sound, None,
        "native Voc lookup leaves the constructor's invalid index on a miss"
    );
    assert_eq!(
        unknown_rules.general.enter_bio_reactor_sound, None,
        "native Voc lookup leaves the constructor's invalid index on a miss"
    );
}

#[test]
fn bio_reactor_entry_sound_uses_the_effective_processed_rules_pass() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=BioReactorEnter\n1=BioReactorExit\n",
    ));
    let rulesmd = IniFile::from_str(
        "[General]\n[AudioVisual]\nEnterBioReactorSound=BioReactorEnter\n\
             LeaveBioReactorSound=BioReactorExit\n",
    );
    let mut layers = RulesLayerStack::new(rulesmd);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nCloakSound=NavalUnitEmerge\n"),
    );
    let processed = layers.process().expect("ordered Rules passes process");
    let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(
        rules.general.enter_bio_reactor_sound.as_deref(),
        Some("BioReactorEnter"),
        "a later pass without the key leaves the earlier effective value"
    );
    assert_eq!(
        rules.general.leave_bio_reactor_sound.as_deref(),
        Some("BioReactorExit"),
        "a later pass without the key leaves the earlier effective value"
    );

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[AudioVisual]\nEnterBioReactorSound=BioReactorExit\n\
                 LeaveBioReactorSound=BioReactorEnter\n",
        ),
    );
    let processed = layers.process().expect("later Rules pass processes");
    let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(
        rules.general.enter_bio_reactor_sound.as_deref(),
        Some("BioReactorExit"),
        "the effective later Rules pass overrides the earlier key"
    );
    assert_eq!(
        rules.general.leave_bio_reactor_sound.as_deref(),
        Some("BioReactorEnter"),
        "the effective later Rules pass overrides the earlier key"
    );

    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str(
            "[AudioVisual]\nEnterBioReactorSound=NotRegistered\n\
                 LeaveBioReactorSound=NotRegistered\n",
        ),
    );
    let processed = layers.process().expect("invalid later pass processes");
    let mut rules = RuleSet::from_processed_rules(&processed).expect("projected Rules");
    rules.bind_type_sound_references(processed.ini(), &sounds);
    assert_eq!(
        rules.general.enter_bio_reactor_sound.as_deref(),
        Some("BioReactorExit"),
        "an unregistered later entry sound retains its earlier valid Voc"
    );
    assert_eq!(
        rules.general.leave_bio_reactor_sound.as_deref(),
        Some("BioReactorEnter"),
        "an unregistered later leave sound retains its earlier valid Voc"
    );
}

#[test]
fn gsi_13_10_extra_object_lights_default_zero_and_parse_signed_truncated_milliunits() {
    let defaults = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
             FixtureOnly=1\n\
             ExtraUnitLight=9.0\n\
             ExtraInfantryLight=8.0\n\
             ExtraAircraftLight=7.0\n",
    ))
    .unwrap()
    .general;
    assert_eq!(defaults.extra_unit_light, 0);
    assert_eq!(defaults.extra_infantry_light, 0);
    assert_eq!(defaults.extra_aircraft_light, 0);

    let parsed = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
             FixtureOnly=1\n\
             ExtraUnitLight=9.0\n\
             ExtraInfantryLight=8.0\n\
             ExtraAircraftLight=7.0\n\
             [AudioVisual]\n\
             ExtraUnitLight=.2\n\
             ExtraInfantryLight=-.1259\n\
             ExtraAircraftLight=.3339\n",
    ))
    .unwrap()
    .general;
    assert_eq!(parsed.extra_unit_light, 200);
    assert_eq!(parsed.extra_infantry_light, -125);
    assert_eq!(parsed.extra_aircraft_light, 333);

    let without_general = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nExtraUnitLight=.2\nExtraInfantryLight=-.1259\nExtraAircraftLight=.3339\n",
    ))
    .unwrap()
    .general;
    assert_eq!(without_general.extra_unit_light, parsed.extra_unit_light);
    assert_eq!(
        without_general.extra_infantry_light,
        parsed.extra_infantry_light
    );
    assert_eq!(
        without_general.extra_aircraft_light,
        parsed.extra_aircraft_light
    );
}

#[test]
fn gsi_04_04_cliff_back_rule_stores_the_ini_integer_low_byte() {
    for (raw, expected) in [(2, 2), (258, 2), (-1, 255), (256, 0)] {
        let ini = IniFile::from_str(&format!("[General]\nCliffBackImpassability={raw}\n"));
        assert_eq!(
            GeneralRules::from_ini(&ini).cliff_back_impassability,
            expected,
            "raw INI integer {raw}"
        );
    }
}

/// The RulesClass constructor (`0x00665650`) stores 0 at `Rules+0x664`
/// (`0x00665F3B`), and RulesClass::ReadGeneral (`0x0066D530`) hands ReadInt
/// that byte as its default (`0x0066F1CB..0x0066F1E6`). Retail RULESMD
/// sets 2, which the overlay registry reads from the processed rules too.
#[test]
fn cliff_back_rule_without_the_key_keeps_the_constructors_zero() {
    assert_eq!(GeneralRules::default().cliff_back_impassability, 0);
    let other_key = IniFile::from_str("[General]\nTreeStrength=200\n");
    assert_eq!(
        GeneralRules::from_ini(&other_key).cliff_back_impassability,
        0
    );
    assert_eq!(read_cliff_back_impassability(None), 0);
    let Some(retail) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(retail.general.cliff_back_impassability, 2);
    assert!(retail.overlay_registry().cliff_back_rock().is_some());
}

#[test]
fn gsi_04_05_base_defense_response_rules_preserve_native_defaults_and_ini_values() {
    let defaults = GeneralRules::default();
    assert_eq!(defaults.computer_base_defense_response, 3);
    assert_eq!(defaults.base_defense_delay_minutes, 0.25);
    assert_eq!(defaults.suspend_priority, 20);
    assert_eq!(defaults.suspend_delay_minutes, 2.0);

    let parsed = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\n\
             BaseDefenseDelay=.125\n\
             SuspendPriority=-2\n\
             SuspendDelay=1.5\n\
             [AI]\nComputerBaseDefenseResponse=-4\n",
    ));
    assert_eq!(parsed.computer_base_defense_response, -4);
    assert_eq!(parsed.base_defense_delay_minutes, 0.125_f32 as f64);
    assert_eq!(parsed.suspend_priority, -2);
    assert_eq!(parsed.suspend_delay_minutes, 1.5_f32 as f64);
}

/// Original Rules ReadAI673E41..673E61 uses the AI section pointer;
/// a later General-only pass cannot overwrite the retained AI value.
/// Executable controls are saved by foot_navigation_coordinate --base-response.
#[test]
fn native_base_response_reads_ai_section_across_rules_passes() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[General]\nComputerBaseDefenseResponse=17\n\
             [AI]\nComputerBaseDefenseResponse=5\n",
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nComputerBaseDefenseResponse=19\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules.general.computer_base_defense_response, 5);
}

/// Original TechnoType ReadINI71464A..71469F retains the current speed
/// when a later rules pass reads -1. The constructor starts it at zero.
/// Executed reader history: spatial_oracle/base_defense_response.json.
#[test]
fn native_base_response_type_speed_retains_minus_one_across_rules_passes() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=V\n[V]\nStrength=100\nSpeed=7\n",
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[V]\nSpeed=-1\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(rules.object("V").unwrap().speed, 7);
    assert_eq!(
        crate::util::fixed_math::ra2_speed_to_leptons_per_frame(rules.object("V").unwrap().speed,),
        17,
    );
}

#[test]
fn gsi_04_05_base_plan_rules_preserve_signed_retry_limit_and_building_identity() {
    assert_eq!(
        GeneralRules::default().maximum_building_placement_failures,
        5
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nMaximumBuildingPlacementFailures=-4\n\
             [BuildingTypes]\n0=GAPOWR\n1=GACNST\n\
             [GAPOWR]\nStrength=750\nIsBaseDefense=yes\n\
             [GACNST]\nStrength=1000\nUndeploysInto=AMCV\n",
    ))
    .expect("base-plan rules");

    assert_eq!(rules.general.maximum_building_placement_failures, -4);
    assert_eq!(rules.building_type_index("gapowr"), Some(0));
    assert_eq!(rules.building_type_index("GACNST"), Some(1));
    let defense = rules
        .object_in_category(ObjectCategory::Building, "GAPOWR")
        .unwrap();
    assert_eq!(defense.base_plan_type_index, 0);
    assert!(defense.is_base_defense);
    assert!(
        rules
            .object_in_category(ObjectCategory::Building, "GACNST")
            .unwrap()
            .undeploys_into
            .is_some()
    );
}

#[test]
fn parse_multiplayer_ai_cm_list_in_source_order() {
    // rulesmd.ini:88 `MultiplayerAICM=400,0,0` (Hard, Normal, Easy).
    let ini = IniFile::from_str("[General]\nMultiplayerAICM=400,0,0\n");
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.multiplayer_ai_cm, vec![400, 0, 0]);

    // RulesClass constructor leaves the +0x1304 vector empty (0x00667034).
    let ini = IniFile::from_str("[General]\n");
    let general = GeneralRules::from_ini(&ini);
    assert!(general.multiplayer_ai_cm.is_empty());
}

#[test]
fn ai_hate_delays_retain_layers_and_saved_rules_lifetime() {
    use crate::rules::native_processing::RulesLayerKind;
    let Some(root) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let mut layers = RulesLayerStack::new(root);
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .ai_hate_delays(),
        [30, 50, 70]
    );
    layers.push(
        RulesLayerKind::LangRule,
        IniFile::from_str("[General]\nAIHateDelays=31,51,71\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .ai_hate_delays(),
        [31, 51, 71]
    );
    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[General]\nAIHateDelays=32,52,72\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .general
            .ai_hate_delays(),
        [32, 52, 72]
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[General]\nAIHateDelays=-7junk,0,15,99\n"),
    );
    let processed = layers.process().unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .general
            .ai_hate_delays(),
        [-7, 0, 15, 99]
    );
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let retained = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let bytes = crate::util::save_codec::serialize(&retained).unwrap();
    let retained = crate::util::save_codec::deserialize(&bytes).unwrap();
    let missing = IniFile::from_str("[General]\naihatedelays=1,2,3\nAIHateDelays=\n");
    let processed = RulesLayerStack::new(missing.clone())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
        .unwrap();
    let retained = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(retained.general.ai_hate_delays(), [-7, 0, 15, 99]);
    let fresh = RuleSet::from_ini(&missing).unwrap();
    assert!(fresh.general.ai_hate_delays().is_empty());
    assert_ne!(
        retained.simulation_config_hash(),
        fresh.simulation_config_hash()
    );
}

#[test]
fn parse_tier1_superweapon_rules() {
    let ini_text = "[General]\n\
ForceShieldRadius=5\n\
ForceShieldDuration=600\n\
MutateExplosion=no\n\
[CombatDamage]\n\
IronCurtainDuration=900\n\
PsychicRevealRadius=12\n\
TreeTargeting=yes\n\
[SpecialWeapons]\n\
MutateWarhead=MyMutate\n\
";
    let ini = IniFile::from_str(ini_text);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.iron_curtain_duration, 900);
    assert_eq!(general.force_shield_radius, 5);
    assert_eq!(general.force_shield_duration, 600);
    assert_eq!(general.psychic_reveal_radius, 12);
    assert_eq!(general.mutate_warhead, "MyMutate");
    assert!(general.tree_targeting);
    assert!(!general.mutate_explosion);
    // Unspecified keys fall back to defaults: the constructor's null type
    // (`Rules+0x348`, `0x00665B1A`).
    assert_eq!(general.iron_curtain_invoke_anim, "");
    // The constructor's null type (`Rules+0x34C`, `0x00665B20`).
    assert_eq!(general.force_shield_invoke_anim, "");
    assert_eq!(general.force_shield_blackout_duration, 800);
    assert_eq!(general.force_shield_fade_sound_time, 50);
    // The constructor's null warhead (`Rules+0xF9C`, `0x00666B40`).
    assert_eq!(general.mutate_explosion_warhead, "");
}

#[test]
fn gsi_04_20_ambient_transition_rules_use_native_scales_and_nonzero_gate() {
    let defaults = GeneralRules::default();
    assert!(defaults.ambient_change_rate_nonzero);
    assert_eq!(defaults.ambient_change_interval_frames, 180);
    assert_eq!(defaults.ambient_change_step, 10);

    let stock = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nAmbientChangeRate=.2\nAmbientChangeStep=.2\n",
    ))
    .unwrap()
    .general;
    assert!(stock.ambient_change_rate_nonzero);
    assert_eq!(stock.ambient_change_interval_frames, 180);
    assert_eq!(stock.ambient_change_step, 20);

    let tiny = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nAmbientChangeRate=.0009\nAmbientChangeStep=.019\n",
    ))
    .unwrap()
    .general;
    assert!(tiny.ambient_change_rate_nonzero);
    assert_eq!(tiny.ambient_change_interval_frames, 0);
    assert_eq!(tiny.ambient_change_step, 1);

    let disabled = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nAmbientChangeRate=0\nAmbientChangeStep=.2\n",
    ))
    .unwrap()
    .general;
    assert!(!disabled.ambient_change_rate_nonzero);
    assert_eq!(disabled.ambient_change_interval_frames, 0);
}

#[test]
fn parse_rules_rocking_coefficients_defaults() {
    // [General] must be present, otherwise GeneralRules::from_ini bails to
    // Self::default(). Missing AudioVisual keys then fall back to defaults.
    let ini = IniFile::from_str("[General]\nFixtureOnly=1\n[AudioVisual]\nFixtureOnly=1\n");
    let r = GeneralRules::from_ini(&ini);
    assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("1.5"));
    assert_eq!(r.fallback_coefficient, SimFixed::lit("0.1"));
}

#[test]
fn parse_spark_gravity_preserves_signed_integer_storage() {
    assert_eq!(GeneralRules::default().gravity, 3);
    let gravity = |text: &str| {
        RuleSet::from_ini(&IniFile::from_str(text))
            .unwrap()
            .general
            .gravity
    };
    // Gravity lives in [AudioVisual] (stock rulesmd.ini), NOT [General].
    assert_eq!(
        gravity("[General]\nFlightLevel=500\n[AudioVisual]\nGravity=6\n"),
        6
    );
    assert_eq!(
        gravity("[General]\nFlightLevel=500\n[AudioVisual]\nGravity=-7\n"),
        -7
    );
    // A [General] Gravity is ignored (the engine reads it in ReadAudioVisual).
    assert_eq!(gravity("[General]\nGravity=9\n"), 3);
}

#[test]
fn item82_scroll_multiplier_parses_from_audio_visual_with_stock_default() {
    assert_eq!(GeneralRules::default().scroll_multiplier, 0.07);
    let parsed = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nScrollMultiplier=.125\n",
    ));
    assert_eq!(parsed.scroll_multiplier, 0.125);
    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert_eq!(absent.scroll_multiplier, 0.07);
}

#[test]
fn gsi_01_04_savour_delay_parses_from_audio_visual_with_native_default() {
    // The already-executed C4 scalar controls exercise the same double
    // *900/Math__ftol body and FPCW as FlagWin4FCA35/FlagLose4FCC61.
    // This is a bounded arithmetic reuse, not a full Flag-body replay.
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/read_double_percent.json",
    ))
    .unwrap();
    let compare_scalar = |minutes: f64, raw: Option<&str>| {
        let row = native["c4"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["raw"].as_str() == raw)
            .unwrap();
        assert_eq!(
            format!("{:016x}", minutes.to_bits()),
            row["bits"].as_str().unwrap()
        );
        assert_eq!(
            i64::from(native_minutes_to_frames(minutes)),
            row["fuse"].as_i64().unwrap()
        );
    };
    let ctor_default = GeneralRules::default().savour_delay_minutes;
    assert_eq!(ctor_default, 0.03);
    compare_scalar(ctor_default, None);
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.1\n",
    ));
    assert_eq!(stock.savour_delay_minutes, 0.1_f32 as f64);
    compare_scalar(stock.savour_delay_minutes, Some("0.1"));
    let explicit_point_zero_three = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nSavourDelay=.03\n",
    ));
    assert_eq!(
        explicit_point_zero_three.savour_delay_minutes,
        0.03_f32 as f64
    );
    compare_scalar(explicit_point_zero_three.savour_delay_minutes, Some(".03"));
    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert_eq!(absent.savour_delay_minutes, 0.03);
    compare_scalar(absent.savour_delay_minutes, None);
}

#[test]
fn retail_savour_delay_uses_production_audio_visual_reader() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let parsed = GeneralRules::from_ini(&ini);
    assert_eq!(
        parsed.savour_delay_minutes.to_bits(),
        (0.1_f32 as f64).to_bits()
    );
    assert_eq!(native_minutes_to_frames(parsed.savour_delay_minutes), 90);
}

#[test]
fn shroud_growth_controls_retain_native_defaults_through_process_reset_and_save() {
    assert!(!GeneralRules::default().shroud_grow);
    assert_eq!(GeneralRules::default().shroud_rate, default_shroud_rate());
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[AudioVisual]\nShroudGrow=yes\nShroudRate=.5\n",
    ));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nshroudgrow=no\n"),
    );
    let processed = layers.process().unwrap();
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let retained = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let saved = crate::util::save_codec::serialize(&retained).unwrap();
    let retained = crate::util::save_codec::deserialize(&saved).unwrap();
    let after_reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
        .unwrap();
    let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
    assert!(rules.general.shroud_grow);
    assert_eq!(rules.general.shroud_rate.bits(), (0.5_f32 as f64).to_bits());
    assert_eq!(rules.general.shroud_growth_interval_frames(), 450);
    if let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() {
        assert!(!retail.rules.general.shroud_grow);
        assert_eq!(retail.rules.general.shroud_rate, default_shroud_rate());
        assert_eq!(retail.rules.general.shroud_growth_interval_frames(), 3600);
    }
}

#[test]
fn fog_configuration_preserves_native_read_defaults_and_campaign_reset() {
    use crate::map::basic::parse_special_flags_section;

    assert_eq!(GeneralRules::default().fog_rate, default_fog_rate());
    let mut layers = RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nFogRate=.05\n"));
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[AudioVisual]\nfograte=invalid\n"),
    );
    let processed = layers.process().unwrap();
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    let retained = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let after_reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), retained)
        .unwrap();
    let rules = RuleSet::from_processed_rules(&after_reset).unwrap();
    assert_eq!(rules.general.fog_rate.bits(), (0.05_f32 as f64).to_bits());

    let prior = parse_special_flags_section(&IniFile::from_str("[SpecialFlags]\nFogOfWar=yes\n"));
    assert_eq!(prior.fog_of_war, Some(true));
    let mut fresh = parse_special_flags_section(&IniFile::empty());
    fresh.resolve_campaign_defaults(Some(&prior));
    assert_eq!(fresh.fog_of_war, None);
    let mut authored = prior.clone();
    authored.resolve_campaign_defaults(Some(&fresh));
    assert_eq!(authored.fog_of_war, Some(true));

    if let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() {
        assert_eq!(
            retail.rules.general.fog_rate.bits(),
            (0.01_f32 as f64).to_bits(),
        );
        assert_eq!(retail.rules.general.fog_regrowth_interval_frames(), 8);
    }
}

#[test]
fn parse_rules_rocking_coefficients_explicit() {
    let ini = IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nDirectRockingCoefficient=2.0\nFallBackCoefficient=0.05\n",
    );
    let r = GeneralRules::from_ini(&ini);
    assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("2"));
    assert_eq!(r.fallback_coefficient, SimFixed::lit("0.05"));
}

#[test]
fn parse_retail_rules_rocking_coefficients() {
    let ini =
        IniFile::from_str("[AudioVisual]\nDirectRockingCoefficient=1.5\nFallBackCoefficient=0.1\n");
    let r = GeneralRules::from_ini(&ini);
    assert_eq!(r.direct_rocking_coefficient, SimFixed::lit("1.5"));
    assert_eq!(r.fallback_coefficient, SimFixed::lit("0.1"));
}

#[test]
fn bridge_animation_vectors_have_empty_constructor_defaults() {
    assert!(GeneralRules::default().metallic_debris.is_empty());
    assert!(BridgeRules::default().explosions.is_empty());
    let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
    assert!(rules.general.metallic_debris.is_empty());
    assert!(rules.bridge_rules.explosions.is_empty());
}

#[test]
fn metallic_debris_parses_from_ini() {
    let ini = IniFile::from_str("[General]\nMetallicDebris=ANIM1,ANIM2,ANIM3\n");
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(
        rules.general.metallic_debris,
        vec!["ANIM1", "ANIM2", "ANIM3"]
    );
}

#[test]
fn resolved_bridge_animation_order_and_duplicates_affect_simulation_hash() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        // X and Y are registered for Ares's Find-only BridgeExplosions.
        "[Animations]\n0=X\n1=Y\n[General]\nMetallicDebris=A,B,A\nBridgeExplosions=X,Y\n",
    ))
    .unwrap();
    let original = rules.simulation_config_hash();
    rules.general.metallic_debris.swap(0, 1);
    assert_ne!(original, rules.simulation_config_hash());
    rules.general.metallic_debris.swap(0, 1);
    rules.general.metallic_debris.pop();
    assert_ne!(original, rules.simulation_config_hash());
    rules.general.metallic_debris.push("A".to_owned());
    assert_eq!(original, rules.simulation_config_hash());
    rules.bridge_rules.explosions.reverse();
    assert_ne!(original, rules.simulation_config_hash());
}

#[test]
fn bridge_rules_load_from_ini() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             BridgeVoxelMax=5\n\
             [AudioVisual]\n\
             RepairBridgeSound=foo\n\
             [CombatDamage]\n\
             BridgeStrength=900\n\
             DestroyableBridges=no\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    assert_eq!(rules.bridge_rules.strength, 900);
    assert!(rules.bridge_rules.destroyable_by_default);
    assert_eq!(rules.bridge_rules.voxel_max, 5);
    assert_eq!(rules.bridge_rules.repair_sound.as_deref(), Some("FOO"));
}

#[test]
fn bridge_rules_voxel_max_clamps_oversize() {
    // Regression: u8 storage clamps oversize INI values to 255 instead
    // of wrapping/truncating.
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             BridgeVoxelMax=999\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    assert_eq!(rules.bridge_rules.voxel_max, 255);
}

#[test]
fn bridge_rules_destroyable_in_specialflags_is_ignored() {
    // Map `[SpecialFlags]` is parsed with map data, not rules.ini.
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [SpecialFlags]\n\
             DestroyableBridges=no\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("Should parse");
    assert!(rules.bridge_rules.destroyable_by_default);
}

/// `RulesClass::ReadGeneral @ 0x0066D530` reads the four rank multipliers
/// as doubles (`+0x670`, `+0x678`, `+0x680`, `+0x690`) and `RepairRate`
/// (`+0x16E0`); `ReadAudioVisual @ 0x006691E0` resolves the two upgrade
/// sounds and `EliteFlashTimer` (`+0xBE8`).
#[test]
fn gsi_08_12_veterancy_multipliers_and_promotion_cues_parse() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nVeteranCombat=1.1\nVeteranSpeed=1.2\nVeteranSight=0.0\nVeteranROF=0.6\nRepairRate=.016\n\
             [AudioVisual]\nUpgradeVeteranSound=UpgradeVeteran\nUpgradeEliteSound=UpgradeElite\nEliteFlashTimer=150\n",
        ))
        .unwrap();
    // `ReadDouble` is a `%f` single widened to a double, so the stored
    // multipliers are the f32-widened values, not the decimal literals.
    assert_eq!(rules.general.veteran_combat, f64::from(1.1f32));
    assert_eq!(rules.general.veteran_speed, f64::from(1.2f32));
    assert_eq!(rules.general.veteran_sight, 0.0);
    assert_eq!(rules.general.veteran_rof, f64::from(0.6f32));
    assert_eq!(rules.general.repair_rate_minutes, f64::from(0.016f32));
    // The widened singles sit clear of every integer boundary the stock
    // consumers reach: the products below truncate the same way at any
    // x87 precision.
    use crate::sim::combat::veterancy::{ftol_scale, self_heal_interval_frames};
    assert_eq!(ftol_scale(50, rules.general.veteran_rof), 30);
    assert_eq!(ftol_scale(51, rules.general.veteran_rof), 30);
    assert_eq!(ftol_scale(52, rules.general.veteran_rof), 31);
    assert_eq!(ftol_scale(65, rules.general.veteran_combat), 71);
    assert_eq!(ftol_scale(15, rules.general.veteran_speed), 18);
    assert_eq!(
        self_heal_interval_frames(rules.general.repair_rate_minutes),
        14
    );
    assert_eq!(rules.general.elite_flash_timer, 150);
    let bare = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[General]\nFixtureOnly=1\n",
        ))
        .unwrap();
    assert_eq!(bare.general.veteran_rof, 1.0);
    assert_eq!(bare.general.upgrade_veteran_sound, None);
    // Rules6656A7..6656B0 supplies the retained ReadInt defaults.
    assert_eq!(bare.general.self_heal_infantry_frames, 1000);
    assert_eq!(bare.general.self_heal_infantry_amount, 1);
    assert_eq!(bare.general.self_heal_unit_frames, 1000);
    assert_eq!(bare.general.self_heal_unit_amount, 1);
}

/// Original constructor6656A7..6656B0 and ReadGeneral66E6E2..66E74B.
/// The retained corpus distinguishes physical empty-value omission from
/// an explicitly prepared stored-empty native cache entry.
#[test]
fn house_self_heal_defaults_and_retained_readers_match_native_execution() {
    use crate::rules::native_processing::RulesLayerKind;

    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/self_heal_status.json");
    let controls = &native["reader_controls"];
    let state = |general: &GeneralRules| {
        serde_json::json!({
            "SelfHealInfantryFrames": general.self_heal_infantry_frames,
            "SelfHealInfantryAmount": general.self_heal_infantry_amount,
            "SelfHealUnitFrames": general.self_heal_unit_frames,
            "SelfHealUnitAmount": general.self_heal_unit_amount,
        })
    };
    assert_eq!(state(&GeneralRules::default()), controls["constructor"]);
    let mut layers = RulesLayerStack::new(IniFile::empty());
    let mut rules = RuleSet::from_rules_layers(&layers).unwrap();
    assert_eq!(state(&rules.general), controls["constructor"]);
    let rows = controls["retained_history"].as_array().unwrap();
    assert_eq!(rows.len(), 11);
    for row in rows {
        assert_eq!(state(&rules.general), row["before"], "{}", row["name"]);
        let mut pass = IniFile::from_str(row["input_text"].as_str().unwrap());
        if row["raw_cached_values"] == true {
            // This row supplies a native cache, rather than a physical
            // file. Use the existing cache API; do not reinterpret empty
            // authored values that the normal lexical loader omits.
            for (name, keys) in row["sections"].as_object().unwrap() {
                let mut section = IniSection::new(name.clone());
                for (key, value) in keys.as_object().unwrap() {
                    section.set(key, value.as_str().unwrap());
                }
                pass.replace_first_section(section);
            }
        }
        layers.push(RulesLayerKind::Scenario, pass);
        rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(state(&rules.general), row["after"], "{}", row["name"]);
    }
}

#[test]
fn starting_force_initial_veteran_reads_only_specialflags() {
    let general_only = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nInitialVeteran=yes\n",
    );
    assert!(!RuleSet::from_ini(&general_only).unwrap().initial_veteran);

    let special_flags = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [General]\nInitialVeteran=no\n\
             [SpecialFlags]\nInitialVeteran=yes\n",
    );
    assert!(RuleSet::from_ini(&special_flags).unwrap().initial_veteran);
}

#[test]
fn test_building_garrisoned_sound_parsed() {
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
BuildingGarrisonedSound=BuildingGarrisoned
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(
        general.building_garrisoned_sound.as_deref(),
        Some("BuildingGarrisoned")
    );
}

/// `[AudioVisual] BaseUnderAttackSound=` — the siren
/// `HouseClass::NotifyUnderAttack` plays at `0x004F95CF` next to the EVA
/// line.
#[test]
fn base_under_attack_sound_parses_and_an_absent_key_is_silence() {
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nBaseUnderAttackSound=BaseUnderAttackSiren\n",
    ));
    assert_eq!(
        stock.base_under_attack_sound.as_deref(),
        Some("BaseUnderAttackSiren")
    );

    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert_eq!(absent.base_under_attack_sound, None);
}

/// `[AudioVisual] BuildingDieSound=` — the global building crumble cue
/// `BuildingClass::DestructionEffects` plays at `0x00441779` when the
/// dying building's type has no `DieSound=` of its own.
#[test]
fn building_die_sound_parses_and_an_absent_key_is_silence() {
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDieSound=BuildingGenericDie\n",
    ));
    assert_eq!(
        stock.building_die_sound.as_deref(),
        Some("BuildingGenericDie")
    );

    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert_eq!(absent.building_die_sound, None);

    let empty = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDieSound=\n",
    ));
    assert_eq!(empty.building_die_sound, None);
}

/// `[AudioVisual] BuildingDamageSound=` — the global struck-building cue
/// `BuildingClass::ReceiveDamage` plays at `0x00442706` when the damaged
/// building's type has no `DamageSound=` of its own
/// (`0x004426D2 CMP [type+0x538],-1`).
#[test]
fn building_damage_sound_remains_unbound_without_the_fixed_catalog() {
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=BuildingDamaged\n",
    ));
    assert_eq!(stock.building_damage_sound(), None);

    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert_eq!(absent.building_damage_sound(), None);

    let empty = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nBuildingDamageSound=\n",
    ));
    assert_eq!(empty.building_damage_sound(), None);
}

/// `[AudioVisual] LightningSounds=` goes through `CCINIClass::ReadSoundList
/// @ 0x00525430`, which tokenises on `","` (`0x00817F70`) after one
/// `ReadString` trim; empty fields collapse and an absent key leaves the
/// vector empty, which is the `0x0053A46A JLE` "play nothing" branch.
#[test]
fn lightning_sounds_reads_the_native_comma_list() {
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=WeatherStrike\n",
    ));
    assert_eq!(stock.lightning_sounds, vec!["WeatherStrike".to_string()]);

    let multi = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=A,,B\n",
    ));
    assert_eq!(
        multi.lightning_sounds,
        vec!["A".to_string(), "B".to_string()],
        "strtok collapses the empty field between the two commas"
    );

    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\n",
    ));
    assert!(absent.lightning_sounds.is_empty());

    let empty = GeneralRules::from_ini(&IniFile::from_str(
        "[General]\nFlightLevel=500\n[AudioVisual]\nLightningSounds=\n",
    ));
    assert!(
        empty.lightning_sounds.is_empty(),
        "stock ships an empty IceCrackSounds= in the same shape"
    );
}

/// `SuperClass::Launch @ 0x006CC390` reads three `[AudioVisual]` cues
/// straight off `RulesClass` — `+0x24C` at `0x006CCE22`, `+0x250` at
/// `0x006CD8CD`, `+0x254` at `0x006CD7B9` — plus `DigSound` (`+0x174`,
/// `0x006CDCA8`), the nuke siren despite the name.
#[test]
fn superweapon_activate_sounds_parse_their_stock_values() {
    let stock = RuleSet::from_ini_with_sounds_for_test(
        &IniFile::from_str(
            "[General]
FlightLevel=500
[AudioVisual]
             PsychicDominatorActivateSound=PsychicDominatorActivate
             GeneticMutatorActivateSound=GeneticMutatorActivate
             PsychicRevealActivateSound=PsychicRevealActivate
             DigSound=NukeSiren		; HACK: Nuke siren here
             StormSound=WeatherIntro
",
        ),
        &[
            "PsychicDominatorActivate",
            "GeneticMutatorActivate",
            "PsychicRevealActivate",
            "NukeSiren",
            "WeatherIntro",
        ],
    )
    .unwrap()
    .general;
    assert_eq!(
        stock.psychic_dominator_activate_sound.as_deref(),
        Some("PsychicDominatorActivate")
    );
    assert_eq!(
        stock.genetic_mutator_activate_sound.as_deref(),
        Some("GeneticMutatorActivate")
    );
    assert_eq!(
        stock.psychic_reveal_activate_sound.as_deref(),
        Some("PsychicRevealActivate")
    );
    assert_eq!(
        stock.dig_sound.as_deref(),
        Some("NukeSiren"),
        "the inline comment is cut before the value is trimmed"
    );
    assert_eq!(stock.storm_sound.as_deref(), Some("WeatherIntro"));

    let absent = GeneralRules::from_ini(&IniFile::from_str(
        "[General]
FlightLevel=500
[AudioVisual]
DigSound=
StormSound=
",
    ));
    assert_eq!(absent.psychic_dominator_activate_sound, None);
    assert_eq!(absent.genetic_mutator_activate_sound, None);
    assert_eq!(absent.psychic_reveal_activate_sound, None);
    assert_eq!(absent.dig_sound, None, "an empty value is silence");
    assert_eq!(absent.storm_sound, None);
}

/// The Spy Plane's camera keys: the sound (`Rules+0x280`, constructor -1
/// at `0x006659EE`), resolved against SOUNDMD with the prior index kept
/// for a missing, empty or unknown name (`0x0066A295..0x0066A2C2`), and
/// the frames (`Rules+0x290`, constructor 16 at `0x00665A06`, read with
/// the field as its default at `0x0066A39A`).
#[test]
fn spy_plane_camera_keys_read_audio_visual_over_the_constructor() {
    let sounds = crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=SpyPlaneSnapshot\n",
    ));
    let read = |audio_visual: &str| {
        let ini = IniFile::from_str(&format!(
            "[General]\nFlightLevel=500\n[AudioVisual]\n{audio_visual}"
        ));
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        assert_eq!(
            rules.general.spy_plane_camera, None,
            "unbound: the constructor's -1"
        );
        rules.bind_type_sound_references(&ini, &sounds);
        rules.general
    };
    let stock = read("SpyPlaneCamera=SpyPlaneSnapshot\nSpyPlaneCameraFrames=12\n");
    assert_eq!(stock.spy_plane_camera.as_deref(), Some("SpyPlaneSnapshot"));
    assert_eq!(stock.spy_plane_camera_frames, 12);
    for silent in ["SpyPlaneCamera=\n", "SpyPlaneCamera=NoSuchSound\n", ""] {
        let general = read(silent);
        assert_eq!(general.spy_plane_camera, None, "{silent:?} keeps -1");
        assert_eq!(general.spy_plane_camera_frames, 16);
    }
}

/// `LightningPrintText` is absent from stock `rulesmd.ini`, so the gate on
/// `StormSound` is decided by `RulesClass::Constructor @ 0x006676BC`, which
/// stores `AL` after `0x00667202 MOV EAX,0x1` with no intervening `CALL`
/// (the function's last is `0x0066715D`).
#[test]
fn lightning_print_text_defaults_true_and_reads_the_general_key() {
    let stock = GeneralRules::from_ini(&IniFile::from_str(
        "[General]
FlightLevel=500
",
    ));
    assert!(
        stock.lightning_print_text,
        "the storm cue is audible on stock data"
    );
    let off = GeneralRules::from_ini(&IniFile::from_str(
        "[General]
FlightLevel=500
LightningPrintText=no
",
    ));
    assert!(!off.lightning_print_text);
}

#[test]
fn test_chute_sound_parsed() {
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChuteSound=CustomDrop
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.chute_sound.as_deref(), Some("CustomDrop"));
}

#[test]
fn test_stock_chute_sound_parsed() {
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChuteSound=ParachuteDrop
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.chute_sound.as_deref(), Some("ParachuteDrop"));
}

#[test]
fn test_stock_chrono_sounds_parsed_from_audiovisual() {
    // Stock ships both keys under [AudioVisual] with value ChronoMinerTeleport.
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
ChronoInSound=ChronoMinerTeleport
ChronoOutSound=ChronoMinerTeleport
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(
        general.chrono_in_sound.as_deref(),
        Some("ChronoMinerTeleport")
    );
    assert_eq!(
        general.chrono_out_sound.as_deref(),
        Some("ChronoMinerTeleport")
    );
}

#[test]
fn test_chrono_sounds_absent_yield_none() {
    // A genuinely-absent key must be silence (None), not a fabricated
    // fallback. Guards against reading the wrong section or re-adding a
    // hardcoded default. [General] present so from_ini does not early-return.
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.chrono_in_sound, None);
    assert_eq!(general.chrono_out_sound, None);
}

#[test]
fn harvester_dump_frames_uses_ceil_of_rate_times_900_not_tenths() {
    // The dump gate crosses at ceil(HarvesterDumpRate × 900). The old code
    // quantized to tenths and could cross a frame early for modded rates.
    let frames = |line: &str| {
        let ini = IniFile::from_str(&format!("[General]\n{line}\n"));
        GeneralRules::from_ini(&ini).harvester_dump_frames
    };
    // Stock (key absent -> default 0.016): ceil(14.4) = 15, unchanged.
    assert_eq!(frames(""), 15, "stock 0.016 stays at 15 frames");
    // Modded 0.0156: ceil(14.04) = 15. Old tenths code crossed at 14 (bug).
    assert_eq!(
        frames("HarvesterDumpRate=0.0156"),
        15,
        "0.0156 must ceil to 15, not 14"
    );
    // Modded 0.02: ceil(18.0) = 18.
    assert_eq!(
        frames("HarvesterDumpRate=0.02"),
        18,
        "0.02 -> exactly 18 frames"
    );
    // Modded 0.0201: ceil(18.09) = 19 (round-to-nearest would give 18).
    assert_eq!(
        frames("HarvesterDumpRate=0.0201"),
        19,
        "0.0201 must ceil up to 19"
    );
}

#[test]
fn test_gui_main_button_sound_parsed() {
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
GUIMainButtonSound=MenuClick
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.gui_main_button_sound.as_deref(), Some("MenuClick"));
}

/// The two OreTwinkle keys are read by different native readers: the anim
/// name from `[General]` (ReadGeneral) and the chance from `[AudioVisual]`
/// (ReadAudioVisual). A key in the other section is invisible to gamemd.
#[test]
fn ore_twinkle_keys_follow_the_native_reader_sections() {
    let ini = IniFile::from_str(
        "[General]\nOreTwinkle=TWNK1\nOreTwinkleChance=7\n\
             [AudioVisual]\nOreTwinkleChance=30\nOreTwinkle=WRONG\n",
    );
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.ore_twinkle.as_deref(), Some("TWNK1"));
    assert_eq!(general.ore_twinkle_chance, 30);

    let defaults = RuleSet::from_ini(&IniFile::from_str("[General]\nOreTwinkle=\n"))
        .unwrap()
        .general;
    assert_eq!(
        defaults.ore_twinkle, None,
        "an empty value keeps the null pointer"
    );
    assert_eq!(
        defaults.ore_twinkle_chance, 50,
        "RulesClass constructor default 0x32"
    );
}

#[test]
fn shell_ui_sound_keys_parse_independently() {
    let ini_str = "\
[General]
FlightLevel=500
[AudioVisual]
GUIMainButtonSound=MainButtonClick
GenericClick=GenericPress
GenericBeep=GenericBeep
GUICheckboxSound=CheckboxTick
GUIComboOpenSound=ComboOpen
GUIComboCloseSound=ComboClose
ShellButtonSlideSound=SlideDone
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(
        general.gui_main_button_sound.as_deref(),
        Some("MainButtonClick")
    );
    assert_eq!(
        general.shell_button_slide_sound.as_deref(),
        Some("SlideDone")
    );
    assert_eq!(general.generic_click_sound, None, "awaits SOUNDMD binding");
    assert_eq!(general.generic_beep_sound.as_deref(), Some("GenericBeep"));
    assert_eq!(general.gui_checkbox_sound.as_deref(), Some("CheckboxTick"));
    assert_eq!(general.gui_combo_open_sound.as_deref(), Some("ComboOpen"));
    assert_eq!(general.gui_combo_close_sound.as_deref(), Some("ComboClose"));
}

#[test]
fn shell_ui_sound_keys_trim_and_ignore_empty_values() {
    let ini_str = concat!(
        "[General]\nFixtureOnly=1\n",
        "[AudioVisual]\n",
        "ChuteSound=  ParachuteDrop  \n",
        "GenericClick=  MenuClick  \n",
        "GenericBeep=   \n",
        "GUICheckboxSound=\n",
        "GUIComboOpenSound=   \n",
        "GUIComboCloseSound=MenuACBClose\n",
    );
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert_eq!(general.chute_sound.as_deref(), Some("ParachuteDrop"));
    assert_eq!(general.generic_click_sound, None, "awaits SOUNDMD binding");
    assert!(general.generic_beep_sound.is_none());
    assert!(general.gui_checkbox_sound.is_none());
    assert!(general.gui_combo_open_sound.is_none());
    assert_eq!(
        general.gui_combo_close_sound.as_deref(),
        Some("MenuACBClose")
    );
}

#[test]
fn barrel_particle_parsed_from_general() {
    let ini_str = "\
[General]
BarrelParticle=SmallGreySSys
";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.barrel_particle.as_deref(), Some("SmallGreySSys"));
}

#[test]
fn barrel_particle_default_none() {
    let ini_str = "[General]\nFixtureOnly=1\n";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert!(general.barrel_particle.is_none());
}

#[test]
fn gsi_05_10_prism_type_is_parsed_as_a_building_identity() {
    let ini = IniFile::from_str("[BuildingTypes]\n1=ATESLA\n[General]\nPrismType= atesla \n");
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.prism_type.as_deref(), Some("ATESLA"));

    let absent = RuleSet::from_ini(&IniFile::from_str("[General]\nFixtureOnly=1\n"))
        .unwrap()
        .general;
    assert!(absent.prism_type.is_none());
}

#[test]
fn barrel_particle_ignored_under_audiovisual() {
    // Per report sec 11.8.H the key lives in [General], not [AudioVisual].
    // Verify the parser doesn't accidentally accept it elsewhere.
    let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
BarrelParticle=SmallGreySSys
";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert!(general.barrel_particle.is_none());
}

#[test]
fn test_building_garrisoned_sound_default_none() {
    let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert!(general.building_garrisoned_sound.is_none());
}

#[test]
fn test_chute_sound_empty_treated_as_none() {
    let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
ChuteSound=
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert!(general.chute_sound.is_none());
}

#[test]
fn cell_scatter_gate_keys_parse_from_their_own_sections() {
    // PlayerScatter is a [CombatDamage] key and Scatter an [IQ] key; neither
    // lives in [General], so a [General]-only file must fall back to the
    // RulesClass constructor values (no / 3).
    let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
    let general = GeneralRules::from_ini(&ini);
    assert!(!general.player_scatter);
    assert_eq!(general.iq_scatter, 3);

    // Stock values.
    let ini = IniFile::from_str(
        "[General]\nFlightLevel=500\n[CombatDamage]\nPlayerScatter=no\n[IQ]\nScatter=2\n",
    );
    let general = GeneralRules::from_ini(&ini);
    assert!(!general.player_scatter);
    assert_eq!(general.iq_scatter, 2);

    let ini = IniFile::from_str(
        "[General]\nFlightLevel=500\n[CombatDamage]\nPlayerScatter=yes\n[IQ]\nScatter=4\n",
    );
    let general = GeneralRules::from_ini(&ini);
    assert!(general.player_scatter);
    assert_eq!(general.iq_scatter, 4);
}

#[test]
fn house_ai_activation_iq_production_preserves_native_signed_iq_binding() {
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str("")).iq_production,
        5,
        "missing [IQ] retains the RulesClass constructor default"
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nProduction=-12\n[IQ]\nFixtureOnly=1\n"
        ))
        .iq_production,
        5,
        "the same key under [General] is not an IQ input"
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n[IQ]\nProduction=5\n"
        ))
        .iq_production,
        5
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n[IQ]\nProduction=-7\n"
        ))
        .iq_production,
        -7,
        "negative custom thresholds remain signed and unclamped"
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str(
            "[General]\nFixtureOnly=1\n[IQ]\nMaxIQLevels=5\nProduction=9\n"
        ))
        .iq_production,
        9,
        "Production is not clamped to MaxIQLevels"
    );
    assert_eq!(
        GeneralRules::from_ini(&IniFile::from_str("[IQ]\nProduction=-3\n")).iq_production,
        -3,
        "ReadIQ remains authoritative when [General] is absent"
    );
}

#[test]
fn base_unit_types_parse_from_general() {
    // Registered for Ares's Find-only BaseUnit read.
    let ini = IniFile::from_str(
        "[VehicleTypes]\n0=AMCV\n1=SMCV\n2=PCV\n[General]\nBaseUnit=AMCV,SMCV,PCV\n",
    );
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.base_unit_types, vec!["AMCV", "SMCV", "PCV"]);
}

#[test]
fn test_parachute_max_fall_rate_parsed() {
    let ini_str = "\
[General]
ParachuteMaxFallRate=-3
";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.parachute_max_fall_rate, -3);
}

#[test]
fn test_parachute_max_fall_rate_default_when_missing() {
    let ini_str = "[General]\nFixtureOnly=1\n";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(
        general.parachute_max_fall_rate, -3,
        "default must be -3 per gamemd Rules+0x7B8"
    );
}

#[test]
fn test_parachute_max_fall_rate_custom() {
    // Mod-friendliness: a non-default value must be respected, not clamped.
    let ini_str = "\
[General]
ParachuteMaxFallRate=-1
";
    let ini = IniFile::from_str(ini_str);
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.parachute_max_fall_rate, -1);
}

#[test]
fn test_missile_rot_var_missing_key_keeps_native_constructor() {
    let ini = IniFile::from_str("[General]\nFixtureOnly=1\n");
    let general = RuleSet::from_ini(&ini).unwrap().general;
    assert_eq!(general.missile_rot_var, 0.25);
}

#[test]
fn test_missile_rot_var_stock_rules_value_parsed() {
    let ini = IniFile::from_str("[General]\nMissileROTVar=.25\n");
    let general = RuleSet::from_ini(&ini).unwrap().general;
    let diff = (general.missile_rot_var - 0.25).abs();
    assert!(diff < 0.001, "got {:?}", general.missile_rot_var);
}

#[test]
fn test_missile_rot_var_parsed() {
    let ini = IniFile::from_str("[General]\nMissileROTVar=2.5\n");
    let general = RuleSet::from_ini(&ini).unwrap().general;
    let diff = (general.missile_rot_var - 2.5).abs();
    assert!(diff < 0.001, "got {:?}", general.missile_rot_var);
}

#[test]
fn test_building_garrisoned_sound_empty_treated_as_none() {
    let ini_str = "\
[General]
FixtureOnly=1
[AudioVisual]
BuildingGarrisonedSound=
";
    let ini = IniFile::from_str(ini_str);
    let general = GeneralRules::from_ini(&ini);
    assert!(general.building_garrisoned_sound.is_none());
}

fn ini_with_general(body: &str) -> IniFile {
    let text = format!("[General]\n{}\n", body);
    IniFile::from_str(&text)
}

#[test]
fn ai_dehardcode_keys_default_and_read_general() {
    let defaults = GeneralRules::default();
    assert!(defaults.ai_fire_sale);
    assert_eq!(defaults.ai_fire_sale_delay, 0);
    assert!(defaults.ai_all_to_hunt);
    assert!(defaults.gather_when_mcv_deploy);

    let read = GeneralRules::from_ini(&ini_with_general(
        "AIFireSale=no\nAIFireSaleDelay=450\nAIAllToHunt=no\nGatherWhenMCVDeploy=no",
    ));
    assert!(!read.ai_fire_sale);
    assert_eq!(read.ai_fire_sale_delay, 450);
    assert!(!read.ai_all_to_hunt);
    assert!(!read.gather_when_mcv_deploy);

    let partial = GeneralRules::from_ini(&ini_with_general("AIFireSaleDelay=-3"));
    assert!(partial.ai_fire_sale && partial.ai_all_to_hunt && partial.gather_when_mcv_deploy);
    assert_eq!(partial.ai_fire_sale_delay, -3);
}

#[test]
fn sparking_probability_defaults_and_override() {
    // Stock INI omits both keys -> the verified RulesClass ctor defaults
    // (0.02 red / 0.01 yellow), so the AI_Update Spark effect is on by default.
    let d = GeneralRules::default();
    assert_eq!(d.condition_red_sparking_probability, 0.02);
    assert_eq!(d.condition_yellow_sparking_probability, 0.01);
    let none = GeneralRules::from_ini(&IniFile::from_str("[Foo]\n"));
    assert_eq!(none.condition_red_sparking_probability, 0.02);
    assert_eq!(none.condition_yellow_sparking_probability, 0.01);
    // A mod overrides them from [General].
    let g = GeneralRules::from_ini(&ini_with_general(
        "ConditionRedSparkingProbability=0.05\n\
             ConditionYellowSparkingProbability=0.03",
    ));
    assert_eq!(g.condition_red_sparking_probability, f64::from(0.05_f32));
    assert_eq!(g.condition_yellow_sparking_probability, f64::from(0.03_f32));
}

#[test]
fn health_conditions_retain_the_native_double_without_scaled_copies() {
    let ini = IniFile::from_str("[AudioVisual]\nConditionYellow=37%\nConditionRed=13%\n");
    let general = GeneralRules::from_ini(&ini);
    let section = ini.section("AudioVisual").unwrap();
    assert_eq!(
        general.condition_yellow.to_bits(),
        section.read_double("ConditionYellow", 0.5).to_bits()
    );
    assert_eq!(
        general.condition_red.to_bits(),
        section
            .read_double("ConditionRed", GeneralRules::default().condition_red)
            .to_bits()
    );
    assert_ne!(
        general.condition_yellow.to_bits(),
        f64::from(general.condition_yellow as f32).to_bits()
    );
}

#[test]
fn targeting_delay_defaults_and_override() {
    // Stock rulesmd.ini carries both keys with exactly the constructor
    // defaults; a missing section must still yield them.
    let d = GeneralRules::default();
    assert_eq!(d.normal_targeting_delay, 27);
    assert_eq!(d.guard_area_targeting_delay, 36);
    let none = GeneralRules::from_ini(&IniFile::from_str("[Foo]\n"));
    assert_eq!(none.normal_targeting_delay, 27);
    assert_eq!(none.guard_area_targeting_delay, 36);
    // Values come FROM the INI, not from a hardcoded constant.
    let g = GeneralRules::from_ini(&ini_with_general(
        "NormalTargetingDelay=9\nGuardAreaTargetingDelay=13",
    ));
    assert_eq!(g.normal_targeting_delay, 9);
    assert_eq!(g.guard_area_targeting_delay, 13);
    // ReadInt keeps a negative delay (no clamp at `0x006701B4` or
    // `0x006701D3`).
    let g = GeneralRules::from_ini(&ini_with_general(
        "NormalTargetingDelay=-3\nGuardAreaTargetingDelay=-10",
    ));
    assert_eq!(g.normal_targeting_delay, -3);
    assert_eq!(g.guard_area_targeting_delay, -10);
}

/// The constructor bands take the executed Spark gate's boundary draws
/// (`0x006FAE2D..0x006FAE40`); a 1-off flips the draw count on a boundary
/// tick. A subnormal and a tiny reader value admit roll 0 alone.
#[test]
fn damage_spark_thresholds_from_the_general_reader() {
    let d = GeneralRules::default();
    let mut checked = 0;
    for (band, draw, passed) in crate::util::native_random::native_gate_rows("spark") {
        for (probability, threshold) in [
            (
                d.condition_red_sparking_probability,
                d.condition_red_spark_threshold,
            ),
            (
                d.condition_yellow_sparking_probability,
                d.condition_yellow_spark_threshold,
            ),
        ] {
            if band.to_bits() == probability.to_bits() {
                assert_eq!(draw < threshold, passed, "band {band}, draw {draw:#x}");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 12);
    assert_eq!(d.condition_red_spark_threshold, 42_949_673);
    assert_eq!(d.condition_yellow_spark_threshold, 21_474_837);

    let g = GeneralRules::from_ini(&ini_with_general(
        "ConditionRedSparkingProbability=1e-45\nConditionYellowSparkingProbability=1e-30",
    ));
    let red = g.condition_red_sparking_probability;
    assert!(red > 0.0 && red < f64::from(f32::MIN_POSITIVE), "{red:e}");
    assert_eq!(g.condition_red_spark_threshold, 1);
    assert_eq!(g.condition_yellow_spark_threshold, 1);
}

/// The constructor empties every paradrop list (`0x00666649..0x006666D8`)
/// and a pass without the keys keeps them.
#[test]
fn paradrop_lists_start_empty() {
    let rules = RuleSet::from_ini(&IniFile::from_str("[Foo]\nBar=1\n")).unwrap();
    let g = &rules.general;
    assert_eq!(g.paradrop_radius, 1024);
    for list in [
        &g.amer_paradrop,
        &g.ally_paradrop,
        &g.sov_paradrop,
        &g.yuri_paradrop,
    ] {
        assert_eq!(list, &ParadropList::default());
    }
}

/// Each list is read on its own: the infantry through the InfantryType
/// list reader, which drops `none`, the counts as ints. Nothing pairs them
/// or compares their lengths. The infantry are registered, as Ares's
/// Find-only read (Buf_General 1005BB00, hook 0x66D55E) needs.
#[test]
fn paradrop_lists_read_separately() {
    let rules = RuleSet::from_ini(&ini_with_general(
        "ParadropRadius=2048\n\
             AmerParaDropInf=E1,GHOST,none,ENGINEER\n\
             AmerParaDropNum=6,-2\n\
             SovParaDropInf=E2\n\
             SovParaDropNum=9,4\n\
             [InfantryTypes]\n0=E1\n1=GHOST\n2=ENGINEER\n3=E2",
    ))
    .unwrap();
    let g = &rules.general;
    assert_eq!(g.paradrop_radius, 2048);
    assert_eq!(g.amer_paradrop.infantry, ["E1", "GHOST", "ENGINEER"]);
    assert_eq!(g.amer_paradrop.counts, [6, -2]);
    assert_eq!(g.sov_paradrop.infantry, ["E2"]);
    assert_eq!(g.sov_paradrop.counts, [9, 4]);
    assert_eq!(g.ally_paradrop, ParadropList::default());
}

/// Retail difficulty rows and country ROF through the production reader:
/// `[Easy] ROF=.8`, `[Normal] ROF=1.0`, `[Difficult] ROF=1.2` (ReadDouble
/// widens the `%f` single), and no retail country sets `ROF=`, so every
/// country keeps the constructor's 1.0.
#[test]
fn retail_difficulty_and_country_rof() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(
        rules.general.difficulty_rows.map(|row| row.rof),
        [f64::from(0.8f32), 1.0, f64::from(1.2f32)]
    );
    for country in ["Americans", "Russians", "YuriCountry"] {
        assert_eq!(
            rules.country_difficulty_biases(country).rof,
            NativeF64Bits::ONE,
            "{country}"
        );
    }
}

/// Retail corpse anims through the production reader: `[General]
/// DeadBodies=` lists six, and the GI and Conscript name none of their own
/// and are not `NotHuman=`, so their deaths pick from the six.
#[test]
fn retail_dead_bodies() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    assert_eq!(
        rules.general.dead_bodies,
        [
            "DEATH_A", "DEATH_B", "DEATH_C", "DEATH_D", "DEATH_E", "DEATH_F"
        ]
    );
    for infantry in ["E1", "E2"] {
        let object = rules.object(infantry).unwrap();
        assert!(
            object.dead_bodies.is_empty() && !object.not_human,
            "{infantry}"
        );
    }
}

/// Retail `FireAngle=` through the production reader: ships, submarines,
/// missiles and the tech outpost set it (`CAOUTP` behind a trailing
/// comment); the Grizzly and Rhino keep the constructor's 8.
#[test]
fn retail_fire_angle() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    for (object, fire_angle) in [
        ("DEST", 32),
        ("BSUB", 64),
        ("V3ROCKET", 1),
        ("CAOUTP", 0),
        ("MTNK", 8),
        ("HTNK", 8),
    ] {
        assert_eq!(
            rules.object(object).unwrap().fire_angle,
            fire_angle,
            "{object}"
        );
    }
}

#[test]
fn weight_reads_keep_the_current_value_between_rules_passes() {
    // TechnoType ctor710C66..710C71 initializes double1.0; reader
    //7124CF..7124EA passes that current value to ReadDouble5283D0.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\n",
    ));
    for (text, expected) in [
        ("[MTNK]\nArmor=heavy\n", SimFixed::ONE),
        ("[MTNK]\nWeight=3.5\n", SimFixed::lit("3.5")),
        ("[MTNK]\nSpeed=6\n", SimFixed::lit("3.5")),
        // Native ReadDouble scans malformed authored text as zero; only
        // an absent key retains the current field.
        ("[MTNK]\nWeight=invalid\n", SimFixed::ZERO),
        ("[MTNK]\nSpeed=7\n", SimFixed::ZERO),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(text));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.object("MTNK").unwrap().weight, expected);
    }
}

#[test]
fn bunker_range_default_and_layered_reads_match_original() {
    use crate::rules::native_processing::RulesLayerKind;

    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_cell_range_bunker.json",
    ))
    .unwrap();
    assert_eq!(
        i64::from(GarrisonRules::default().bunker_weapon_range_bonus),
        native["inputs"]["constructor"]["bunker_bonus"]
            .as_i64()
            .unwrap()
    );
    for row in native["inputs"]["bunker_reader_controls"]
        .as_array()
        .unwrap()
    {
        let mut layers = RulesLayerStack::new(IniFile::empty());
        for (keys, expected) in row["layers"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["values"].as_array().unwrap())
        {
            let mut text = String::from("[CombatDamage]\n");
            for (key, value) in keys.as_object().unwrap() {
                text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(&text));
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                i64::from(rules.garrison_rules.bunker_weapon_range_bonus),
                expected.as_i64().unwrap(),
                "{}",
                row["name"]
            );
        }
    }
}

#[test]
fn retail_lasher_range_inputs_match_original_readers() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_cell_range_bunker.json",
    ))
    .unwrap();
    let tank = rules.object("LTNK").unwrap();
    let primary = tank.primary().unwrap();
    let weapon = rules.weapon(primary).unwrap();
    let projectile_name = weapon.projectile.as_deref().unwrap();
    let projectile = rules.projectile(projectile_name).unwrap();
    // This check compares the same physical RULESMD pass. The joined
    // Battle/map fixture separately covers production scenario loading.
    assert_eq!(
        serde_json::json!({
            "primary": primary,
            "projectile": projectile_name,
            "range": weapon.range_leptons,
            "minimum": weapon.minimum_range_leptons,
            "cell_rangefinding": weapon.cell_rangefinding,
            "arcing": projectile.arcing,
            "subject_to_elevation": projectile.subject_to_elevation,
            "floater": projectile.floater,
            "gravity": rules.general.gravity,
            "bunker_bonus": rules.garrison_rules.bunker_weapon_range_bonus,
            "open_topped_bonus": rules.garrison_rules.open_topped_range_bonus,
        }),
        native["inputs"]["layers"][0]["state"],
    );
}

#[test]
fn cheer_sound_binds_registered_names_with_reached_pass_retention() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::sound_ini::SoundRegistry;

    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=FirstCheer\n1=MapCheer\n[FirstCheer]\n[MapCheer]\n",
    ));
    for (later, expected) in [
        ("", Some("FirstCheer")),
        ("CheerSound=\n", Some("FirstCheer")),
        ("CheerSound=Unknown\n", Some("FirstCheer")),
        ("cheersound=MapCheer\n", Some("FirstCheer")),
        ("CheerSound=mapcheer\n", Some("MapCheer")),
    ] {
        let mut layers =
            RulesLayerStack::new(IniFile::from_str("[AudioVisual]\nCheerSound=FirstCheer\n"));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!("[AudioVisual]\n{later}")),
        );
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        assert_eq!(rules.general.cheer_sound(), expected, "{later:?}");
    }
    let ini = IniFile::empty();
    let mut absent = RuleSet::from_ini(&ini).unwrap();
    absent.bind_type_sound_references(&ini, &sounds);
    assert_eq!(absent.general.cheer_sound(), None, "constructor6658E0=-1");
}

/// Retail `[ElevationModel]` through the production reader: four levels a
/// step, two cells a step, two cells at most. Without the section the
/// constructor's 0, 1.0 and 0.0 stay.
#[test]
fn retail_elevation_model() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let two = NativeF64Bits::from_bits(2.0f64.to_bits());
    let model = rules.elevation_model;
    assert_eq!(
        (model.increment, model.increment_bonus, model.bonus_cap),
        (4, two, two)
    );
    let model = ElevationModel::from_ini(&IniFile::from_str("[General]\n"));
    assert_eq!(
        (model.increment, model.increment_bonus, model.bonus_cap),
        (0, NativeF64Bits::ONE, NativeF64Bits::POSITIVE_ZERO)
    );
}

#[test]
fn parallel_ai_queue_switches_default_the_gate_on_and_read_global_controls() {
    // Phobos RulesExt constructs AllowParallelAIQueues { true }.
    let empty = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let controls = empty.parallel_ai_queues();
    assert!(controls.allows_parallel());
    assert!(!controls.forbids_category(ObjectCategory::Building, false));

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[GlobalControls]\nAllowParallelAIQueues=no\n\
             ForbidParallelAIQueues.Infantry=yes\nForbidParallelAIQueues.Navy=yes\n",
    ))
    .unwrap();
    let controls = rules.parallel_ai_queues();
    assert!(!controls.allows_parallel());
    assert!(controls.forbids_category(ObjectCategory::Infantry, false));
    assert!(!controls.forbids_category(ObjectCategory::Vehicle, false));
    assert!(controls.forbids_category(ObjectCategory::Vehicle, true));
    assert!(!controls.forbids_category(ObjectCategory::Aircraft, false));
    assert!(!controls.forbids_category(ObjectCategory::Building, false));
}

#[test]
fn techno_forbid_parallel_ai_queues_reads_per_type() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=BLOCK\n1=OPEN\n[BLOCK]\nForbidParallelAIQueues=yes\n[OPEN]\n",
    ))
    .unwrap();
    assert!(rules.object("BLOCK").unwrap().forbid_parallel_ai_queues);
    assert!(!rules.object("OPEN").unwrap().forbid_parallel_ai_queues);
}
