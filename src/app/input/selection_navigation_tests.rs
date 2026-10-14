//! Original-executed Next/Previous4AA2B0/4AA380, P5367F0 ->732280,
//! Health733380 and Y5369F0 ->7336C0.
//! Inputs, native results and execution boundaries are retained together in
//! tools/input_oracle/selection_navigation.{py,json,meta.json,md}.
//!
//! These fixtures supply existing objects and retained display order. They do
//! not establish object construction, display registration, RobotOffline,
//! power/planning mode lifecycle, window redraw or final camera raster parity.
//! Selection histories compare successful-add voices through the existing
//! Main/voice owner. Whole native cleanup boundaries are supplied inputs here;
//! actual Rust expiry production/transport has separate lifecycle coverage.

use super::super::{
    SelectionMutation, reconcile_selection_order_for_sim, resolve_selection_mutation,
    selected_stable_ids_in_order,
};
use super::{
    CategoryNavigation, ObjectDirection, category_key, category_selection, combatant_selection,
    entity_navigation_category, navigation_feedback, next_object, selection_navigation_allowed,
};
use crate::app::input::{camera, entity_pick};
use crate::app::types::{CategoryNavigationKind, TypeSelectInputState, TypeSelectOutcome};
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::Health;
use crate::sim::command::Command;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::{BunkerLink, GameEntity};
use crate::sim::house_state::HouseState;
use crate::sim::movement::ground_pose;
use crate::sim::world::{Simulation, display_layers::DisplayLayer};
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Deserialize)]
struct Corpus {
    schema: u32,
    search_cases: Vec<SearchCase>,
    command_histories: Vec<CommandHistory>,
    combatant_histories: Vec<CombatantHistory>,
    combatant_cleanup_histories: Vec<CombatantHistory>,
    veterancy_histories: Vec<CombatantHistory>,
    health_navigation_histories: Vec<CombatantHistory>,
    veterancy_cleanup_histories: Vec<CombatantHistory>,
    veterancy_rank_cases: Vec<serde_json::Value>,
    enslaved_voice_histories: Vec<CombatantHistory>,
}

#[derive(Deserialize)]
struct Actor {
    id: u64,
    kind: String,
    health: i32,
    #[serde(default = "fixture_strength_default")]
    strength: i32,
    limbo: bool,
    alive: bool,
    in_playfield: bool,
    discovered: bool,
    selectable: bool,
    #[serde(default)]
    is_selectable_combatant: bool,
    #[serde(default = "positive_primary_default")]
    positive_primary_damage: bool,
    #[serde(default = "single_voice_default")]
    voice_list: Vec<i64>,
    #[serde(default)]
    enslaved_voice_list: Option<Vec<i64>>,
    #[serde(default)]
    veterancy_raw_bits: Option<u32>,
    #[serde(default)]
    cost: i32,
    owner: String,
    slave: bool,
    robot_offline: bool,
    bunker: bool,
    docked: bool,
    cell_building: bool,
    locomotor_swap: bool,
    virtual_1d4: bool,
    mission_only: bool,
    techno_cast: bool,
    coord: [i32; 3],
}

fn positive_primary_default() -> bool {
    true
}

fn fixture_strength_default() -> i32 {
    // SelectionFixture supplies type+A0=100 unless a control declares it.
    100
}

fn single_voice_default() -> Vec<i64> {
    vec![101]
}

#[derive(Default, Deserialize)]
struct HouseControls {
    campaign: bool,
    other_human: bool,
    other_control: bool,
    #[serde(default)]
    other_passive: bool,
}

#[derive(Deserialize)]
struct Inputs {
    actors: Vec<Actor>,
    layers: [Vec<Option<u64>>; 5],
    #[serde(default)]
    house: HouseControls,
    #[serde(default = "fixture_seed_default")]
    seed: u64,
    #[serde(default)]
    navigation_trace: bool,
    #[serde(default)]
    enslaved_voice_trace: bool,
}

fn fixture_seed_default() -> u64 {
    1
}

#[derive(Deserialize)]
struct SearchCase {
    id: String,
    #[serde(flatten)]
    inputs: Inputs,
    anchor: Option<u64>,
    direction: String,
    candidate: Option<u64>,
    visits: Vec<Visit>,
    rng_unchanged: bool,
}

#[derive(Deserialize)]
struct Visit {
    id: u64,
    gate: String,
    accepted: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Modes {
    repair: bool,
    sell: bool,
    power: bool,
    planning: bool,
}

#[derive(Deserialize)]
struct CommandHistory {
    id: String,
    #[serde(flatten)]
    inputs: Inputs,
    selected: Vec<u64>,
    directions: Vec<String>,
    follow: Option<u64>,
    #[serde(default)]
    modes: Modes,
    #[serde(default)]
    placement: bool,
    steps: Vec<CommandStep>,
}

#[derive(Deserialize)]
struct CommandStep {
    direction: String,
    selected: Vec<u64>,
    selected_flags: BTreeMap<String, bool>,
    follow: Option<u64>,
    follow_enabled: bool,
    modes: Modes,
    placement: bool,
    selection_mode: u32,
    selection_across_map: bool,
    center_coord: Option<[i32; 3]>,
    redraw: Vec<u32>,
    call_order: Vec<String>,
}

#[derive(Deserialize)]
struct CombatantHistory {
    id: String,
    #[serde(flatten)]
    inputs: Inputs,
    screen_order: Vec<Option<u64>>,
    map_order: Vec<u64>,
    selected: Vec<u64>,
    #[serde(default = "positive_primary_default")]
    voice_enabled: bool,
    #[serde(default)]
    command_guard: u32,
    #[serde(default)]
    csf_sha256: Option<String>,
    steps: Vec<serde_json::Value>,
    rng_before_hex: BTreeMap<String, String>,
    rng_after_hex: BTreeMap<String, String>,
    main_next_four: Vec<u32>,
}

fn corpus() -> Corpus {
    let corpus: Corpus = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .expect("checked original selection-navigation corpus");
    assert_eq!(corpus.schema, 1);
    corpus
}

fn boundary_ids(boundary: &serde_json::Value) -> Vec<u64> {
    serde_json::from_value(boundary["selected"].clone()).unwrap()
}

fn boundary_screen_ids(boundary: &serde_json::Value) -> Vec<u64> {
    // Actual Tactical expiry clears a record's pointer while retaining its
    // slot/count. NULL holes do no work in731F70; compact only those holes.
    serde_json::from_value::<Vec<Option<u64>>>(boundary["screen_order"].clone())
        .unwrap()
        .into_iter()
        .flatten()
        .collect()
}

/// Supply original mode/scope inputs through the existing input owner.
fn boundary_mode(boundary: &serde_json::Value) -> TypeSelectInputState {
    let mut input = TypeSelectInputState::default();
    match boundary["selection_mode"].as_u64().unwrap() {
        0 => {}
        1 => input.finish_combatant_selection(TypeSelectOutcome::Empty, false),
        2 => input.finish_tap(TypeSelectOutcome::Empty, false),
        3 => input.finish_category_navigation(CategoryNavigationKind::Health, true),
        4 => input.finish_category_navigation(CategoryNavigationKind::Veterancy, true),
        mode => panic!("unexpected original selection mode {mode}"),
    }
    input.across_map = boundary["across_map"].as_bool().unwrap();
    input
}

#[test]
fn veterancy_navigation_keeps_shared_snapshot_without_pointer_expiry() {
    // Native733160 removes only a notified pointer; changing Alive or
    // Deselect alone does not expire the shared Health/Y snapshot.
    let corpus = corpus();
    let history = &corpus.combatant_cleanup_histories[0];
    let (mut sim, rules) = fixture(&history.inputs, &[]);
    let mut input = crate::app::input::state::MatchInputState::new(Default::default());
    input.category_navigation.prepare(
        CategoryNavigationKind::Health,
        false,
        vec![20, 40, 50],
        vec![],
    );
    input
        .category_navigation
        .record_category(CategoryNavigationKind::Health, 0);
    input
        .type_select
        .finish_category_navigation(CategoryNavigationKind::Health, true);
    input.type_select.across_map = true;
    sim.entities_mut()
        .get_mut(20)
        .unwrap()
        .lifecycle
        .object_alive = false;
    sim.entities_mut().get_mut(40).unwrap().selected = false;

    reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules));

    assert_eq!(input.category_navigation.candidates, [20, 40, 50]);
    assert_eq!(input.type_select.selection_scope_view().0, "health");
    assert!(input.type_select.across_map);
}

fn native_last_category(value: &serde_json::Value) -> Option<u8> {
    match value.as_i64().unwrap() {
        -1 => None,
        category @ 0..=2 => Some(category as u8),
        other => panic!("unexpected supplied native category {other}"),
    }
}

fn assert_navigation_rng(sim: &Simulation, native: &serde_json::Value, label: &str) {
    for (name, state) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert_eq!(
            state,
            native[name].as_str().unwrap(),
            "{label}: full {name} cursor/state"
        );
    }
}

fn assert_navigation_boundary(
    input: &crate::app::input::state::MatchInputState,
    sim: &Simulation,
    native: &serde_json::Value,
    label: &str,
) {
    assert_eq!(
        input.selection_order,
        boundary_ids(native),
        "{label}: ledger"
    );
    assert_eq!(
        input.follow_target,
        serde_json::from_value::<Option<u64>>(native["follow"].clone()).unwrap(),
        "{label}: Follow target"
    );
    assert_eq!(
        input.follow_target.is_some(),
        native["follow_enabled"].as_bool().unwrap(),
        "{label}: Follow validity"
    );
    assert_eq!(
        input.type_select.selection_scope_view().0,
        boundary_mode(native).selection_scope_view().0,
        "{label}: shared navigation mode"
    );
    assert_eq!(
        input.type_select.across_map,
        native["across_map"].as_bool().unwrap(),
        "{label}: independent map byte"
    );
    let (candidates, health_category, veterancy_category) =
        input.category_navigation.navigation_view();
    assert_eq!(
        candidates,
        serde_json::from_value::<Vec<u64>>(native["retained_navigation"].clone()).unwrap(),
        "{label}: shared retained snapshot/order"
    );
    assert_eq!(
        health_category,
        native_last_category(&native["health_category"]),
        "{label}: last Health category"
    );
    assert_eq!(
        veterancy_category,
        native_last_category(&native["veterancy_category"]),
        "{label}: last Y category"
    );
    for (id, actor) in native["actor_state"].as_object().unwrap() {
        let id: u64 = id.parse().unwrap();
        // Removed registry storage is a declared input in one cleanup control;
        // native's still-mapped bytes do not prove a Rust destructor/free.
        let Some(entity) = sim.entities().get(id) else {
            assert!(
                !native["map_order"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|member| member.as_u64() == Some(id)),
                "{label}: unexpected registry loss for{id}"
            );
            continue;
        };
        assert_eq!(
            entity.selected,
            actor["selected"].as_bool().unwrap(),
            "{label}: committed bit{id}"
        );
        assert_eq!(
            entity.lifecycle.object_alive,
            actor["alive"].as_bool().unwrap(),
            "{label}: Alive{id}"
        );
        assert_eq!(
            entity.lifecycle.in_limbo,
            actor["limbo"].as_bool().unwrap(),
            "{label}: Limbo{id}"
        );
        assert_eq!(
            u64::from(entity.veterancy.raw().bits()),
            actor["veterancy_raw_bits"].as_u64().unwrap(),
            "{label}: raw experience{id}"
        );
        if let Some(present) = actor["slave_owner_present"].as_bool() {
            assert_eq!(
                entity.slave.owner().is_some(),
                present,
                "{label}: existing SlaveOwner{id}"
            );
        }
    }
}

fn navigation_csf(histories: &[CombatantHistory]) -> Option<crate::assets::csf_file::CsfFile> {
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let bytes = assets
        .load_file_from_mix("ra2md.csf")
        .expect("active-retail language table")
        .bytes;
    let sha = crate::util::sha256::sha256_hex(&bytes);
    for history in histories {
        assert_eq!(
            history.csf_sha256.as_deref(),
            Some(sha.as_str()),
            "{}: physical language identity",
            history.id
        );
    }
    Some(crate::assets::csf_file::CsfFile::from_bytes(&bytes).unwrap())
}

fn apply_history_selection(
    sim: &mut Simulation,
    rules: &RuleSet,
    input: &mut crate::app::input::state::MatchInputState,
    mutation: &SelectionMutation,
    voice_enabled: bool,
    step: &serde_json::Value,
    label: &str,
) -> Vec<serde_json::Value> {
    use crate::sim::rng::trace_draws;
    use serde_json::json;

    let before_rng = sim.rng_state();
    let effects = resolve_selection_mutation(
        sim,
        Some(rules),
        std::mem::take(&mut input.selection_order),
        input.follow_target,
        step["before"]["placement"].as_bool().unwrap(),
        mutation,
    );
    assert_eq!(
        sim.rng_state(),
        before_rng,
        "{label}: selection consumes no RNG"
    );
    let native_adds: Vec<_> = step["selection_calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["name"] == "select" && call["accepted"] == true)
        .map(|call| call["id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        effects.successful_adds, native_adds,
        "{label}: Select admission order"
    );
    if effects.native_selection_mode_reset {
        input.type_select.note_successful_selection_mutation(false);
    }
    input.selection_order = effects.ordered;
    input.follow_target = effects.follow_target;
    let (requests, draws) = trace_draws(|| {
        effects.successful_adds.iter().filter_map(|&id| {
            sim.selection_voice_request(rules, id, voice_enabled).map(|sound| {
                json!({"id": id, "sound_id": sound.strip_prefix("Sound").unwrap().parse::<i64>().unwrap()})
            })
        }).collect::<Vec<_>>()
    });
    assert_eq!(
        requests,
        *step
            .get("accepted_voice_requests")
            .unwrap_or(&step["voice_requests"])
            .as_array()
            .unwrap(),
        "{label}: accepted presentation voices"
    );
    assert_eq!(
        draws
            .iter()
            .map(|draw| draw["value"].clone())
            .collect::<Vec<_>>(),
        *step["main_draws"].as_array().unwrap(),
        "{label}: ordered raw Main voice draws"
    );
    // Exercise the real complete-snapshot receiver. Its simulation stores the
    // flags; the one app ledger retains ObjectSelect's prepend/append order.
    assert!(sim.apply_command(
        "local",
        &Command::Select {
            entity_ids: input.selection_order.clone(),
            additive: !mutation.clear,
        },
        Some(rules),
    ));
    requests
}

fn compare_category_feedback(
    sim: &Simulation,
    rules: &RuleSet,
    input: &crate::app::input::state::MatchInputState,
    kind: CategoryNavigationKind,
    category: u8,
    has_candidates: bool,
    csf: Option<&crate::assets::csf_file::CsfFile>,
    step: &serde_json::Value,
    label: &str,
) {
    let mixed = input.selection_order.iter().any(|&id| {
        entity_navigation_category(sim, rules, id, kind).is_some_and(|value| value != category)
    });
    let mut keys = vec![category_key(kind, category)];
    let displayed_category = if mixed {
        keys.push("MSG:Mixed");
        "MSG:Mixed"
    } else {
        keys[0]
    };
    keys.push(if !has_candidates {
        "MSG:NavEmpty"
    } else if input.selection_order.is_empty() {
        "MSG:NoUnitsSel"
    } else {
        "MSG:UnitsWorth"
    });
    assert_eq!(
        serde_json::json!(keys),
        step["message_keys"],
        "{label}: localized keys"
    );
    let Some(csf) = csf else {
        // No-archive runs still compare membership, keys and all RNG states.
        // Strict retail checks require the physical localized text below.
        return;
    };
    let worth = input
        .selection_order
        .iter()
        .filter_map(|&id| sim.entities().get(id))
        .filter_map(|entity| {
            rules
                .object(sim.interner.resolve(entity.type_ref()))
                .map(|object| sim.cost_of(entity.owner(), object, rules))
        })
        .fold(0_i32, i32::wrapping_add);
    let text = navigation_feedback(
        has_candidates,
        input.selection_order.len(),
        worth,
        &csf.text(displayed_category),
        &csf.text("MSG:NavEmpty"),
        &csf.text("MSG:NoUnitsSel"),
        &csf.text("MSG:UnitsWorth"),
    );
    let messages = step["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "{label}: one silent navigation message");
    assert_eq!(
        text,
        messages[0]["text"].as_str().unwrap(),
        "{label}: native formatted text"
    );
    assert_eq!(
        messages[0]["silent"], 1,
        "{label}: native message has no sound"
    );
}

fn replay_category_history(
    history: &CombatantHistory,
    csf: Option<&crate::assets::csf_file::CsfFile>,
) -> usize {
    let (mut sim, rules) = fixture(&history.inputs, &history.selected);
    let before = &history.steps[0]["before"];
    let mut input = crate::app::input::state::MatchInputState::new(Default::default());
    input.type_select = boundary_mode(before);
    input.selection_order = history.selected.clone();
    input.follow_target = serde_json::from_value(before["follow"].clone()).unwrap();
    // These are supplied native inputs, including the deliberately forged
    // mode-with-category-minus-one and duplicate-pointer boundary controls.
    input.category_navigation = CategoryNavigation {
        candidates: serde_json::from_value(before["retained_navigation"].clone()).unwrap(),
        health_category: native_last_category(&before["health_category"]),
        veterancy_category: native_last_category(&before["veterancy_category"]),
    };
    let current_house = sim.session.current_house.unwrap();
    sim.houses.get_mut(&current_house).unwrap().is_defeated = history.command_guard != 0;
    // SelectionFixture supplies original A8ED84=1000 for these histories.
    sim.session.binary_frame = 1000;
    let rank_cache: BTreeMap<_, _> = sim
        .entities()
        .values()
        .map(|entity| {
            (
                entity.stable_id(),
                (entity.veterancy.rank_cache(), entity.flashing.frames()),
            )
        })
        .collect();
    let mut target_lines = crate::app::presentation::target_lines::TargetLineState::default();
    target_lines.start_timer(before["action_timer"][0].as_u64().unwrap() as u32);
    let timer_before = target_lines.remaining_frames(sim.session.binary_frame);
    let mut category_steps = 0;
    // This records outputs returned by the real shared voice owner. It is an
    // observer of native queued-byte continuation, not a second admission or
    // RNG implementation and not an audio-device playback claim.
    let mut pending_voices: BTreeMap<u64, i64> = if history.inputs.enslaved_voice_trace {
        before["actor_state"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, actor)| (id.parse().unwrap(), actor["queued_voice"].as_i64().unwrap()))
            .collect()
    } else {
        BTreeMap::new()
    };
    for (index, step) in history.steps.iter().enumerate() {
        let label = format!("{} step {index} {}", history.id, step["command"]);
        assert_navigation_boundary(&input, &sim, &step["before"], &label);
        assert_navigation_rng(&sim, &step["rng_before_hex"], &label);
        if history.inputs.enslaved_voice_trace {
            assert!(
                step["accepted_voice_requests"].is_array(),
                "{label}: actual native writes"
            );
            for (&id, &sound) in &pending_voices {
                assert_eq!(
                    sound,
                    step["before"]["actor_state"][id.to_string()]["queued_voice"]
                        .as_i64()
                        .unwrap(),
                    "{label}: prior queued voice{id}"
                );
            }
        }
        let mut accepted_voice_requests = Vec::new();
        match step["command"].as_str().unwrap() {
            "health" | "veterancy" => {
                let kind = if step["command"] == "health" {
                    CategoryNavigationKind::Health
                } else {
                    CategoryNavigationKind::Veterancy
                };
                let screen = boundary_screen_ids(&step["before"]);
                let before_rng = sim.rng_state();
                let result = category_selection(
                    &sim,
                    &rules,
                    &mut input.category_navigation,
                    &input.type_select,
                    kind,
                    &screen,
                    input.selection_order.clone(),
                    step["key_word"].as_u64().unwrap() & 0x100 == 0,
                );
                assert_eq!(
                    sim.rng_state(),
                    before_rng,
                    "{label}: snapshot/rank scan draws no RNG"
                );
                if let Some(result) = result {
                    accepted_voice_requests = apply_history_selection(
                        &mut sim,
                        &rules,
                        &mut input,
                        &result.mutation,
                        history.voice_enabled,
                        step,
                        &label,
                    );
                    compare_category_feedback(
                        &sim,
                        &rules,
                        &input,
                        kind,
                        result.category,
                        result.has_candidates,
                        csf,
                        step,
                        &label,
                    );
                    // Native7335B6/733900 and7335C1/73390B write the final
                    // category/mode only after voices and formatted feedback.
                    input
                        .category_navigation
                        .record_category(kind, result.category);
                    input
                        .type_select
                        .finish_category_navigation(kind, result.has_candidates);
                    assert_eq!(
                        input.type_select.selection_scope_view().2,
                        None,
                        "{label}: category feedback supersedes P/T outcome"
                    );
                } else {
                    assert!(
                        sim.houses[&current_house].is_defeated,
                        "{label}: authoritative early guard"
                    );
                    for key in [
                        "selection_calls",
                        "voice_requests",
                        "main_draws",
                        "message_keys",
                        "scope_writes",
                        "category_writes",
                    ] {
                        assert!(
                            step[key].as_array().unwrap().is_empty(),
                            "{label}: guarded {key}"
                        );
                    }
                }
                super::super::apply_selection_action_line_policy_at_frame(
                    &mut target_lines,
                    sim.session.binary_frame,
                    super::super::SelectionActionLinePolicy::Preserve,
                );
                assert_eq!(
                    target_lines.remaining_frames(sim.session.binary_frame),
                    timer_before,
                    "{label}: actual Preserve timer owner"
                );
                assert_eq!(
                    step["action_timer"], step["before"]["action_timer"],
                    "{label}: original timer bytes retained"
                );
                for key in ["redraw", "cursor_calls", "timer_writes", "detach_calls"] {
                    assert!(
                        step[key].as_array().unwrap().is_empty(),
                        "{label}: no {key}"
                    );
                }
                assert!(step["center_coord"].is_null(), "{label}: no camera request");
                assert_eq!(
                    step["modes"], step["before"]["modes"],
                    "{label}: targeting modes retained"
                );
                assert_eq!(
                    step["placement"], step["before"]["placement"],
                    "{label}: placement retained"
                );
                category_steps += 1;
            }
            "combatant" => {
                input.type_select.prepare_combatant_scope();
                let screen = boundary_screen_ids(&step["before"]);
                let map: Vec<u64> =
                    serde_json::from_value(step["before"]["map_order"].clone()).unwrap();
                let result = combatant_selection(
                    &sim,
                    &rules,
                    &screen,
                    &map,
                    &input.selection_order,
                    step["key_word"].as_u64().unwrap() & 0x100 == 0,
                    input.type_select.across_map,
                );
                accepted_voice_requests = apply_history_selection(
                    &mut sim,
                    &rules,
                    &mut input,
                    &result.mutation,
                    history.voice_enabled,
                    step,
                    &label,
                );
                let outcome = if input.selection_order.is_empty() {
                    TypeSelectOutcome::Empty
                } else if result.across_map {
                    TypeSelectOutcome::Map
                } else {
                    TypeSelectOutcome::Screen
                };
                input
                    .type_select
                    .finish_combatant_selection(outcome, result.across_map);
            }
            "ordinary_select" | "ordinary_deselect" | "unselect_all" => {
                let mut mutation = SelectionMutation::default();
                match step["command"].as_str().unwrap() {
                    "ordinary_select" => mutation.select.push(step["id"].as_u64().unwrap()),
                    "ordinary_deselect" => mutation.deselect.push(step["id"].as_u64().unwrap()),
                    _ => mutation.clear = true,
                }
                accepted_voice_requests = apply_history_selection(
                    &mut sim,
                    &rules,
                    &mut input,
                    &mutation,
                    history.voice_enabled,
                    step,
                    &label,
                );
            }
            "pointer_expiry" => input
                .category_navigation
                .pointer_expired(step["id"].as_u64().unwrap()),
            "supply_veterancy_raw_bits" => {
                sim.entities_mut()
                    .get_mut(step["id"].as_u64().unwrap())
                    .unwrap()
                    .veterancy
                    .set_raw_for_test(crate::util::native_x87::NativeF32Bits::from_bits(
                        step["bits"].as_u64().unwrap() as u32,
                    ));
            }
            "supply_actor_state" => {
                let entity = sim
                    .entities_mut()
                    .get_mut(step["id"].as_u64().unwrap())
                    .unwrap();
                for (field, value) in step["fields"].as_object().unwrap() {
                    match field.as_str() {
                        "alive" => entity.lifecycle.object_alive = value.as_bool().unwrap(),
                        "limbo" => entity.lifecycle.in_limbo = value.as_bool().unwrap(),
                        "selected" => entity.selected = value.as_bool().unwrap(),
                        "health" => entity.health.set_for_test(value.as_i64().unwrap() as i32),
                        other => panic!("unsupported declared actor-state writer {other}"),
                    }
                }
            }
            "supply_command_guard" => {
                // This compares the bounded admission condition only; arbitrary
                // writes to A8B538 do not establish the whole defeat lifecycle.
                sim.houses.get_mut(&current_house).unwrap().is_defeated =
                    step["value"].as_u64().unwrap() != 0;
            }
            "free_slave_owner_writer" => {
                // The fixture executes this original FreeSlaves writer only.
                // Supply its resulting boundary through the existing link;
                // this replay does not establish the whole release lifecycle.
                assert_eq!(step["entry"], "0x6b0b6d", "{label}: original writer");
                assert_eq!(step["stop"], "0x6b0b98", "{label}: writer boundary");
                let entity = sim
                    .entities_mut()
                    .get_mut(step["id"].as_u64().unwrap())
                    .unwrap();
                entity.slave = crate::sim::slave_manager::SlaveLink::for_test(
                    None,
                    entity.slave.cargo().to_vec(),
                );
            }
            "reset_selection_mode" => input.type_select.note_successful_selection_mutation(false),
            "object_detach_all" | "object_conceal" => {
                // Supply the executed native lifecycle boundary, then test the
                // shared navigation leaf and its next command. Actual Rust
                // broadcast/frame-output/app consumption is covered separately.
                let id = step["id"].as_u64().unwrap();
                input.category_navigation.pointer_expired(id);
                accepted_voice_requests = apply_history_selection(
                    &mut sim,
                    &rules,
                    &mut input,
                    &SelectionMutation {
                        deselect: vec![id],
                        ..Default::default()
                    },
                    history.voice_enabled,
                    step,
                    &label,
                );
                let entity = sim.entities_mut().get_mut(id).unwrap();
                entity.lifecycle.object_alive = step["actor_state"][id.to_string()]["alive"]
                    .as_bool()
                    .unwrap();
                entity.lifecycle.in_limbo = step["actor_state"][id.to_string()]["limbo"]
                    .as_bool()
                    .unwrap();
            }
            "supply_selection_sources" => {
                let map: Vec<u64> = serde_json::from_value(step["map_order"].clone()).unwrap();
                for actor in &history.inputs.actors {
                    if !map.contains(&actor.id) {
                        sim.entities_mut().remove(actor.id);
                    }
                }
            }
            "type" => {
                // The complete TypeSelect AppState path owns a window. This is
                // an explicit original interleave boundary, not a Rust TypeSelect
                // claim; subsequent Y snapshots are derived by the actual owner.
                input.selection_order = boundary_ids(step);
                input.follow_target = serde_json::from_value(step["follow"].clone()).unwrap();
                input.type_select = boundary_mode(step);
                for actor in &history.inputs.actors {
                    sim.entities_mut().get_mut(actor.id).unwrap().selected = step["selected_flags"]
                        [actor.id.to_string()]
                    .as_bool()
                    .unwrap();
                }
                for draw in step["main_draws"].as_array().unwrap() {
                    assert_eq!(
                        u64::from(sim.main_rng.next_u32()),
                        draw.as_u64().unwrap(),
                        "{label}: supplied TypeSelect continuation"
                    );
                }
            }
            other => panic!("unsupported original history command {other}"),
        }
        if history.inputs.enslaved_voice_trace {
            for request in accepted_voice_requests {
                pending_voices.insert(
                    request["id"].as_u64().unwrap(),
                    request["sound_id"].as_i64().unwrap(),
                );
            }
            for (&id, &sound) in &pending_voices {
                assert_eq!(
                    sound,
                    step["queued_voices"][id.to_string()].as_i64().unwrap(),
                    "{label}: native queued voice{id}"
                );
            }
        }
        assert_navigation_boundary(&input, &sim, step, &label);
        assert_navigation_rng(&sim, &step["rng_after_hex"], &label);
        let mut continuation = sim.main_rng.snapshot_for_test();
        for draw in step["main_next_four"].as_array().unwrap() {
            assert_eq!(
                u64::from(continuation.next_u32()),
                draw.as_u64().unwrap(),
                "{label}: next raw Main continuation"
            );
        }
        for entity in sim.entities().values() {
            assert_eq!(
                (entity.veterancy.rank_cache(), entity.flashing.frames()),
                rank_cache[&entity.stable_id()],
                "{label}: navigation samples raw without announcing/promoting {}",
                entity.stable_id()
            );
        }
    }
    assert_eq!(
        sim.main_rng.native_state_hex(),
        history.rng_after_hex["main"],
        "{}: whole Main history",
        history.id
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        history.rng_after_hex["scenario"],
        "{}: whole Scenario history",
        history.id
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        history.rng_after_hex["mapgen"],
        "{}: whole MapGen history",
        history.id
    );
    for &draw in &history.main_next_four {
        assert_eq!(
            sim.main_rng.next_u32(),
            draw,
            "{}: final Main continuation",
            history.id
        );
    }
    category_steps
}

#[test]
fn veterancy_histories_match_native_snapshot_cycle_feedback_and_rng() {
    let corpus = corpus();
    assert_eq!(corpus.veterancy_histories.len(), 38);
    let csf = navigation_csf(&corpus.veterancy_histories);
    let mut excluded = BTreeSet::new();
    let mut compared = 0;
    for history in &corpus.veterancy_histories {
        if history
            .inputs
            .actors
            .iter()
            .any(|actor| actor.robot_offline || !actor.techno_cast)
        {
            excluded.insert(history.id.as_str());
            continue;
        }
        compared += replay_category_history(history, csf.as_ref());
    }
    // RobotOffline still lacks a lifecycle owner; GameEntity supplies a typed
    // Techno identity. Neither is silently mapped to a convenient fixture bit.
    assert_eq!(
        excluded,
        BTreeSet::from(["fallback_robot_offline", "fallback_no_techno_cast"])
    );
    assert_eq!(compared, 62);
}

#[test]
fn health_navigation_histories_match_native_shared_owner_source_and_guard() {
    let corpus = corpus();
    assert_eq!(corpus.health_navigation_histories.len(), 14);
    let csf = navigation_csf(&corpus.health_navigation_histories);
    let compared: usize = corpus
        .health_navigation_histories
        .iter()
        .map(|history| replay_category_history(history, csf.as_ref()))
        .sum();
    assert_eq!(compared, 23);
}

#[test]
fn veterancy_cleanup_histories_match_native_expiry_deselect_and_duplicate_order() {
    let corpus = corpus();
    assert_eq!(corpus.veterancy_cleanup_histories.len(), 13);
    let csf = navigation_csf(&corpus.veterancy_cleanup_histories);
    let compared: usize = corpus
        .veterancy_cleanup_histories
        .iter()
        .map(|history| replay_category_history(history, csf.as_ref()))
        .sum();
    assert_eq!(compared, 18);
}

#[test]
fn enslaved_selection_voices_match_native_lists_release_boundary_gates_and_rng() {
    let corpus = corpus();
    assert_eq!(corpus.enslaved_voice_histories.len(), 28);
    let csf = navigation_csf(&corpus.enslaved_voice_histories);
    let mut category_steps = 0;
    let mut steps = 0;
    let mut writer_boundaries = 0;
    for history in &corpus.enslaved_voice_histories {
        assert!(history.inputs.enslaved_voice_trace && history.inputs.navigation_trace);
        assert!(history.inputs.actors.iter().all(|actor| {
            actor.enslaved_voice_list.is_some() && !actor.robot_offline && actor.techno_cast
        }));
        for step in &history.steps {
            for boundary in [&step["before"], step] {
                for actor in boundary["actor_state"].as_object().unwrap().values() {
                    assert!(actor["slave_owner_present"].is_boolean());
                    assert!(actor["queued_voice"].is_i64());
                }
            }
            assert_eq!(
                step["voice_writes"].as_array().unwrap().len(),
                step["accepted_voice_requests"].as_array().unwrap().len(),
                "{}: retained actual QueueVoice writes, including rejection controls",
                history.id
            );
            writer_boundaries += usize::from(step["command"] == "free_slave_owner_writer");
        }
        category_steps += replay_category_history(history, csf.as_ref());
        steps += history.steps.len();
    }
    assert_eq!((steps, category_steps, writer_boundaries), (76, 68, 4));
}

#[test]
fn veterancy_navigation_samples_original_raw_rank_without_cached_rank_writes() {
    let corpus = corpus();
    assert_eq!(corpus.veterancy_rank_cases.len(), 16);
    let history = &corpus.veterancy_histories[0];
    for row in &corpus.veterancy_rank_cases {
        let (mut sim, rules) = fixture(&history.inputs, &[]);
        let id = history.inputs.actors[0].id;
        let entity = sim.entities_mut().get_mut(id).unwrap();
        entity
            .veterancy
            .set_raw_for_test(crate::util::native_x87::NativeF32Bits::from_bits(
                row["raw_bits"].as_u64().unwrap() as u32,
            ));
        // The executed750030 leaf never reads Techno's announced-rank cache.
        // A deliberately stale valid value catches using it instead of raw.
        entity
            .veterancy
            .set_rank_cache_for_test(((row["rank"].as_u64().unwrap() + 1) % 3) as i8);
        let cache = entity.veterancy.rank_cache();
        let flash = entity.flashing.frames();
        let before_rng = sim.rng_state();
        assert_eq!(
            entity_navigation_category(&sim, &rules, id, CategoryNavigationKind::Veterancy),
            Some(row["rank"].as_u64().unwrap() as u8),
            "{}: original750030 rank",
            row["id"]
        );
        assert_eq!(sim.rng_state(), before_rng, "{}: no RNG", row["id"]);
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(
            (entity.veterancy.rank_cache(), entity.flashing.frames()),
            (cache, flash),
            "{}: read-only raw rank",
            row["id"]
        );
        assert_navigation_rng(&sim, &row["rng_after_hex"], row["id"].as_str().unwrap());
    }
}

#[test]
fn combatant_native_cleanup_boundaries_reconcile_without_resetting_map_scope() {
    let corpus = corpus();
    assert_eq!(corpus.combatant_cleanup_histories.len(), 6);
    let mut compared = 0;
    for history in &corpus.combatant_cleanup_histories {
        let native_p = history.steps.last().unwrap();
        assert_eq!(native_p["command"], "combatant");
        let cleanup = &native_p["before"];
        for pending in [false, true] {
            let label = format!("{} pending={pending}", history.id);
            let (mut sim, rules) = fixture(&history.inputs, &history.selected);
            let mut input = crate::app::input::state::MatchInputState::new(Default::default());
            input.selection_order = history.selected.clone();
            input.selection_order_pending = pending;
            input.type_select = boundary_mode(&history.steps[0]["before"]);
            // Supply the executed native lifecycle boundary, not a second
            // Detach/Conceal implementation. The separate phase regression
            // executes Rust's actual Destroy owner and command transport.
            let native_map: Vec<u64> =
                serde_json::from_value(cleanup["map_order"].clone()).unwrap();
            for actor in &history.inputs.actors {
                if !native_map.contains(&actor.id) {
                    sim.entities_mut().remove(actor.id);
                    continue;
                }
                let boundary = &cleanup["actor_state"][actor.id.to_string()];
                let entity = sim.entities_mut().get_mut(actor.id).unwrap();
                entity.lifecycle.object_alive = boundary["alive"].as_bool().unwrap();
                entity.lifecycle.in_limbo = boundary["limbo"].as_bool().unwrap();
                entity.selected = boundary["selected"].as_bool().unwrap();
            }
            // A consumed input can leave a different committed selection
            // after lifecycle/refusal. Matching selected bits alone must not
            // keep that provisional ledger alive once its queue is empty.
            assert!(sim.pending_command_snapshot().is_empty());
            reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules));
            assert_eq!(
                input.selection_order,
                boundary_ids(cleanup),
                "{label}: cleanup membership"
            );
            assert!(
                !input.selection_order_pending,
                "{label}: consumed transport"
            );
            assert_eq!(
                input.type_select.selection_scope_view().0,
                "combatant",
                "{label}: Deselect/expiry retain mode1"
            );
            assert_eq!(
                input.type_select.across_map,
                cleanup["across_map"].as_bool().unwrap(),
                "{label}: cleanup retains map scope"
            );
            let screen: Vec<u64> = serde_json::from_value(cleanup["screen_order"].clone()).unwrap();
            input.type_select.prepare_combatant_scope();
            let result = combatant_selection(
                &sim,
                &rules,
                &screen,
                &native_map,
                &input.selection_order,
                true,
                input.type_select.across_map,
            );
            let effects = resolve_selection_mutation(
                &sim,
                Some(&rules),
                input.selection_order,
                None,
                false,
                &result.mutation,
            );
            assert_eq!(
                effects.ordered,
                boundary_ids(native_p),
                "{label}: next P membership/order"
            );
            assert!(result.across_map, "{label}: next P scans the retained map");
            let native_adds: Vec<_> = native_p["selection_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["name"] == "select" && call["accepted"] == true)
                .map(|call| call["id"].as_u64().unwrap())
                .collect();
            assert_eq!(
                effects.successful_adds, native_adds,
                "{label}: next P admission order"
            );
            for &id in &effects.successful_adds {
                assert!(sim.selection_voice_request(&rules, id, true).is_some());
            }
            assert_eq!(
                sim.main_rng.native_state_hex(),
                history.rng_after_hex["main"],
                "{label}: Main continuation"
            );
            for native_draw in &history.main_next_four {
                assert_eq!(
                    sim.main_rng.next_u32(),
                    *native_draw,
                    "{label}: next raw Main"
                );
            }
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                history.rng_after_hex["scenario"],
                "{label}: Scenario unchanged"
            );
            assert_eq!(
                sim.mapgen_rng.native_state_hex(),
                history.rng_after_hex["mapgen"],
                "{label}: MapGen unchanged"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 12);
}

#[test]
fn combatant_pending_transport_uses_selection_queue_and_keeps_native_cleanup_scope() {
    use crate::sim::command::CommandEnvelope;
    let corpus = corpus();
    let history = corpus
        .combatant_cleanup_histories
        .iter()
        .find(|row| row.id == "initialized_object_conceal_then_P")
        .unwrap();
    for queued_selection in [false, true] {
        let (mut sim, rules) = fixture(&history.inputs, &[]);
        let mut input = crate::app::input::state::MatchInputState::new(Default::default());
        input.type_select = boundary_mode(&history.steps[0]["before"]);
        input.selection_order = history.selected.clone();
        input.selection_order_pending = true;
        let actor = sim.entities_mut().get_mut(20).unwrap();
        actor.lifecycle.in_limbo = true;
        let owner = sim.session.current_house.unwrap();
        let command = if queued_selection {
            Command::Select {
                entity_ids: vec![20],
                additive: false,
            }
        } else {
            Command::Stop { entity_id: 40 }
        };
        sim.queue_command(CommandEnvelope::new(owner, 2, command));
        reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules));
        assert_eq!(input.selection_order_pending, queued_selection);
        assert_eq!(
            input.selection_order,
            if queued_selection { vec![20] } else { vec![] }
        );
        assert_eq!(input.type_select.selection_scope_view().0, "combatant");
        assert!(input.type_select.across_map);
        assert_eq!(
            sim.pending_command_snapshot().len(),
            1,
            "reconciliation is read-only on queued work"
        );

        // Future selection retains the provisional ledger until its existing
        // queue owner drains it. A refused Select after Conceal then closes
        // pending state instead of waiting for impossible bit equality.
        sim.session.tick = 1;
        let due = sim.take_due_commands();
        for command in due {
            let _ = sim.apply_command("local", &command.payload, Some(&rules));
        }
        reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules));
        assert!(!input.selection_order_pending);
        assert!(input.selection_order.is_empty());
        assert_eq!(input.type_select.selection_scope_view().0, "combatant");
        assert!(input.type_select.across_map);
    }
}

#[test]
fn combatant_histories_match_native_membership_scope_follow_and_voice_continuation() {
    use crate::sim::rng::trace_draws;
    use serde_json::json;

    let corpus = corpus();
    assert_eq!(corpus.combatant_histories.len(), 71);
    let mut excluded = BTreeSet::new();
    let mut p_steps = 0;
    for history in &corpus.combatant_histories {
        if history.inputs.actors.iter().any(|actor| {
            actor.robot_offline || !actor.techno_cast || actor.voice_list.contains(&-1)
        }) || history.command_guard != 0
        {
            excluded.insert(history.id.as_str());
            continue;
        }
        let label = &history.id;
        let (mut sim, rules) = fixture(&history.inputs, &history.selected);
        let mut ordered = history.selected.clone();
        let mut input = boundary_mode(&history.steps[0]["before"]);
        let mut follow: Option<u64> =
            serde_json::from_value(history.steps[0]["before"]["follow"].clone()).unwrap();
        assert_eq!(
            sim.main_rng.native_state_hex(),
            history.rng_before_hex["main"],
            "{label}"
        );
        let scenario_before = sim.scenario_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        for (index, step) in history.steps.iter().enumerate() {
            if step["command"] != "combatant" {
                if step["command"] == "ordinary_deselect" {
                    let effects = resolve_selection_mutation(
                        &sim,
                        Some(&rules),
                        ordered,
                        follow,
                        false,
                        &SelectionMutation {
                            deselect: vec![step["id"].as_u64().unwrap()],
                            ..Default::default()
                        },
                    );
                    if effects.native_selection_mode_reset {
                        input.note_successful_selection_mutation(false);
                    }
                    ordered = effects.ordered;
                    follow = effects.follow_target;
                    assert_eq!(
                        ordered,
                        boundary_ids(step),
                        "{label}: direct Deselect membership"
                    );
                    assert_eq!(
                        follow,
                        serde_json::from_value::<Option<u64>>(step["follow"].clone()).unwrap(),
                        "{label}: direct Deselect Follow"
                    );
                    assert_eq!(
                        input.selection_scope_view().0,
                        "combatant",
                        "{label}: direct Deselect retains mode1"
                    );
                    assert_eq!(
                        input.across_map,
                        step["across_map"].as_bool().unwrap(),
                        "{label}: direct Deselect retains map latch"
                    );
                    continue;
                }
                // Interleaved commands supply observed original boundary state.
                // Their algorithms are covered by their own corpora/captures;
                // this does not assert their Rust implementation from replaying
                // original draws. Consecutive P steps retain Rust's own result.
                ordered = boundary_ids(step);
                follow = serde_json::from_value(step["follow"].clone()).unwrap();
                input = boundary_mode(step);
                for draw in step["main_draws"].as_array().unwrap() {
                    assert_eq!(
                        u64::from(sim.main_rng.next_u32()),
                        draw.as_u64().unwrap(),
                        "{label} step {index}"
                    );
                }
                continue;
            }
            let before = &step["before"];
            assert_eq!(
                ordered,
                boundary_ids(before),
                "{label} step {index}: retained ledger"
            );
            assert_eq!(
                follow,
                serde_json::from_value::<Option<u64>>(before["follow"].clone()).unwrap(),
                "{label} step {index}: retained Follow"
            );
            input.prepare_combatant_scope();
            let rng_before = sim.rng_state();
            let screen: Vec<_> = history.screen_order.iter().flatten().copied().collect();
            let result = combatant_selection(
                &sim,
                &rules,
                &screen,
                &history.map_order,
                &ordered,
                step["key_word"].as_u64().unwrap() & 0x100 == 0,
                input.across_map,
            );
            let effects = resolve_selection_mutation(
                &sim,
                Some(&rules),
                ordered,
                follow,
                before["placement"].as_bool().unwrap(),
                &result.mutation,
            );
            assert_eq!(
                sim.rng_state(),
                rng_before,
                "{label} step {index}: selection scan/admission draws no RNG"
            );
            let native_adds: Vec<_> = step["selection_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["name"] == "select" && call["accepted"] == true)
                .map(|call| call["id"].as_u64().unwrap())
                .collect();
            assert_eq!(
                effects.successful_adds, native_adds,
                "{label} step {index}: actual Select order"
            );
            ordered = effects.ordered;
            follow = effects.follow_target;
            assert_eq!(
                ordered,
                boundary_ids(step),
                "{label} step {index}: immediate membership/order"
            );
            assert_eq!(
                follow,
                serde_json::from_value::<Option<u64>>(step["follow"].clone()).unwrap(),
                "{label} step {index}: Follow cleanup"
            );
            assert_eq!(
                follow.is_some(),
                step["follow_enabled"].as_bool().unwrap(),
                "{label} step {index}"
            );
            for actor in &history.inputs.actors {
                assert_eq!(
                    ordered.contains(&actor.id),
                    step["selected_flags"][actor.id.to_string()]
                        .as_bool()
                        .unwrap(),
                    "{label} step {index}: bit{}",
                    actor.id
                );
            }
            let outcome = if ordered.is_empty() {
                TypeSelectOutcome::Empty
            } else if result.across_map {
                TypeSelectOutcome::Map
            } else {
                TypeSelectOutcome::Screen
            };
            input.finish_combatant_selection(outcome, result.across_map);
            assert_eq!(
                step["selection_mode"], 1,
                "{label} step {index}: original P mode"
            );
            assert_eq!(
                input.selection_scope_view(),
                (
                    "combatant",
                    step["across_map"].as_bool().unwrap(),
                    Some(step["message_keys"][0].as_str().unwrap())
                ),
                "{label} step {index}: mode/scope/feedback"
            );
            let (requests, draws) = trace_draws(|| {
                effects.successful_adds.iter().filter_map(|&id| {
                    sim.selection_voice_request(&rules, id, history.voice_enabled).map(|sound| {
                        json!({"id": id, "sound_id": sound.strip_prefix("Sound").unwrap().parse::<i64>().unwrap()})
                    })
                }).collect::<Vec<_>>()
            });
            assert_eq!(
                requests,
                *step["voice_requests"].as_array().unwrap(),
                "{label} step {index}: queued voices in admission order"
            );
            assert_eq!(
                draws
                    .iter()
                    .map(|draw| draw["value"].clone())
                    .collect::<Vec<_>>(),
                *step["main_draws"].as_array().unwrap(),
                "{label} step {index}: raw Main draws"
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                scenario_before,
                "{label}: Scenario"
            );
            assert_eq!(
                sim.mapgen_rng.logical_state(),
                mapgen_before,
                "{label}: MapGen"
            );
            assert_eq!(
                step["modes"], before["modes"],
                "{label}: no mode cancellation"
            );
            assert_eq!(
                step["placement"], before["placement"],
                "{label}: placement retained"
            );
            assert_eq!(
                step["action_timer"], before["action_timer"],
                "{label}: selected-action timer retained"
            );
            for key in ["redraw", "cursor_calls", "timer_writes", "detach_calls"] {
                assert!(
                    step[key].as_array().unwrap().is_empty(),
                    "{label}: no {key}"
                );
            }
            assert!(
                step["center_coord"].is_null(),
                "{label}: no camera centering"
            );
            // The app sends this complete ledger to the existing snapshot
            // receiver, which preserves requested old members and removes
            // omitted ones regardless of the gesture's additive marker.
            assert!(sim.apply_command(
                "local",
                &Command::Select {
                    entity_ids: ordered.clone(),
                    additive: !result.mutation.clear,
                },
                Some(&rules)
            ));
            for actor in &history.inputs.actors {
                assert_eq!(
                    sim.entities().get(actor.id).unwrap().selected,
                    step["selected_flags"][actor.id.to_string()]
                        .as_bool()
                        .unwrap(),
                    "{label} step {index}: committed bit{}",
                    actor.id
                );
            }
            p_steps += 1;
        }
        assert_eq!(
            sim.main_rng.native_state_hex(),
            history.rng_after_hex["main"],
            "{label}: complete Main continuation"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            history.rng_after_hex["scenario"],
            "{label}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            history.rng_after_hex["mapgen"],
            "{label}"
        );
        for &expected in &history.main_next_four {
            assert_eq!(
                sim.main_rng.next_u32(),
                expected,
                "{label}: following raw draw"
            );
        }
    }
    assert_eq!(p_steps, 76);
    // These are declared bounds, not silent mappings to convenient Rust bits.
    // RobotOffline has no lifecycle owner; a typed GameEntity has a Techno
    // identity. GuardA8B538 belongs to the wider keyboard/session lifecycle.
    // Raw numeric sound-1 is not a bound production SOUNDMD name; the voice
    // owner's existing direct corpus covers its spent-draw/rejected-queue tail.
    assert_eq!(
        excluded,
        BTreeSet::from([
            "screen_null_and_nontechno",
            "map_alive_gate_accepts_nontechno",
            "infantry_robot_offline_distinguishes_escalation_and_final_select",
            "unit_robot_offline_distinguishes_escalation_and_final_select",
            "guarded_P_preserves_all",
            "voice_minus_one_already_spent_draw",
        ])
    );
}

#[test]
fn combatant_shared_mode_health_retains_native_map_latch() {
    let mut compared = 0;
    for history in &corpus().combatant_histories {
        for step in &history.steps {
            if step["command"] != "health" {
                continue;
            }
            // The three controls contain either an empty world/snapshot or
            // ordinary on-screen actors. Health's first category may select
            // none even while its retained snapshot is nonempty.
            let mut input = boundary_mode(&step["before"]);
            input.finish_category_navigation(
                CategoryNavigationKind::Health,
                !history.inputs.actors.is_empty(),
            );
            assert_eq!(
                input.category_navigation_continues(CategoryNavigationKind::Health),
                step["selection_mode"] == 3,
                "{}: Health mode",
                history.id
            );
            assert_eq!(
                input.across_map,
                step["across_map"].as_bool().unwrap(),
                "{}: Health retains the shared map latch",
                history.id
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 3);
}

#[test]
fn combatant_defeat_admission_matches_original_early_guard() {
    let corpus = corpus();
    let row = corpus
        .combatant_histories
        .iter()
        .find(|history| history.id == "guarded_P_preserves_all")
        .unwrap();
    let (mut sim, _) = fixture(&row.inputs, &row.selected);
    assert!(selection_navigation_allowed(&sim));
    let local = sim.session.current_house.unwrap();
    sim.houses.get_mut(&local).unwrap().is_defeated = true;
    assert!(!selection_navigation_allowed(&sim));
    let step = &row.steps[0];
    for key in [
        "selected",
        "selected_flags",
        "follow",
        "follow_enabled",
        "modes",
        "placement",
        "selection_mode",
        "across_map",
        "action_timer",
    ] {
        assert_eq!(
            step[key], step["before"][key],
            "original early guard preserves {key}"
        );
    }
    for key in [
        "selection_calls",
        "voice_requests",
        "main_draws",
        "message_keys",
        "timer_writes",
        "detach_calls",
    ] {
        assert!(
            step[key].as_array().unwrap().is_empty(),
            "original early guard has no {key}"
        );
    }
    assert_eq!(row.rng_before_hex, row.rng_after_hex);
}

fn direction(value: &str) -> ObjectDirection {
    match value {
        "next" => ObjectDirection::Next,
        "previous" => ObjectDirection::Previous,
        other => panic!("unexpected native direction {other}"),
    }
}

fn fixture(inputs: &Inputs, selected: &[u64]) -> (Simulation, RuleSet) {
    // Per-actor type names keep the native type Selectable byte independent.
    // These are explicit source inputs to the production INI reader, not
    // asserted retail rosters or a second implementation of the predicates.
    let mut ini = String::new();
    if inputs.navigation_trace {
        // Declared native SelectionFixture rules+1700/+1708 inputs, not the
        // constructor defaults: red defaults0.5 before retail authors25%.
        ini.push_str("[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n");
    }
    for (section, kind) in [
        ("InfantryTypes", "infantry"),
        ("VehicleTypes", "unit"),
        ("BuildingTypes", "building"),
    ] {
        writeln!(ini, "[{section}]").unwrap();
        for (index, actor) in inputs.actors.iter().filter(|a| a.kind == kind).enumerate() {
            writeln!(ini, "{index}=ACTOR{}", actor.id).unwrap();
        }
        if kind == "building" {
            writeln!(ini, "helper=DOCKBUILDING").unwrap();
        }
    }
    for actor in &inputs.actors {
        assert!(actor.voice_list.iter().all(|&id| id >= -1));
        assert!(
            actor
                .enslaved_voice_list
                .iter()
                .flatten()
                .all(|&id| id >= -1)
        );
        writeln!(
            ini,
            "[ACTOR{}]\nStrength={}\nCost={}\nSelectable={}\nIsSelectableCombatant={}\nPrimary={}\nVoiceSelect={}",
            actor.id,
            actor.strength,
            actor.cost,
            if actor.selectable { "yes" } else { "no" },
            if actor.is_selectable_combatant { "yes" } else { "no" },
            if actor.positive_primary_damage { "GUN" } else { "QUIETGUN" },
            actor.voice_list.iter().filter(|&&id| id >= 0).map(|id| format!("Sound{id}")).collect::<Vec<_>>().join(","),
        )
        .unwrap();
        if let Some(voices) = &actor.enslaved_voice_list {
            writeln!(
                ini,
                "VoiceSelectEnslaved={}",
                voices
                    .iter()
                    .filter(|&&id| id >= 0)
                    .map(|id| format!("Sound{id}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
            .unwrap();
        }
    }
    ini.push_str(
        "[DOCKBUILDING]\nStrength=100\nFoundation=1x1\n[GUN]\nDamage=10\n[QUIETGUN]\nDamage=0\n",
    );
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).expect("native control types");
    for actor in &inputs.actors {
        let unbound = |voices: &[i64]| voices.contains(&-1);
        if unbound(&actor.voice_list) || actor.enslaved_voice_list.as_deref().is_some_and(unbound) {
            assert!(
                inputs.enslaved_voice_trace,
                "raw unresolved vectors require the declared native boundary"
            );
            let supplied_names = |voices: &[i64]| {
                voices
                    .iter()
                    .map(|&id| match id {
                        -1 => String::new(),
                        0.. => format!("Sound{id}"),
                        other => panic!("unsupported supplied sound index {other}"),
                    })
                    .collect()
            };
            // Original fixture vectors may deliberately contain ID-1; the
            // real ReadSoundList cannot produce it. Supply only that declared
            // boundary through RuleSet's fixture capability. Ordinary lists
            // above still come from the actual exact-case INI reader.
            rules.supply_selection_voice_vectors_for_test(
                &format!("ACTOR{}", actor.id),
                supplied_names(&actor.voice_list),
                supplied_names(actor.enslaved_voice_list.as_deref().unwrap_or_default()),
            );
        }
    }
    let mut sim = Simulation::with_seed(inputs.seed);
    let local = sim.interner.intern("local");
    let other = sim.interner.intern("other");
    sim.session.current_house = Some(local);
    sim.session.game_mode_nonzero = !inputs.house.campaign;
    sim.session.house_order = vec![local, other];
    sim.houses
        .insert(local, HouseState::new(local, 0, None, true, 0, 10));
    let mut other_house = HouseState::new(other, 1, None, inputs.house.other_human, 0, 10);
    other_house.player_control = inputs.house.other_control;
    other_house.multiplay_passive = inputs.house.other_passive;
    sim.houses.insert(other, other_house);
    for actor in &inputs.actors {
        assert!(!actor.robot_offline && actor.techno_cast);
        let category = match actor.kind.as_str() {
            "infantry" => EntityCategory::Infantry,
            "unit" => EntityCategory::Unit,
            "building" => EntityCategory::Structure,
            other => panic!("unexpected native class {other}"),
        };
        let owner = match actor.owner.as_str() {
            "local" => local,
            "other" => other,
            other => panic!("unexpected native owner {other}"),
        };
        let type_ref = sim.interner.intern(&format!("ACTOR{}", actor.id));
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            actor.id,
            0,
            0,
            0,
            0,
            owner,
            Health::new(actor.health),
            type_ref,
            category,
            0,
            5,
            false,
        );
        entity.lifecycle.object_alive = actor.alive;
        entity.lifecycle.in_limbo = actor.limbo;
        entity.in_playfield = actor.in_playfield;
        entity.discovery.discovered_by_current_house = actor.discovered;
        entity.selected = selected.contains(&actor.id);
        if let Some(bits) = actor.veterancy_raw_bits {
            entity
                .veterancy
                .set_raw_for_test(crate::util::native_x87::NativeF32Bits::from_bits(bits));
        }
        entity.foot_locomotor_swap_active = actor.locomotor_swap;
        if actor.mission_only {
            entity.mark_mission_only();
        }
        if actor.virtual_1d4 {
            entity.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(999);
        }
        if actor.slave {
            entity.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(999), Vec::new());
        }
        if actor.bunker {
            entity.bunker_link = BunkerLink::Installed(999);
        }
        if actor.docked {
            entity.dock_entered_with = Some(999);
        }
        let [x, y, z] = actor.coord;
        ground_pose::put_location(&mut entity.position, DriveCoord { x, y, z });
        if actor.cell_building {
            let building_type = sim.interner.intern("DOCKBUILDING");
            let mut building = GameEntity::new_at_frame_zero_for_test(
                1000 + actor.id,
                entity.position.rx,
                entity.position.ry,
                0,
                0,
                owner,
                Health::new(100),
                building_type,
                EntityCategory::Structure,
                0,
                0,
                false,
            );
            building.lifecycle.object_alive = true;
            building.lifecycle.in_limbo = false;
            sim.entities_mut().insert(building);
        }
        sim.entities_mut().insert(entity);
    }
    // Retained layer state is an input, independent of storage ID order and
    // current sort keys. Use its existing persisted representation. Native
    // NULL holes do no work; the Rust owner forbids ID zero and stores only
    // members, so compact those holes without changing encounter order.
    let layers = inputs
        .layers
        .each_ref()
        .map(|ids| ids.iter().flatten().copied().collect::<Vec<_>>());
    let mut state = serde_json::to_value(&sim).expect("serialize selection fixture");
    state["substrate"]["display"] = serde_json::to_value(&layers).unwrap();
    let mut sim: Simulation = serde_json::from_value(state).expect("retained display state");
    // Installing retained display through serde invokes the intentional native
    // load resets for Scenario/Main. This is a supplied fresh-object fixture,
    // not a load history: restore its declared seed through the existing test
    // pair owner. SelectionFixture177..182 also seeds MapGen identically;
    // that explicit input differs from production's fresh MapGen Seed(0).
    sim.initialize_random_number_system(inputs.seed as u32);
    sim.mapgen_rng = crate::sim::rng::SimRng::new(inputs.seed);
    for (index, ids) in layers.iter().enumerate() {
        assert_eq!(
            sim.display_layers()
                .members(DisplayLayer::from_index(index as u8).unwrap()),
            ids
        );
    }
    (sim, rules)
}

#[test]
fn next_previous_search_and_candidate_gates_match_original_execution() {
    let corpus = corpus();
    assert_eq!(corpus.search_cases.len(), 94);
    let mut excluded = BTreeSet::new();
    let mut compared = 0;
    for row in &corpus.search_cases {
        if row
            .inputs
            .actors
            .iter()
            .any(|a| a.robot_offline || !a.techno_cast)
        {
            excluded.insert(row.id.as_str());
            continue;
        }
        let (sim, rules) = fixture(&row.inputs, &[]);
        assert_eq!(
            next_object(&sim, Some(&rules), row.anchor, direction(&row.direction)),
            row.candidate,
            "{}: retained display search",
            row.id
        );
        for visit in &row.visits {
            let entity = sim.entities().get(visit.id).unwrap();
            let accepted = match visit.gate.as_str() {
                "local_gate" => {
                    entity_pick::is_local_player_selectable_object(&sim, Some(&rules), entity)
                }
                "dynamic_gate" => {
                    entity_pick::selection_dynamic_prefix(
                        entity,
                        sim.entities(),
                        Some(&rules),
                        Some(&sim.interner),
                    ) && sim.object_is_selectable(visit.id, Some(&rules))
                }
                other => panic!("unexpected native gate {other}"),
            };
            assert_eq!(
                accepted, visit.accepted,
                "{}: {} for {}",
                row.id, visit.gate, visit.id
            );
        }
        assert!(row.rng_unchanged, "{}: native search RNG boundary", row.id);
        compared += 1;
    }
    assert_eq!(compared, 86);
    // RobotOffline's producer/lifecycle has no owner yet. The raw cast-bit
    // controls deliberately supply a malformed Techno, which a typed Rust
    // GameEntity cannot represent. Neither is silently mapped to another bit.
    assert_eq!(
        excluded,
        BTreeSet::from([
            "infantry_robot_offline_next",
            "infantry_robot_offline_previous",
            "unit_robot_offline_next",
            "unit_robot_offline_previous",
            "infantry_no_techno_cast_next",
            "infantry_no_techno_cast_previous",
            "unit_no_techno_cast_next",
            "unit_no_techno_cast_previous",
        ])
    );
}

#[test]
fn execute_histories_match_selection_follow_modes_placement_and_camera() {
    let corpus = corpus();
    assert_eq!(corpus.command_histories.len(), 20);
    let mut unrepresented_modes = BTreeSet::new();
    let mut compared_steps = 0;
    for row in &corpus.command_histories {
        let (mut sim, rules) = fixture(&row.inputs, &row.selected);
        let mut ordered = row.selected.clone();
        let mut pending = false;
        let mut follow = row.follow;
        let mut modes = SidebarGadgetState {
            repair_mode_on: row.modes.repair,
            sell_mode_on: row.modes.sell,
            ..Default::default()
        };
        if row.modes.power || row.modes.planning {
            unrepresented_modes.insert(row.id.as_str());
        }
        // Native fixture input B0FE54=3, B0FE58=1. Reach that state through
        // the existing mode owner, then use its ordinary-selection reset.
        let mut type_select = TypeSelectInputState::default();
        type_select.finish_category_navigation(CategoryNavigationKind::Health, true);
        type_select.across_map = true;
        assert_eq!(row.directions.len(), row.steps.len());
        for (index, (requested, expected)) in row.directions.iter().zip(&row.steps).enumerate() {
            assert_eq!(requested, &expected.direction);
            modes.disable_selection_modes(row.placement);
            let current = selected_stable_ids_in_order(Some(&sim), Some(&rules), &ordered, pending);
            let candidate = next_object(
                &sim,
                Some(&rules),
                current.first().copied(),
                direction(requested),
            );
            let mut center_coord = None;
            if let Some(id) = candidate {
                let effects = resolve_selection_mutation(
                    &sim,
                    Some(&rules),
                    current,
                    follow,
                    row.placement,
                    &SelectionMutation {
                        clear: true,
                        select: vec![id],
                        ..Default::default()
                    },
                );
                assert!(effects.native_selection_mode_reset);
                assert_eq!(
                    effects.successful_adds, expected.selected,
                    "{} step {index}: actual Select admission",
                    row.id
                );
                ordered = effects.ordered;
                follow = effects.follow_target;
                pending = true;
                type_select.reset_scope();
                let coords: Vec<_> = ordered
                    .iter()
                    .map(|id| {
                        let entity = sim.entities().get(*id).unwrap();
                        let coord =
                            ground_pose::object_location(entity, sim.resolved_terrain.as_ref());
                        (coord.x, coord.y, coord.z)
                    })
                    .collect();
                center_coord =
                    camera::selection_view_centre_leptons(&coords).map(|(x, y, z)| [x, y, z]);
            }
            assert_eq!(
                ordered, expected.selected,
                "{} step {index}: immediate membership",
                row.id
            );
            assert_eq!(
                follow, expected.follow,
                "{} step {index}: Follow cleanup",
                row.id
            );
            assert_eq!(
                follow.is_some(),
                expected.follow_enabled,
                "{} step {index}: Follow enable",
                row.id
            );
            assert_eq!(
                modes.repair_mode_on, expected.modes.repair,
                "{} step {index}: Repair",
                row.id
            );
            assert_eq!(
                modes.sell_mode_on, expected.modes.sell,
                "{} step {index}: Sell",
                row.id
            );
            if !row.modes.power && !row.modes.planning {
                assert!(!expected.modes.power && !expected.modes.planning);
            }
            assert_eq!(
                row.placement, expected.placement,
                "{} step {index}: placement retained",
                row.id
            );
            assert_eq!(
                type_select.category_navigation_continues(CategoryNavigationKind::Health),
                expected.selection_mode == 3,
                "{} step {index}: selection mode",
                row.id
            );
            assert_eq!(
                type_select.across_map, expected.selection_across_map,
                "{} step {index}: selection scope",
                row.id
            );
            assert_eq!(
                center_coord, expected.center_coord,
                "{} step {index}: camera coordinate",
                row.id
            );
            // Check native platform handoff reachability, without claiming a
            // window redraw or a camera projection from this headless seam.
            assert_eq!(
                candidate.is_some(),
                expected
                    .call_order
                    .iter()
                    .any(|call| call == "unselect_all"),
                "{} step {index}: UnselectAll caller",
                row.id
            );
            assert_eq!(
                candidate.is_some(),
                !expected.redraw.is_empty(),
                "{} step {index}: redraw handoff",
                row.id
            );
            for actor in &row.inputs.actors {
                assert_eq!(
                    ordered.contains(&actor.id),
                    expected.selected_flags[&actor.id.to_string()],
                    "{} step {index}: pending bit for {}",
                    row.id,
                    actor.id
                );
                assert_eq!(
                    sim.entities().get(actor.id).unwrap().selected,
                    row.selected.contains(&actor.id),
                    "{} step {index}: sim must remain uncommitted",
                    row.id
                );
            }
            compared_steps += 1;
        }
        // Only after the whole key sequence, commit the final local snapshot
        // through the production command receiver and its shared Select gate.
        assert!(sim.apply_command(
            "local",
            &Command::Select {
                entity_ids: ordered.clone(),
                additive: false
            },
            Some(&rules)
        ));
        let expected = row.steps.last().unwrap();
        for actor in &row.inputs.actors {
            assert_eq!(
                sim.entities().get(actor.id).unwrap().selected,
                expected.selected_flags[&actor.id.to_string()],
                "{}: committed bit for {}",
                row.id,
                actor.id
            );
        }
        assert_eq!(
            selected_stable_ids_in_order(Some(&sim), Some(&rules), &ordered, false),
            expected.selected,
            "{}: post-commit ledger",
            row.id
        );
    }
    assert_eq!(compared_steps, 31);
    // Selection, Follow, placement and camera still compare in these four
    // histories. Only power/planning mode state lacks a production owner.
    assert_eq!(
        unrepresented_modes,
        BTreeSet::from([
            "cancel_power",
            "cancel_planning",
            "placement_preserves_power_refuses_selection",
            "placement_preserves_planning_refuses_selection",
        ])
    );
}

#[test]
fn snapshot_restores_singleton_anchor_and_retained_layer_order_for_next_key() {
    use crate::sim::snapshot::GameSnapshot;

    let corpus = corpus();
    let row = corpus
        .command_histories
        .iter()
        .find(|row| row.id == "consecutive_next_previous")
        .unwrap();
    let (mut sim, rules) = fixture(&row.inputs, &row.selected);
    // Supply a valid monotonic allocator/Logic state for the authored IDs so
    // the production restore transaction can validate the fixture normally.
    for _ in 0..row
        .inputs
        .actors
        .iter()
        .map(|actor| actor.id)
        .max()
        .unwrap()
    {
        sim.allocate_stable_id();
    }
    for actor in &row.inputs.actors {
        sim.register_live_object(actor.id);
    }
    sim.session.map_name = row.id.clone();
    let current = selected_stable_ids_in_order(Some(&sim), Some(&rules), &row.selected, false);
    let candidate = next_object(
        &sim,
        Some(&rules),
        current.first().copied(),
        direction(&row.directions[0]),
    )
    .unwrap();
    let first = resolve_selection_mutation(
        &sim,
        Some(&rules),
        current,
        row.follow,
        false,
        &SelectionMutation {
            clear: true,
            select: vec![candidate],
            ..Default::default()
        },
    );
    assert_eq!(first.ordered, row.steps[0].selected);
    assert_eq!(first.ordered.len(), 1);
    assert_eq!(first.follow_target, row.steps[0].follow);
    assert!(sim.apply_command(
        "local",
        &Command::Select {
            entity_ids: first.ordered,
            additive: false,
        },
        Some(&rules),
    ));

    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "N/M continuation", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, &row.id)
        .expect("current VERA snapshot and matching content identity")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("ordinary fixture references and retained membership");
    for index in 0..5 {
        let layer = DisplayLayer::from_index(index).unwrap();
        assert_eq!(
            restored.display_layers().members(layer),
            sim.display_layers().members(layer)
        );
    }
    for actor in &row.inputs.actors {
        assert_eq!(
            restored.entities().get(actor.id).unwrap().selected,
            row.steps[0].selected_flags[&actor.id.to_string()]
        );
    }

    // This supplies the empty-cache/nonpending state produced by the app's
    // reset_for_world_replacement. It does not execute that whole AppState
    // wrapper or claim comparison with native save-stream bytes.
    let current = selected_stable_ids_in_order(Some(&restored), Some(&rules), &[], false);
    assert_eq!(current, row.steps[0].selected);
    let candidate = next_object(
        &restored,
        Some(&rules),
        current.first().copied(),
        direction(&row.directions[1]),
    )
    .unwrap();
    let second = resolve_selection_mutation(
        &restored,
        Some(&rules),
        current,
        first.follow_target,
        false,
        &SelectionMutation {
            clear: true,
            select: vec![candidate],
            ..Default::default()
        },
    );
    assert_eq!(second.ordered, row.steps[1].selected);
    assert_eq!(second.successful_adds, row.steps[1].selected);
    assert_eq!(second.follow_target, row.steps[1].follow);
}
