//! The physical retail Harrier's existing shot/debit/return/rearm loop feeds
//! the shared display query. Native raster controls separately establish the
//! original filled-only PIPS2 frame and drawing/material/placement behavior.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

fn frames(sim: &Simulation, rules: &RuleSet, id: u64, hover: Option<u64>) -> Vec<usize> {
    let before = sim.state_hash();
    let entity = sim.entities().get(id).expect("Harrier survives");
    let result = super::ui_overlays::scale_pip_draws(
        sim,
        entity,
        hover,
        Some(sim.interner.resolve(entity.owner())),
        rules,
    )
    .into_iter()
    .map(|pip| pip.frame)
    .collect();
    assert_eq!(
        before,
        sim.state_hash(),
        "display cannot spend or reload Ammo"
    );
    result
}

fn native_stage_frames(stage: &str) -> Vec<usize> {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/ammo_pips.json");
    let row = native["raster_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == stage)
        .expect("original Ammo transition control");
    row["draws"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|draw| draw["asset"] == "PIPS2.SHP")
        .map(|draw| draw["frame"].as_u64().unwrap() as usize)
        .collect()
}

#[test]
#[ignore = "physical retail map, rules and art; natural ORCA strike, return and rearm"]
fn stock_harrier_ammo_pips_follow_real_strike_return_reload_and_selection() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let crate::sim::arena_fixture::RetailHarrierDockScene {
        mut runtime,
        player,
        airfield,
        harriers,
        target,
    } = crate::sim::arena_fixture::retail_harrier_dock_scene(&retail).expect("retail scene");
    let full = native_stage_frames("full_a127");
    let spent = native_stage_frames("depleted");
    let rearmed = native_stage_frames("rearmed");
    let owner = runtime.simulation.interner.resolve(player).to_owned();
    runtime.simulation.fog.reveal_all_for_owner(player);
    assert!(runtime.simulation.apply_command(
        &owner,
        &Command::Select {
            entity_ids: harriers.to_vec(),
            additive: false,
        },
        Some(&runtime.resources.rules)
    ));
    for id in harriers {
        assert_eq!(
            frames(&runtime.simulation, &runtime.resources.rules, id, None),
            full
        );
    }
    let orders: Vec<_> = harriers
        .into_iter()
        .map(|attacker_id| {
            CommandEnvelope::new(
                player,
                runtime.simulation.session.tick() + 1,
                Command::Attack {
                    attacker_id,
                    target_id: target,
                },
            )
        })
        .collect();
    let mut saw_spent = [false; 2];
    let mut saw_enter = [false; 2];
    let mut saw_landed = [false; 2];
    let mut saw_rearm = [false; 2];
    let mut guarding = [false; 2];
    for frame in 0..9000 {
        runtime
            .advance_frame_for_tooling(
                if frame == 0 { &orders } else { &[] },
                crate::headless_scenario::SIM_TICK_MS,
            )
            .expect("ordinary frame");
        let sim = &runtime.simulation;
        for (index, id) in harriers.into_iter().enumerate() {
            let entity = sim.entities().get(id).expect("Harrier survives");
            let ammo = entity.ammo().expect("authoritative Foot Ammo").current();
            if ammo == 0 && !saw_spent[index] {
                saw_spent[index] = true;
                assert_eq!(frames(sim, &runtime.resources.rules, id, None), spent);
            }
            let mission = entity.mission.current().known();
            saw_enter[index] |= saw_spent[index] && mission == Some(MissionType::Enter);
            let linked = entity.radio_contacts.contains(airfield)
                && sim
                    .entities()
                    .get(airfield)
                    .unwrap()
                    .radio_contacts
                    .contains(id);
            let height = crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            saw_landed[index] |= saw_spent[index] && linked && height == 0;
            if saw_landed[index] && ammo == 1 && !saw_rearm[index] {
                saw_rearm[index] = true;
                assert_eq!(frames(sim, &runtime.resources.rules, id, None), rearmed);
            }
            guarding[index] |=
                saw_rearm[index] && linked && height == 0 && mission == Some(MissionType::Guard);
        }
        if guarding.into_iter().all(|value| value) {
            break;
        }
    }
    assert!(
        saw_spent.into_iter().all(|value| value),
        "both Harriers spend their round"
    );
    assert!(
        saw_enter.into_iter().all(|value| value),
        "both Harriers return"
    );
    assert!(
        saw_landed.into_iter().all(|value| value),
        "both Harriers land on linked pads"
    );
    assert!(
        saw_rearm.into_iter().all(|value| value),
        "both Harriers rearm"
    );
    assert!(
        guarding.into_iter().all(|value| value),
        "both Harriers return to Guard"
    );
    assert!(runtime.simulation.apply_command(
        &owner,
        &Command::Select {
            entity_ids: vec![],
            additive: false,
        },
        Some(&runtime.resources.rules)
    ));
    for id in harriers {
        assert!(frames(&runtime.simulation, &runtime.resources.rules, id, None).is_empty());
        assert_eq!(
            frames(&runtime.simulation, &runtime.resources.rules, id, Some(id)),
            rearmed
        );
    }
}
