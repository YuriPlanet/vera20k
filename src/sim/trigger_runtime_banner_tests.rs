//! Phobos banner actions 800/801/802 over the live action dispatch.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::meteor_tests::formed_sim;

/// Phobos banner actions 800/801/802 (`Ext/TAction/Body.cpp:813-875`): the
/// action's Text finds the `[BannerTypes]` entry, Param3 ids the banner,
/// Param4/Param5 position it as view percents, Param6 names the variable,
/// 801 marks that variable global; 802 deletes the first banner with the
/// action's +90 value.
#[test]
fn banner_actions_create_replace_and_delete_by_id() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str(
            "[BannerTypes]\n0=INTRO\n\n[INTRO]\nCSF=Name:Title\n",
        ),
        &crate::rules::ini_parser::IniFile::empty(),
    )
    .unwrap();
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        "[Actions]\n\
         CUE=1,800,4,INTRO,7,50,40,3\n\
         REP=1,801,4,INTRO,7,60,20,5\n\
         DEL=1,802,0,7,0,0,0,0\n",
    ));
    let resolve = |name| {
        RuntimeDefinitions::default().resolve_action(
            actions[name].entries()[0].clone(),
            None,
            None,
            None,
        )
    };
    let mut sim = formed_sim(31, 2);
    let mut effects = Vec::new();
    let ctx = TriggerActionContext {
        live_index: None,
        incoming_house: None,
        input: TagEventInput::default(),
    };

    assert!(sim.apply_live_trigger_action(
        &resolve("CUE"),
        ctx,
        &mut effects,
        &rules,
        None,
        crate::sim::world::FrameEffects::empty()
    ));
    assert_eq!(
        sim.session.banners(),
        vec![crate::sim::banner::RuntimeBanner {
            id: 7,
            type_name: "INTRO".to_owned(),
            position_percent: [50, 40],
            variable: 3,
            global_variable: false,
        }],
        "action 800 appends the banner from its params"
    );

    assert!(sim.apply_live_trigger_action(
        &resolve("REP"),
        ctx,
        &mut effects,
        &rules,
        None,
        crate::sim::world::FrameEffects::empty()
    ));
    assert_eq!(
        sim.session.banners().len(),
        1,
        "the id replaces, not appends"
    );
    let banner = &sim.session.banners()[0];
    assert_eq!(banner.position_percent, [60, 20]);
    assert_eq!(banner.variable, 5);
    assert!(
        banner.global_variable,
        "action 801 marks the variable global"
    );

    assert!(sim.apply_live_trigger_action(
        &resolve("DEL"),
        ctx,
        &mut effects,
        &rules,
        None,
        crate::sim::world::FrameEffects::empty()
    ));
    assert!(sim.session.banners().is_empty(), "802 erases by id");
}

/// Phobos actions 501/502/504 mutate the extended variables' `Value`
/// (`Ext/TAction/Body.cpp:135-260`) and 503 prints it (`:212-223`); every
/// arm requires an existing entry — locals authored in the map, globals in
/// rules `[VariableNames]` (`Ext/Scenario/Body.cpp:47-61`).
#[test]
fn variable_value_actions_edit_roll_combine_and_print() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::rules::ini_parser::IniFile::from_str("[General]\nFixtureOnly=1\n"),
        &crate::rules::ini_parser::IniFile::empty(),
    )
    .unwrap();
    let actions = crate::map::actions::parse_actions(&crate::rules::ini_parser::IniFile::from_str(
        "[Actions]\n\
         ADD=1,501,0,2,1,5,1,0\n\
         SET=1,501,0,3,0,6,0,0\n\
         SHL=1,501,0,3,6,2,0,0\n\
         MISS=1,501,0,9,7,2,0,0\n\
         ROLL=1,502,0,3,10,20,0,0\n\
         COPY=1,504,0,2,0,3,1,0\n\
         SHOW=1,503,0,3,0,0,0,0\n",
    ));
    let resolve = |name| {
        RuntimeDefinitions::default().resolve_action(
            actions[name].entries()[0].clone(),
            None,
            None,
            None,
        )
    };
    let (add, set, shl, miss, roll, copy, show) = (
        resolve("ADD"),
        resolve("SET"),
        resolve("SHL"),
        resolve("MISS"),
        resolve("ROLL"),
        resolve("COPY"),
        resolve("SHOW"),
    );
    let mut sim = formed_sim(31, 2);
    sim.trigger_runtime.locals_values.insert(3, 10);
    sim.trigger_runtime.globals_values.insert(2, 7);
    let ctx = TriggerActionContext {
        live_index: None,
        incoming_house: None,
        input: TagEventInput::default(),
    };
    let mut effects = Vec::new();
    let run = |sim: &mut Simulation, action: &RuntimeAction, effects: &mut Vec<TriggerEffect>| {
        sim.apply_live_trigger_action(
            action,
            ctx,
            effects,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        )
    };

    assert!(run(&mut sim, &add, &mut effects));
    assert_eq!(
        sim.trigger_runtime.globals_values[&2], 12,
        "501 adds the operand to the global's value"
    );
    assert!(run(&mut sim, &set, &mut effects));
    assert_eq!(sim.trigger_runtime.locals_values[&3], 6, "op 0 assigns");
    assert!(run(&mut sim, &shl, &mut effects));
    assert_eq!(
        sim.trigger_runtime.locals_values[&3], 24,
        "op 6 shifts left by the masked count"
    );
    assert!(run(&mut sim, &miss, &mut effects));
    assert_eq!(
        sim.trigger_runtime.locals_values.get(&9),
        None,
        "an unauthored entry never springs into existence"
    );
    assert!(run(&mut sim, &roll, &mut effects));
    let rolled = sim.trigger_runtime.locals_values[&3];
    assert!((10..=20).contains(&rolled), "502 rolls inside the range");

    assert!(run(&mut sim, &copy, &mut effects));
    assert_eq!(
        sim.trigger_runtime.globals_values[&2], rolled,
        "504 assigns the source entry through op 0"
    );
    assert!(run(&mut sim, &show, &mut effects));
    assert_eq!(
        effects.last(),
        Some(&TriggerEffect::VariableValuePrinted {
            text: rolled.to_string(),
        }),
        "503 prints the value L\"%d\""
    );
}
