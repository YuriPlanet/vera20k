//! Release-mode composition witness on unmodified retail Hills.mmx.
//! Run: cargo run --release --example bridge_forcefire -- /path/to/retail
//! Native scalar comparisons: tools/spatial_oracle/bridge_damage_admission.py.
//! This uses the production headless loader/runtime; it is not rendered parity.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use vera20k::sim::command::{Command, CommandEnvelope};

fn main() {
    let retail = std::env::args().nth(1).expect("retail installation path");
    let mut scenario =
        vera20k::headless_scenario::load(Path::new(&retail), "Hills.mmx", 0x0B21_D6E5)
            .expect("load retail Hills through the production loader");
    let owner = scenario
        .sim()
        .session
        .house_order
        .iter()
        .copied()
        .find(|id| {
            scenario
                .sim()
                .houses
                .get(id)
                .is_some_and(|house| house.is_human)
        })
        .expect("human launch house");
    let owner_name = scenario.sim().interner.resolve(owner).to_owned();
    let target = (64, 69);
    let runtime = &mut scenario.runtime;
    assert_eq!(runtime.resources.rules.bridge_rules.strength, 1500);
    assert!(
        runtime
            .simulation
            .bridge_state
            .as_ref()
            .unwrap()
            .is_bridge_walkable(target.0, target.1)
    );
    let attacker = runtime
        .simulation
        .spawn_object(
            "MTNK",
            &owner_name,
            64,
            72,
            0,
            &runtime.resources.rules,
            &runtime.resources.height_map,
        )
        .expect("spawn Grizzly on the retail bank");
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);
    let mut seen = BTreeSet::new();
    let mut prior = BTreeMap::new();
    let mut moved = false;
    let mut ended = false;
    let mut previous_bridge = None;
    for frame in 0..18000 {
        let commands = if frame == 0
            || runtime
                .simulation
                .entities()
                .get(attacker)
                .unwrap()
                .attack_target
                .is_none()
        {
            vec![CommandEnvelope::new(
                owner,
                runtime.simulation.session.tick + 1,
                Command::ForceAttackCell {
                    attacker_id: attacker,
                    target_rx: target.0,
                    target_ry: target.1,
                },
            )]
        } else {
            Vec::new()
        };
        runtime
            .advance_frame_for_tooling(&commands, vera20k::headless_scenario::SIM_TICK_MS)
            .expect("advance production frame");
        for (&id, &position) in &prior {
            match runtime.simulation.projectiles.get(id) {
                Some(shell) => moved |= shell.position != position,
                None => ended = true,
            }
        }
        prior.clear();
        for (&id, shell) in runtime
            .simulation
            .projectiles
            .iter()
            .filter(|(_, shell)| shell.source_id == attacker)
        {
            assert_eq!(shell.launch_target.z, 1040, "retail deck aim");
            seen.insert(id);
            prior.insert(id, shell.position);
        }
        let bridge = runtime
            .simulation
            .bridge_state
            .as_ref()
            .unwrap()
            .cell(target.0, target.1)
            .unwrap();
        let current_bridge = bridge.damage_state;
        if previous_bridge != Some(current_bridge) || frame % 2000 == 0 {
            println!(
                "frame {frame}: {} launched shells; bridge {current_bridge:?}",
                seen.len()
            );
        }
        previous_bridge = Some(current_bridge);
        if !runtime
            .simulation
            .bridge_state
            .as_ref()
            .unwrap()
            .is_bridge_walkable(target.0, target.1)
        {
            assert!(moved && ended && !seen.is_empty());
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(attacker)
                    .unwrap()
                    .attack_target
                    .is_none()
            );
            println!(
                "Hills.mmx: {target:?} collapsed at frame {frame}; {} Cannon shells, flight and target release observed",
                seen.len()
            );
            return;
        }
    }
    panic!("bridge remained walkable after 18000 ordinary frames");
}
