//! Physical ALL02 definitions through an ordinary mind-control attack.
//! The selected map actors retain their authored placements, missions and Tags.
//! The flat arena and already-acquired YURI are explicit fixture inputs: this
//! does not replay the Engineer acquisition/base-building campaign opening.

use super::*;

#[test]
fn retail_all02_mind_control_attack_starts_the_authored_stallone_script() {
    use crate::map::actions::{ActionDefinitionReference, DefinitionSelector};
    use crate::map::houses::parse_house_roster;
    use crate::map::map_file::MapFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::rules::retail_ini_fixture;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::scenario_bootstrap::{FreshScenarioPrefix, ScenarioBootstrapRng};
    use crate::sim::scenario_session::ScenarioDescriptor;
    use crate::sim::world::SimSoundEvent;

    let Some((_, assets)) = retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = MapFile::from_bytes(assets.get_ref("ALL02UMD.MAP").expect("retail ALL02"))
        .expect("physical ALL02 parses");
    let campaign_ini = assets.resolve_ref("ALL02UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL02UMD.INI")
            .expect("present campaign INI parses")
            .ini
    });
    let mut process = retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, processed, art, receipt) = process
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(campaign_ini.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(retail_ini_fixture::art_registry(&art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let overlay =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&processed, Some(&art));
    let ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .expect("retail AIMD parses")
        .ini;
    let registry = TeamAiIniRegistry::from_sources(&ai, &map.ini, false);
    let discovered_actions = map.actions["065C3EDC"].entries();
    assert_eq!(
        discovered_actions
            .iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>(),
        [14, 4, 21],
        "the physical discovery script changes House, creates its Team and speaks"
    );
    // The expansion patch overrides ALL02's original TeamType and Voice
    // references. Follow the selected map's complete authored actions, just
    // as the production archive resolver and trigger reader do.
    let team_type_name = match discovered_actions[1].definition_reference() {
        Some(ActionDefinitionReference::Team(DefinitionSelector::Name(name))) => name,
        other => panic!("ALL02's CreateTeam has a named TeamType: {other:?}"),
    };
    assert!(
        registry
            .team_types
            .iter()
            .any(|definition| definition.id == team_type_name),
        "the selected map's CreateTeam reference {team_type_name} is registered"
    );
    assert_eq!(discovered_actions[2].params[0], "6");
    let voice_name = &discovered_actions[2].params[1];
    let audio = retail_ini_fixture::retail_audio_definitions().expect("same retail assets");
    let stallone_cue = audio
        .eva()
        .index_of(voice_name)
        .expect("the authored Voice action resolves through retail EVAMD");

    // Retain the complete physical map's Rules, Team, TaskForce, Script, Tag,
    // event and action definitions; only terrain and the admitted actor set
    // are bounded here. No replacement trigger or shortened action list.
    let roster = parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let descriptor = ScenarioDescriptor {
        seed: 1,
        ..Default::default()
    };
    let (mut sim, projection) = ScenarioBootstrapRng::new(descriptor.seed)
        .into_fresh_staged_simulation(
            &descriptor,
            FreshScenarioPrefix::Campaign {
                native_rules_receipt: receipt,
                rules: &rules,
                map_data: &map,
                house_roster: &roster,
                difficulty: 1,
                mission_counter: 2,
                game_options: None,
                campaign_index: 0,
                restart_carry_over: crate::sim::campaign_carry_over::CampaignCarryOver::CLEARED,
            },
        )
        .unwrap();
    assert!(projection.is_none());
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let extent = (map.header.width + map.header.height) as u16;
    let clear = rules.terrain_rules.semantics_for_land_type(0).unwrap();
    let mut terrain = crate::map::resolved_terrain::test_grid(extent, extent, |x, y| {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
        cell.speed_costs = clear.speed_costs;
        cell.base_speed_costs = clear.speed_costs;
        cell
    });
    terrain.set_land_speed_rules_for_test(&rules.terrain_rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.session.set_map_width_for_test(map.header.width as u16);
    sim.session
        .set_map_height_for_test(map.header.height as u16);
    sim.set_playfield_bounds_for_test(Some(
        crate::map::playfield::PlayfieldBounds::from_map_header(&map.header),
    ));
    sim.set_playfield_size_height_for_test(Some(map.header.height as i32));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    sim.session.install_waypoints(&map.waypoints);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &registry, Some(audio.eva()), Some(audio.sounds()));

    let tank = map
        .entities
        .iter()
        .find(|entity| {
            entity.type_id == "LTNK"
                && (entity.cell_x, entity.cell_y) == (158, 81)
                && entity.trigger_tag.as_deref() == Some("08E3237C")
        })
        .expect("ALL02's actual Start_Stallone Lasher");
    let stallone = map
        .entities
        .iter()
        .find(|entity| {
            entity.type_id == "STLN"
                && (entity.cell_x, entity.cell_y) == (156, 74)
                && entity.trigger_tag.as_deref() == Some("065C688C")
        })
        .expect("ALL02's actual Stallone_Discovered actor");
    assert_eq!(
        sim.spawn_from_map(&[tank.clone(), stallone.clone()], Some(&rules)),
        2
    );
    sim.classify_map_triggers(&rules);
    let find_actor = |sim: &Simulation, type_name: &str| {
        sim.substrate
            .entities
            .values()
            .find(|entity| sim.resolve(entity.type_ref()) == type_name)
            .unwrap()
            .stable_id()
    };
    let victim = find_actor(&sim, "LTNK");
    let stallone = find_actor(&sim, "STLN");
    let americans = sim.interner.get("Americans").unwrap();
    let neutral = sim.interner.get("Neutral").unwrap();
    assert_eq!(sim.session.current_house(), Some(americans));
    assert_eq!(
        sim.substrate.entities.get(stallone).unwrap().owner(),
        neutral
    );
    let starting_hp = sim.substrate.entities.get(victim).unwrap().health.current();
    let attacked_tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
    let start = live_trigger_of_type(&sim, trigger_type_index(&sim, "08E32CFC"));
    let discovered = live_trigger_of_type(&sim, trigger_type_index(&sim, "065C3EDC"));
    assert!(sim.trigger_runtime.trigger(start).unwrap().enabled);
    assert!(!sim.trigger_runtime.trigger(discovered).unwrap().enabled);
    let team_type = sim
        .interner
        .get(team_type_name)
        .expect("installing the selected map registry interns its authored TeamType");
    assert!(
        !sim.team_script_vm
            .teams_in_order()
            .any(|team| team.team_type_id() == Some(team_type))
    );

    // Acquisition is supplied. The stock weapon, attack mission, firing,
    // projectile detonation, Tag callbacks and capture all run normally.
    let controller = sim
        .spawn_object_at_height("YURI", "Americans", 152, 81, 0, 0, &rules)
        .expect("stock YURI admits on the flat arena");
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: controller,
            target_id: victim,
        },
    ));
    let mut heard_stallone = false;
    for _ in 0..180 {
        let commands = sim.take_due_commands();
        sim.advance_master_frame(
            &commands,
            &rules,
            Some(&overlay),
            67,
            TickLane::Ordinary,
            true,
            FrameEffects::default(),
        )
        .expect("ordinary campaign fixture frame");
        heard_stallone |= sim.sound_events().iter().any(
            |event| matches!(event, SimSoundEvent::TriggerEva { index } if *index == stallone_cue),
        );
        if heard_stallone
            && sim.team_script_vm.teams_in_order().any(|team| {
                team.team_type_id() == Some(team_type)
                    && team.members().any(|member| member == stallone)
            })
        {
            break;
        }
    }

    let tank = sim
        .substrate
        .entities
        .get(victim)
        .expect("captured tank survives");
    assert_eq!(
        tank.health.current(),
        starting_hp,
        "MindControl does not damage the tank"
    );
    assert_eq!(tank.mind_control.controller(), Some(controller));
    assert_eq!(tank.owner(), americans);
    assert!(
        sim.trigger_runtime
            .dispatch_trace
            .contains(&(attacked_tag, 6))
    );
    assert!(!sim.trigger_runtime.trigger(start).unwrap().enabled);
    assert_eq!(
        sim.substrate.entities.get(stallone).unwrap().owner(),
        americans
    );
    assert!(
        sim.team_script_vm.teams_in_order().any(|team| {
            team.team_type_id() == Some(team_type)
                && team.owner() == americans
                && team.members().any(|member| member == stallone)
        }),
        "the authored CreateTeam recruits the actual map STLN after ChangeHouse"
    );
    assert!(
        heard_stallone,
        "the authored Voice action reaches its resolved EVA cue"
    );
    let action_kinds: Vec<_> = sim
        .trigger_runtime
        .action_trace
        .iter()
        .map(|row| row.0)
        .collect();
    assert!(action_kinds.windows(2).any(|window| window == [53, 54]));
    assert!(action_kinds.windows(3).any(|window| window == [14, 4, 21]));
}
