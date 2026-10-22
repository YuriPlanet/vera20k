//! Legal authored skirmish Tag columns, not a shipped tagged-MCV witness.
//! Unit7399D4/7399F8 selects before transfer; Building44A0B4/44A137 reverses it.
//! Retail rules/ART/SHP/SOUNDMD; clear cells and the human House roster are priors.
use super::*;
use crate::sim::command::Command;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{LifecycleOutput, SimFrameOutput, TickLane, UninitContext};
use std::cell::{Cell, RefCell};

fn scene(building: bool) -> Option<(SimRuntime, u64, usize)> {
    let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_building_buildup_assets(&assets, "TEMPERATE");
    let [_, frames, rate] = retail.rules.buildup_control("GACNST");
    assert!(retail.rules.has_buildup("GACNST") && frames > 1 && rate > 0);
    let source = if building {
        "[Structures]\n0=Americans,GACNST,256,16,16,128,MCV_TAG,0,0,1,0,0,None,None,None,0,0\n"
    } else {
        "[Units]\n0=Americans,AMCV,256,16,16,128,Guard,MCV_TAG,0,-1,0,-1,1,1\n[Structures]\n"
    };
    let map = crate::map::map_file::MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,16,16\nLocalSize=0,0,16,16\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\nPlayer=Americans\n\
         [Triggers]\nSELECT=Americans,CLOCK,Selected cue,0,1,1,1,0\n\
         CLOCK=Americans,<none>,Retained clock,1,1,1,1,0\n\
         [Tags]\nMCV_TAG=2,Repeat selection cue,SELECT\n\
         [Events]\nSELECT=1,33,0,0\nCLOCK=1,0,0,0\n\
         [Actions]\nSELECT=1,19,7,CommandBar,0,0,0,0,A\nCLOCK=0\n\
         {source}1=Americans,GAPOWR,256,12,20,0,None,0,0,1,0,0,None,None,None,0,0\n\
         2=Russians,GAPOWR,256,20,12,0,None,0,0,1,0,0,None,None,None,0,0\n"
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(
        map.entities
            .iter()
            .filter(|entity| entity.trigger_tag.as_deref() == Some("MCV_TAG"))
            .count(),
        1
    );
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()?;
    assert!(definitions.sounds().get("CommandBar").is_some());
    let teams = crate::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &crate::rules::retail_ini_fixture::retail_ini("aimd.ini")?,
        &map.ini,
        true,
    );
    let rules = retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7399f8);
    // A nonzero constructor timestamp exposes accidental live-Trigger rebuilds.
    // The disabled linked trigger is not an elapsed-timer gameplay witness.
    sim.session.set_binary_frame_for_test(137);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    let rival = sim.interner().get("Russians").unwrap();
    sim.houses
        .get_mut(&rival)
        .unwrap()
        .set_is_human_for_test(true);
    sim.houses
        .get_mut(&rival)
        .unwrap()
        .set_current_iq_for_test(0);
    sim.session.set_current_house_for_test(Some(owner));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().mcv_redeploy = true;
    sim.session.game_options_mut_for_test().short_game = false;
    // Same prepared definitions and shared population entry as app loading.
    // Materialization/roster are supplied; all map entity/tag reads are real.
    sim.initialize_trigger_definitions(
        &map,
        &teams,
        Some(definitions.eva()),
        Some(definitions.sounds()),
    );
    let terrain = sim.resolved_terrain().unwrap().clone();
    crate::sim::runtime::populate_staged_scenario_with_generated_inits(
        &mut sim,
        &map,
        &terrain,
        "TEMPERATE",
        &rules,
        Some(&registry),
        None,
        crate::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
        &crate::sim::scenario_session::ScenarioDescriptor::default(),
        None,
        |_| {},
    )
    .unwrap();
    let source_type = if building { "GACNST" } else { "AMCV" };
    let source = sim
        .entities()
        .values()
        .find(|entity| sim.interner().resolve(entity.type_ref()) == source_type)
        .unwrap()
        .stable_id();
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(source)];
    assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, 1);
    assert_eq!(sim.trigger_runtime.tag(tag).unwrap().triggers.len(), 2);
    assert!(sim.trigger_runtime.attachments.len() == 1);
    Some((
        SimRuntime {
            simulation: sim,
            resources: SimResources {
                rules,
                overlay_registry: registry,
                terrain_template: Some(terrain),
            },
        },
        source,
        tag,
    ))
}

fn selected_cue(runtime: &mut SimRuntime, object: u64) {
    assert!(runtime.simulation.techno_select(
        object,
        false,
        UninitContext::new(
            &runtime.resources.rules,
            Some(&runtime.resources.overlay_registry)
        )
    ));
    let sounds = runtime.simulation.take_sound_events_for_test();
    assert_eq!(
        cue_count(&sounds),
        1,
        "canonical Selected33 must run the registered action19"
    );
    runtime.simulation.lifecycle_outputs.clear_for_test();
}

fn cue_count(events: &[SimSoundEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(event,
        SimSoundEvent::VocCentered { sound_id } if sound_id.eq_ignore_ascii_case("CommandBar"))
        })
        .count()
}

fn retained(runtime: &SimRuntime, tag: usize) -> (LiveTag, Vec<(usize, LiveTrigger)>) {
    let owner = &runtime.simulation.trigger_runtime;
    let live = owner.tag(tag).unwrap().clone();
    let triggers = live
        .triggers
        .iter()
        .map(|&id| (id, owner.trigger(id).unwrap().clone()))
        .collect();
    (live, triggers)
}

fn assert_retained(
    runtime: &SimRuntime,
    source: u64,
    replacement: u64,
    tag: usize,
    before: &(LiveTag, Vec<(usize, LiveTrigger)>),
) {
    let owner = &runtime.simulation.trigger_runtime;
    assert_eq!(
        owner.attachments.get(&TagAttachment::Object(replacement)),
        Some(&tag)
    );
    assert!(
        !owner
            .attachments
            .contains_key(&TagAttachment::Object(source))
    );
    assert_eq!(
        retained(runtime, tag),
        *before,
        "exact live Tag, refs, links and timer state survive"
    );
    assert_eq!(owner.tag_order, [tag]);
    assert!(runtime.simulation.entities().get(source).is_none());
}

// Action19's real synchronous sound flush exposes the new MCV's Selected33
// before old Building UnInit. This is observation, not an authored callback.
struct SelectBoundary {
    delivered: Cell<usize>,
    source: u64,
    tag: usize,
    selected: RefCell<Vec<u64>>,
}
impl crate::sim::world::SoundBoundary for SelectBoundary {
    fn consume(
        &self,
        sim: &Simulation,
        _: &crate::rules::ruleset::RuleSet,
        events: &[SimSoundEvent],
    ) {
        for event in &events[self.delivered.get()..] {
            if !matches!(event, SimSoundEvent::VocCentered { sound_id } if sound_id.eq_ignore_ascii_case("CommandBar"))
            {
                continue;
            }
            let owner = &sim.trigger_runtime;
            assert!(
                sim.entities().get(self.source).is_some(),
                "Selected33 must precede source destruction"
            );
            assert!(
                !owner
                    .attachments
                    .contains_key(&TagAttachment::Object(self.source))
            );
            let replacement = owner
                .attachments
                .iter()
                .find_map(|(attachment, &tag)| match attachment {
                    TagAttachment::Object(id) if tag == self.tag => Some(*id),
                    _ => None,
                })
                .unwrap();
            assert_ne!(replacement, self.source);
            assert_eq!(owner.tag(self.tag).unwrap().refs, 1);
            assert!(sim.lifecycle_outputs.iter().any(|output| matches!(output,
                LifecycleOutput::ObjectSelected { stable_id } if *stable_id == replacement)));
            self.selected.borrow_mut().push(replacement);
        }
        self.delivered.set(events.len());
    }
    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

fn frame(runtime: &mut SimRuntime, source: u64, tag: usize) -> (SimFrameOutput, Vec<u64>) {
    use crate::sim::world::SoundBoundary;
    let boundary = SelectBoundary {
        delivered: Cell::new(0),
        source,
        tag,
        selected: RefCell::new(Vec::new()),
    };
    let output = runtime
        .advance_frame(
            &[],
            33,
            TickLane::Ordinary,
            FrameEffects::for_sound(&boundary),
        )
        .unwrap();
    // Only inline action19 observations count; a frame-tail fallback cannot
    // establish that its source was still stored before UnInit.
    assert_eq!(boundary.delivered(), output.sound_events_delivered);
    (output, boundary.selected.into_inner())
}

#[test]
fn authored_mcv_tag_moves_after_yard_select_and_fires_on_later_reselection() {
    let Some((mut runtime, source, tag)) = scene(false) else {
        return;
    };
    selected_cue(&mut runtime, source);
    let before = retained(&runtime, tag);
    assert!(runtime.apply_command_for_test("Americans", &Command::DeployMcv { entity_id: source }));
    let mut yard = None;
    for _ in 0..8 {
        let (output, callbacks) = frame(&mut runtime, source, tag);
        assert!(callbacks.is_empty());
        assert_eq!(
            cue_count(&output.sound_events),
            0,
            "yard Select precedes Tag handoff"
        );
        yard = runtime
            .simulation
            .entities()
            .values()
            .find(|entity| runtime.simulation.interner().resolve(entity.type_ref()) == "GACNST")
            .map(|entity| entity.stable_id());
        if let Some(yard) = yard {
            assert!(
                output
                    .lifecycle_outputs
                    .iter()
                    .any(|output| matches!(output,
                LifecycleOutput::ObjectSelected { stable_id } if *stable_id == yard))
            );
            break;
        }
    }
    let yard = yard.expect("scheduled AMCV deployment");
    assert_retained(&runtime, source, yard, tag, &before);
    let [_, frames, rate] = runtime.resources.rules.buildup_control("GACNST");
    for _ in 0..(frames * rate * 4 + 240) {
        if !runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .building_up()
        {
            break;
        }
        frame(&mut runtime, source, tag);
    }
    assert!(
        !runtime
            .simulation
            .entities()
            .get(yard)
            .unwrap()
            .building_up()
    );
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::Select {
            entity_ids: Vec::new(),
            additive: false
        }
    ));
    selected_cue(&mut runtime, yard);
    assert_retained(&runtime, source, yard, tag, &before);
}

#[test]
fn authored_yard_tag_moves_before_new_mcv_selected_callback() {
    let Some((mut runtime, source, tag)) = scene(true) else {
        return;
    };
    assert!(
        !runtime
            .simulation
            .entities()
            .get(source)
            .unwrap()
            .building_up()
    );
    selected_cue(&mut runtime, source);
    let before = retained(&runtime, tag);
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::SetRally {
            rx: 20,
            ry: 20,
            producer_ids: vec![source]
        }
    ));
    assert!(crate::sim::production::can_sell_building(
        &runtime.simulation,
        &runtime.resources.rules,
        source
    ));
    assert!(
        runtime.apply_command_for_test("Americans", &Command::SellBuilding { entity_id: source })
    );
    let [_, frames, rate] = runtime.resources.rules.buildup_control("GACNST");
    let mut replacement = None;
    let mut callbacks = Vec::new();
    let mut cues = 0;
    for _ in 0..(frames * rate * 4 + 240) {
        let (output, selected) = frame(&mut runtime, source, tag);
        callbacks.extend(selected);
        cues += cue_count(&output.sound_events);
        replacement = runtime
            .simulation
            .entities()
            .values()
            .find(|entity| runtime.simulation.interner().resolve(entity.type_ref()) == "AMCV")
            .map(|entity| entity.stable_id());
        if replacement.is_some() {
            break;
        }
    }
    let replacement = replacement.expect("scheduled yard repack");
    assert_eq!(
        callbacks,
        [replacement],
        "real Selected33 action19 runs synchronously on the inherited Tag"
    );
    assert_eq!(cues, 1);
    assert_retained(&runtime, source, replacement, tag, &before);
    assert!(
        runtime
            .simulation
            .entities()
            .get(replacement)
            .unwrap()
            .selection
            .is_selected()
    );
}
