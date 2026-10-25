//! First Draw's retained Country reads straddle one real E_multi prefix.
//! Native call ordering: PrepareSession52E123, early6847B7/552D58,
//! E_multi68741D..687451, Draw687588, TypeReset6876AC. The parent-copy and
//! second-primary-read behavior has executed component controls in
//! tools/rules_oracle/ares_country_loading.{py,json,meta.json}.
//! These are owner/lifecycle regressions, not whole-loading native captures.

use super::*;
use crate::rules::ruleset::CountryIdx;
use std::panic::{AssertUnwindSafe, catch_unwind};

const ROOT: &str = "\
[Countries]
0=PARENT
1=CHILD
[Colors]
First=17,128,240
[PARENT]
File.LoadScreen=parent.shp
File.LoadScreenPAL=parent.pal
LoadScreenText.Name=Name:Parent
LoadScreenText.Color=First
[CHILD]
ParentCountry=PARENT
File.LoadScreen=child.shp
File.LoadScreenPAL=child.pal
LoadScreenText.Name=Name:Child
LoadScreenText.Color=First
";

fn owner(root: &str) -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(root),
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::default()),
        Arc::new(EvaRegistry::default()),
    )
    .unwrap()
}

fn enter_shell(owner: &mut NativeRulesProcessOwner) {
    owner.read_multiplayer_country(&IniFile::empty(), 1);
    owner.refresh_multiplayer_country_bodies();
}

fn child(owner: &NativeRulesProcessOwner) -> &crate::rules::country_loading::AresCountryLoading {
    owner.country_loading(CountryIdx(1)).unwrap().0
}

#[test]
fn ares_country_loading_prefix_exposes_both_primary_passes_before_type_reset() {
    let mut owner = owner(ROOT);
    enter_shell(&mut owner);
    let early = child(&owner).clone();
    assert_eq!(early.files(), ("parent.shp", "parent.pal"));
    assert_eq!(early.labels()[0], "Name:Parent");
    assert_eq!(
        owner.country_loading_text_hsv(CountryIdx(1)),
        Some([17, 128, 240])
    );

    let preparation = owner.prepare_noncampaign_scenario_prefix();
    assert_eq!(child(&owner).files(), ("child.shp", "child.pal"));
    assert_eq!(child(&owner).labels()[0], "Name:Child");
    assert_eq!(early.files().1, "parent.pal", "early PAL stays immutable");
    assert!(owner.saved_scenario_rules().is_none());

    let loaded = owner
        .load_scenario(
            NativeScenarioRulesPrefix::PreparedNonCampaign {
                preparation,
                mode_rules_override: None,
            },
            &IniFile::empty(),
        )
        .unwrap();
    assert_eq!(loaded.into_parts().3.pre_reset().event_count(), 0);
    // Destructive reset creates fresh Countries; its first root body can copy
    // the parent again. A late final RuleSet/registry is not Draw's authority.
    assert_eq!(child(&owner).files(), ("parent.shp", "parent.pal"));
}

#[test]
fn ares_country_loading_prefix_consumes_original_constructor_receipt_without_rereading() {
    let root = format!("{ROOT}[General]\nAmerParaDropInf=BEFORE\n");
    let mut owner = owner(&root);
    enter_shell(&mut owner);
    let preparation = owner.prepare_noncampaign_scenario_prefix();

    // Test-only source mutation makes an accidental second E_multi visible in
    // pre-reset constructor chronology. Production sources are frozen private
    // snapshots; no runtime setter is introduced for this observation seam.
    owner.sources.selected_rules_root =
        IniFile::from_str(&format!("{ROOT}[General]\nAmerParaDropInf=AFTER\n"));
    let loaded = owner
        .load_scenario(
            NativeScenarioRulesPrefix::PreparedNonCampaign {
                preparation,
                mode_rules_override: None,
            },
            &IniFile::empty(),
        )
        .unwrap();
    let (_, _, _, receipt) = loaded.into_parts();
    let pre_ids: Vec<_> = receipt
        .pre_reset()
        .events
        .iter()
        .map(NativeTypeConstructionEvent::native_stored_id)
        .collect();
    assert!(pre_ids.contains(&"BEFORE"));
    assert!(
        !pre_ids.contains(&"AFTER"),
        "prepared E_multi cannot run again"
    );
    assert!(
        receipt
            .post_reset()
            .events
            .iter()
            .any(|event| event.native_stored_id() == "AFTER"),
        "the ordinary reset still reads the current selected source"
    );
}

#[test]
fn ares_country_loading_prefix_cancel_keeps_registry_and_rejects_stale_or_implicit_reuse() {
    let mut owner = owner(ROOT);
    let initial = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::from_str("[GlobalControls]\nAllowBypassBuildLimit=yes,yes,yes\n"),
        )
        .unwrap()
        .into_parts()
        .0;
    assert!((0..3).all(|level| initial.ares_ai_bypasses_build_limit(level)));
    enter_shell(&mut owner);
    let preparation = owner.prepare_noncampaign_scenario_prefix();
    // Retain an identity only inside this private test to exercise a replay
    // that public callers cannot construct: the public ticket is not Clone.
    let retired_identity = Arc::clone(&preparation.identity);
    assert!(
        catch_unwind(AssertUnwindSafe(
            || owner.refresh_multiplayer_country_bodies()
        ))
        .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = owner.load_scenario(
                NativeScenarioRulesPrefix::NonCampaign(None),
                &IniFile::empty(),
            );
        }))
        .is_err()
    );
    assert_eq!(child(&owner).files().0, "child.shp");
    owner.cancel_prepared_noncampaign_scenario_prefix(preparation);
    assert_eq!(
        child(&owner).files().0,
        "child.shp",
        "cancel cannot rewind parent state"
    );

    let next = owner.prepare_noncampaign_scenario_prefix();
    let wrong_owner = NativeNonCampaignPrefixPreparation {
        identity: Arc::new(()),
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            owner.cancel_prepared_noncampaign_scenario_prefix(wrong_owner);
        }))
        .is_err()
    );
    let stale = NativeNonCampaignPrefixPreparation {
        identity: retired_identity,
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            owner.cancel_prepared_noncampaign_scenario_prefix(stale);
        }))
        .is_err()
    );
    let loaded = owner
        .load_scenario(
            NativeScenarioRulesPrefix::PreparedNonCampaign {
                preparation: next,
                mode_rules_override: None,
            },
            &IniFile::empty(),
        )
        .unwrap();
    let (rules, _, _, receipt) = loaded.into_parts();
    assert_eq!(receipt.pre_reset().event_count(), 0);
    assert!(
        (0..3).all(|level| !rules.ares_ai_bypasses_build_limit(level)),
        "ordinary preparation still clears the outgoing RulesExt controls"
    );

    let reused_after_consume = NativeNonCampaignPrefixPreparation {
        identity: Arc::new(()),
    };
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            owner.cancel_prepared_noncampaign_scenario_prefix(reused_after_consume);
        }))
        .is_err()
    );
    assert!(
        owner.saved_scenario_rules().is_some(),
        "rejection preserves completed owner"
    );
}
