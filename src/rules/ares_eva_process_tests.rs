//! Fixed EVAMD selection through the actual process lifecycle, not merged INI.
//! Tag/phase semantics have executed component controls in ares_eva.json;
//! these are Rust production-integration regressions, not whole-native parity.

use super::*;

fn fixed_eva() -> Arc<EvaRegistry> {
    Arc::new(EvaRegistry::from_ini(&IniFile::from_str(
        "[EVATypes]\n0=VoiceA\n1=VoiceB\n[DialogList]\n0=EVA_TEST\n\
         [EVA_TEST]\nAllied=STOCK_A\nRussian=STOCK_R\nYuri=STOCK_Y\n\
         VoiceA=EXTRA_A\nVoiceB=EXTRA_B\n",
    )))
}

fn owner(root: &str, langrule: Option<&str>, eva: Arc<EvaRegistry>) -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(root),
        langrule.map(IniFile::from_str),
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
        eva,
    )
    .unwrap()
}

#[test]
fn fixed_catalog_reaches_real_side_passes_and_saved_projection() {
    let eva = fixed_eva();
    let mut process = owner(
        "[Sides]\nGDI=ONE\nNod=TWO\nThirdSide=THREE\nFourth=FOUR\n\
         [Fourth]\nEVA.Tag=VoiceA\n",
        Some("[Fourth]\nEVA.Tag=VoiceB\n"),
        eva.clone(),
    );
    let startup = process.startup_compatibility_projection().unwrap();
    let startup_rules = RuleSet::from_processed_rules(&startup).unwrap();
    assert_eq!(startup_rules.side_eva_voice_index(3), Some(4));

    // The map's unknown tag must retain the mode's explicit clear. Looking
    // only at merged text would lose this reached earlier write.
    let mode = IniFile::from_str("[Fourth]\nEVA.Tag=<none>\n");
    let map = IniFile::from_str("[Fourth]\nEVA.Tag=UNKNOWN\n[EVATypes]\n0=UNRELATED\n");
    let loaded = process
        .load_scenario(NativeScenarioRulesPrefix::NonCampaign(Some(&mode)), &map)
        .unwrap();
    let (rules, _, _, _) = loaded.into_parts();
    assert_eq!(rules.side_eva_voice_index(3), Some(-1));
    assert!(Arc::ptr_eq(&eva, &process.sources.fixed_eva));
    assert_eq!(
        process.sources.fixed_eva.voice_index_for_tag("VoiceB"),
        Some(4)
    );
    assert_eq!(
        process.sources.fixed_eva.voice_index_for_tag("UNRELATED"),
        None
    );

    let saved = process.saved_scenario_rules().unwrap();
    let bytes = crate::util::save_codec::serialize(saved).unwrap();
    let restored: ProcessedRulesLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
    let restored_rules = process.project_saved_scenario_rules(&restored).unwrap();
    assert_eq!(restored_rules.side_eva_voice_index(3), Some(-1));
    assert_eq!(
        rules.simulation_config_hash(),
        restored_rules.simulation_config_hash()
    );

    // A later fresh scenario rebuilds the same owner, rereads root/LANG and
    // receives VoiceB again. It does not retain the outgoing map's -1.
    let (fresh, _, _, _) = process
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    assert_eq!(fresh.side_eva_voice_index(3), Some(4));
}

#[test]
fn a_side_created_after_the_hook_never_reads_an_earlier_orphan_tag() {
    let mut process = owner(
        "[Countries]\n0=FOUR\n[FOUR]\nSide=LateSide\n\
         [LateSide]\nEVA.Tag=VoiceA\n",
        None,
        fixed_eva(),
    );
    let (rules, _, _, _) = process
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    let side = rules.side_index("LateSide").unwrap();
    assert_eq!(rules.side_eva_voice_index(i32::from(side.0)), Some(0));
}
