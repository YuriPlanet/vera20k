//! `[AIPersonalities]` reading: registry order, `Country=` binding, trigger
//! overrides, feedback entries, and the three properties the sim relies on —
//! assignment is opt-in, an absent section changes nothing, and an explicitly
//! named commander is never silently replaced.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::RulesLayerStack;
use crate::rules::ruleset::RuleSet;

fn processed(ini: &str) -> RuleSet {
    let processed = RulesLayerStack::new(IniFile::from_str(ini))
        .process()
        .expect("personality rules parse");
    RuleSet::from_processed_rules(&processed).expect("personality projection")
}

/// Two commanders, one with a country list, a description and both strategy
/// knobs. Everything the AI layer reads comes out of the same section.
#[test]
fn personalities_parse_country_overrides_and_entries() {
    let rules = processed(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Sides]\nAllied=Americans\nSoviet=Russians\n\
         [AIPersonalities]\n0=Warren\n1=Oleg\n\
         [Warren]\n\
         Country=Americans, British\n\
         DisplayName=Personality:Warren\n\
         DisplayDesc=Personality:Warren:Desc\n\
         TriggerDeny=0CA2F7C0-G,0ca2f7c1-g\n\
         TriggerWeight.0CA2F7C0-G=150\n\
         TriggerRatio=80\n\
         TeamDelayPercent=130\n\
         Entry0.Key=StartGame\nEntry0.Video=AWAGR1\nEntry0.To=ALLIES\n\
         Entry2.Key=FirstSeen\nEntry2.To=ENEMIES\nEntry2.Timeout=9999999\n\
         [Oleg]\nCountry=Russians\n",
    );
    let catalog = rules.personalities();
    assert_eq!(catalog.len(), 2);

    let warren = catalog.get(0).expect("Warren is the first registration");
    assert_eq!(warren.name, "Warren");
    let countries = warren.countries.as_ref().expect("Country= is read");
    assert!(countries.contains("AMERICANS"));
    assert!(countries.contains("BRITISH"), "the list is split on spaces");
    assert_eq!(
        warren.display_desc.as_deref(),
        Some("Personality:Warren:Desc"),
        "the selector's description is a reserved slot, not simulation data"
    );
    assert!(warren.denies("0ca2f7c0-g"));
    assert!(
        warren.denies("0CA2F7C1-G"),
        "trigger IDs match any case, written either way"
    );
    assert!(!warren.denies("0CA2F7C2-G"));
    assert_eq!(warren.weight_percent("0ca2f7c0-g"), Some(150));
    assert_eq!(warren.weight_percent("0CA2F7C2-G"), None);
    assert_eq!(warren.trigger_ratio, 80);
    assert_eq!(warren.team_delay_percent, 130);

    // Entries keep their declared order even though Entry1 is missing.
    let events: Vec<FeedbackEvent> = warren.feedback.iter().map(|e| e.event).collect();
    assert_eq!(
        events,
        vec![FeedbackEvent::StartGame, FeedbackEvent::FirstSeen]
    );
    let start = &warren.feedback[0];
    assert_eq!(start.video.as_deref(), Some("AWAGR1"));
    assert!(start.audience.contains(FeedbackAudience::ALLIES));
    assert!(!start.audience.contains(FeedbackAudience::ENEMIES));
    assert_eq!(start.timeout_seconds, None);
    assert_eq!(warren.feedback[1].timeout_seconds, Some(9_999_999));

    let oleg = catalog.get(1).expect("Oleg is the second registration");
    assert_eq!(
        oleg.countries.as_ref().map(|set| set.len()),
        Some(1),
        "a one-country list stays one country"
    );
    assert!(oleg.deny.is_empty());
    assert!(
        oleg.feedback.is_empty(),
        "a personality may be numbers only"
    );

    // Eligibility is a per-country question: the two lists do not overlap.
    assert_eq!(catalog.candidates_for("Americans"), vec![0]);
    assert_eq!(catalog.candidates_for("British"), vec![0]);
    assert!(
        catalog.candidates_for("French").is_empty(),
        "a country the list omits gets nothing"
    );
    assert_eq!(catalog.candidates_for("Russians"), vec![1]);
    assert!(
        catalog.candidates_for("French").is_empty(),
        "a Country= miss is a miss: no other country's list can widen it"
    );
}

/// `TriggerWeight.` is the only prefixed key, and its prefix is matched whole,
/// in any case. A stray `TRIGGERWEIGHT.` used to be dropped without a word.
#[test]
fn trigger_weight_prefix_is_case_insensitive_but_not_a_substring() {
    let rules = processed(
        "[Countries]\n0=Americans\n\
         [AIPersonalities]\n0=W\n\
         [W]\n\
         TRIGGERWEIGHT.ABCD=40\n\
         TriggerWeight.=50\n\
         TriggerWeighted.ABCD=60\n\
         triggerweight.EFGH=70\n",
    );
    let w = rules.personalities().get(0).expect("W is registered");
    assert_eq!(w.weight_percent("ABCD"), Some(40));
    assert_eq!(w.weight_percent("EFGH"), Some(70));
    assert_eq!(
        w.trigger_weight_percent.len(),
        2,
        "an empty ID and a longer key are not trigger weights"
    );
}

/// An absent `[AIPersonalities]` leaves the feature inert: no candidates, no
/// draw, and every consumer falls back to the native value.
#[test]
fn absent_registry_leaves_the_feature_inert() {
    let rules = processed("[Countries]\n0=Americans\n[Sides]\nAllied=Americans\n");
    let catalog = rules.personalities();
    assert!(catalog.is_empty());
    assert_eq!(catalog.draw_for("Americans", 0), None);
    assert_eq!(catalog.candidates_for("Americans"), Vec::<u16>::new());
}

/// The random button is a pure function of its key: two clients, or a replay,
/// must not disagree about which commander a house got.
#[test]
fn the_random_draw_is_deterministic_and_stays_inside_the_country_list() {
    let rules = processed(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Sides]\nAllied=Americans\nSoviet=Russians\n\
         [AIPersonalities]\n0=A\n1=B\n2=C\n3=D\n\
         [A]\nCountry=Americans\n[B]\nCountry=Americans\n[C]\nCountry=Americans\n\
         [D]\nDisplayName=Personality:D\n",
    );
    let catalog = rules.personalities();
    assert_eq!(
        catalog.candidates_for("Americans"),
        vec![0, 1, 2, 3],
        "a personality without Country= joins every country's pool"
    );
    assert_eq!(
        catalog.candidates_for("Russians"),
        vec![3],
        "only the unbound one is left for the other country"
    );

    for key in [0_u64, 1, 7, 0xDEAD_BEEF] {
        assert_eq!(
            catalog.draw_for("Americans", key),
            catalog.draw_for("Americans", key),
            "the same key must always draw the same commander"
        );
    }
    // Distinct keys do reach distinct registrations; if they never did, the
    // draw would be a constant rather than a choice.
    let drawn: Vec<u16> = (0..16)
        .filter_map(|key| catalog.draw_for("Americans", key))
        .collect();
    assert!(drawn.iter().all(|&index| index < 4));
    assert!(drawn.iter().any(|&index| index != drawn[0]));

    // Percent scaling: 100 is the identity the empty case relies on.
    assert_eq!(scale_weight(37, NEUTRAL_WEIGHT_PERCENT), 37);
    assert_eq!(scale_weight(100, 150), 150);
    assert_eq!(scale_weight(3, 50), 1);
    assert_eq!(scale_weight(200, 0), 0);

    // Team pacing: only the difficulty row is scaled, never below one frame.
    assert_eq!(scale_team_delay(300, NEUTRAL_WEIGHT_PERCENT), 300);
    assert_eq!(scale_team_delay(300, 130), 390);
    assert_eq!(scale_team_delay(300, 50), 150);
    assert_eq!(
        scale_team_delay(1, 1),
        1,
        "a zero-frame restart never happens"
    );
}

/// The decision the AI selector asks per trigger: keep the native weight,
/// reweight it, or drop the trigger. A personality that names nothing must
/// answer "keep" for every trigger — that is what makes it inert.
#[test]
fn trigger_decisions_keep_reweight_or_drop() {
    let personality = PersonalityDef {
        name: "Test".into(),
        deny: ["AT_NAVY".to_owned()].into_iter().collect(),
        trigger_weight_percent: [
            ("AT_TANK".to_owned(), 150_u16),
            ("AT_AIR".to_owned(), 50_u16),
        ]
        .into_iter()
        .collect(),
        ..PersonalityDef::default()
    };
    const SENTINEL: i32 = 5000;

    assert_eq!(personality.effective_weight("AT_Navy", 80, SENTINEL), None);
    assert_eq!(
        personality.effective_weight("AT_Tank", 80, SENTINEL),
        Some(120)
    );
    assert_eq!(
        personality.effective_weight("at_air", 80, SENTINEL),
        Some(40)
    );
    assert_eq!(
        personality.effective_weight("AT_Unnamed", 80, SENTINEL),
        Some(80),
        "an unnamed trigger keeps its own weight"
    );
    assert_eq!(
        personality.effective_weight("AT_Tank", SENTINEL, SENTINEL),
        Some(SENTINEL),
        "the priority sentinel is precedence, not magnitude"
    );

    let inert = PersonalityDef {
        name: "Inert".into(),
        ..PersonalityDef::default()
    };
    assert_eq!(inert.effective_weight("AT_Tank", 80, SENTINEL), Some(80));
    assert_eq!(
        inert.trigger_ratio, NATIVE_TRIGGER_RATIO,
        "an unwritten TriggerRatio= is the native 100, not a tilt"
    );
    assert_eq!(inert.team_delay_percent, NEUTRAL_WEIGHT_PERCENT);
}

/// Assignment is opt-in, and a name that was asked for is never silently
/// replaced: a misspelling leaves the house with no commander instead of
/// handing it someone else's voice.
#[test]
fn assignment_is_opt_in_and_an_explicit_name_wins() {
    let rules = processed(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Sides]\nAllied=Americans\nSoviet=Russians\n\
         [AIPersonalities]\n0=Warren\n1=Oleg\n\
         [Warren]\nCountry=Americans\n[Oleg]\nCountry=Russians\n",
    );
    let catalog = rules.personalities();
    let standard = catalog.standard().expect("Standard is always registered");

    assert_eq!(
        personality_for_house(catalog, "Americans", None),
        None,
        "a launch that asks for nobody gets nobody: the AI stays native"
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("Warren")),
        Some(0)
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("Standard")),
        Some(standard),
        "any case, by name"
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("standard")),
        Some(standard)
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("none")),
        None,
        "a house with no commander at all"
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("off")),
        None
    );
    assert_eq!(
        personality_for_house(catalog, "Americans", Some("Nobody")),
        None,
        "an unknown name is a data error, not a reason to pick someone else"
    );
    assert_eq!(
        personality_for_house(catalog, "Russians", Some("Warren")),
        Some(0),
        "an explicit request outranks the Country= filter, with a warning"
    );
}

/// `Side=` is gone: it restated what a `[Countries]` key already says, and two
/// filters for one question invite data that disagrees with itself. A section
/// that still writes it is not silently reinterpreted — the key is ignored, so
/// the personality reaches every country, and the loader warns once.
#[test]
fn the_removed_side_binding_is_ignored() {
    let rules = processed(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Sides]\nAllied=Americans\nSoviet=Russians\n\
         [AIPersonalities]\n0=Old\n1=New\n\
         [Old]\nSide=Soviet\n[New]\nCountry=Russians\n",
    );
    let catalog = rules.personalities();
    assert_eq!(
        catalog.candidates_for("Americans"),
        vec![0],
        "Side= limits nothing any more, so the section reaches both countries"
    );
    assert_eq!(catalog.candidates_for("Russians"), vec![0, 1]);
    assert!(
        catalog.get(0).is_some_and(|old| old.countries.is_none()),
        "the removed key leaves no eligibility behind"
    );
}

/// Every catalog carries one reserved personality: no country, no trigger deny,
/// no weights, no clips — a house given it behaves exactly as retail's AI does
/// and never shows a talking head. It is never offered by the draw, only named.
#[test]
fn the_reserved_personality_is_never_drawn_but_can_be_named() {
    let rules = processed(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Sides]\nAllied=Americans\nSoviet=Russians\n\
         [AIPersonalities]\n0=Warren\n1=Oleg\n\
         [Warren]\nCountry=Americans\n[Oleg]\nCountry=Russians\n",
    );
    let catalog = rules.personalities();
    assert_eq!(catalog.len(), 2, "the reserved entry is not a personality");
    let standard = catalog.standard().expect("Standard is always registered");
    let definition = catalog.get(standard).expect("and resolvable");
    assert!(definition.reserved);
    assert!(definition.feedback.is_empty(), "it has no clips");
    assert!(definition.deny.is_empty(), "and no strategy tilt");
    assert!(definition.trigger_weight_percent.is_empty());
    assert_eq!(
        catalog.candidates_for("Americans"),
        vec![0],
        "the draw never offers the reserved entry"
    );
    assert_eq!(catalog.draw_for("Americans", 7), Some(0));
}

/// `[CommanderFeedback]`: on by default, but no house gets a commander until
/// something names one — and `Assign.` is the per-slot seam a test uses.
#[test]
fn commander_feedback_defaults_and_rules_overrides() {
    let stock = processed("[Countries]\n0=Americans\n[Sides]\nAllied=Americans\n");
    let rules = stock.commander_feedback();
    assert!(rules.enabled);
    assert!(
        rules.personalities,
        "the feature is available by default; it is assignment that is opt-in"
    );
    assert_eq!(rules.trouble_buildings_lost, 1, "one building is enough");
    assert_eq!(rules.trouble_quiet_seconds, 180);
    assert!(rules.assign.is_empty(), "nobody is named by default");

    let off = processed(
        "[Countries]\n0=Americans\n[Sides]\nAllied=Americans\n\
         [CommanderFeedback]\nEnabled=no\nTroubleBuildingsLost=4\nTroubleQuiet=30\n",
    )
    .commander_feedback();
    assert!(!off.enabled);
    assert!(
        off.personalities,
        "clips off, strategy tilt kept: the two switches are independent"
    );
    assert_eq!(off.trouble_buildings_lost, 4);
    assert_eq!(off.trouble_quiet_seconds, 30);

    // The way back to retail's own AI: no commander at all, so no
    // `TriggerDeny=` / `TriggerWeight.` tilt applies and nothing can be said.
    let native = processed(
        "[Countries]\n0=Americans\n[Sides]\nAllied=Americans\n\
         [CommanderFeedback]\nPersonalities=no\n",
    )
    .commander_feedback();
    assert!(!native.personalities);
    assert!(native.enabled, "the clip switch is a different switch");

    // `Assign.` names one launch slot, matched without regard to case, and is
    // the only way to test a personality before a selector exists.
    let assigned = processed(
        "[Countries]\n0=Americans\n[Sides]\nAllied=Americans\n\
         [CommanderFeedback]\nAssign.Computer1=Warren\nassign.computer3=Oleg\n",
    )
    .commander_feedback();
    assert_eq!(assigned.assigned("Computer1"), Some("Warren"));
    assert_eq!(assigned.assigned("computer3"), Some("Oleg"));
    assert_eq!(assigned.assigned("Computer2"), None);
}
