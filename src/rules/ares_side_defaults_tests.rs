//! Ares 3.0 SideExt values through the retained production Rules owner.
//!
//! `SideExt::initialize @ 0x1002E190` defaults and the reader prefix
//! `0x1002E450..0x1002EA4C` are bounded in
//! `tools/rules_oracle/ares_side_sidebar.py`; these tests exercise the same
//! projection the gameplay consumers read.

use super::*;
use crate::rules::ruleset::RuleSet;

fn processed(ini: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .process()
        .expect("side extension rules parse");
    RuleSet::from_processed_rules(&processed).expect("side extension projection")
}

const TYPES: &str = "\
[InfantryTypes]\n\
0=ACREW\n1=SCREW\n2=TCREW\n3=ENG\n4=TECH\n5=ADIS\n6=SDIS\n7=TDIS\n8=APD\n\
9=CREWX\n10=ENGX\n11=TECHX\n12=DISGX\n\
[VehicleTypes]\n0=UNITX\n\
[AircraftTypes]\n0=PDPLANE\n1=PLANEX\n\
[BuildingTypes]\n0=ABLD\n1=SBLD\n2=TBLD\n3=BLDA\n4=BLDB\n\
[Animations]\n0=ANIMX\n";

const GENERAL: &str = "\
[General]\n\
AlliedCrew=ACREW\nSovietCrew=SCREW\nThirdCrew=TCREW\n\
Engineer=ENG\nTechnician=TECH\n\
AlliedDisguise=ADIS\nSovietDisguise=SDIS\nThirdDisguise=TDIS\n\
AlliedSurvivorDivisor=100\nSovietSurvivorDivisor=200\nThirdSurvivorDivisor=300\n\
AlliedBaseDefenseCounts=1,2,3\nSovietBaseDefenseCounts=4,5,6\nThirdBaseDefenseCounts=7,8,9\n\
AllyParaDropInf=APD\nAllyParaDropNum=3\n\
SovParaDropInf=APD\nSovParaDropNum=2\nYuriParaDropInf=APD\nYuriParaDropNum=1\n";

const AI: &str = "\
[AI]\n\
AlliedBaseDefenses=ABLD\nSovietBaseDefenses=SBLD\nThirdBaseDefenses=TBLD\n";

#[test]
fn ares_fourth_side_uses_documented_allied_defaults() {
    let rules = processed(&format!(
        "{TYPES}{GENERAL}{AI}\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
         [Fourth]\n"
    ));

    assert_eq!(rules.side_crew(3), Some(Some("ACREW")));
    assert_eq!(rules.side_engineer(3), Some(Some("ENG")));
    assert_eq!(rules.side_technician(3), Some(Some("TECH")));
    assert_eq!(rules.side_disguise(3), Some(Some("ADIS")));
    assert_eq!(rules.side_survivor_divisor(3), Some(100));
    assert_eq!(
        rules.side_base_defenses(3),
        Some(&[String::from("ABLD")][..])
    );
    assert_eq!(rules.base_defense_types(3), &[String::from("ABLD")][..]);
    assert_eq!(rules.side_base_defense_counts(3), Some(&[1, 2, 3][..]));
    assert_eq!(
        rules.side_paradrop(3),
        Some((&[String::from("APD")][..], &[3][..]))
    );
    assert_eq!(rules.side_paradrop_aircraft(3), Some("PDPLANE"));
    assert_eq!(rules.side_parachute_override(3), None);
    assert_eq!(rules.side_tooltip_color(3), Some([164, 210, 255]));
    assert_eq!(rules.side_message_text_color_index(3), Some(21));
    assert_eq!(
        rules.side_sidebar_mix_file_index(crate::rules::ruleset::SideIdx(3)),
        Some(1)
    );
    assert!(
        !rules
            .side_sidebar_yuri_file_names(crate::rules::ruleset::SideIdx(3))
            .unwrap()
    );
    assert_eq!(rules.side_eva_voice_index(3), Some(0));
}

#[test]
fn ares_fourth_side_reads_explicit_overrides_in_native_order() {
    let rules = processed(&format!(
        "{TYPES}{GENERAL}{AI}\
         [Colors]\nDarkBlue=153,214,212\nDarkRed=0,230,255\n\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
         [Fourth]\n\
         AI.BaseDefenseCounts=4,5,6\n\
         AI.BaseDefenses=BLDA,BLDB\n\
         Crew=CREWX\nEngineer=ENGX\nTechnician=TECHX\nDefaultDisguise=DISGX\n\
         Parachute.Anim=ANIMX\nParaDrop.Aircraft=PLANEX\n\
         ParaDrop.Types=CREWX,UNITX\nParaDrop.Num=2,3\n\
         Sidebar.MixFileIndex=4\nSidebar.YuriFileNames=yes\n\
         ToolTipColor=1,2,3\nSurvivorDivisor=123\n\
         MessageTextColor=DarkBlue\nHunterSeeker=UNITX\n"
    ));

    assert_eq!(rules.side_crew(3), Some(Some("CREWX")));
    assert_eq!(rules.side_engineer(3), Some(Some("ENGX")));
    assert_eq!(rules.side_technician(3), Some(Some("TECHX")));
    assert_eq!(rules.side_disguise(3), Some(Some("DISGX")));
    assert_eq!(rules.side_survivor_divisor(3), Some(123));
    assert_eq!(
        rules.side_base_defenses(3),
        Some(&[String::from("BLDA"), String::from("BLDB")][..])
    );
    assert_eq!(
        rules.base_defense_types(3),
        &[String::from("BLDA"), String::from("BLDB")][..]
    );
    assert_eq!(rules.side_base_defense_counts(3), Some(&[4, 5, 6][..]));
    assert_eq!(
        rules.side_paradrop(3),
        Some((
            &[String::from("CREWX"), String::from("UNITX")][..],
            &[2, 3][..]
        ))
    );
    assert_eq!(rules.side_paradrop_aircraft(3), Some("PLANEX"));
    assert_eq!(rules.side_parachute_override(3), Some(Some("ANIMX")));
    assert_eq!(rules.side_tooltip_color(3), Some([1, 2, 3]));
    // `DarkBlue` is the first retained `[Colors]` entry; the shared kind-1
    // lookup `0x1001BB30` finds its scheme at runtime index `2 * 0`.
    assert_eq!(rules.side_message_text_color_index(3), Some(0));
    assert_eq!(
        rules.side_sidebar_mix_file_index(crate::rules::ruleset::SideIdx(3)),
        Some(4)
    );
    assert!(
        rules
            .side_sidebar_yuri_file_names(crate::rules::ruleset::SideIdx(3))
            .unwrap()
    );
    assert_eq!(rules.side_hunter_seeker(3), Some("UNITX"));
}

#[test]
fn ares_side_empty_and_unknown_values_retain_while_none_clears() {
    let base = format!(
        "{TYPES}{GENERAL}{AI}\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n"
    );
    let parsed = IniFile::from_str(&base);
    let mut sections: Vec<IniSection> = parsed
        .section_names()
        .iter()
        .map(|name| parsed.section(name).unwrap().clone())
        .collect();
    let mut fourth = IniSection::new("Fourth".into());
    fourth.set("Crew", "");
    fourth.set("Engineer", "UNKNOWN");
    fourth.set("Technician", "none");
    fourth.set("AI.BaseDefenses", "<default>");
    fourth.set("ParaDrop.Types", "");
    fourth.set("ParaDrop.Num", "2");
    fourth.set("Sidebar.MixFileIndex", "not-a-number");
    fourth.set("MessageTextColor", "<none>");
    fourth.set("CampaignScore.Background", "");
    fourth.set("CampaignScore.Palette", "none");
    sections.push(fourth);
    let processed = RulesLayerStack::new(IniFile::from_sections_for_test(sections))
        .process()
        .expect("side extension rules parse");
    let rules = RuleSet::from_processed_rules(&processed).expect("side extension projection");

    // ReadString returning 0 (empty) and an unknown name both keep the unset
    // field, which falls back; only `none`/`<none>` (`0x10003950`) stores null.
    assert_eq!(rules.side_crew(3), Some(Some("ACREW")));
    assert_eq!(rules.side_engineer(3), Some(Some("ENG")));
    assert_eq!(rules.side_technician(3), Some(None));
    assert_eq!(rules.side_message_text_color_index(3), Some(-1));
    // `<default>` returns to the side fallback; the empty `ParaDrop.Types`
    // keeps the unset vector, so the getter still pairs the Allied lists.
    assert_eq!(
        rules.side_base_defenses(3),
        Some(&[String::from("ABLD")][..])
    );
    assert_eq!(
        rules.side_paradrop(3),
        Some((&[String::from("APD")][..], &[3][..]))
    );
    // An unparsable Ares int retains the initialized sidebar index.
    assert_eq!(
        rules.side_sidebar_mix_file_index(crate::rules::ruleset::SideIdx(3)),
        Some(1)
    );
    // Fixed strings: empty keeps the initialized file, `none` clears it.
    let (_, fields) = processed.side_projection().nth(3).expect("fourth side");
    let fields = fields.expect("allocated Side owns its Ares extension");
    assert_eq!(
        fields.campaign_score_background.as_deref(),
        Some("SYCRBKMD.SHP")
    );
    assert_eq!(fields.campaign_score_palette, None);
}

#[test]
fn ares_side_graphical_text_slots_keep_the_literal_fallback() {
    let rules = processed(&format!(
        "{TYPES}{GENERAL}\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
         [Fourth]\n\
         GraphicalText.Image=mytext.shp\n"
    ));
    // One slot named: the other slot keeps InitGame's literal file for the
    // consumer, exactly as the two getters at 0x1002DF00/0x1002DF10 fall back.
    assert_eq!(
        rules.side_graphical_text(3),
        Some((Some("mytext.shp"), None))
    );
    // A side naming nothing leaves both slots empty, so the process art is
    // already the native result.
    assert_eq!(rules.side_graphical_text(0), Some((None, None)));
    // No SideExt exists for an unallocated side.
    assert_eq!(rules.side_graphical_text(9), None);
}

/// The score shells read SideExt through the live process owner, which holds
/// it once a scenario's Rules passes have run.
fn process_owner(ini: &str) -> crate::rules::process_owner::NativeRulesProcessOwner {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(ini),
        None,
        IniFile::empty(),
        Default::default(),
        Default::default(),
    )
    .expect("side score cold start");
    owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .expect("side score scenario rules");
    owner
}

#[test]
fn ares_side_score_files_follow_the_side_buffers_the_hooks_copy() {
    let owner = process_owner(&format!(
        "{TYPES}{GENERAL}\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
         [Fourth]\n\
         MultiplayerScore.Bars=MyBars~~.pcx\n\
         MultiplayerScore.Palette=\n"
    ));
    // Side 0 keeps `SideExt::initialize`'s Allied row: the hooks copy these
    // buffers over gamemd's own, so a fourth side takes the Yuri row it is
    // initialized with, not the stock fallback.
    assert_eq!(
        owner.side_score_art(0).campaign_background,
        Some("ASCRBKMD.SHP")
    );
    let fourth = owner.side_score_art(3);
    assert_eq!(fourth.campaign_background, Some("SYCRBKMD.SHP"));
    assert_eq!(fourth.multiplayer_bars, Some("MyBars~~.pcx"));
    // An empty value retains the initialized name (AresFixedString read).
    assert_eq!(fourth.multiplayer_palette, Some("MPYSCRN.PAL"));
    // No SideExt: every name keeps its caller's fixed table.
    assert_eq!(owner.side_score_art(9), Default::default());
    assert_eq!(owner.side_score_art(-1), Default::default());
}

#[test]
fn ares_side_score_themes_follow_the_two_native_branch_sites() {
    let owner = process_owner(&format!(
        "{TYPES}{GENERAL}\
         [Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
         [Fourth]\n\
         CampaignScore.UnderParTheme=MYUND\n\
         MultiplayerScore.LoseTheme=MYLOSE\n"
    ));
    // 0x1002F920 keeps +0x178 while elapsed is at or under par, and
    // 0x1002F4B0 keeps +0x1B8 while HouseClass+0x1F5 is clear; the untouched
    // fields stay on SideExt::initialize's literal SCORE.
    use crate::rules::ruleset::ScoreTheme;
    assert_eq!(
        owner.side_score_theme(3, ScoreTheme::Campaign { over_par: false }),
        Some("MYUND")
    );
    assert_eq!(
        owner.side_score_theme(3, ScoreTheme::Campaign { over_par: true }),
        Some("SCORE")
    );
    assert_eq!(
        owner.side_score_theme(3, ScoreTheme::Multiplayer { defeated: false }),
        Some("SCORE")
    );
    assert_eq!(
        owner.side_score_theme(3, ScoreTheme::Multiplayer { defeated: true }),
        Some("MYLOSE")
    );
    assert_eq!(
        owner.side_score_theme(0, ScoreTheme::Multiplayer { defeated: true }),
        Some("SCORE")
    );
    assert_eq!(
        owner.side_score_theme(9, ScoreTheme::Multiplayer { defeated: true }),
        None
    );
}
