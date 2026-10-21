//! Supplied healthy flat base and constructor Super registry; retail layered
//! rules, normal charge completion, building sale/power sampling, sidebar,
//! gadget, live selection admission and Nuke command execution are production.
use super::*;
use crate::rules::object_type::ObjectCategory;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, SimFrameOutput, SimSoundEvent, TickLane};

fn frame(sim: &mut Simulation, rules: &RuleSet) -> SimFrameOutput {
    let due = sim.take_due_commands();
    sim.advance_app_frame(
        &due,
        rules,
        None,
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .expect("ordinary authoritative frame")
}

fn command(sim: &mut Simulation, owner: InternedId, payload: Command) {
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session().tick() + 1,
        payload,
    ));
}

fn click(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    item: &SidebarItem,
    input: &mut MatchInputState,
    gadgets: &mut SidebarGadgetState,
    right: bool,
) -> SuperWeaponClick {
    let SidebarAction::SuperWeaponPress { section, right } = gadget_press(item, right) else {
        panic!("real Super gadget must forward the press");
    };
    let views = superweapon_views_for_owner(sim, rules, &owner);
    let live = views.iter().find(|view| view.display_name == section);
    press(
        input,
        gadgets,
        live,
        rules
            .super_weapon(&section)
            .and_then(|sw| sw.action.as_deref()),
        right,
    )
}

#[test]
fn retail_supers_recheck_live_admission_after_power_loss_and_recovery() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut rules = retail.rules;
    // This headless scene has no SHP loader. A finite supplied sale control
    // lets the ordinary Sell mission finish; native animation timing is not
    // under comparison, and no retail rules field is overridden.
    rules.set_buildup_control_for_test("NANRCT", [0, 3, 1]);
    rules.set_buildup_control_for_test("NAMISL", [0, 3, 1]);
    let mut covered = Vec::new();
    for name in &rules.super_weapon_order {
        let sw = rules.super_weapon(name).unwrap();
        assert!(
            !sw.use_charge_drain,
            "stock charge-drain assumption: {name}"
        );
        let Some(provider) = rules
            .all_objects()
            .filter(|object| {
                object.category == ObjectCategory::Building
                    && [
                        object.super_weapon.as_deref(),
                        object.super_weapon2.as_deref(),
                    ]
                    .contains(&Some(name.as_str()))
            })
            .min_by_key(|object| &object.id)
        else {
            continue;
        };
        let mut sim = Simulation::with_seed(0x6CC3_6000);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        sim.set_input_delay_ticks(0);
        let owner = sim.intern("Americans");
        let id = sim.intern(name);
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, Some(owner), true, 0, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        sim.fog_mut_for_test().reveal_all_for_owner(owner);
        // Supplied House-constructor registry, before any grant or charge.
        for entry in &rules.super_weapon_order {
            let type_id = sim.intern(entry);
            sim.super_weapons_mut_for_test()
                .entry(owner)
                .or_default()
                .insert(type_id, SuperWeaponInstance::new(type_id, owner, 0));
        }
        let reactor = sim
            .spawn_object("NANRCT", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let provider_id = sim
            .spawn_object(&provider.id, "Americans", 15, 15, 0, &rules)
            .unwrap();
        // Even an airport with zero Power must experience an actual power
        // deficit after reactor sale. The stock barracks supplies that drain.
        sim.spawn_object("GAPILE", "Americans", 24, 5, 0, &rules)
            .unwrap();
        for _ in 0..8 {
            frame(&mut sim, &rules);
        }
        assert!(
            sim.super_weapons()[&owner][&id].is_active(),
            "{name}: provider grant"
        );
        assert!(
            !sim.power_states()[&owner].is_low_power(),
            "{name}: initial power"
        );
        assert!(
            !sim.super_weapons()[&owner][&id].is_ready(),
            "{name}: charging"
        );
        let mut input = MatchInputState::new(Default::default());
        let mut gadgets = SidebarGadgetState::default();
        input.begin_building_placement("GAPILE", 0, owner);
        gadgets.repair_mode_on = true;
        let charging_item = sidebar_item(&sim, &rules, owner, name);
        assert_eq!(
            click(
                &sim,
                &rules,
                owner,
                &charging_item,
                &mut input,
                &mut gadgets,
                false
            ),
            SuperWeaponClick::Refused,
            "{name}: charging click"
        );
        assert!(
            input
                .targeting_mode
                .as_ref()
                .unwrap()
                .as_building_placement()
                .is_some()
        );
        assert!(gadgets.repair_mode_on);
        // Advance the supplied frame clock past the real recharge deadline;
        // the production Super AI, rather than a fixture flag, makes it ready.
        let instance = &sim.super_weapons()[&owner][&id];
        let ready_frame = (instance.charge_start_tick() + instance.charge_duration() + 1) as u32;
        sim.session_mut_for_test()
            .set_binary_frame_for_test(ready_frame);
        frame(&mut sim, &rules);
        assert!(
            sim.super_weapons()[&owner][&id].is_ready(),
            "{name}: real ready transition"
        );
        let cached_online_item = sidebar_item(&sim, &rules, owner, name);
        assert!(cached_online_item.enabled && cached_online_item.is_ready);
        command(
            &mut sim,
            owner,
            Command::SellBuilding { entity_id: reactor },
        );
        for _ in 0..64 {
            frame(&mut sim, &rules);
            if sim.power_states()[&owner].is_low_power() {
                break;
            }
        }
        assert!(
            sim.power_states()[&owner].is_low_power(),
            "{name}: actual reactor loss"
        );
        let instance = &sim.super_weapons()[&owner][&id];
        assert!(instance.is_ready(), "{name}: hold retains charge");
        assert_eq!(
            instance.is_suspended(),
            sw.is_powered,
            "{name}: retail IsPowered"
        );
        let offline_item = sidebar_item(&sim, &rules, owner, name);
        assert!(offline_item.is_ready);
        assert_eq!(
            offline_item.enabled, !sw.is_powered,
            "{name}: draw availability"
        );
        for item in [&cached_online_item, &offline_item] {
            let outcome = click(&sim, &rules, owner, item, &mut input, &mut gadgets, false);
            if sw.is_powered {
                assert_eq!(
                    outcome,
                    SuperWeaponClick::Refused,
                    "{name}: live held click"
                );
                assert!(
                    input
                        .targeting_mode
                        .as_ref()
                        .unwrap()
                        .as_building_placement()
                        .is_some()
                );
                assert!(gadgets.repair_mode_on);
            } else {
                assert_eq!(
                    outcome,
                    SuperWeaponClick::Target,
                    "{name}: unpowered control"
                );
            }
        }
        if name == "NukeSpecial" {
            // A forced tactical command still meets the separate ClickFire
            // guard; sidebar admission never weakens execution admission.
            command(
                &mut sim,
                owner,
                Command::LaunchSuperWeapon {
                    sw_type_id: id,
                    target_rx: 22,
                    target_ry: 22,
                },
            );
            let output = frame(&mut sim, &rules);
            assert!(!output.sound_events.iter().any(|event| matches!(event,
                SimSoundEvent::SuperWeaponLaunched { sw_type, .. } if *sw_type == id)));
            assert!(sim.super_weapons()[&owner][&id].is_ready());
        }
        sim.spawn_object("NANRCT", "Americans", 5, 20, 0, &rules)
            .unwrap();
        for _ in 0..8 {
            frame(&mut sim, &rules);
        }
        assert!(
            !sim.power_states()[&owner].is_low_power(),
            "{name}: restored power"
        );
        assert!(sim.super_weapons()[&owner][&id].is_ready());
        assert!(!sim.super_weapons()[&owner][&id].is_suspended());
        let restored_item = sidebar_item(&sim, &rules, owner, name);
        assert_eq!(
            click(
                &sim,
                &rules,
                owner,
                &restored_item,
                &mut input,
                &mut gadgets,
                false
            ),
            SuperWeaponClick::Target,
            "{name}: restored click"
        );
        assert_eq!(
            input.targeting_mode.as_ref().unwrap().as_super_weapon(),
            Some(name.as_str())
        );
        assert!(!gadgets.repair_mode_on);
        assert_eq!(
            click(
                &sim,
                &rules,
                owner,
                &offline_item,
                &mut input,
                &mut gadgets,
                true
            ),
            SuperWeaponClick::None
        );
        assert!(
            input.targeting_mode.is_none(),
            "{name}: stale/offline right-click cancels"
        );
        if name == "NukeSpecial" {
            command(
                &mut sim,
                owner,
                Command::LaunchSuperWeapon {
                    sw_type_id: id,
                    target_rx: 22,
                    target_ry: 22,
                },
            );
            let output = frame(&mut sim, &rules);
            assert!(output.sound_events.iter().any(|event| matches!(event,
                SimSoundEvent::SuperWeaponLaunched { sw_type, .. } if *sw_type == id)));
            assert!(!sim.super_weapons()[&owner][&id].is_ready());
            command(
                &mut sim,
                owner,
                Command::SellBuilding {
                    entity_id: provider_id,
                },
            );
            for _ in 0..64 {
                frame(&mut sim, &rules);
                if !sim.super_weapons()[&owner][&id].is_active() {
                    break;
                }
            }
            assert!(
                !sim.super_weapons()[&owner][&id].is_active(),
                "silo loss revokes Super"
            );
            assert_eq!(
                click(
                    &sim,
                    &rules,
                    owner,
                    &cached_online_item,
                    &mut input,
                    &mut gadgets,
                    false
                ),
                SuperWeaponClick::None,
                "stale ready cameo after provider loss"
            );
            assert!(input.targeting_mode.is_none());
        }
        covered.push((name.as_str(), sw.is_powered));
    }
    assert!(covered.contains(&("NukeSpecial", true)), "{covered:?}");
    assert!(
        covered.iter().any(|(_, powered)| !powered),
        "unpowered controls: {covered:?}"
    );
    eprintln!("retail Super click/power controls: {covered:?}");
}
