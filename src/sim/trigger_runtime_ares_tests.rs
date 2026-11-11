//! Ares 3.0 trigger events and actions (TEventExt::HasOccured 1004F7A0,
//! TActionExt::Execute 100396C0) through the production Tag owners and the
//! raise sites cited beside the code.

use super::tests::{
    event_ingress_damage, event_ingress_fixture_with_rules, install_event_ingress_map,
    spawn_event_ingress_unit,
};
use super::*;
use crate::sim::combat::{KillCallback, KillSource};
use crate::sim::world::UninitContext;

/// One repeating Tag `TAG` holding the Triggers `ids` (owned by `owner`) in
/// that order; `events[i]` is the i-th Trigger's event and it sets local `i`.
fn chained_tag(owner: &str, events: &[&str]) -> String {
    let mut triggers = String::from("[Triggers]\n");
    let mut event_lines = String::from("[Events]\n");
    let mut actions = String::from("[Actions]\n");
    for (index, event) in events.iter().enumerate() {
        let next = if index + 1 < events.len() {
            format!("T{}", index + 1)
        } else {
            "<none>".to_owned()
        };
        triggers.push_str(&format!("T{index}={owner},{next},T{index},0,1,1,1,0\n"));
        event_lines.push_str(&format!("T{index}=1,{event}\n"));
        actions.push_str(&format!("T{index}=1,56,0,{index},0,0,0,0,A\n"));
    }
    format!("{triggers}[Tags]\nTAG=2,Tag,T0\n{event_lines}{actions}")
}

fn locals(sim: &Simulation, count: u32) -> Vec<bool> {
    (0..count)
        .map(|index| sim.trigger_local_is_set_for_test(index))
        .collect()
}

/// A surviving hit raises 84 (by the attacker's House index) and 83 (hook
/// 0x5F57B5); RecordKill's E4 raises 85 with its incoming House (hook
/// 0x7032B0), compared by House index.
#[test]
fn damage_and_kill_events_compare_the_attacking_house() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules("");
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let attacker = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        &chained_tag(
            "Country0",
            &["83,0,0", "84,0,1", "84,0,2", "85,0,1", "85,0,2"],
        ),
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        attacker,
        1,
        FrameEffects::default(),
    );
    assert_eq!(locals(&sim, 5), [true, true, false, false, false]);

    sim.record_the_kill(
        victim,
        KillSource::House(Some(houses[1])),
        KillCallback::OwnerChange,
        UninitContext::new(&rules, Some(&overlays)),
    );
    assert_eq!(locals(&sim, 5), [true, true, false, true, false]);
}

/// TechTypeExists (1004FE60): a significant type counts each House's
/// tracked objects, an Insignificant one its owned objects of the class
/// array. 81/82 ask the Trigger's House, 86 and the replaced 60/61 everyone.
#[test]
fn owned_type_events_count_the_houses_objects() {
    let (mut sim, rules, _overlays, houses) = event_ingress_fixture_with_rules(
        "[BuildingTypes]\n1=TAGPOST\n[TAGPOST]\nStrength=100\nInsignificant=yes\n",
    );
    for (house, cell) in [(0, (12, 12)), (0, (14, 12)), (1, (16, 12))] {
        spawn_event_ingress_unit(&mut sim, &rules, houses[house], cell);
    }
    sim.spawn_object_at_height("TAGPOST", "House0", 18, 18, 0, 0, &rules)
        .unwrap();
    install_event_ingress_map(
        &mut sim,
        &rules,
        &chained_tag(
            "Country0",
            &[
                "81,2,2,tagunit",
                "81,2,3,TAGUNIT",
                "82,2,2,TAGUNIT",
                "82,2,1,TAGUNIT",
                "86,2,3,TAGUNIT",
                "86,2,2,TAGUNIT",
                "81,2,1,TAGPOST",
                "60,2,3,TAGUNIT",
                "61,2,0,TAGPOST",
            ],
        ),
    );
    let tag = sim.trigger_runtime.house_tags[&houses[0]][0];
    let mut effects = Vec::new();
    sim.dispatch_live_tag_event(
        tag,
        TagEventInput {
            kind: EVENT_ANY,
            ..Default::default()
        },
        &mut effects,
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        locals(&sim, 9),
        [true, false, true, false, true, false, true, true, false],
        "names compare without case; 82/86 test one more than the value"
    );
}

/// 87/88 read House `value`'s KeepAlive counts, and a Player@X value names a
/// start slot's House (ResolveHouseParam). The shared Country lookup at
/// 71F069 refuses a Player@X slot without a House (hook 0x71F06C), where the
/// stock body answers true.
#[test]
fn house_value_events_resolve_player_slots_and_keep_alive_counts() {
    let (mut sim, rules, _overlays, houses) = event_ingress_fixture_with_rules(
        "[BuildingTypes]\n1=TAGHQ\n[TAGHQ]\nStrength=1000\n\
         [InfantryTypes]\n2=KEEPER\n[KEEPER]\nStrength=100\nSpeed=4\nSpeedType=Foot\n\
         KeepAlive=yes\nLocomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n",
    );
    sim.spawn_object_at_height("TAGHQ", "House0", 15, 15, 0, 0, &rules)
        .unwrap();
    sim.spawn_object_at_height("KEEPER", "House1", 18, 15, 0, 0, &rules)
        .unwrap();
    sim.session
        .start_slot_houses_mut_for_test()
        .insert(1, houses[1]);
    let player_a = crate::rules::native_processing::HOUSE_TYPE_PLAYER_AT_SELECTORS.start;
    install_event_ingress_map(
        &mut sim,
        &rules,
        &chained_tag(
            "Country0",
            &[
                "88,0,0",
                "88,0,7",
                "87,0,7",
                &format!("88,0,{}", player_a + 1),
                &format!("10,0,{player_a}"),
                &format!("10,0,{}", player_a + 1),
            ],
        ),
    );
    let tag = sim.trigger_runtime.house_tags[&houses[0]][0];
    let mut effects = Vec::new();
    sim.dispatch_live_tag_event(
        tag,
        TagEventInput {
            kind: EVENT_ANY,
            ..Default::default()
        },
        &mut effects,
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        locals(&sim, 6),
        [false, true, false, true, false, true],
        "House1 keeps a KeepAlive Unit but no KeepAlive Building"
    );
    assert_eq!(
        [houses[0], houses[1]].map(|house| sim.houses[&house].tracking().keep_alive_counts()),
        [(1, 1), (1, 0)]
    );
}

/// ClickFire's accepted Launch of a new type raises 77 (named type near a
/// waypoint) and then 75 (type index) on the owner's House Tags
/// (SWTypeExt::Activate 10032020); clicking a draining Battery off raises 76
/// (Deactivate 100325D0).
#[test]
fn superweapon_launch_and_deactivation_spring_the_owner_house_tags() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules(
        "[SuperWeaponTypes]\n0=Cell\n1=Other\n\
         [Cell]\nType=Battery\nRechargeTime=1\nIsPowered=no\nSW.AlwaysGranted=yes\n\
         [Other]\nType=Battery\nRechargeTime=1\nIsPowered=no\n",
    );
    let owner = houses[0];
    let cell = sim.interner.get("Cell").unwrap();
    for house in houses {
        crate::sim::superweapon::construct_house_supers(&mut sim, &rules, house, false);
    }
    sim.session
        .install_waypoints(&std::collections::HashMap::from([
            (
                0,
                crate::map::waypoints::Waypoint {
                    index: 0,
                    rx: 13,
                    ry: 14,
                },
            ),
            (
                1,
                crate::map::waypoints::Waypoint {
                    index: 1,
                    rx: 16,
                    ry: 10,
                },
            ),
        ]));
    install_event_ingress_map(
        &mut sim,
        &rules,
        &chained_tag(
            "Country0",
            &["77,2,0,cell", "77,2,1,Cell", "75,0,0", "75,0,1", "76,0,0"],
        ),
    );
    sim.session.set_binary_frame_for_test(100);
    crate::sim::superweapon::grant_provided_supers(&mut sim, &rules, owner);
    crate::sim::superweapon::update_owned_supers(&mut sim, &rules, owner);
    sim.session.set_binary_frame_for_test(1000);
    crate::sim::superweapon::tick_house_superweapon_instances(&mut sim, &rules, owner);
    let fire = |sim: &mut Simulation| {
        sim.apply_command_with_overlays(
            "House0",
            &crate::sim::command::Command::LaunchSuperWeapon {
                sw_type_id: cell,
                target_rx: 10,
                target_ry: 10,
            },
            &rules,
            Some(&overlays),
            FrameEffects::default(),
        )
    };
    assert!(fire(&mut sim));
    assert_eq!(locals(&sim, 5), [true, false, true, false, false]);
    fire(&mut sim);
    assert_eq!(locals(&sim, 5), [true, false, true, false, true]);
}

/// The abduction raises 72 (by the abductor's House) and 71 on the abductee,
/// then 74 (of the abductee's previous House) and 73 on the abductor
/// (conductAbduction 10054A80).
#[test]
fn an_abduction_raises_its_events_on_both_objects() {
    // The ingress fixture's Houses, with an abductor among its VehicleTypes.
    let mut text = String::from("[Countries]\n");
    for index in 0..8 {
        text.push_str(&format!("{index}=Country{index}\n[Country{index}]\n"));
    }
    text.push_str(
        "[VehicleTypes]\n0=TAGUNIT\n1=CATCHER\n\
         [TAGUNIT]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [CATCHER]\nStrength=400\nSpeed=4\nPassengers=10\nSizeLimit=10\nPrimary=Grab\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         [Grab]\nDamage=30\nRange=3\nProjectile=Invisible\nWarhead=SA\nAbductor=yes\n\
         [Invisible]\nInviso=yes\n",
    );
    let (mut sim, rules, _overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(&text);
    let houses: [_; 3] = std::array::from_fn(|index| {
        let house = sim.interner.intern(&format!("House{index}"));
        let country = sim.interner.intern("Country0");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house,
                index as u8,
                Some(country),
                false,
                0,
                10,
            ));
        house
    });
    let target = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let catcher = sim
        .spawn_object_at_height("CATCHER", "House1", 16, 15, 0, 0, &rules)
        .unwrap();
    install_event_ingress_map(
        &mut sim,
        &rules,
        &chained_tag(
            "Country0",
            &["71,0,0", "72,0,1", "72,0,0", "73,0,0", "74,0,0", "74,0,2"],
        ),
    );
    sim.attach_object_trigger_tag(target, Some("TAG"));
    assert!(sim.ares_conduct_abduction(
        catcher,
        target,
        rules.weapon("Grab").unwrap(),
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert_eq!(
        locals(&sim, 6),
        [true, true, false, false, false, false],
        "the abductor has no Tag"
    );
    sim.attach_object_trigger_tag(catcher, Some("TAG"));
    let other = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (17, 16));
    assert!(sim.ares_conduct_abduction(
        catcher,
        other,
        rules.weapon("Grab").unwrap(),
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert_eq!(
        locals(&sim, 6),
        [true, true, false, true, false, true],
        "74 compares House2, the previous owner"
    );
}

/// Actions 146 (AuxPower of the House Value names, by Value2), 149 (the
/// triggering object's Group), 148 (the announcer voice, for the client)
/// and 147 (kill the drivers of every Foot of this Trigger's Tag, giving
/// them to the House Value names).
#[test]
fn ares_actions_write_power_group_voice_and_drivers() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture_with_rules("");
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let other = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    let untagged = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (19, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nT=Country0,<none>,T,0,1,1,1,0\n[Tags]\nTAG=2,Tag,T\n\
         [Events]\nT=1,83,0,0\n\
         [Actions]\nT=4,146,11,0,0,0,0,0,25,149,0,4,0,0,0,0,A,148,0,5,0,0,0,0,A,\
         147,0,7,0,0,0,0,A\n",
    );
    for id in [actor, other] {
        sim.attach_object_trigger_tag(id, Some("TAG"));
    }
    sim.drain_trigger_effects();
    sim.dispatch_object_tag_event(
        actor,
        83,
        Some(actor),
        None,
        UninitContext::new(&rules, Some(&overlays)),
    );
    sim.assess_power_for_test(&rules);
    assert_eq!(sim.power_states[&houses[0]].total_output(), 25);
    assert_eq!(sim.entities().get(actor).unwrap().object_group(), 4);
    assert_eq!(
        sim.drain_trigger_effects(),
        [TriggerEffect::EvaVoice { index: 5 }]
    );
    let owner = |id| sim.entities().get(id).unwrap().owner();
    assert_eq!(
        [owner(actor), owner(other), owner(untagged)],
        [houses[1], houses[1], houses[0]]
    );
}

/// PlayAudioAtRandomWP's replacement (10039B60, hook 0x6E1780) draws only
/// when some waypoint is defined.
#[test]
fn random_waypoint_sound_without_waypoints_draws_nothing() {
    let (mut sim, rules, _overlays, _houses) = event_ingress_fixture_with_rules("");
    sim.session
        .install_waypoints(&std::collections::HashMap::new());
    let before = sim.scenario_rng.logical_state();
    assert!(sim.trigger_play_sound_at_random_waypoint(None, &rules, FrameEffects::default()));
    assert_eq!(sim.scenario_rng.logical_state(), before);
}
