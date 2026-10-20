//! N/M536610/536A80, P5367F0 ->732280, Health536950 ->733380, Y5369F0 ->7336C0.
//! Original body/caller/data and executed controls:
//! tools/input_oracle/selection_navigation.{py,json,meta.json,md}.

use super::{AppState, SelectionMutation};
use crate::app::types::CategoryNavigationKind;
use crate::assets::csf_file::{CsfArg, format_csf};
use crate::map::entities::EntityCategory;
use crate::sim::combat::health_category;

#[cfg(test)]
#[path = "selection_navigation_tests.rs"]
mod object_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectDirection {
    Next,
    Previous,
}

/// Display4AA2B0/4AA380 walk retained Ground/Air/Top vectors, reversing
/// both the layer and member order for Previous. No navigation cache or sort.
fn next_object(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    anchor: Option<u64>,
    direction: ObjectDirection,
) -> Option<u64> {
    use crate::sim::world::display_layers::DisplayLayer;
    let layers = sim.display_layers();
    let ids = layers
        .members(DisplayLayer::GROUND)
        .iter()
        .chain(layers.members(DisplayLayer::AIR))
        .chain(layers.members(DisplayLayer::TOP))
        .copied();
    let eligible = |id| {
        let Some(entity) = sim.entities().get(id) else {
            return false;
        };
        crate::app::input::entity_pick::is_local_player_selectable_object(sim, rules, entity)
            && crate::app::input::entity_pick::selection_dynamic_prefix(
                entity,
                sim.entities(),
                rules,
                Some(&sim.interner),
            )
            && sim.object_is_selectable(id, rules)
    };
    match direction {
        ObjectDirection::Next => after_anchor(ids, anchor, eligible),
        ObjectDirection::Previous => after_anchor(ids.rev(), anchor, eligible),
    }
}

fn after_anchor(
    ids: impl Iterator<Item = u64>,
    anchor: Option<u64>,
    mut eligible: impl FnMut(u64) -> bool,
) -> Option<u64> {
    let mut first = None;
    let mut passed_anchor = anchor.is_none();
    for id in ids {
        if !eligible(id) {
            continue;
        }
        first.get_or_insert(id);
        if passed_anchor {
            return Some(id);
        }
        passed_anchor = anchor == Some(id);
    }
    // Missing and ineligible anchors wrap just like the final eligible member.
    first
}

pub(super) fn execute_object_navigation(state: &mut AppState, direction: ObjectDirection) {
    let placement = state.armed_building_type().is_some();
    state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .disable_selection_modes(placement);
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let current = super::selected_stable_ids_in_order(
        Some(sim),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let Some(id) = next_object(sim, state.rules(), current.first().copied(), direction) else {
        return;
    };
    // UnselectAll is unconditional after a found candidate, including a
    // singleton wrap. Select may refuse (e.g. pending building placement).
    super::apply_selection_mutation(
        state,
        SelectionMutation {
            clear: true,
            select: vec![id],
            ..Default::default()
        },
        true,
        super::SelectionVoicePolicy::EveryAdded,
    );
    crate::app::input::camera::center_view_on_selection(state);
    state.platform.window.request_redraw();
    // No selected-action-line timer start, Scenario draw or Detach in this path.
}

struct CombatantSelection {
    mutation: SelectionMutation,
    across_map: bool,
}

/// P732280, Health733380 and Y7336C0 share the A8B538 defeat guard:
/// House4FC1B0..4FC205 sets it only for PlayerPtr; session52DA94 and
/// cooperative setup5C38A8..5C3ABE clear it. The House defeat owner supplies
/// this state; input keeps no competing defeat flag.
fn selection_navigation_allowed(sim: &crate::sim::world::Simulation) -> bool {
    !sim.local_player_defeated()
}

/// Shared owner arm of the P732580 and Health/Y7335F0 source predicates.
/// Campaign reads only House+1ED; otherwise use the existing50B6F0 port.
fn navigation_owner_controlled(
    sim: &crate::sim::world::Simulation,
    owner: crate::sim::intern::InternedId,
) -> bool {
    if sim.session.game_mode_nonzero() {
        sim.house_is_human_player(owner)
    } else {
        sim.houses
            .get(&owner)
            .is_some_and(|house| house.player_control())
    }
}

/// P732280's local/alive collection precedes its combatant/dynamic screen
/// preflight. Final ObjectSelect admission remains in the shared ledger;
/// a refused Select can keep this call on-screen with no final members.
fn combatant_selection(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    screen_order: &[u64],
    map_order: &[u64],
    selected: &[u64],
    replace: bool,
    mut across_map: bool,
) -> CombatantSelection {
    let mut mutation = SelectionMutation {
        clear: replace,
        ..Default::default()
    };
    let player_owned = |owner| navigation_owner_controlled(sim, owner);
    // Tactical6DA770 drops a lone non-player-controlled selection before
    // testing the shared mode. Deselect clears Follow but retains that mode.
    let dropped = selected.first().copied().filter(|id| {
        selected.len() == 1
            && sim
                .entities()
                .get(*id)
                .is_some_and(|entity| !player_owned(entity.owner()))
    });
    if let Some(id) = dropped {
        mutation.deselect.push(id);
    }
    // TypeSelect__AliveLocalOwnerPredicate732580 does not read Health,
    // Limbo, discovery or in-playfield state. These are not N/M candidates.
    let local_alive = |id: &u64| {
        sim.entities()
            .get(*id)
            .is_some_and(|entity| entity.lifecycle.object_alive() && player_owned(entity.owner()))
    };
    // Final7325C0 calls the existing dynamic13C prefix (which refuses the
    // offline latch +1C8) and Object138 owner.
    let combatant = |id: &u64| {
        sim.entities().get(*id).is_some_and(|entity| {
            entity.category() != EntityCategory::Structure
                && sim
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.is_selectable_combatant)
                && crate::app::input::entity_pick::selection_dynamic_prefix(
                    entity,
                    sim.entities(),
                    Some(rules),
                    Some(&sim.interner),
                )
                && sim.object_is_selectable(*id, Some(rules))
        })
    };
    let screen: Vec<_> = if across_map {
        Vec::new()
    } else {
        screen_order.iter().copied().filter(local_alive).collect()
    };
    if !across_map {
        let selected: std::collections::HashSet<_> = selected.iter().copied().collect();
        across_map = !screen
            .iter()
            .any(|id| combatant(id) && (dropped == Some(*id) || !selected.contains(id)));
    }
    let candidates = if across_map {
        map_order.iter().copied().filter(local_alive).collect()
    } else {
        screen
    };
    mutation.select = candidates.into_iter().filter(combatant).collect();
    CombatantSelection {
        mutation,
        across_map,
    }
}

pub(super) fn execute_combatant_selection(state: &mut AppState) {
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|rt| !selection_navigation_allowed(&rt.simulation))
    {
        return;
    }
    state
        .match_state
        .input
        .type_select
        .prepare_combatant_scope();
    let result = {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        else {
            return;
        };
        let Some(rules) = state.rules() else {
            return;
        };
        let selected = super::selected_stable_ids_in_order(
            Some(sim),
            Some(rules),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        combatant_selection(
            sim,
            rules,
            &crate::app::presentation::instances::tactical_screen_entity_encounter_order(state),
            &super::map_entity_creation_order(sim.entities()),
            &selected,
            !state.match_state.input.hotkey_modifiers.shift_key(),
            state.match_state.input.type_select.across_map,
        )
    };
    super::apply_selection_mutation(
        state,
        result.mutation,
        false,
        super::SelectionVoicePolicy::EveryAdded,
    );
    let selected = super::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let outcome = if selected.is_empty() {
        crate::app::types::TypeSelectOutcome::Empty
    } else if result.across_map {
        crate::app::types::TypeSelectOutcome::Map
    } else {
        crate::app::types::TypeSelectOutcome::Screen
    };
    state
        .match_state
        .input
        .type_select
        .finish_combatant_selection(outcome, result.across_map);
    crate::app::input::messages::post_type_select_feedback(state, outcome.csf_key());
    super::apply_selection_action_line_policy(state, super::SelectionActionLinePolicy::Preserve);
}

#[derive(Debug, Default)]
pub(crate) struct CategoryNavigation {
    candidates: Vec<u64>,
    // Original845560/845564 start at -1. None retains that sentinel without
    // treating a forged continuing mode as a previously visited category.
    health_category: Option<u8>,
    veterancy_category: Option<u8>,
}

impl CategoryNavigation {
    /// Original733160 scans backward and stable-erases one last match. It
    /// changes neither category, selection mode nor the independent map byte.
    pub(crate) fn pointer_expired(&mut self, id: u64) {
        if let Some(index) = self
            .candidates
            .iter()
            .rposition(|candidate| *candidate == id)
        {
            self.candidates.remove(index);
        }
    }

    /// Read-only capture of the authoritative B0FE6C source and last categories.
    pub(crate) fn navigation_view(&self) -> (&[u64], Option<u8>, Option<u8>) {
        (
            &self.candidates,
            self.health_category,
            self.veterancy_category,
        )
    }

    fn category(&self, kind: CategoryNavigationKind) -> Option<u8> {
        match kind {
            CategoryNavigationKind::Health => self.health_category,
            CategoryNavigationKind::Veterancy => self.veterancy_category,
        }
    }

    fn record_category(&mut self, kind: CategoryNavigationKind, category: u8) {
        debug_assert!(category < 3);
        match kind {
            CategoryNavigationKind::Health => self.health_category = Some(category),
            CategoryNavigationKind::Veterancy => self.veterancy_category = Some(category),
        }
    }

    /// Fresh entry snapshots before clearing and starts at critical/elite
    /// even with Shift. Continuing taps retain the vector and advance only
    /// this command's category. The caller records it after feedback.
    fn prepare(
        &mut self,
        kind: CategoryNavigationKind,
        continuing: bool,
        selected: Vec<u64>,
        visible: Vec<u64>,
    ) -> u8 {
        if continuing {
            self.category(kind).map_or(0, |category| (category + 1) % 3)
        } else {
            self.candidates = if selected.is_empty() {
                visible
            } else {
                selected
            };
            0
        }
    }
}

/// ClearScene6853D8..685408 destroys outgoing objects: Unit735892,
/// Infantry517E75 and Building43BD67 dispatch7258D0 ->733160. Retire their
/// pointer candidates even when incoming objects reuse the same stable IDs.
/// Categories845560/845564, modeB0FE54, scopeB0FE64 and held input are process
/// state; neither fresh Full_Init nor saved load resets them here. The separate
/// saved CurrentObjects vector replaces the outgoing selection ledger later.
/// Body/caller evidence and executed expiry: input_oracle/selection_navigation.
pub(crate) fn reset_for_world_replacement(input: &mut crate::app::input::state::MatchInputState) {
    input.category_navigation.candidates.clear();
    input.selection_order.clear();
    input.selection_order_pending = false;
}

fn category_key(kind: CategoryNavigationKind, category: u8) -> &'static str {
    match (kind, category) {
        (CategoryNavigationKind::Health, 0) => "MSG:Critical",
        (CategoryNavigationKind::Health, 1) => "MSG:HeavilyDamaged",
        (CategoryNavigationKind::Health, _) => "MSG:Healthy",
        (CategoryNavigationKind::Veterancy, 0) => "MSG:Elite",
        (CategoryNavigationKind::Veterancy, 1) => "MSG:Veteran",
        (CategoryNavigationKind::Veterancy, _) => "MSG:LittleExperience",
    }
}

fn entity_navigation_category(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    id: u64,
    kind: CategoryNavigationKind,
) -> Option<u8> {
    let entity = sim.entities().get(id)?;
    match kind {
        CategoryNavigationKind::Health => {
            let object = sim.interner.object_type(entity.type_ref(), rules)?;
            Some(health_category(
                entity.health.current(),
                object.strength,
                rules.general.condition_red,
                rules.general.condition_yellow,
            ))
        }
        // Y7336C0 calls750030 on live raw+150, never the announced-rank cache.
        CategoryNavigationKind::Veterancy => {
            Some(crate::sim::combat::veterancy::veterancy_level(entity.veterancy.raw()) as u8)
        }
    }
}

struct CategorySelection {
    mutation: SelectionMutation,
    category: u8,
    has_candidates: bool,
}

fn category_selection(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    navigation: &mut CategoryNavigation,
    mode: &crate::app::types::TypeSelectInputState,
    kind: CategoryNavigationKind,
    screen_order: &[u64],
    selected: Vec<u64>,
    replace: bool,
) -> Option<CategorySelection> {
    if !selection_navigation_allowed(sim) {
        return None;
    }
    let continuing = mode.category_navigation_continues(kind);
    // Rust's typed GameEntity store represents the Techno abstract bit. The
    // selected source732050 has no Alive/owner/building gate. Screen731F70
    // applies7335F0 only when that selected source is empty.
    let selected: Vec<_> = selected
        .into_iter()
        .filter(|id| sim.entities().contains(*id))
        .collect();
    let visible = if !continuing && selected.is_empty() {
        screen_order
            .iter()
            .copied()
            .filter(|id| {
                sim.entities().get(*id).is_some_and(|entity| {
                    entity.lifecycle.object_alive()
                        && entity.category() != EntityCategory::Structure
                        && navigation_owner_controlled(sim, entity.owner())
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let category = navigation.prepare(kind, continuing, selected, visible);
    let has_candidates = !navigation.candidates.is_empty();
    let mutation = SelectionMutation {
        clear: !continuing || replace,
        select: navigation
            .candidates
            .iter()
            .copied()
            .filter(|id| entity_navigation_category(sim, rules, *id, kind) == Some(category))
            .collect(),
        ..Default::default()
    };
    Some(CategorySelection {
        mutation,
        category,
        has_candidates,
    })
}

pub(super) fn execute_category_navigation(state: &mut AppState, kind: CategoryNavigationKind) {
    // Reject before asking the presentation owner for its tactical array.
    if state
        .match_state
        .sim_runtime
        .as_ref()
        .is_none_or(|rt| !selection_navigation_allowed(&rt.simulation))
    {
        return;
    }
    let current = super::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let screen = if !state
        .match_state
        .input
        .type_select
        .category_navigation_continues(kind)
        && current.is_empty()
    {
        crate::app::presentation::instances::tactical_screen_entity_encounter_order(state)
    } else {
        Vec::new()
    };
    let result = {
        let runtime = state.match_state.sim_runtime.as_ref().unwrap();
        let rules = &runtime.resources.rules;
        let input = &mut state.match_state.input;
        category_selection(
            &runtime.simulation,
            rules,
            &mut input.category_navigation,
            &input.type_select,
            kind,
            &screen,
            current,
            !input.hotkey_modifiers.shift_key(),
        )
    };
    let Some(result) = result else {
        return;
    };
    super::apply_selection_mutation(
        state,
        result.mutation,
        false,
        super::SelectionVoicePolicy::EveryAdded,
    );
    // The native direct Select calls do not start the mouse action-line timer.
    super::apply_selection_action_line_policy(state, super::SelectionActionLinePolicy::Preserve);
    let selected = super::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let sim = &state.match_state.sim_runtime.as_ref().unwrap().simulation;
    let rules = state.rules().unwrap();
    let mixed = selected.iter().any(|id| {
        entity_navigation_category(sim, rules, *id, kind)
            .is_some_and(|category| category != result.category)
    });
    let label = localized(
        state,
        if mixed {
            "MSG:Mixed"
        } else {
            category_key(kind, result.category)
        },
    );
    // Native731E29 sums each object's Cost_Of for its own house
    // (`0x00731E2B..0x00731E42`).
    let worth = selected
        .iter()
        .filter_map(|id| sim.entities().get(*id))
        .filter_map(|entity| {
            let rules = state.rules()?;
            let object = sim.interner.object_type(entity.type_ref(), rules)?;
            Some(sim.cost_of(entity.owner(), object, rules))
        })
        .fold(0_i32, |total, cost| total.wrapping_add(cost));
    let text = navigation_feedback(
        result.has_candidates,
        selected.len(),
        worth,
        &label,
        &localized(state, "MSG:NavEmpty"),
        &localized(state, "MSG:NoUnitsSel"),
        &localized(state, "MSG:UnitsWorth"),
    );
    crate::app::input::messages::post_selection_navigation_text(state, &text);
    let input = &mut state.match_state.input;
    input
        .category_navigation
        .record_category(kind, result.category);
    input
        .type_select
        .finish_category_navigation(kind, result.has_candidates);
}

fn localized(state: &AppState, key: &str) -> String {
    state
        .process_assets
        .csf
        .as_ref()
        .map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned())
}

fn navigation_feedback(
    has_candidates: bool,
    count: usize,
    worth: i32,
    category: &str,
    empty: &str,
    none: &str,
    units: &str,
) -> String {
    // 731DC6 -> 7DDCD6 uppercases the category before inserting it. The
    // original C-locale branch changes only ASCII a..z.
    let category = category.to_ascii_uppercase();
    if !has_candidates {
        empty.to_owned()
    } else if count == 0 {
        format_csf(none, &[CsfArg::Str(&category)])
    } else {
        format_csf(
            units,
            &[
                CsfArg::Int(count as i64),
                CsfArg::Str(&category),
                CsfArg::Int(i64::from(worth)),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_health_ratio_corpus_matches_navigation_category() {
        for row in crate::sim::health_ratio_fixture::rows() {
            assert_eq!(
                health_category(
                    row.input.current,
                    row.input.strength,
                    row.input.red(),
                    row.input.yellow()
                ),
                row.output.navigation_category,
                "{row:?}"
            );
        }
    }

    #[test]
    fn world_replacement_drops_candidates_and_preserves_process_navigation() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/selection_navigation.json",
        ))
        .unwrap();
        let native = corpus["veterancy_cleanup_histories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "all_pointers_expired_empty_snapshot")
            .unwrap();
        let outgoing: Vec<u64> =
            serde_json::from_value(native["retained_navigation"].clone()).unwrap();
        let mut input = crate::app::input::state::MatchInputState::new(Default::default());
        input.category_navigation.prepare(
            CategoryNavigationKind::Veterancy,
            false,
            outgoing.clone(),
            vec![],
        );
        input.category_navigation.record_category(
            CategoryNavigationKind::Veterancy,
            native["veterancy_category"].as_u64().unwrap() as u8,
        );
        input
            .type_select
            .finish_category_navigation(CategoryNavigationKind::Veterancy, true);
        input.type_select.across_map = native["across_map"].as_bool().unwrap();
        input.type_select.press(
            winit::keyboard::KeyCode::KeyT,
            std::time::Instant::now(),
            false,
        );
        input.selection_order = outgoing;
        input.selection_order_pending = true;
        input.cursor_coordinates = true;

        // ClearScene6853D8..685408 destroys outgoing objects; each destructor
        // dispatches7258D0 ->733160. The executed third expiry supplies the
        // resulting empty vector and retained process latches. This shared
        // replacement helper is not a full native save-stream execution.
        reset_for_world_replacement(&mut input);
        let expired = &native["steps"][2];
        let (candidates, health, veterancy) = input.category_navigation.navigation_view();
        assert_eq!(
            serde_json::json!(candidates),
            expired["retained_navigation"]
        );
        assert_eq!(health.map_or(-1, i64::from), expired["health_category"]);
        assert_eq!(
            veterancy.map_or(-1, i64::from),
            expired["veterancy_category"]
        );
        assert!(
            input
                .type_select
                .category_navigation_continues(CategoryNavigationKind::Veterancy)
        );
        assert_eq!(input.type_select.across_map, expired["across_map"]);
        assert!(input.type_select.held());
        assert!(input.type_select.owns_key(winit::keyboard::KeyCode::KeyT));
        assert!(input.selection_order.is_empty());
        assert!(!input.selection_order_pending);
        assert!(input.cursor_coordinates);

        // A reused stable ID in the incoming selection cannot resurrect an
        // outgoing navigation candidate. The next Y advances only its retained
        // category, matching the fourth original command in this history.
        let category = input.category_navigation.prepare(
            CategoryNavigationKind::Veterancy,
            input
                .type_select
                .category_navigation_continues(CategoryNavigationKind::Veterancy),
            vec![20],
            vec![],
        );
        assert!(input.category_navigation.candidates.is_empty());
        assert_eq!(category, native["steps"][3]["veterancy_category"]);
    }

    #[test]
    fn veterancy_lifecycle_frame_handoff_expires_surviving_objects() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::combat::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::{
            ConcealOutcome, FrameEffects, LifecycleOutput, Simulation, TickLane,
        };

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
             [BuildingTypes]\n[MTNK]\nStrength=300\n",
        ))
        .unwrap();
        for conceal in [false, true] {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let type_id = sim.interner.intern("MTNK");
            for id in [10, 20, 30] {
                sim.entities_mut()
                    .insert(GameEntity::new_at_frame_zero_for_test(
                        id,
                        2,
                        3,
                        0,
                        0,
                        owner,
                        Health::new(100),
                        type_id,
                        EntityCategory::Unit,
                        0,
                        5,
                        true,
                    ));
            }
            sim.reveal(20);
            sim.advance_app_frame(
                &[],
                None,
                None,
                67,
                TickLane::Ordinary,
                FrameEffects::empty(),
            )
            .unwrap();
            let mut input = crate::app::input::state::MatchInputState::new(Default::default());
            input.category_navigation.prepare(
                CategoryNavigationKind::Veterancy,
                false,
                vec![10, 20, 30],
                vec![],
            );
            input
                .category_navigation
                .record_category(CategoryNavigationKind::Health, 2);
            input
                .category_navigation
                .record_category(CategoryNavigationKind::Veterancy, 1);
            input
                .type_select
                .finish_category_navigation(CategoryNavigationKind::Veterancy, true);
            input.type_select.across_map = true;
            input.selection_order = vec![20];
            // Deselect and its normal app reconciliation have no expiry fact.
            sim.entities_mut()
                .get_mut(20)
                .unwrap()
                .selection
                .set_selected_for_test(false);
            super::super::reconcile_selection_order_for_sim(&mut input, &sim, Some(&rules));
            assert_eq!(input.category_navigation.candidates, [10, 20, 30]);

            if conceal {
                assert_eq!(sim.object_conceal(20), ConcealOutcome::Concealed);
            } else {
                sim.detach_all_pointer_expired(20, &rules, None, FrameEffects::empty());
            }
            assert!(sim.entities().get(20).unwrap().is_object_alive());
            let frame = sim
                .advance_app_frame(
                    &[],
                    None,
                    None,
                    67,
                    TickLane::Ordinary,
                    FrameEffects::empty(),
                )
                .unwrap();
            assert_eq!(
                frame
                    .lifecycle_outputs
                    .iter()
                    .filter(|output| {
                        matches!(
                            output,
                            LifecycleOutput::ObjectPointerExpired { stable_id: 20 }
                        )
                    })
                    .count(),
                1,
                "one control-insensitive navigation notification"
            );
            if conceal {
                let expiry = frame
                    .lifecycle_outputs
                    .iter()
                    .position(|output| {
                        matches!(
                            output,
                            LifecycleOutput::ObjectPointerExpired { stable_id: 20 }
                        )
                    })
                    .unwrap();
                let display = frame
                    .lifecycle_outputs
                    .iter()
                    .position(|output| {
                        matches!(output, LifecycleOutput::DisplayRemove { stable_id: 20 })
                    })
                    .unwrap();
                assert!(expiry < display);
            }
            // Consume actual lifecycle producer facts through the same owner
            // sim_tick calls, without constructing a windowed AppState. This
            // tests the Rust frame handoff, not full native world lifecycle.
            for output in frame.lifecycle_outputs {
                if let LifecycleOutput::ObjectPointerExpired { stable_id } = output {
                    input.category_navigation.pointer_expired(stable_id);
                }
            }
            assert_eq!(
                input.category_navigation.navigation_view(),
                (&[10, 30][..], Some(2), Some(1))
            );
            assert_eq!(
                input.type_select.selection_scope_view(),
                ("veterancy", true, None)
            );
            assert!(sim.entities().get(20).unwrap().is_object_alive());
            let next = sim
                .advance_app_frame(
                    &[],
                    None,
                    None,
                    67,
                    TickLane::Ordinary,
                    FrameEffects::empty(),
                )
                .unwrap();
            assert!(
                !next.lifecycle_outputs.iter().any(|output| {
                    matches!(output, LifecycleOutput::ObjectPointerExpired { .. })
                }),
                "frame facts drain once"
            );
        }
    }

    #[test]
    fn feedback_distinguishes_empty_snapshot_empty_band_and_selection() {
        let render = |has, count| {
            navigation_feedback(
                has,
                count,
                1500,
                "Critical",
                "Empty",
                "No %s",
                "%d %s worth %d",
            )
        };
        assert_eq!(render(false, 0), "Empty");
        assert_eq!(render(true, 0), "No CRITICAL");
        assert_eq!(render(true, 2), "2 CRITICAL worth 1500");
    }
}
