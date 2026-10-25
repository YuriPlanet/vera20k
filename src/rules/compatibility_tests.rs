use crate::rules::compatibility::RulesCompatibility;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::rules::{ARES_ENABLED, PHOBOS_ENABLED, VERAX_ENABLED};

fn processed() -> ProcessedRulesLayers {
    RulesLayerStack::new(IniFile::from_str(
        "[BuildingTypes]\n0=GATE\n[GATE]\nStrength=100\n\
         [InfantryTypes]\n0=INF\n[INF]\nStrength=100\n\
         [SuperWeaponTypes]\n0=REVEAL\n1=STRIKE\n2=DELIVERY\n\
         [REVEAL]\nType=PsychicReveal\nRechargeTime=2\nAuxBuilding=GATE\n\
         Money.Amount=-500\nSW.NegBuildings=GATE\n\
         [STRIKE]\nType=GenericWarhead\nSW.Warhead=PAYLOAD\nSW.Damage=75\n\
         [DELIVERY]\nType=UnitDelivery\nDeliver.Types=INF\nSW.Deferment=7\n",
    ))
    .process()
    .unwrap()
}

#[test]
fn compiled_capability_controls_real_readers_and_roundtrip() {
    let original = processed();
    let bytes = crate::util::save_codec::serialize(&original).unwrap();
    let restored: ProcessedRulesLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    let reveal = rules.super_weapon("REVEAL").unwrap();
    assert_eq!(reveal.kind, SuperWeaponKind::PsychicReveal);
    assert_eq!(reveal.aux_building.as_deref(), Some("GATE"));
    assert_eq!(reveal.recharge_time_frames, 1800);
    if ARES_ENABLED {
        assert_eq!(reveal.money_amount, -500);
        assert_eq!(
            reveal.building_requirements.as_ref().unwrap().negative,
            ["GATE"]
        );
        let strike = rules.super_weapon("STRIKE").unwrap();
        assert_eq!(strike.generic_warhead.as_ref().unwrap().damage, 75);
        assert!(rules.warhead("PAYLOAD").is_some());
        let delivery = rules.super_weapon("DELIVERY").unwrap();
        assert_eq!(delivery.unit_delivery.as_ref().unwrap().deferment, 7);
        assert_eq!(delivery.unit_delivery.as_ref().unwrap().types.len(), 1);
    } else {
        assert_eq!(reveal.money_amount, 0);
        assert!(reveal.building_requirements.is_none());
        for id in ["STRIKE", "DELIVERY"] {
            let sw = rules.super_weapon(id).unwrap();
            assert_eq!(sw.kind, SuperWeaponKind::NoEffect);
            assert!(sw.generic_warhead.is_none() && sw.unit_delivery.is_none());
            assert_eq!(sw.money_amount, 0);
        }
        assert!(rules.warhead("PAYLOAD").is_none());
    }
    assert_eq!(
        rules.simulation_config_hash(),
        RuleSet::from_processed_rules(&original)
            .unwrap()
            .simulation_config_hash()
    );
}

#[test]
fn retained_rules_reject_foreign_capability_before_projection() {
    let mut wire = serde_json::to_value(processed()).unwrap();
    let mode = &mut wire["native_type_construction_trace"]["registry_state"]["compatibility"];
    assert!(
        mode.is_string(),
        "the wire fixture must reach the retained owner"
    );
    *mode = serde_json::to_value(match RulesCompatibility::default() {
        RulesCompatibility::Retail => RulesCompatibility::Ares,
        _ => RulesCompatibility::Retail,
    })
    .unwrap();
    let restored: ProcessedRulesLayers = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        RuleSet::from_processed_rules(&restored),
        Err(crate::rules::error::RulesError::InvalidSavedState(
            "saved Rules require a different compiled extension capability"
        ))
    ));
}

#[test]
fn stock_rules_hash_records_the_compiled_capability() {
    let rules = RuleSet::from_ini(&IniFile::from_str("[General]\nBuildSpeed=.7\n")).unwrap();
    // Each build configuration's validation receipts compare this actual
    // production hash for identical stock input. No extension tags decide the mode.
    println!(
        "BUILD_CAPABILITY_HASH ares={ARES_ENABLED} phobos={PHOBOS_ENABLED} verax={VERAX_ENABLED} hash={}",
        rules.simulation_config_hash()
    );
}

/// Every capability combination round-trips on the wire and reports exactly the
/// capabilities it was built from, so a build refuses any other combination
/// while the features themselves stay mixable.
#[test]
fn all_capability_combinations_round_trip_and_describe_themselves() {
    // save_codec writes a fixed little-endian u32 variant index. Appending
    // VeraX modes must not reinterpret the existing Ares+Phobos wire tag.
    let legacy = [4, 0, 0, 0];
    assert_eq!(
        crate::util::save_codec::serialize(&RulesCompatibility::AresPhobos).unwrap(),
        legacy
    );
    assert_eq!(
        crate::util::save_codec::deserialize::<RulesCompatibility>(&legacy).unwrap(),
        RulesCompatibility::AresPhobos
    );
    for ares in [false, true] {
        for phobos in [false, true] {
            for npatch in [false, true] {
                for verax in [false, true] {
                    let mode = RulesCompatibility::from_capabilities(ares, phobos, npatch, verax);
                    assert_eq!(mode.has_ares(), ares);
                    assert_eq!(mode.has_phobos(), phobos);
                    assert_eq!(mode.has_npatch(), npatch);
                    assert_eq!(mode.has_verax(), verax);
                    let wire = serde_json::to_value(mode).unwrap();
                    let restored: RulesCompatibility = serde_json::from_value(wire).unwrap();
                    assert_eq!(restored, mode, "{mode:?} must survive the wire");
                }
            }
        }
    }
    assert!(
        RulesCompatibility::default().matches_build(),
        "the compiled build matches its own mode"
    );
    let other = RulesCompatibility::from_capabilities(
        !ARES_ENABLED,
        !PHOBOS_ENABLED,
        !crate::rules::NPATCH_ENABLED,
        !VERAX_ENABLED,
    );
    assert!(
        !other.matches_build(),
        "a different combination must be refused"
    );
}
