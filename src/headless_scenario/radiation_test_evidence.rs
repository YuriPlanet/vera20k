//! One normal paid Iraqi DESO opening shared by core and client witnesses.
//!
//! The profile is an ordinary App discovery, not native timing/RNG goldens.
//! Factory/exit, Infantry51F6E0/521320, Bullet and RadSite owners all execute;
//! this fixture writes no ready object, Doing, mission, field or light state.

use super::HeadlessScenario;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::light_sources::LightingEvent;
use crate::sim::world::{FrameEffects, SimFrameOutput, TickLane};

/// Visit loaded/active/faded/expired/settled and every intervening output.
///
/// `active` is the first real site/source publication, `faded` its first
/// changed source event, and `expired` the actual site removal/source=None.
/// `settled` follows 32 unchanged ordinary frames. These labels are semantic
/// production boundaries, not a promise to match an isolated native epoch.
///
/// The visitor receives each unmodified output exactly once, including its
/// complete ordered lighting_events. A client must consume those before any
/// sampled draw; it must not reconstruct lights from the final simulation.
/// Mutable scene access permits the existing fog-view preparation consumer,
/// not gameplay forcing. Loading is the sole scenario construction here.
pub fn visit_paid_deso_light_lifecycle(
    profile_path: &'static str,
    mut visit: impl FnMut(Option<&str>, &mut HeadlessScenario, Option<&SimFrameOutput>),
) {
    let profile = crate::test_fixture::json(profile_path);
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    assert_eq!(
        launch.local.country,
        crate::skirmish_launch::LaunchCountry::Iraq
    );
    let local_name = launch.player_name.clone();
    let map = launch.selected_map_file.as_deref().unwrap().to_owned();
    let retail =
        std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut scene = super::load_session(
        &retail,
        &map,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .expect("normal Iraqi Battle scene");
    // Match the App's initial batches. HeadlessScenario::tick's 66ms lane is
    // deliberately not used: the accepted capture advances ordinary22ms.
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    let owner = scene.sim().session.current_house.unwrap();
    assert_eq!(scene.sim().resolve(owner), local_name);
    assert_eq!(
        scene
            .sim()
            .resolve(scene.sim().houses[&owner].house_type_id()),
        "Arabs"
    );
    let mut orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|order| {
            assert_eq!(order["owner"], local_name);
            CommandEnvelope::new(
                owner,
                order["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(order["payload"].clone()).unwrap(),
            )
        })
        .collect();
    let queued: Vec<_> = orders
        .iter()
        .filter_map(|order| match order.payload {
            Command::QueueProduction { type_id } => Some(scene.sim().resolve(type_id)),
            _ => None,
        })
        .collect();
    assert_eq!(queued, ["NAPOWR", "NAHAND", "NAREFN", "NARADR", "DESO"]);
    let (deso, center) = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
            } => Some((entity_id, (target_rx, target_ry))),
            _ => None,
        })
        .expect("captured paid DESO Move input");
    let deso_type = scene.runtime.resources.rules.object("DESO").unwrap();
    assert!(deso_type.deployer && deso_type.deploy_fire && deso_type.immune_to_radiation);
    let health = deso_type.strength;
    assert_eq!(health, 150, "physical stock DESO reader");
    let weapon = scene
        .runtime
        .resources
        .rules
        .weapon(deso_type.secondary().unwrap())
        .unwrap();
    assert_eq!(weapon.id, "RadEruptionWeapon");
    assert!(weapon.area_fire && weapon.rad_level > 0);
    let rad_level = weapon.rad_level;
    let spread = crate::sim::combat::cell_spread::whole_cells(
        scene
            .runtime
            .resources
            .rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .cell_spread_f64,
    );

    // Explicit admitted-input seam: the App capture independently proves the
    // two real command-bar Deploy producers at these steps. Do not duplicate
    // a UI command/string mapper in core, or add Toggle to capture allowlists.
    let deploy_steps = [5002, 5200];
    for (index, step) in deploy_steps.into_iter().enumerate() {
        assert_eq!(profile["gestures"][index + 1]["issue_after_step"], step);
        assert_eq!(
            profile["gestures"][index + 1]["gesture"],
            serde_json::json!({"kind":"command_bar","command":"Deploy"})
        );
        orders.push(CommandEnvelope::new(
            owner,
            step,
            Command::ToggleInfantryDeploy { entity_id: deso },
        ));
    }
    orders.sort_by_key(|order| order.execute_tick);
    assert!(scene.sim().radiation.sites().next().is_none());
    visit(Some("loaded"), &mut scene, None);

    let mut dispatched = 0;
    let mut paid_start = None;
    let mut held = false;
    let mut delivered = false;
    let mut deployed = false;
    let mut undeploying = false;
    let mut undeployed = false;
    let mut shots = 0;
    let mut active = false;
    let mut faded = false;
    let mut last_source = None;
    let mut expired_at = None;
    for step in 1..=profile["ticks"].as_u64().unwrap() {
        let due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step - 1)
            .cloned()
            .collect();
        for order in &due {
            if let Command::QueueProduction { type_id } = order.payload
                && scene.sim().resolve(type_id) == "DESO"
            {
                // Begin_Production's event receiver does not repeat the full
                // sidebar prerequisite test. Observe that existing authority
                // before submitting the ordinary paid request.
                let option = crate::sim::production::all_build_options_for_owner(
                    scene.sim(),
                    &scene.runtime.resources.rules,
                    &local_name,
                )
                .into_iter()
                .find(|option| option.type_id == type_id)
                .unwrap();
                assert!(
                    option.enabled && option.visible_in_sidebar(),
                    "legal paid DESO option"
                );
                for name in ["NACNST", "NAPOWR", "NAHAND", "NAREFN", "NARADR"] {
                    assert!(
                        scene
                            .sim()
                            .entities()
                            .values()
                            .any(|entity| entity.owner() == owner
                                && scene.sim().resolve(entity.type_ref()) == name
                                && entity.is_alive()
                                && !entity.lifecycle.in_limbo()
                                && entity.building_art.actually_placed()),
                        "ordinary prerequisite {name} must be placed"
                    );
                }
                paid_start = Some((
                    scene.sim().houses[&owner].economy.spent_credits(),
                    option.cost,
                ));
                assert!(!held && !delivered);
            }
        }
        if due
            .iter()
            .any(|order| matches!(order.payload, Command::ToggleInfantryDeploy { .. }))
        {
            let actor = scene
                .sim()
                .entities()
                .get(deso)
                .expect("actual paid DESO before Deploy");
            assert!(delivered && actor.is_object_alive() && !actor.lifecycle.in_limbo());
            assert!(actor.lifecycle.cell_marked() && actor.in_logic_vector.is_member());
            assert_eq!(
                (actor.position.rx(), actor.position.ry()),
                center,
                "ordinary Move must arrive before the admitted deploy input"
            );
        }
        dispatched += due.len();
        let output = scene
            .runtime
            .advance_frame(&due, 22, TickLane::Ordinary, FrameEffects::default())
            .expect("ordinary paid DESO frame");
        assert!(output.tick.frame_committed);
        if let Some(actor) = scene.sim().entities().get(deso) {
            assert_eq!(scene.sim().resolve(actor.type_ref()), "DESO");
            assert_eq!(actor.owner(), owner);
            assert!(actor.is_alive() && actor.health.current() == health);
            if actor.lifecycle.in_limbo() {
                assert!(
                    !delivered,
                    "delivered paid identity cannot return to held state"
                );
                held = true;
            } else {
                assert!(
                    held,
                    "actual Factory-held identity precedes its barracks exit"
                );
                let (spent, cost) = paid_start.expect("ordinary DESO production was requested");
                assert!(
                    scene.sim().houses[&owner].economy.spent_credits() >= spent + cost,
                    "paid head must finish charging before world delivery"
                );
                delivered = true;
                let doing = actor.mission_leaf.as_infantry().unwrap().doing();
                if undeployed {
                    assert!(
                        !(27..=31).contains(&doing),
                        "ordinary undeploy must remain released"
                    );
                }
                deployed |= (27..=30).contains(&doing);
                if step > deploy_steps[1] {
                    undeploying |= doing == 31;
                    undeployed |= undeploying && doing == 0;
                }
            }
        }
        for shot in output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == deso)
        {
            assert!(
                delivered && deployed && !undeployed,
                "real self-fire after ordinary deployment"
            );
            assert_eq!(scene.sim().resolve(shot.weapon_id), "RadEruptionWeapon");
            assert_eq!(
                shot.weapon_slot,
                crate::sim::combat::combat_weapon::WeaponSlot::Secondary
            );
            assert_eq!(
                shot.target,
                crate::sim::combat::TargetKind::Cell(center.0, center.1)
            );
            shots += 1;
        }
        let mut phase = None;
        for event in &output.lighting_events {
            if let LightingEvent::Radiation { center: at, source } = event {
                assert_eq!(*at, center, "one ordinary local radiation site");
                if let Some(source) = source {
                    let site = scene
                        .sim()
                        .radiation
                        .site_at(center)
                        .expect("published live site");
                    assert_eq!(site.spread, spread);
                    assert_eq!(site.level, rad_level);
                    assert!(site.radius_leptons > 0);
                    assert_eq!(source.radius_leptons, site.radius_leptons);
                    assert!(source.active && expired_at.is_none());
                    if !active {
                        assert!(shots > 0, "actual FireAt/Bullet creates the site");
                        active = true;
                        phase = Some("active");
                    } else if !faded && last_source.as_ref() != Some(source) {
                        assert!(
                            site.remaining < site.duration,
                            "source decay follows activation"
                        );
                        faded = true;
                        phase = Some("faded");
                    }
                } else {
                    assert!(
                        active && faded && undeployed,
                        "real fade/undeploy precedes expiry"
                    );
                    assert!(
                        scene.sim().radiation.site_at(center).is_none(),
                        "actual site retirement"
                    );
                    assert!(
                        expired_at.replace(step).is_none(),
                        "one terminal source removal"
                    );
                    phase = Some("expired");
                }
                last_source = source.clone();
            }
        }
        if let Some(expired) = expired_at {
            assert!(scene.sim().radiation.site_at(center).is_none());
            assert_eq!(
                shots, 1,
                "undeployed source cannot refresh/recreate the field"
            );
            if step >= expired + 32 {
                assert!(held && delivered && deployed && undeploying && undeployed && faded);
                assert_eq!(dispatched, orders.len());
                assert_eq!(
                    scene
                        .sim()
                        .entities()
                        .get(deso)
                        .unwrap()
                        .mission
                        .current()
                        .known(),
                    Some(crate::sim::mission::MissionType::Guard),
                    "released source settles in ordinary Guard"
                );
                visit(Some("settled"), &mut scene, Some(&output));
                return;
            }
        }
        visit(phase, &mut scene, Some(&output));
    }
    panic!(
        "paid DESO must deploy, fade, undeploy, expire and settle within the normal profile bound"
    );
}

#[test]
#[ignore = "slow; requires physical retail paid Iraqi production through ordinary deploy/undeploy and complete radiation lifetime"]
fn paid_deso_deploy_undeploy_retires_its_source_through_normal_frames() {
    assert_paid_deso_lifecycle("tools/map_observation.radiation-light.example.json");
}

#[test]
#[ignore = "slow; requires physical retail paid Snow Iraqi production through ordinary deploy/undeploy and complete radiation lifetime"]
fn paid_snow_deso_deploy_undeploy_retires_its_source_through_normal_frames() {
    assert_paid_deso_lifecycle("tools/map_observation.radiation-snow.example.json");
}

fn assert_paid_deso_lifecycle(profile_path: &'static str) {
    let mut phases = Vec::new();
    let mut frames = 0;
    visit_paid_deso_light_lifecycle(profile_path, |phase, scene, output| {
        if let Some(output) = output {
            frames += 1;
            assert!(output.tick.frame_committed);
            assert_eq!(scene.sim().session.total_sim_ms, frames * 22);
        }
        if let Some(phase) = phase {
            phases.push(phase.to_owned());
        }
    });
    assert_eq!(phases, ["loaded", "active", "faded", "expired", "settled"]);
    assert!(
        frames > 5200,
        "ordinary paid opening and stop-refresh inputs really executed"
    );
}
