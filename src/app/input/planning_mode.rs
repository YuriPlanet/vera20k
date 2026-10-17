//! Local PlanningMode edges; world::planning owns synchronized path state.
//! Native PlanMgr6379C0/637A10, command536750 and connect63AD50.

use crate::app::AppState;
use crate::app::input::hotkeys::{HotkeyCommand, HotkeyResolution};
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::net::lockstep::SynchronizedCommand;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::CommandEnvelope;
use crate::sim::selection::SelectionState;
use crate::sim::world::Simulation;
use winit::event::ElementState;
use winit::keyboard::KeyCode;

#[derive(Debug, Default)]
pub(crate) struct PlanningInputState {
    active: bool,
    key: Option<KeyCode>,
    refusal_tick: Option<u64>,
    /// PlanMgr AC4BF0, a local display association retained after node delete.
    highlighted_node: Option<u64>,
}

impl PlanningInputState {
    pub(crate) fn active(&self) -> bool {
        self.active
    }
    pub(crate) fn highlighted_node(&self) -> Option<u64> {
        self.highlighted_node
    }
    pub(crate) fn node_deleted(&mut self, node_id: u64, next_node: Option<u64>) {
        if next_node.is_some() {
            self.highlighted_node = next_node;
        } else if self.highlighted_node == Some(node_id) {
            self.highlighted_node = None;
        }
    }
    pub(crate) fn owns_key(&self, key: KeyCode) -> bool {
        self.key == Some(key)
    }
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn restore_active(&mut self, active: bool) {
        self.active = active;
        self.key = None;
        self.refusal_tick = None;
        self.highlighted_node = None;
    }

    /// The single PlanMgr6379C0/637A10 transition, also reached by
    /// House4FC1EE. Return the native OutList event to the caller's existing
    /// scheduler; a House visit must admit it before this frame's dispatch.
    /// Original1cdd controls: tools/spatial_oracle/planning_local_defeat.
    pub(crate) fn set_active(
        &mut self,
        active: bool,
        sim: Option<&Simulation>,
        rules: Option<&RuleSet>,
        selection: &mut SelectionState,
        sounds: &mut SoundEventQueue,
    ) -> Option<CommandEnvelope> {
        // Command536750 ->731A50 rejects entry after Session+300 is set.
        // Release731A70 still reaches637A10, including after defeat.
        if self.active == active || (active && sim.is_some_and(Simulation::local_player_defeated)) {
            return None;
        }
        let commit = if active {
            None
        } else {
            sim.and_then(|sim| {
                let owner = sim.session.current_house()?;
                let record = sim.encode_plan_commit_record(owner)?;
                SynchronizedCommand::opaque(record).decode_for_simulation(sim, sim.session.tick)
            })
        };
        if !active {
            self.highlighted_node = None;
        }
        self.active = active;
        selection.cancel_drag();
        let sound = rules.and_then(|rules| {
            if active {
                rules.general.start_planning_mode_sound.clone()
            } else {
                rules.general.end_planning_mode_sound.clone()
            }
        });
        if let Some(sound_id) = sound {
            sounds.push(GameSoundEvent::UiSound { sound_id });
        }
        commit
    }

    // Native63753F resets AC4C08 in the per-frame PlanMgr637270 pass.
    fn admit_refusal(&mut self, tick: u64) -> bool {
        if self.refusal_tick == Some(tick) {
            return false;
        }
        self.refusal_tick = Some(tick);
        true
    }

    fn key_transition(
        &mut self,
        planning: bool,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> Option<bool> {
        if pressed {
            if !planning {
                return None;
            }
            if !repeat {
                self.key = Some(key);
            }
            Some(if repeat { self.active } else { true })
        } else if self.key == Some(key) {
            self.key = None;
            Some(false)
        } else {
            None
        }
    }
}

pub(crate) fn key_edge(
    state: &mut AppState,
    resolution: HotkeyResolution,
    key: KeyCode,
    edge: ElementState,
    repeat: bool,
) -> bool {
    let planning = resolution == HotkeyResolution::Command(HotkeyCommand::PlanningMode);
    let Some(active) =
        state
            .match_state
            .input
            .planning
            .key_transition(planning, key, edge.is_pressed(), repeat)
    else {
        return false;
    };
    set_active(state, active);
    true
}

pub(crate) fn toggle(state: &mut AppState) {
    let active = !state.match_state.input.planning.active();
    set_active(state, active);
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlanningRefusal {
    Guard,
    Stop,
    Scatter,
    Capacity,
}

impl PlanningRefusal {
    fn csf_key(self) -> &'static str {
        match self {
            Self::Guard => "MSG:PlanningModeNoGuardArea",
            Self::Stop => "MSG:PlanningModeNoStop",
            Self::Scatter => "MSG:PlanningModeNoScatter",
            Self::Capacity => "MSG:PlannerMaximum",
        }
    }
}

/// Guard730D60 ->63A0D0, ClickedEvent6FFE00 ->639FD0 and639130 share
/// AC4C08, silent730A90 duration1E0, and Rules+700 ScoldSound.
pub(crate) fn refuse(state: &mut AppState, action: PlanningRefusal) {
    let Some(tick) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| rt.simulation.session.tick)
    else {
        return;
    };
    if !state.match_state.input.planning.admit_refusal(tick) {
        return;
    }
    crate::app::input::messages::post_planning_refusal(state, action.csf_key());
    let sound = state
        .rules()
        .and_then(|rules| rules.general.scold_sound.clone());
    ui_sound(state, sound);
}

/// Host adaptation: a lost release must not leave the local mode latched.
/// Commit the streamed plan once before dropping the physical-key owner;
/// native WM_ACTIVATEAPP planning equivalence is not established here.
pub(crate) fn release_on_focus_change(state: &mut AppState) {
    state.match_state.input.planning.key = None;
    set_active(state, false);
}

fn ui_sound(state: &mut AppState, sound: Option<String>) {
    if let Some(sound_id) = sound {
        state
            .match_state
            .match_audio
            .sound_events
            .push(GameSoundEvent::UiSound { sound_id });
    }
}

fn set_active(state: &mut AppState, active: bool) {
    let rt = state.match_state.sim_runtime.as_mut();
    let command = state.match_state.input.planning.set_active(
        active,
        rt.as_ref().map(|rt| &rt.simulation),
        rt.as_ref()
            .map(|rt| &rt.resources.rules)
            .or(state.frontend.frontend_rules.as_ref()),
        &mut state.match_state.input.selection_state,
        &mut state.match_state.match_audio.sound_events,
    );
    if let (Some(rt), Some(command)) = (rt, command) {
        rt.simulation.queue_command(command);
    }
}

/// Encode resolved missions through the planning producer. Building primary
/// selection keeps its ordinary Event17 route; unsupported actions are
/// consumed, never downgraded to immediate orders.
pub(crate) fn queue_orders(
    state: &mut AppState,
    selected: &[u64],
    orders: Vec<CommandEnvelope>,
) -> bool {
    let Some(rt) = state.match_state.sim_runtime.as_mut() else {
        return false;
    };
    let Some(owner) = rt.simulation.session.current_house() else {
        return false;
    };
    match queue_orders_in_sim(&mut rt.simulation, owner, selected, orders) {
        Ok(_) => true,
        Err(refusal) => {
            refuse(state, refusal);
            false
        }
    }
}

fn queue_orders_in_sim(
    sim: &mut crate::sim::world::Simulation,
    owner: crate::sim::intern::InternedId,
    selected: &[u64],
    orders: Vec<CommandEnvelope>,
) -> Result<bool, PlanningRefusal> {
    //4ABF7F's selection-wide admission precedes4AE750 for primary clicks
    // and ordinary planned missions. It is not a receiver/encoding guard.
    if !sim.planning_selection_has_capacity(owner, selected) {
        return Err(PlanningRefusal::Capacity);
    }
    let mut added = false;
    for order in orders {
        if matches!(
            order.payload,
            crate::sim::command::Command::TogglePrimaryFactory { .. }
        ) {
            // Selection4AE750 reaches Building443410's direct Event17.
            // It is not a planning MegaMission or a plan-node append.
            sim.queue_command(order);
            added = true;
            continue;
        }
        let Some(record) = sim.encode_planning_order_record(order.owner, &order.payload) else {
            continue;
        };
        if let Some(envelope) =
            SynchronizedCommand::opaque(record).decode_for_simulation(sim, order.execute_tick)
        {
            sim.queue_command(envelope);
            added = true;
        }
    }
    Ok(added)
}

pub(crate) fn node_at_cursor(state: &AppState) -> Option<(u16, u16)> {
    let point = hover_native_screen(state)?;
    let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
    let owner = sim.session.current_house()?;
    sim.planning_node_at_screen(owner, point)
}

/// The drawing projection and input hit test share the native tactical point.
pub(crate) fn hover_native_screen(state: &AppState) -> Option<[i32; 2]> {
    if !state.match_state.input.planning.active() {
        return None;
    }
    let (width, height) = crate::app::input::camera::tactical_viewport_size(state);
    let (x, y) = (
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    if x < 0. || y < 0. || x >= width as f32 || y >= height as f32 {
        return None;
    }
    Some(cursor_native_screen(state))
}

fn cursor_native_screen(state: &AppState) -> [i32; 2] {
    let (x, y, _, _) = crate::app::input::camera::tactical_viewport_px(state);
    input_native_screen(&state.match_state.input, [x as i32, y as i32])
}

fn input_native_screen(
    input: &crate::app::input::state::MatchInputState,
    origin: [i32; 2],
) -> [i32; 2] {
    // Match the overlay's rounded camera and logical clip origin, using the
    // shared inverse view transform before quantizing its native hit point.
    let point = crate::app::match_runtime::sim_tick::screen_point_to_world_with_camera(
        (input.cursor_x, input.cursor_y),
        (input.camera_x.round(), input.camera_y.round()),
        input.zoom_level,
    );
    [
        point.0.floor() as i32 - origin[0],
        point.1.floor() as i32 - origin[1],
    ]
}

pub(crate) fn connect_at_cursor(state: &mut AppState) -> bool {
    let Some(_) = node_at_cursor(state) else {
        return false;
    };
    let selected = crate::app::input::dispatch::selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let point = cursor_native_screen(state);
    if let Some(rt) = state.match_state.sim_runtime.as_mut()
        && let Some(owner) = rt.simulation.session.current_house()
    {
        for actor in rt.simulation.planning_connect_actors(owner, &selected) {
            if let Some(record) = rt
                .simulation
                .encode_plan_connect_record_at_screen(owner, actor, point)
                && let Some(command) = SynchronizedCommand::opaque(record)
                    .decode_for_simulation(&rt.simulation, rt.simulation.session.tick)
            {
                rt.simulation.queue_command(command);
            }
        }
    }
    true
}

pub(crate) fn delete_at_cursor(state: &mut AppState) {
    let Some(_) = node_at_cursor(state) else {
        return;
    };
    let point = cursor_native_screen(state);
    if let Some(rt) = state.match_state.sim_runtime.as_mut()
        && let Some(owner) = rt.simulation.session.current_house()
        && let Some(record) = rt
            .simulation
            .encode_plan_node_delete_record_at_screen(owner, point)
        && let Some(command) = SynchronizedCommand::opaque(record)
            .decode_for_simulation(&rt.simulation, rt.simulation.session.tick)
    {
        rt.simulation.queue_command(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::command::Command;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::Simulation;

    #[test]
    fn zoomed_planning_node_hit_connect_and_delete_use_drawn_logical_pixels() {
        for zoom in [0.75, 1.0, 1.25, 2.0] {
            let (mut sim, local) = populated_sim();
            let record = sim
                .encode_planning_order_record(
                    local,
                    &Command::Move {
                        entity_id: 42,
                        target_rx: 5,
                        target_ry: 7,
                    },
                )
                .unwrap();
            let command = sim.decode_native_command_record(&record, 0).unwrap();
            assert!(sim.apply_command("Local", &command.payload, None));
            let coord = sim.planning_draw_graph(local, None, None).nodes[0].coord;
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
            let mut input = crate::app::input::state::MatchInputState::new(Default::default());
            input.camera_x = x - 100.25;
            input.camera_y = y - 80.75;
            input.zoom_level = zoom;
            let origin = [13, 17];
            // Forward projection mirrors the render viewport, including its
            // rounded camera and logical clip origin.
            input.cursor_x = (x as i32 - input.camera_x.round() as i32 + origin[0]) as f32 * zoom;
            input.cursor_y = (y as i32 - input.camera_y.round() as i32 + origin[1]) as f32 * zoom;
            let point = input_native_screen(&input, origin);
            assert_eq!(
                sim.planning_node_at_screen(local, point),
                Some((5, 7)),
                "zoom {zoom}"
            );
            let connect = sim
                .encode_plan_connect_record_at_screen(local, 43, point)
                .unwrap();
            let command = sim.decode_native_command_record(&connect, 0).unwrap();
            assert!(sim.apply_command("Local", &command.payload, None));
            assert_eq!(sim.planning_paths(local).len(), 2);
            let delete = sim
                .encode_plan_node_delete_record_at_screen(local, point)
                .unwrap();
            let command = sim.decode_native_command_record(&delete, 0).unwrap();
            assert!(sim.apply_command("Local", &command.payload, None));
            assert!(sim.planning_paths(local).is_empty());
        }
    }

    #[test]
    fn delete_handoff_keeps_local_highlight_until_its_node_expires() {
        let mut input = PlanningInputState::default();
        input.node_deleted(2, Some(3));
        assert_eq!(input.highlighted_node(), Some(3));
        input.node_deleted(7, None);
        assert_eq!(input.highlighted_node(), Some(3));
        input.node_deleted(3, None);
        assert_eq!(input.highlighted_node(), None);
        input.node_deleted(8, Some(9));
        input.restore_active(true);
        assert_eq!(input.highlighted_node(), None);
    }

    fn populated_sim() -> (Simulation, crate::sim::intern::InternedId) {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let kind = sim.interner.intern("TESTUNIT");
        sim.houses
            .insert(local, HouseState::new(local, 0, None, false, 0, 10));
        sim.session.house_order = vec![local];
        for id in [42, 43] {
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                1,
                1,
                0,
                0,
                local,
                Health::new(100),
                kind,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            entity.lifecycle.set_in_limbo_for_test(false);
            sim.substrate.entities.insert(entity);
        }
        (sim, local)
    }
    #[test]
    fn planning_edges_ignore_repeat_and_release_original_physical_key() {
        let mut input = PlanningInputState::default();
        assert_eq!(
            input.key_transition(true, KeyCode::KeyZ, true, false),
            Some(true)
        );
        input.active = true;
        assert_eq!(
            input.key_transition(true, KeyCode::KeyZ, true, true),
            Some(true)
        );
        assert_eq!(
            input.key_transition(false, KeyCode::ShiftLeft, false, false),
            None
        );
        assert_eq!(
            input.key_transition(false, KeyCode::KeyZ, false, false),
            Some(false)
        );
        assert_eq!(
            input.key_transition(false, KeyCode::KeyZ, false, false),
            None
        );
    }
    #[test]
    fn unsupported_planning_actions_never_fall_back_to_ordinary_execution() {
        let (mut sim, local) = populated_sim();
        for payload in [
            Command::Stop { entity_id: 42 },
            Command::Scatter { entity_id: 42 },
        ] {
            assert!(
                queue_orders_in_sim(
                    &mut sim,
                    local,
                    &[42],
                    vec![CommandEnvelope::new(local, 0, payload)]
                )
                .is_ok_and(|added| !added)
            );
            assert!(sim.pending_command_snapshot().is_empty());
        }
    }

    fn retail_primary_planning_fixture() -> Option<(
        Simulation,
        crate::rules::retail_ini_fixture::RetailBattleRules,
        u64,
        u64,
    )> {
        let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
        let rules = &retail.rules;
        let mut sim = Simulation::with_seed(0x004A_BFA9);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.game_mode_nonzero = true;
        sim.session.game_options.short_game = false;
        let owner = sim.intern("Americans");
        let side = rules.country_side_index("Americans").unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
        house.project_country_mults(rules, &sim.interner);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        sim.session.current_house = Some(owner);
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        let first = sim
            .spawn_object("GAWEAP", "Americans", 6, 6, 0, rules)
            .expect("stock initial primary War Factory");
        let second = sim
            .spawn_object("GAWEAP", "Americans", 20, 6, 0, rules)
            .expect("stock nonprimary War Factory");
        assert!(sim.production.is_primary_factory(first));
        assert!(!sim.production.is_primary_factory(second));
        Some((sim, retail, first, second))
    }

    #[test]
    fn retail_planning_primary_selection_keeps_its_ordinary_scheduled_event() {
        use crate::rules::overlay_types::OverlayTypeRegistry;
        use crate::sim::runtime::{SimResources, SimRuntime};
        use crate::sim::world::{FrameEffects, TickLane};

        let Some((mut sim, retail, first, second)) = retail_primary_planning_fixture() else {
            return;
        };
        let owner = sim.session.current_house().unwrap();
        let overlays =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let order = CommandEnvelope::new(
            owner,
            2,
            Command::TogglePrimaryFactory { entity_id: second },
        );
        assert!(queue_orders_in_sim(&mut sim, owner, &[second], vec![order.clone()]).unwrap());
        assert_eq!(
            sim.pending_command_snapshot(),
            vec![order],
            "the planning boundary preserves the Event17 target, issuer and execute tick"
        );
        assert!(sim.planning_paths(owner).is_empty());

        let mut resources = SimResources::empty();
        resources.rules = rules;
        resources.overlay_registry = overlays;
        let mut runtime = SimRuntime {
            simulation: sim,
            resources,
        };
        for selected in [false, true] {
            let due = runtime.simulation.take_due_commands();
            runtime
                .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                .expect("ordinary planning-active primary-selection frame");
            assert_eq!(
                runtime.simulation.production.is_primary_factory(second),
                selected
            );
            assert_eq!(
                runtime.simulation.production.is_primary_factory(first),
                !selected
            );
            assert!(runtime.simulation.planning_paths(owner).is_empty());
        }
        assert!(runtime.simulation.pending_command_snapshot().is_empty());
    }

    #[test]
    fn retail_planning_primary_click_matches_native_capacity_boundaries() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/building_manual_primary.json");
        let admission = &corpus["planning_admission"];
        assert_eq!(
            PlanningRefusal::Capacity.csf_key(),
            admission["csf"]["key"].as_str().unwrap()
        );
        for case in admission["controls"].as_array().unwrap() {
            let Some((mut sim, retail, first, second)) = retail_primary_planning_fixture() else {
                return;
            };
            let owner = sim.session.current_house().unwrap();
            assert_eq!(
                retail.rules.general.scold_sound.as_deref(),
                admission["scold_sound"]["name"].as_str()
            );
            let count = case["input"]["planning_admission"]["prior_house_nonempty_paths"]
                .as_u64()
                .unwrap() as u16;
            // Create the native control's other retained plans through the
            // production producer/receiver, rather than a fabricated counter.
            for index in 0..count {
                let actor = sim
                    .spawn_object(
                        "MTNK",
                        "Americans",
                        1 + index % 16,
                        16 + index / 16,
                        0,
                        &retail.rules,
                    )
                    .expect("retail tank for one retained House plan");
                let record = sim
                    .encode_planning_order_record(
                        owner,
                        &Command::Move {
                            entity_id: actor,
                            target_rx: 25,
                            target_ry: 25,
                        },
                    )
                    .unwrap();
                let envelope = sim.decode_native_command_record(&record, 0).unwrap();
                assert!(sim.apply_command("Americans", &envelope.payload, Some(&retail.rules)));
            }
            assert_eq!(sim.planning_paths(owner).len(), usize::from(count));
            assert!(sim.pending_command_snapshot().is_empty());
            let before = sim.state_hash();
            let result = queue_orders_in_sim(
                &mut sim,
                owner,
                &[second],
                vec![CommandEnvelope::new(
                    owner,
                    2,
                    Command::TogglePrimaryFactory { entity_id: second },
                )],
            );
            let native = &case["phases"][0];
            let admitted = native["admitted"].as_bool().unwrap();
            assert_eq!(result.is_ok_and(|added| added), admitted, "count {count}");
            let pending = sim.pending_command_snapshot();
            assert_eq!(pending.len(), native["issued"].as_u64().unwrap() as usize);
            if admitted {
                assert_eq!(pending[0].execute_tick, 2);
                assert!(sim.apply_command("Americans", &pending[0].payload, Some(&retail.rules)));
            } else {
                assert!(matches!(result, Err(PlanningRefusal::Capacity)));
                assert_eq!(
                    sim.state_hash(),
                    before,
                    "refusal may not queue or mutate gameplay"
                );
            }
            for (index, actor) in [first, second].into_iter().enumerate() {
                assert_eq!(
                    u8::from(sim.production.is_primary_factory(actor)),
                    native["state"]["primary"][index].as_u64().unwrap() as u8
                );
            }
            assert_eq!(
                sim.planning_paths(owner).len(),
                native["state"]["planning"]["house_nonempty_paths"][0]
                    .as_u64()
                    .unwrap() as usize
            );
            if !admitted {
                // Commit consumes each single-leg path but retains its
                // executing record until acknowledgement. Those128 empty
                // paths must release capacity before their records complete.
                let commit = sim.encode_plan_commit_record(owner).unwrap();
                let envelope = sim.decode_native_command_record(&commit, 0).unwrap();
                assert!(sim.apply_command("Americans", &envelope.payload, Some(&retail.rules)));
                assert!(sim.planning_paths(owner).is_empty());
                assert_eq!(sim.pending_command_snapshot().len(), usize::from(count));
                assert!(sim.planning_selection_has_capacity(owner, &[second]));
            }
        }
    }

    #[test]
    fn planning_producer_streams_resolved_moves_in_selection_order() {
        let (mut sim, local) = populated_sim();
        let orders: Vec<_> = [43, 42]
            .into_iter()
            .map(|entity_id| {
                CommandEnvelope::new(
                    local,
                    17,
                    Command::Move {
                        entity_id,
                        target_rx: 5,
                        target_ry: 7,
                    },
                )
            })
            .collect();
        assert!(queue_orders_in_sim(&mut sim, local, &[43, 42], orders.clone()).unwrap());
        let pending = sim.pending_command_snapshot();
        assert_eq!(pending.len(), 2);
        for (planned, ordinary) in pending.iter().zip(&orders) {
            assert_eq!(planned.owner, ordinary.owner);
            assert_eq!(planned.execute_tick, ordinary.execute_tick);
            assert_eq!(
                planned.payload,
                Command::PlanningRecord {
                    record: sim
                        .encode_planning_order_record(local, &ordinary.payload)
                        .unwrap()
                }
            );
        }
        assert!(
            sim.entities()
                .get(42)
                .unwrap()
                .navigation
                .nav_com()
                .is_none()
        );
        assert!(
            sim.entities()
                .get(43)
                .unwrap()
                .navigation
                .nav_com()
                .is_none()
        );
    }

    #[test]
    fn restored_planning_preserves_mode_without_retaining_physical_key() {
        let mut input = PlanningInputState::default();
        input.key_transition(true, KeyCode::KeyZ, true, false);
        input.restore_active(true);
        assert!(input.active());
        assert!(!input.owns_key(KeyCode::KeyZ));
        input.reset();
        assert!(!input.active());
    }

    #[test]
    fn local_planning_transitions_match_executed_native_defeat_histories() {
        let corpus = crate::test_fixture::json("tools/spatial_oracle/planning_local_defeat.json");
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = &retail.rules;
        let cases = corpus["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 4);
        for case in cases {
            let mut sim = Simulation::with_seed(31);
            let local = sim.intern("Local");
            let index = case["input"]["house_index"].as_u64().unwrap();
            for slot in 0..=index {
                let owner = if slot == index {
                    local
                } else {
                    sim.intern(&format!("Other{slot}"))
                };
                sim.session.house_order.push(owner);
                sim.houses
                    .insert(owner, HouseState::new(owner, 0, None, slot == index, 0, 10));
            }
            sim.session.current_house = Some(local);
            sim.houses.get_mut(&local).unwrap().is_defeated = case["initial"]["session300"] != 0;
            let before_rng = sim.rng_state();
            let mut input = PlanningInputState::default();
            input.restore_active(case["initial"]["active"].as_bool().unwrap());
            input.node_deleted(0, case["initial"]["highlight"].as_u64());
            let mut selection = SelectionState::new();
            let mut sounds = SoundEventQueue::new();
            assert_eq!(case["rng_before"], case["rng_after"]);
            assert_eq!(case["rng_requests"], serde_json::json!([]));
            assert_eq!(case["raw_advances"], serde_json::json!([]));
            assert_eq!(case["detach"], serde_json::json!([]));
            for phase in case["phases"].as_array().unwrap() {
                let frame = phase["frame"].as_u64().unwrap();
                sim.session.tick = frame;
                sim.session.binary_frame = frame as u32;
                let requested = match phase["input"].as_str().unwrap() {
                    "house_defeat_tail" => {
                        // The native component starts after the House flag
                        // store. Its production trigger is tested through the
                        // real lifecycle/House rung in planning_defeat_tests.
                        sim.houses.get_mut(&local).unwrap().is_defeated = true;
                        Some(false)
                    }
                    "keyboard_press" => input.key_transition(true, KeyCode::KeyZ, true, false),
                    "keyboard_release" => input.key_transition(true, KeyCode::KeyZ, false, false),
                    "direct_press" => Some(true),
                    "direct_release" => Some(false),
                    other => panic!("unhandled native operation {other}"),
                };
                if let Some(active) = requested
                    && let Some(commit) = input.set_active(
                        active,
                        Some(&sim),
                        Some(rules),
                        &mut selection,
                        &mut sounds,
                    )
                {
                    sim.queue_command(commit);
                }
                assert_eq!(
                    input.active(),
                    phase["state"]["active"].as_bool().unwrap(),
                    "{} {}",
                    case["input"]["name"],
                    phase["input"]
                );
                assert_eq!(
                    input.highlighted_node(),
                    phase["state"]["highlight"].as_u64()
                );
                let events: Vec<_> = sim.pending_command_snapshot().iter().map(|event| {
                    let Command::PlanningRecord { record } = &event.payload else { panic!("unexpected non-planning event") };
                    serde_json::json!({"opcode":record.opcode(),"issuer":record.house_id(),"frame":record.frame_stamp()})
                }).collect();
                assert_eq!(serde_json::json!(events), phase["state"]["events"]);
                let actual_cues: Vec<_> = sounds
                    .drain()
                    .into_iter()
                    .map(|event| {
                        let GameSoundEvent::UiSound { sound_id } = event else {
                            panic!("unexpected planning cue")
                        };
                        sound_id
                    })
                    .collect();
                let native_cues: Vec<_> = phase["effects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|effect| effect["event"] == "sound")
                    .map(|effect| {
                        let role = effect["role"].as_str().unwrap();
                        corpus["inputs"]["sound_bindings"][role]["name"]
                            .as_str()
                            .unwrap()
                            .to_string()
                    })
                    .collect();
                assert_eq!(actual_cues, native_cues);
                assert_eq!(
                    sim.rng_state(),
                    before_rng,
                    "local transition must not draw from any simulation stream"
                );
                assert_eq!(phase["rng"], case["rng_before"]);
                assert_eq!(
                    phase["state"]["house_timer"],
                    case["initial"]["house_timer"]
                );
                assert_eq!(
                    phase["state"]["action_line_timer"],
                    case["initial"]["action_line_timer"]
                );
            }
        }
    }

    #[test]
    fn planning_refusal_is_latched_for_one_simulation_frame() {
        let mut input = PlanningInputState::default();
        assert!(input.admit_refusal(17));
        assert!(!input.admit_refusal(17));
        assert!(input.admit_refusal(18));
        input.restore_active(true);
        assert!(input.admit_refusal(18));
    }
}
