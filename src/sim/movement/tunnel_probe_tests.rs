//! TEMPORARY probe (not for commit): a Tunnel MTNK on XMP29U2 with the
//! ares-play rules, ordered through the real command path.

use std::path::Path;

use crate::headless_scenario;
use crate::sim::command::{Command, CommandEnvelope};
use crate::skirmish_launch::LaunchCountry;
use crate::util::ini_roots::{IniProfile, IniRootConfig};

#[test]
#[ignore = "probe"]
fn tunnel_probe() {
    let dir = Path::new("/Users/guoxingchai/Downloads/Command & Conquer Red Alert II");
    let ini = IniRootConfig {
        profile: IniProfile::Custom,
        rules: Some("rulesares.ini".into()),
        art: Some("artmd.ini".into()),
        ai: Some("aimd.ini".into()),
        ..Default::default()
    };
    let map = "XMP29U2.MAP";
    let launch = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
        headless_scenario::battle_session(map, &[]),
    )
    .unwrap();
    let mut sc = headless_scenario::load_with_launch_and_ini(
        dir,
        map,
        1,
        launch,
        &ini,
        crate::sim::scenario_session::ScenarioGameMode::Skirmish,
        None,
        None,
    )
    .unwrap();
    let house = sc.add_human_house("Probe", LaunchCountry::America);
    sc.runtime.simulation.make_house_multiplay_passive(house);
    let rt = &mut sc.runtime;
    let mut spawned = Vec::new();
    for (type_name, from) in [("MTNK", (50u16, 80u16)), ("E1", (52, 80))] {
        let id = (0..10u16)
            .flat_map(|d| (0..10u16).map(move |e| (from.0 + d, from.1 + e)))
            .find_map(|(rx, ry)| {
                rt.simulation
                    .spawn_object(type_name, "Probe", rx, ry, 0, &rt.resources.rules)
            })
            .expect("spawn");
        spawned.push((type_name, id));
    }
    rt.simulation.resolve_type_handles(&rt.resources.rules);
    let first = rt.simulation.session().tick() + 1;
    let orders: Vec<CommandEnvelope> = spawned
        .iter()
        .map(|&(_, id)| {
            CommandEnvelope::new(
                house,
                first,
                Command::Move {
                    entity_id: id,
                    target_rx: 74,
                    target_ry: 22,
                },
            )
        })
        .collect();
    for tick in 0..900u64 {
        let commands = if tick == 0 { orders.clone() } else { Vec::new() };
        rt.advance_frame_for_tooling(&commands, headless_scenario::SIM_TICK_MS)
            .unwrap();
        if tick < 5 || tick % 30 == 0 {
            for &(name, id) in &spawned {
                let sim = &rt.simulation;
                let Some(e) = sim.substrate.entities.get(id) else {
                    println!("t{tick} {name} gone");
                    continue;
                };
                let loc = super::ground_pose::object_location(e, sim.resolved_terrain.as_ref());
                let loco = e.locomotor.as_ref();
                println!(
                    "t{tick} {name} cell=({},{}) z={} kind={:?} tunnel={:?} mission={:?} nav={:?} limbo={}",
                    loc.x / 256,
                    loc.y / 256,
                    loc.z,
                    loco.map(|l| l.active_kind()),
                    loco.and_then(|l| l.tunnel_runtime()).map(|r| r.state()),
                    e.mission.current(),
                    e.navigation.nav_com(),
                    e.lifecycle.in_limbo(),
                );
            }
        }
    }
}
