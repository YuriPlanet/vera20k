//! The process owner binds native sound IDs to fixed SOUNDMD names, retaining
//! valid values across root/LANG/mode/map readers (712FF1/7130A5/6699C8).

use std::sync::Arc;

use super::NativeRulesProcessOwner;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;

/// Rules669C68..669CA3 shares the fixed catalog/current-ID reader with
/// Construction. The Engineer sound is an AudioVisual key in every pass.
#[test]
fn building_repaired_sound_uses_the_fixed_catalog_and_retains_prior_ids() {
    let ini = IniFile::from_str;
    let sounds = Arc::new(SoundRegistry::from_ini(&ini(
        "[SoundList]\n0=Repair\n1=Wrong\n[NotRegistered]\nSounds=sample\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        ini(
            "[AudioVisual]\nBuildingRepairedSound=Repair\n[General]\nBuildingRepairedSound=Wrong\n",
        ),
        Some(ini("[AudioVisual]\nBuildingRepairedSound=unknown\n")),
        ini(""),
        Arc::clone(&sounds),
    )
    .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&ini(
                "[AudioVisual]\nBuildingRepairedSound= rEpAiR \n",
            ))),
            &ini("[AudioVisual]\nBuildingRepairedSound=\nbuildingrepairedsound=Wrong\n"),
        )
        .unwrap()
        .into_parts();
    assert_eq!(
        rules.general.building_repaired_sound.as_deref(),
        Some("Repair")
    );
    assert!(Arc::ptr_eq(owner.fixed_sounds(), &sounds));

    let mut invalid = NativeRulesProcessOwner::from_cold_start_sources(
        ini("[AudioVisual]\nBuildingRepairedSound=NotRegistered\n"),
        None,
        ini(""),
        sounds,
    )
    .unwrap();
    let (rules, _, _, _) = invalid
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &ini(""),
        )
        .unwrap()
        .into_parts();
    assert!(rules.general.building_repaired_sound.is_none());
}

/// Original Rules66A979..66A9B7 reads AudioVisual/Construction and
/// BuildingType460738..46078B reads BuildupSound, both through ReadString128
/// and Voc Find7514D0. The startup catalog is fixed across the rules passes.
#[test]
fn construction_sound_references_follow_native_sections_and_retention() {
    let ini = IniFile::from_str;
    let root = ini("[BuildingTypes]\n0=POWER\n[POWER]\nBuildupSound=Build\n\
         [VehicleTypes]\n0=TANK\n[TANK]\nBuildupSound=Build\n\
         [AudioVisual]\nConstruction=Dummy\n[General]\nConstruction=Wrong\n");
    let sounds = Arc::new(SoundRegistry::from_ini(&ini(
        "[SoundList]\n0=Dummy\n1=Build\n2=Wrong\n[Dummy]\nVolume=0\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        Some(ini(
            "[POWER]\nBuildupSound=unknown\n[AudioVisual]\nConstruction=none\n",
        )),
        ini(""),
        sounds,
    )
    .unwrap();
    let (rules, _, _, _) = owner.load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
        Some(&ini("[POWER]\nBuildupSound= bUiLd \n"))),
        &ini("[POWER]\nBuildupSound=\nbuildupsound=Wrong\n[AudioVisual]\nConstruction=\nconstruction=Wrong\n"),
    ).unwrap().into_parts();
    assert_eq!(rules.general.construction_sound.as_deref(), Some("Dummy"));
    assert_eq!(
        rules.object("POWER").unwrap().buildup_sound.as_deref(),
        Some("Build")
    );
    assert!(rules.object("TANK").unwrap().buildup_sound.is_none());
}

#[test]
fn sinking_sound_references_keep_valid_prior_ids_and_exact_reader_scope() {
    let ini = IniFile::from_str;
    let root = ini(
        "[VehicleTypes]\n0=SHIP\n[SHIP]\nSinkingSound=Hull\nVoiceSinking=Voice\n\
         [AudioVisual]\nSinkingSound=Fallback\n[General]\nSinkingSound=WrongSection\n",
    );
    let lang = ini("[SHIP]\nSinkingSound=unknown\nVoiceSinking=none\n");
    let mode = ini("[SHIP]\nSinkingSound=  hUlL  \n[AudioVisual]\nSinkingSound=unknown\n");
    let map = ini("[SHIP]\nSinkingSound=\nVoiceSinking=not_registered\nvoicesinking=Hull\n");
    let sounds = Arc::new(SoundRegistry::from_ini(&ini(
        "[SoundList]\n0=Hull\n1=Voice\n2=Fallback\n3=WrongSection\n\
         [not_registered]\nSounds=sample\n",
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        Some(lang),
        ini(""),
        Arc::clone(&sounds),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&sounds, owner.fixed_sounds()));
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &map,
        )
        .unwrap()
        .into_parts();
    let ship = rules.object("SHIP").unwrap();
    assert_eq!(ship.sinking_sound.as_deref(), Some("Hull"));
    assert_eq!(ship.voice_sinking.as_deref(), Some("Voice"));
    assert_eq!(rules.general.sinking_sound.as_deref(), Some("Fallback"));

    let mut empty_catalog = NativeRulesProcessOwner::from_cold_start_sources(
        ini("[VehicleTypes]\n0=SHIP\n[SHIP]\nSinkingSound=Hull\n[General]\nSinkingSound=Hull\n"),
        None,
        ini(""),
        Arc::default(),
    )
    .unwrap();
    let (rules, _, _, _) = empty_catalog
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &ini(""),
        )
        .unwrap()
        .into_parts();
    assert!(rules.object("SHIP").unwrap().sinking_sound.is_none());
    assert!(rules.object("SHIP").unwrap().voice_sinking.is_none());
    assert!(rules.general.sinking_sound.is_none());
}

#[test]
fn sinking_sound_reference_uses_readstring128_before_lookup() {
    let sounds = SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=Hull\n"));
    // 123 spaces plus the four-character name fill the 127 usable bytes.
    let ini = IniFile::from_str(&format!("[SHIP]\nSinkingSound={}HullX\n", " ".repeat(123)));
    // The physical INI parser trims its value before the later ReadString.
    // Preserve the reader boundary explicitly in a projected section instead.
    let mut section = ini.section("SHIP").unwrap().clone();
    section.set("SinkingSound", &format!("{}HullX", " ".repeat(123)));
    assert_eq!(
        sounds
            .read_rules_reference(&section, "SinkingSound")
            .as_deref(),
        Some("Hull")
    );
    section.set("SinkingSound", &format!("{}Hull", " ".repeat(124)));
    assert!(
        sounds
            .read_rules_reference(&section, "SinkingSound")
            .is_none()
    );
}

#[test]
fn infantry_water_sounds_use_fixed_catalog_and_retain_valid_ids_across_passes() {
    let ini = IniFile::from_str;
    let root = ini(
        "[InfantryTypes]\n0=GHOST\n[GHOST]\nEnterWaterSound=Enter\nLeaveWaterSound=Leave\n[VehicleTypes]\n0=MTNK\n[MTNK]\nEnterWaterSound=Enter\n",
    );
    let lang = ini("[GHOST]\nEnterWaterSound=unknown\nLeaveWaterSound=\n");
    let mode = ini("[GHOST]\nEnterWaterSound=  eNtEr  \nLeaveWaterSound=other\n");
    let map =
        ini("[GHOST]\nEnterWaterSound=\nLeaveWaterSound=unregistered\nenterwatersound=Wrong\n");
    let sounds = Arc::new(SoundRegistry::from_ini(&ini(
        "[SoundList]\n0=Enter\n1=Leave\n2=Wrong\n",
    )));
    let mut owner =
        NativeRulesProcessOwner::from_cold_start_sources(root.clone(), Some(lang), ini(""), sounds)
            .unwrap();
    let (rules, _, _, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(&mode)),
            &map,
        )
        .unwrap()
        .into_parts();
    let ghost = rules.object("GHOST").unwrap();
    assert_eq!(ghost.enter_water_sound.as_deref(), Some("Enter"));
    assert_eq!(ghost.leave_water_sound.as_deref(), Some("Leave"));
    assert!(rules.object("MTNK").unwrap().enter_water_sound.is_none());
    let mut empty =
        NativeRulesProcessOwner::from_cold_start_sources(root, None, ini(""), Arc::default())
            .unwrap();
    let (rules, _, _, _) = empty
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &ini(""),
        )
        .unwrap()
        .into_parts();
    assert!(rules.object("GHOST").unwrap().enter_water_sound.is_none());
    assert!(rules.object("GHOST").unwrap().leave_water_sound.is_none());
}
