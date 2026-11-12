//! Local PlanningMode edges; world::planning owns synchronized path state.
//! Native PlanMgr6379C0/637A10, command536750 and connect63AD50.

use crate::app::AppState;
use crate::app::input::dispatch::selected_in_order;
use crate::app::input::hotkeys::{HotkeyCommand, HotkeyResolution};
use crate::app::match_runtime::sim_host::SimWrite;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::CommandEnvelope;
use crate::sim::selection::SelectionState;
use crate::sim::world::{
    PlanningAppendMessage, PlanningPressRefusal, PlanningSelectionCheck, Simulation,
};
use winit::event::ElementState;
use winit::keyboard::KeyCode;

/// The local house's PLANCOMMIT that leaving Planning Mode issues
/// (Planning__EndMode ->637A10), stamped for the current tick.
pub(crate) fn plan_commit_command(sim: &Simulation) -> Option<CommandEnvelope> {
    let owner = sim.session().current_house()?;
    let record = sim.encode_plan_commit_record(owner)?;
    Some(CommandEnvelope::synchronized(record, sim.session().tick()))
}

#[derive(Debug, Default)]
pub(crate) struct PlanningInputState {
    active: bool,
    key: Option<KeyCode>,
    /// AC4C08: one refusal message and ScoldSound per PlanMgr637270 pass.
    refusal_latched: bool,
    /// PlanMgr AC4BF0, the node a click with nothing selected retains
    /// (0x0063B02F) for Delete (0x00731A10). Local display state, never
    /// synchronized; a destroyed node's id no longer resolves.
    highlighted_node: Option<u64>,
    /// AC4C90: the append's message for the next frame pass (0x006383BF).
    frame_pass_message: Option<PlanningAppendMessage>,
    /// AC4BF4..AC4C04: the help counters 0x0063A120 (0 Intro1Button,
    /// 1 Intro1Key, 2 Intro3) and 0x0063A1B0 (3 LoopHelp, 4 DeleteHelp)
    /// index. Planning__ResetState clears them (0x00637212).
    help_shown: [i32; 5],
    /// AC4C68/AC4C6C: the right press point 0x0063AAC0 keeps for the
    /// highlight's release test, -1 when it keeps none.
    right_press: [i32; 2],
    /// AC4CB0 set by the right press 0x0063AAC0, holding the hover AC4CCC
    /// that 0x00638E60 then stops updating, until the release 0x0063AB00.
    right_hover: Option<Option<u64>>,
    /// The hover node last found with the world home, which a draw beside
    /// a lent tick reads (docs/sim-thread.md): the hover asks the selection,
    /// which reads the live objects.
    hover_seen: std::cell::RefCell<Option<u64>>,
}

impl PlanningInputState {
    pub(crate) fn active(&self) -> bool {
        self.active
    }
    pub(crate) fn highlighted_node(&self) -> Option<u64> {
        self.highlighted_node
    }
    /// Planning__ExecuteEvent's delete hands the local house's highlight to
    /// the deleted node's first member successor (0x006380EE); the node's
    /// destructor clears a highlight on the node itself.
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
    /// Install supplied state for local-handler comparison fixtures. Player
    /// load uses reset; it does not restore active YR planning.
    #[cfg(test)]
    pub(crate) fn restore_active(&mut self, active: bool) {
        *self = Self {
            active,
            ..Self::default()
        };
    }

    /// The single PlanMgr6379C0/637A10 transition, also reached by
    /// House4FC1EE. Return the native OutList event to the caller's existing
    /// scheduler; a House visit must admit it before this frame's dispatch.
    /// Original gamemd controls: tools/spatial_oracle/planning_local_defeat.
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
            sim.and_then(plan_commit_command)
        };
        // Planning__EndMode clears AC4CCC and AC4BF0 (0x00637A52..0x00637A5E).
        if !active {
            self.highlighted_node = None;
            if let Some(held) = self.right_hover.as_mut() {
                *held = None;
            }
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

    fn admit_refusal(&mut self) -> bool {
        !std::mem::replace(&mut self.refusal_latched, true)
    }

    /// One key walk's `calls` to 639FD0 (ClickedEvent6FFE00 in Planning
    /// Mode). Each call posts the message with a ScoldSound when AC4C08 is
    /// clear (`0x00639FD6..0x0063A09E`), then plays ScoldSound again whatever
    /// the latch (`0x0063A0A3..0x0063A0BB`). Returns whether the message
    /// posts and how many ScoldSounds start.
    fn clicked_event_refusal(&mut self, calls: usize) -> (bool, usize) {
        let posts = calls > 0 && self.admit_refusal();
        (posts, calls + usize::from(posts))
    }

    /// The help counter test of 0x0063A1B0 (`threshold` 1) and 0x0063A120,
    /// which counts every attempt it shows.
    fn admit_help(&mut self, index: usize, threshold: i32) -> bool {
        if self.help_shown[index] >= threshold {
            return false;
        }
        self.help_shown[index] += 1;
        true
    }

    /// FUN_0063A120 after PlanningModeCommand__OnPress 0x00731A50 (DL=1) or
    /// __OnRelease 0x00731A70 (DL=0): the message to show, if any. The
    /// keyboard's (CL=0) show once per scenario; the button's (CL=1) compare
    /// against count+1 and so show every time, still counting.
    fn intro(&mut self, button: bool, press: bool) -> Option<&'static str> {
        let (index, csf_key) = match (press, button) {
            (true, true) => (0, "MSG:PlanningModeIntro1Button"),
            (true, false) => (1, "MSG:PlanningModeIntro1Key"),
            (false, _) => (2, "MSG:PlanningModeIntro3"),
        };
        let threshold = if button {
            self.help_shown[index] + 1
        } else {
            1
        };
        self.admit_help(index, threshold).then_some(csf_key)
    }

    /// Planning__ConnectSelectionToNode 0x0063AD50 with nothing selected
    /// (0x0063AD79 -> 0x0063B02F): retain the node as AC4BF0, then
    /// 0x0063A1B0(4) answers DeleteHelp once.
    fn retain_clicked_node(&mut self, node: u64) -> Option<&'static str> {
        self.highlighted_node = Some(node);
        self.admit_help(4, 1)
            .then_some("MSG:PlanningModeDeleteHelp")
    }

    /// FUN_0063AAC0: hold the current hover (AC4CB0) and keep `point` only
    /// while planning with nothing selected, else -1.
    fn right_press(&mut self, hover: Option<u64>, empty_selection: bool, point: [i32; 2]) {
        self.right_hover = Some(hover);
        self.right_press = if self.active && empty_selection {
            point
        } else {
            [-1, -1]
        };
    }

    /// Game__PlanningManager_WM_RBUTTONUP 0x0063AB00: release the hover; a
    /// release within 3 pixels of the press, planning with nothing selected,
    /// drops the retained highlight.
    fn right_release(&mut self, empty_selection: bool, point: [i32; 2]) {
        self.right_hover = None;
        if self.active
            && empty_selection
            && self.highlighted_node.is_some()
            && (point[0] - self.right_press[0]).abs() <= 3
            && (point[1] - self.right_press[1]).abs() <= 3
        {
            self.highlighted_node = None;
        }
    }

    /// PlanMgr_Process_Frame's input head: a selection drops the retained
    /// highlight (0x0063727B..0x0063728F); answers the queued AC4C90
    /// message (0x0063744E), consumed (0x00637524).
    fn begin_frame_pass(&mut self, empty_selection: bool) -> Option<PlanningAppendMessage> {
        if !empty_selection {
            self.highlighted_node = None;
        }
        self.frame_pass_message.take()
    }

    /// PlanMgr_Process_Frame's input tail: AC4C08 resets (0x0063753F).
    fn end_frame_pass(&mut self) {
        self.refusal_latched = false;
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
    // PlanningModeCommandClass::Execute 0x00536750 passes CL=0 (keyboard).
    set_active(state, active, (!repeat).then_some(false));
    true
}

/// TabClass::Action 0x006D07B4/0x006D07BE passes CL=1 (the command-bar
/// button); the retail hotkey route is the keyboard's.
pub(crate) fn toggle(state: &mut AppState, button: bool) {
    let active = !state.match_state.input.planning.active();
    set_active(state, active, Some(button));
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlanningRefusal {
    Guard,
    Stop,
    Scatter,
    Deploy,
    Capacity,
    Heterogeneous,
    /// Refused with no message, sound or latch (a selection with no Techno).
    Silent,
    Press(PlanningPressRefusal),
    Append(PlanningAppendMessage),
}

impl PlanningRefusal {
    fn csf_key(self) -> Option<&'static str> {
        Some(match self {
            Self::Guard => "MSG:PlanningModeNoGuardArea",
            Self::Stop => "MSG:PlanningModeNoStop",
            Self::Scatter => "MSG:PlanningModeNoScatter",
            Self::Deploy => "MSG:PlanningModeNoDeploy",
            Self::Capacity => "MSG:PlannerMaximum",
            Self::Heterogeneous => "MSG:PlanningModeHeteroSel",
            // 0x0063A5A0 loads these literals; "X" is part of the key.
            Self::Press(PlanningPressRefusal::InvalidNode) => "MSG:PlanningModeInvalidNodeX",
            Self::Press(PlanningPressRefusal::NoAircraft) => "MSG:PlanningModeNoAircraft",
            Self::Press(PlanningPressRefusal::NoBuildings) => "MSG:PlanningModeNoBuildings",
            Self::Press(PlanningPressRefusal::NoUnknowns) => "MSG:PlanningModeNoUnknowns",
            // 0x00638120 and PlanMgr637270 load these literals.
            Self::Append(PlanningAppendMessage::PostTerminatingCommand) => {
                "MSG:PostTerminatingCommand"
            }
            Self::Append(PlanningAppendMessage::PostContinualCommand) => "MSG:PostContinualCommand",
            Self::Append(PlanningAppendMessage::PostLoop) => "MSG:PostLoop",
            Self::Append(PlanningAppendMessage::NodeMaximum) => "MSG:NodeMaximum",
            Self::Silent => return None,
        })
    }
}

/// Guard730D60 ->63A0D0, 639130, the append 638120 and PlanMgr637270 share
/// AC4C08, silent730A90 duration1E0, and Rules+700 ScoldSound.
/// ClickedEvent6FFE00 ->639FD0 shares them too and scolds once more:
/// [`refuse_clicked_event`].
pub(crate) fn refuse(state: &mut AppState, action: PlanningRefusal) {
    let Some(csf_key) = action.csf_key() else {
        return;
    };
    if !state.match_state.input.planning.admit_refusal() {
        return;
    }
    crate::app::input::messages::post_planning_message(state, csf_key);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.scold_sound.clone());
    ui_sound(state, sound);
}

/// ClickedEvent6FFE00 ->639FD0 for the Stop, Scatter and Deploy keys: the
/// latched message of [`refuse`], and an unlatched second ScoldSound on every
/// call. The Stop and Scatter walks end at their first refusal
/// (`0x00730EF5`, ScatterSelectedObjects `0x00730FE0`), one call; the Deploy
/// walk goes on (`0x00730D1D`), one call per object it admits.
pub(crate) fn refuse_clicked_event(state: &mut AppState, action: PlanningRefusal, calls: usize) {
    let Some(csf_key) = action.csf_key() else {
        return;
    };
    let (posts, scolds) = state
        .match_state
        .input
        .planning
        .clicked_event_refusal(calls);
    if posts {
        crate::app::input::messages::post_planning_message(state, csf_key);
    }
    let sound = state
        .rules()
        .and_then(|rules| rules.general.scold_sound.clone());
    for _ in 0..scolds {
        ui_sound(state, sound.clone());
    }
}

/// A sim lifecycle output of the append 0x00638120 for `owner`; only the
/// local house's are shown (event house == PlayerPtr, 0x0063814C).
pub(crate) fn append_output(
    state: &mut AppState,
    local: bool,
    message: PlanningAppendMessage,
    frame_pass: bool,
) {
    if !local {
        return;
    }
    if frame_pass {
        state.match_state.input.planning.frame_pass_message = Some(message);
    } else {
        refuse(state, PlanningRefusal::Append(message));
    }
}

/// The input tail of PlanMgr_Process_Frame 0x00637270, once per logic
/// frame: a selection drops the retained highlight (0x0063727B..0x0063728F);
/// a queued AC4C90 message deselects everything through
/// Tactical::Unselect_All 0x006DA740 (0x00637463) and posts under the latch
/// (0x0063746C..0x0063751F); then AC4C08 resets (0x0063753F).
/// Residual: Tactical::Unselect_All skips the beacon ClearSelection that
/// VERA's shared unselect owner (0x0048DC90) also runs. Trigger: a selected
/// beacon when a planned terminal order or fifteenth node executes. Effect:
/// the beacon is deselected too. Frequency: rare.
pub(crate) fn frame_pass(state: &mut AppState) {
    let empty = selected_in_order(state).is_empty();
    if let Some(message) = state.match_state.input.planning.begin_frame_pass(empty) {
        crate::app::input::dispatch::unselect_all(state);
        refuse(state, PlanningRefusal::Append(message));
    }
    state.match_state.input.planning.end_frame_pass();
}

/// 0x0063A120/0x0063A1B0's help: a silent 730A90 post with duration 1E0,
/// no latch and no sound.
fn post_help(state: &mut AppState, csf_key: Option<&str>) {
    if let Some(csf_key) = csf_key {
        crate::app::input::messages::post_planning_message(state, csf_key);
    }
}

/// Host adaptation: a lost release must not leave the local mode latched.
/// Commit the streamed plan once before dropping the physical-key owner;
/// native WM_ACTIVATEAPP planning equivalence is not established here.
pub(crate) fn release_on_focus_change(state: &mut AppState) {
    state.match_state.input.planning.key = None;
    set_active(state, false, None);
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

/// `intro_source` is the OnPress/OnRelease CL source (true for the button);
/// None for the host focus release, which has no native command.
fn set_active(state: &mut AppState, active: bool, intro_source: Option<bool>) {
    // 0x00731A50/0x00731A78 test Session+300 before 63A120.
    let defeated = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|rt| rt.simulation.local_player_defeated());
    let rt = state.match_state.sim_runtime.write_sim(SimWrite::Command);
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
        rt.simulation.issue_local_command(command);
    }
    if let Some(button) = intro_source
        && !defeated
    {
        let message = state.match_state.input.planning.intro(button, active);
        post_help(state, message);
    }
}

/// Encode resolved missions through the planning producer. Building primary
/// selection keeps its ordinary Event17 route; unsupported actions are
/// consumed, never downgraded to immediate orders.
pub(crate) fn queue_orders(
    state: &mut AppState,
    selected: &[u64],
    orders: Vec<CommandEnvelope>,
    clicked_cell: Option<(u16, u16)>,
) -> bool {
    let Some(rt) = state.match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return false;
    };
    let Some(owner) = rt.simulation.session().current_house() else {
        return false;
    };
    match queue_orders_in_sim(&mut rt.simulation, owner, selected, orders, clicked_cell) {
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
    clicked_cell: Option<(u16, u16)>,
) -> Result<bool, PlanningRefusal> {
    //4ABF7F's selection-wide admission precedes4AE750 for primary clicks
    // and ordinary planned missions. It is not a receiver/encoding guard.
    // 0x004ABF78 runs both checks before testing either, so under the
    // per-frame refusal latch the mixed-selection message wins.
    let check = sim.planning_selection_check(selected);
    let capacity = sim.planning_selection_has_capacity(owner, selected);
    match check {
        PlanningSelectionCheck::Admit => {}
        PlanningSelectionCheck::Heterogeneous => return Err(PlanningRefusal::Heterogeneous),
        PlanningSelectionCheck::Refuse if capacity => return Err(PlanningRefusal::Silent),
        PlanningSelectionCheck::Refuse => {}
    }
    if !capacity {
        return Err(PlanningRefusal::Capacity);
    }
    let mut added = false;
    for order in orders {
        if matches!(
            order.payload,
            crate::sim::command::Command::TogglePrimaryFactory { .. }
                | crate::sim::command::Command::SetRally { .. }
                | crate::sim::command::Command::ArchiveObject { .. }
                | crate::sim::command::Command::SellBuilding { .. }
                | crate::sim::command::Command::UndeployBuilding { .. }
                | crate::sim::command::Command::MoveFlash { .. }
        ) {
            // Selection4AE750 -> Building443410/4436F0 sends direct
            // PRIMARY17 / ARCHIVE1E / SELL16. These do not use
            // Player_Send_Command6FFBE0. The deploy key is separately
            // refused as DEPLOY9; a mobile self-click is Unload16.
            // ClickedAction_Cell builds the MoveFlash (`0x004D7EB5`) before
            // its Move reaches Player_Send_Command.
            let Some(order) = super::commands::encode_ordinary_local_command(sim, order) else {
                continue;
            };
            sim.issue_local_command(order);
            added = true;
            continue;
        }
        let Some(record) =
            sim.encode_planning_order_record(order.owner, &order.payload, clicked_cell)
        else {
            continue;
        };
        sim.issue_local_command(CommandEnvelope::synchronized(record, order.execute_tick));
        added = true;
    }
    Ok(added)
}

/// The hover node AC4CCC (0x00638E60): an own node under the cursor the
/// selection may connect to. While a right press holds AC4CB0 it keeps the
/// node it had at the press (0x00638E60's early return).
pub(crate) fn hover_node(state: &AppState) -> Option<u64> {
    if let Some(held) = state.match_state.input.planning.right_hover {
        // Planning nodes are not objects: the presentation copy keeps them.
        let sim = &state.match_state.sim_runtime.runtime()?.simulation;
        return held.filter(|&node| sim.planning_node_exists(node));
    }
    live_hover_node(state)
}

fn live_hover_node(state: &AppState) -> Option<u64> {
    crate::app::match_runtime::sim_host::home_answer(
        &state.match_state.sim_runtime,
        &state.match_state.input.planning.hover_seen,
        || find_hover_node(state),
    )
}

fn find_hover_node(state: &AppState) -> Option<u64> {
    let point = hover_native_screen(state)?;
    let sim = &state.match_state.sim_runtime.runtime()?.simulation;
    let owner = sim.session().current_house()?;
    // Only a node under the cursor reads the selection, which walks every
    // object: the cursor asks this at every draw.
    sim.planning_node_id_at_screen(Some(owner), point)?;
    sim.planning_hover_node(owner, &selected_in_order(state), point)
}

/// Planning__UpdateNodeHoverCursor 0x0063AB60 answers the planning cursor
/// over the hover node, and nothing while AC4CB0 is set (0x0063AB76).
pub(crate) fn node_at_cursor(state: &AppState) -> Option<u64> {
    if state.match_state.input.planning.right_hover.is_some() {
        return None;
    }
    live_hover_node(state)
}

/// ScrollClass's left press runs PlanMgr 0x0063A5A0 (0x00693187) before it
/// takes the capture. A refusal consumes the press, even while the AC4C08
/// latch keeps its message and ScoldSound back, so the press arms nothing
/// and its release finds no capture (0x00693232).
pub(crate) fn refuse_press_at_cursor(state: &mut AppState) -> bool {
    let Some(point) = hover_native_screen(state) else {
        return false;
    };
    let selected = selected_in_order(state);
    let Some(refusal) = state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|rt| rt.simulation.planning_press_refusal(&selected, point))
    else {
        return false;
    };
    refuse(state, PlanningRefusal::Press(refusal));
    true
}

/// The drawing projection and input hit test share the native tactical point.
pub(crate) fn hover_native_screen(state: &AppState) -> Option<[i32; 2]> {
    if !state.match_state.input.planning.active() {
        return None;
    }
    if !crate::app::input::camera::cursor_over_battlefield(state) {
        return None;
    }
    Some(cursor_native_screen(state))
}

fn cursor_native_screen(state: &AppState) -> [i32; 2] {
    input_native_screen(&state.match_state.input)
}

fn input_native_screen(input: &crate::app::input::state::MatchInputState) -> [i32; 2] {
    // Match the overlay's rounded camera, using the shared inverse view
    // transform before quantizing its native hit point. The camera is the
    // world point at the frame's corner, so the cursor's frame pixel needs
    // no battlefield origin.
    let point = crate::app::match_runtime::sim_tick::screen_point_to_world_with_camera(
        (input.cursor_x, input.cursor_y),
        (input.camera_x.round(), input.camera_y.round()),
        input.zoom_level,
    );
    [point.0.floor() as i32, point.1.floor() as i32]
}

/// PlanMgr 0x0063A8E0 on the left release: connect at an admitted hover
/// node, or swallow a release over a node the selection may not connect to.
/// Planning__ConnectSelectionToNode 0x0063AD50 retains the node as AC4BF0
/// when nothing is selected (0x0063AD79 -> 0x0063B02F) and shows DeleteHelp
/// once (0x0063B034); otherwise LoopHelp follows any collected actor
/// (0x0063AFD9..0x0063B008).
/// Residual: the press-held AC4C38 is not kept, so this connects the hover
/// node of the release, and a left release during a held right press uses
/// the live hover, not the AC4CCC 0x0063AAC0 froze. Trigger: a press and
/// release over different nodes, or a left click while the right button is
/// held. Effect: the connect or highlight lands on the release's node.
/// Frequency: rare.
pub(crate) fn connect_at_cursor(state: &mut AppState) -> bool {
    let selected = selected_in_order(state);
    let Some(node) = live_hover_node(state) else {
        return hover_native_screen(state).is_some_and(|point| {
            state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|rt| rt.simulation.planning_release_refused(&selected, point))
        });
    };
    if selected.is_empty() {
        let message = state.match_state.input.planning.retain_clicked_node(node);
        post_help(state, message);
        return true;
    }
    let point = cursor_native_screen(state);
    let mut collected = false;
    if let Some(rt) = state.match_state.sim_runtime.write_sim(SimWrite::Command)
        && let Some(owner) = rt.simulation.session().current_house()
    {
        for actor in rt.simulation.planning_connect_actors(&selected) {
            collected = true;
            if let Some(record) = rt
                .simulation
                .encode_plan_connect_record_at_screen(owner, actor, point)
            {
                let command = CommandEnvelope::synchronized(record, rt.simulation.session().tick());
                rt.simulation.issue_local_command(command);
            }
        }
    }
    if collected && state.match_state.input.planning.admit_help(3, 1) {
        post_help(state, Some("MSG:PlanningModeLoopHelp"));
    }
    true
}

/// DeleteCommand__DeletePlanNodeOrBeacon 0x00731A10: with a retained
/// highlight (FUN_0063AC60) queue its node delete event
/// (Planning__QueueNodeDeleteEvent 0x00637D00); false without one, where
/// Delete turns to the selected beacon (0x00731A24).
pub(crate) fn delete_highlighted(state: &mut AppState) -> bool {
    let Some(node) = state.match_state.input.planning.highlighted_node else {
        return false;
    };
    let Some(rt) = state.match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return false;
    };
    if !rt.simulation.planning_node_exists(node) {
        return false;
    }
    if let Some(owner) = rt.simulation.session().current_house()
        && let Some(record) = rt
            .simulation
            .encode_plan_node_delete_record_for_node(owner, node)
    {
        let command = CommandEnvelope::synchronized(record, rt.simulation.session().tick());
        rt.simulation.issue_local_command(command);
    }
    true
}

/// The tactical client point (logical pixels) ScrollClass hands PlanMgr's
/// right-button handlers (0x006932BD..0x006932C4).
fn tactical_point(state: &AppState) -> [i32; 2] {
    let input = &state.match_state.input;
    [
        (input.cursor_x / input.zoom_level).floor() as i32,
        (input.cursor_y / input.zoom_level).floor() as i32,
    ]
}

/// FUN_0063AAC0, called on every tactical right press (0x006932D0) before
/// the capture test.
pub(crate) fn right_press(state: &mut AppState) {
    let hover = hover_node(state);
    let empty = selected_in_order(state).is_empty();
    let point = tactical_point(state);
    state
        .match_state
        .input
        .planning
        .right_press(hover, empty, point);
}

/// 0x0063AB00, called on every tactical right release (0x0069338A) before
/// the capture test.
pub(crate) fn right_release(state: &mut AppState) {
    let empty = selected_in_order(state).is_empty();
    let point = tactical_point(state);
    state.match_state.input.planning.right_release(empty, point);
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
                    None,
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
            // Forward projection mirrors the render viewport, including its
            // rounded camera.
            input.cursor_x = (x as i32 - input.camera_x.round() as i32) as f32 * zoom;
            input.cursor_y = (y as i32 - input.camera_y.round() as i32) as f32 * zoom;
            let point = input_native_screen(&input);
            let node = sim.planning_hover_node(local, &[], point);
            assert!(node.is_some(), "zoom {zoom}");
            let connect = sim
                .encode_plan_connect_record_at_screen(local, 43, point)
                .unwrap();
            let command = sim.decode_native_command_record(&connect, 0).unwrap();
            assert!(sim.apply_command("Local", &command.payload, None));
            assert_eq!(sim.planning_paths(local).len(), 2);
            let delete = sim
                .encode_plan_node_delete_record_for_node(local, node.unwrap())
                .unwrap();
            let command = sim.decode_native_command_record(&delete, 0).unwrap();
            assert!(sim.apply_command("Local", &command.payload, None));
            assert!(sim.planning_paths(local).is_empty());
        }
    }

    #[test]
    fn empty_selection_click_retains_node_until_a_selection_or_right_click() {
        let mut input = PlanningInputState::default();
        input.restore_active(true);
        assert_eq!(
            input.retain_clicked_node(5),
            Some("MSG:PlanningModeDeleteHelp")
        );
        assert_eq!(input.retain_clicked_node(5), None, "DeleteHelp is once");
        assert_eq!(input.highlighted_node(), Some(5));
        // 63AB00 compares |dx|,|dy| <= 3 against the 63AAC0 press point.
        input.right_press(Some(9), true, [10, 10]);
        assert_eq!(input.right_hover, Some(Some(9)));
        input.right_release(true, [14, 10]);
        assert_eq!(input.right_hover, None);
        assert_eq!(input.highlighted_node(), Some(5));
        input.right_press(None, true, [10, 10]);
        input.right_release(true, [13, 7]);
        assert_eq!(input.highlighted_node(), None);
        // A press with a selection keeps -1, so its release keeps the node.
        input.retain_clicked_node(5);
        input.right_press(None, false, [10, 10]);
        input.right_release(true, [10, 10]);
        assert_eq!(input.highlighted_node(), Some(5));
        assert_eq!(input.begin_frame_pass(true), None);
        assert_eq!(input.highlighted_node(), Some(5));
        input.begin_frame_pass(false);
        assert_eq!(input.highlighted_node(), None);
    }

    /// 639FD0: the message and its ScoldSound are latched by AC4C08, the
    /// second ScoldSound (`0x0063A0BB`) is not, and the Deploy walk calls it
    /// once per admitted object.
    #[test]
    fn a_key_refusal_scolds_again_on_every_call_whatever_the_latch() {
        let mut input = PlanningInputState::default();
        assert_eq!(input.clicked_event_refusal(1), (true, 2), "Stop key");
        assert_eq!(input.clicked_event_refusal(1), (false, 1), "repeat press");
        input.end_frame_pass();
        assert_eq!(input.clicked_event_refusal(3), (true, 4), "Deploy, 3 units");
        assert_eq!(input.clicked_event_refusal(0), (false, 0));
    }

    #[test]
    fn planning_intro_shows_once_by_key_and_every_time_by_button() {
        let mut input = PlanningInputState::default();
        assert_eq!(input.intro(false, true), Some("MSG:PlanningModeIntro1Key"));
        assert_eq!(input.intro(false, true), None);
        for _ in 0..2 {
            assert_eq!(
                input.intro(true, true),
                Some("MSG:PlanningModeIntro1Button")
            );
            assert_eq!(input.intro(true, false), Some("MSG:PlanningModeIntro3"));
        }
        // The button's releases counted Intro3, so the key's never shows.
        assert_eq!(input.intro(false, false), None);
        input.reset();
        assert_eq!(input.intro(false, false), Some("MSG:PlanningModeIntro3"));
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
        let local = sim.intern("Local");
        let kind = sim.intern("TESTUNIT");
        sim.houses_mut_for_test()
            .register(HouseState::new(local, 0, None, false, 0, 10));
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
            sim.entities_mut_for_test().insert(entity);
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
                    vec![CommandEnvelope::new(local, 0, payload)],
                    None,
                )
                .is_ok_and(|added| !added)
            );
            assert!(sim.pending_command_snapshot().is_empty());
        }
    }

    #[test]
    fn planning_keeps_c4_capture_enter_and_harvest_orders() {
        let (mut sim, local) = populated_sim();
        let commands = [
            (
                17,
                Command::PlantC4 {
                    attacker_id: 42,
                    target_building_id: 43,
                },
            ),
            (
                8,
                Command::CaptureBuilding {
                    engineer_id: 42,
                    target_building_id: 43,
                },
            ),
            (
                7,
                Command::clicked_mission(
                    42,
                    crate::sim::mission::MissionType::Enter,
                    crate::sim::combat::TargetKind::Entity(43),
                ),
            ),
            (
                10,
                Command::clicked_mission(
                    42,
                    crate::sim::mission::MissionType::Harvest,
                    crate::sim::combat::TargetKind::Cell(5, 6),
                ),
            ),
        ];
        for (mission, payload) in commands {
            assert!(
                queue_orders_in_sim(
                    &mut sim,
                    local,
                    &[42],
                    vec![CommandEnvelope::new(local, 0, payload.clone())],
                    None,
                )
                .is_ok_and(|added| added),
                "lost {payload:?}"
            );
            let queued = sim.take_frame_commands_for_test();
            assert_eq!(queued.len(), 1);
            let Command::PlanningRecord { record } = &queued[0].payload else {
                panic!("ordinary order escaped planning");
            };
            assert_eq!(record.payload()[5], mission);
            assert_eq!(record.payload()[22], 1);
        }
    }

    #[test]
    fn planning_preserves_direct_building_events_and_encodes_mobile_self_clicks() {
        let (mut sim, local) = populated_sim();
        for payload in [
            Command::SetRally {
                producer_ids: vec![42],
                rx: 5,
                ry: 6,
            },
            Command::ArchiveObject {
                entity_id: 42,
                target_id: 43,
            },
            Command::SellBuilding { entity_id: 42 },
            Command::UndeployBuilding { entity_id: 42 },
            Command::MoveFlash { rx: 5, ry: 6 },
        ] {
            let envelope = CommandEnvelope::new(local, 0, payload);
            assert!(
                queue_orders_in_sim(&mut sim, local, &[42], vec![envelope.clone()], None).unwrap()
            );
            assert_eq!(sim.take_frame_commands_for_test(), vec![envelope]);
        }
        for payload in [
            Command::DeployMcv { entity_id: 42 },
            Command::ToggleInfantryDeploy { entity_id: 42 },
            Command::UnloadPassengers { transport_id: 42 },
        ] {
            assert!(
                queue_orders_in_sim(
                    &mut sim,
                    local,
                    &[42],
                    vec![CommandEnvelope::new(local, 0, payload)],
                    None,
                )
                .unwrap()
            );
            let queued = sim.take_frame_commands_for_test();
            assert_eq!(queued.len(), 1);
            let Command::PlanningRecord { record } = &queued[0].payload else {
                panic!("mobile self-click escaped Planning Mode");
            };
            let decoded = crate::sim::command::MegaMissionRecord::decode(&{
                let mut ordinary = record.clone();
                ordinary.payload_mut()[22] = 0;
                ordinary
            })
            .unwrap();
            assert!(matches!(
                decoded.order,
                crate::sim::command::MegaMissionOrder::Mission {
                    mission: crate::sim::mission::MissionType::Unload,
                    target: crate::sim::command::MegaMissionTarget::Null,
                    destination: crate::sim::command::MegaMissionTarget::Null,
                }
            ));
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
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        let owner = sim.intern("Americans");
        let side = rules.country_side_index("Americans").unwrap().0;
        let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
        house.project_country_mults(rules, sim.interner());
        sim.houses_mut_for_test().register(house);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        let first = sim
            .spawn_object("GAWEAP", "Americans", 6, 6, 0, rules)
            .expect("stock initial primary War Factory");
        let second = sim
            .spawn_object("GAWEAP", "Americans", 20, 6, 0, rules)
            .expect("stock nonprimary War Factory");
        assert!(sim.production().is_primary_factory(first));
        assert!(!sim.production().is_primary_factory(second));
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
        let owner = sim.session().current_house().unwrap();
        let overlays =
            OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
        let rules = retail.rules;
        let order = CommandEnvelope::new(
            owner,
            2,
            Command::TogglePrimaryFactory { entity_id: second },
        );
        assert!(
            queue_orders_in_sim(&mut sim, owner, &[second], vec![order.clone()], None).unwrap()
        );
        let (opcode, frame_stamp) = {
            let queues = sim.network_command_queues();
            let records = queues.local_records().unwrap();
            assert_eq!(records.len(), 1);
            (records[0].opcode(), records[0].frame_stamp())
        };
        assert_eq!(opcode, 0x11);
        assert_eq!(frame_stamp, sim.session().binary_frame() as i32);
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
            resources: resources.into(),
        };
        // The raw native stamp controls execution; the preserved diagnostic
        // issue ordinal 2 above cannot postpone an event issued at frame0.
        for selected in [true, true] {
            let due = runtime.simulation.take_due_commands();
            runtime
                .advance_frame(&due, 67, TickLane::Ordinary, FrameEffects::default())
                .expect("ordinary planning-active primary-selection frame");
            assert_eq!(
                runtime.simulation.production().is_primary_factory(second),
                selected
            );
            assert_eq!(
                runtime.simulation.production().is_primary_factory(first),
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
            admission["csf"]["key"].as_str()
        );
        for case in admission["controls"].as_array().unwrap() {
            let Some((mut sim, retail, first, second)) = retail_primary_planning_fixture() else {
                return;
            };
            let owner = sim.session().current_house().unwrap();
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
                        None,
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
                None,
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
                    u8::from(sim.production().is_primary_factory(actor)),
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
                // PlanMgr 0x00637550 publishes the consumed orders after the DoList.
                sim.publish_planning_commands_for_test();
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
        assert!(queue_orders_in_sim(&mut sim, local, &[43, 42], orders.clone(), None).unwrap());
        let pending = sim.pending_command_snapshot();
        assert_eq!(pending.len(), 2);
        for (planned, ordinary) in pending.iter().zip(&orders) {
            assert_eq!(planned.owner, ordinary.owner);
            assert_eq!(planned.execute_tick, ordinary.execute_tick);
            assert_eq!(
                planned.payload,
                Command::PlanningRecord {
                    record: sim
                        .encode_planning_order_record(local, &ordinary.payload, None)
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
    fn fixture_activation_does_not_retain_physical_key() {
        let mut input = PlanningInputState::default();
        input.key_transition(true, KeyCode::KeyZ, true, false);
        input.restore_active(true);
        assert!(input.active());
        assert!(!input.owns_key(KeyCode::KeyZ));
        input.key_transition(true, KeyCode::KeyZ, true, false);
        input.node_deleted(99, Some(7));
        assert!(input.owns_key(KeyCode::KeyZ));
        input.reset();
        assert!(!input.active());
        assert!(!input.owns_key(KeyCode::KeyZ));
        assert_eq!(input.highlighted_node(), None);
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
                sim.houses_mut_for_test().register(HouseState::new(
                    owner,
                    0,
                    None,
                    slot == index,
                    0,
                    10,
                ));
            }
            sim.session_mut_for_test()
                .set_current_house_for_test(Some(local));
            sim.houses_mut_for_test()
                .get_mut(&local)
                .unwrap()
                .set_is_defeated_for_test(case["initial"]["session300"] != 0);
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
                sim.session_mut_for_test().set_tick_for_test(frame);
                sim.session_mut_for_test()
                    .set_binary_frame_for_test(frame as u32);
                let requested = match phase["input"].as_str().unwrap() {
                    "house_defeat_tail" => {
                        // The native component starts after the House flag
                        // store. Its production trigger is tested through the
                        // real lifecycle/House rung in planning_defeat_tests.
                        sim.houses_mut_for_test()
                            .get_mut(&local)
                            .unwrap()
                            .set_is_defeated_for_test(true);
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
    fn planning_refusal_is_latched_until_the_frame_pass() {
        let mut input = PlanningInputState::default();
        assert!(input.admit_refusal());
        assert!(!input.admit_refusal());
        input.refusal_latched = false;
        assert!(input.admit_refusal());
        input.restore_active(true);
        assert!(input.admit_refusal());
    }

    #[test]
    fn a_queued_append_message_posts_under_the_latch_then_the_latch_resets() {
        let mut input = PlanningInputState::default();
        input.frame_pass_message = Some(PlanningAppendMessage::NodeMaximum);
        assert!(input.admit_refusal(), "an earlier refusal this frame");
        assert_eq!(
            input.begin_frame_pass(true),
            Some(PlanningAppendMessage::NodeMaximum)
        );
        assert!(!input.admit_refusal(), "the queued post shares AC4C08");
        assert_eq!(input.begin_frame_pass(true), None, "consumed once");
        input.end_frame_pass();
        assert!(input.admit_refusal(), "0x0063753F resets the latch");
    }
}
