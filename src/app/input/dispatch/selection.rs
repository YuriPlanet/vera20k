//! The player-side selection: hotkey modifier state, the ordered selection
//! ledger and its reconciliation with the simulation, and selection mutations
//! with their voice and action-line policies.

use super::{ORDINARY_SELECTION_VOICE_POLICY, SelectionActionLinePolicy, SelectionVoicePolicy};
use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner, schedule_command};
use crate::app::input::entity_pick::SelectionMutation;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::sim::command::Command;
use std::collections::{HashSet, VecDeque};

/// The modifier bits a hotkey press carries.
///
/// The engine packs Shift, Ctrl and Alt into the key value as separate bits and
/// the command dispatcher matches the whole value, so every binding names an
/// exact modifier set. A command bound to a bare key is *rejected* while any
/// modifier is held — the base "does this command accept a modified form?"
/// predicate returns false for every stock command — and the dispatcher then
/// looks for a binding of the full chord instead. That is why holding Ctrl to
/// force-fire and tapping a letter does nothing in retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct KeyModifiers {
    pub(crate) shift: bool,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
}

impl KeyModifiers {
    #[cfg(feature = "dev-overlays")]
    pub(super) fn from_modifiers_state(modifiers: winit::keyboard::ModifiersState) -> Self {
        Self {
            shift: modifiers.shift_key(),
            ctrl: modifiers.control_key(),
            alt: modifiers.alt_key(),
        }
    }

    /// No modifier held — the only state in which a bare-key binding fires.
    pub(crate) fn none(self) -> bool {
        !self.shift && !self.ctrl && !self.alt
    }

    pub(crate) fn any(self) -> bool {
        !self.none()
    }

    pub(crate) fn only_shift(self) -> bool {
        self.shift && !self.ctrl && !self.alt
    }

    pub(crate) fn only_ctrl(self) -> bool {
        self.ctrl && !self.shift && !self.alt
    }

    pub(crate) fn only_alt(self) -> bool {
        self.alt && !self.shift && !self.ctrl
    }

    /// The VERA-internal dev chord. Stock binds nothing to Ctrl+Shift, so it
    /// collides with no retail hotkey.
    #[cfg(any(feature = "dev-overlays", test))]
    pub(crate) fn dev_chord(self) -> bool {
        self.ctrl && self.shift && !self.alt
    }
}

pub(crate) fn is_shift_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.shift_key()
}

pub(crate) fn is_ctrl_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.control_key()
}

/// Return `true` if either Alt key is currently held.
///
/// Used in order resolution to detect Alt+Ctrl = attack-move (NOT force-fire),
/// matching gamemd's `What_Action_OnCell` Alt-overrides-Ctrl rule.
pub(crate) fn is_alt_held(state: &AppState) -> bool {
    state.match_state.input.hotkey_modifiers.alt_key()
}

/// [`selected_stable_ids_in_order`] of `state`'s world and ledger. That walks
/// every object, so a draw reads this once and hands it to its consumers, and
/// while the ledger stands as the frame's reconciliation left it
/// ([`CommittedSelection`]) the ledger is the answer and nothing is walked.
pub(crate) fn selected_in_order(state: &AppState) -> Vec<u64> {
    let input = &state.match_state.input;
    let sim = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation);
    let read = || {
        selected_stable_ids_in_order(
            sim,
            state.rules(),
            &input.selection_order,
            input.selection_order_pending,
        )
    };
    if let Some(sim) = sim
        && !input.selection_order_pending
        && input
            .selection_committed
            .as_ref()
            .is_some_and(|committed| committed.holds(sim, state.rules(), &input.selection_order))
    {
        debug_assert!(
            !crate::sim::live_read_check_enabled() || input.selection_order == read(),
            "the committed selection ledger differs from the selected objects"
        );
        return input.selection_order.clone();
    }
    read()
}

/// The ledger a frame's reconciliation committed, with the world it read: the
/// rules' serial and the world's revision. Selected bits and object lifetimes
/// change only inside a frame, which moves the revision, so while both hold
/// and the ledger is still this one, it lists exactly the selected objects.
#[derive(Debug)]
pub(crate) struct CommittedSelection {
    rules: Option<u64>,
    revision: u64,
    order: Vec<u64>,
}

impl CommittedSelection {
    fn of(
        sim: &crate::sim::world::Simulation,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        order: &[u64],
    ) -> Self {
        Self {
            rules: rules.map(crate::rules::ruleset::RuleSet::serial),
            revision: sim.revision(),
            order: order.to_vec(),
        }
    }

    pub(super) fn holds(
        &self,
        sim: &crate::sim::world::Simulation,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        order: &[u64],
    ) -> bool {
        self.revision == sim.revision()
            && self.rules == rules.map(crate::rules::ruleset::RuleSet::serial)
            && self.order == order
    }
}

/// Read the player-side selection vector in native order. While a selection
/// command is queued the ledger is already the newest local state; after the
/// sim tick, reconciliation trusts the committed selected bits and admits any
/// lifecycle-transferred selection that was not issued by input.
pub(crate) fn selected_stable_ids_in_order(
    sim: Option<&crate::sim::world::Simulation>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selection_order: &[u64],
    selection_order_pending: bool,
) -> Vec<u64> {
    let Some(sim) = sim else {
        return Vec::new();
    };
    let mut ordered = Vec::new();
    for &id in selection_order {
        if let Some(entity) = sim.entities().get(id)
            && (selection_order_pending || entity.seldom.selection.is_selected())
        {
            ordered.push(id);
        }
    }
    if selection_order_pending {
        ordered
    } else {
        admit_missing_selected_ids(ordered, sim, rules, &[])
    }
}

/// The frame's Selects the game made itself
/// ([`crate::sim::world::LifecycleOutput::ObjectSelected`]), in call order.
pub(super) fn logic_selects(outputs: &[crate::sim::world::LifecycleOutput]) -> Vec<u64> {
    outputs
        .iter()
        .filter_map(|output| match output {
            crate::sim::world::LifecycleOutput::ObjectSelected { stable_id } => Some(*stable_id),
            _ => None,
        })
        .collect()
}

/// Insert the frame's logic Selects (`logic_selects`, in call order) into the
/// ledger with ObjectClass::Select's front/back rule. Only an unselected
/// object's Select succeeds, so an entry already in a committed ledger left
/// the vector at its Deselect (`0x005F44A0` removes it from `0x00A8ECB8`) and
/// re-enters at its last Select. A pending ledger is input's newest order,
/// which the simulation has not yet committed; its entries keep their place.
fn admit_logic_selects(
    ordered: &mut VecDeque<u64>,
    members: &mut HashSet<u64>,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    logic_selects: &[u64],
    committed: bool,
) {
    if logic_selects.is_empty() {
        return;
    }
    if committed {
        let reselected: HashSet<_> = logic_selects.iter().copied().collect();
        ordered.retain(|id| !reselected.contains(id));
        members.retain(|id| !reselected.contains(id));
    }
    for (index, &id) in logic_selects.iter().enumerate() {
        let last = !logic_selects[index + 1..].contains(&id);
        if last
            && sim
                .entities()
                .get(id)
                .is_some_and(|entity| entity.seldom.selection.is_selected())
            && members.insert(id)
        {
            insert_selected_id(ordered, id, sim, rules);
        }
    }
}

/// Selection membership is a transient index, never an iteration authority.
/// The ledger keeps prior order; the frame's logic Selects enter in call
/// order ([`admit_logic_selects`]); other lifecycle transfers (the raw Select
/// writers) still enter in entity-store order, with the same native
/// front/back insertion as input selection. A deque avoids shifting the whole
/// list for each missing armed object.
fn admit_missing_selected_ids(
    ordered: Vec<u64>,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    logic_selects: &[u64],
) -> Vec<u64> {
    let mut members: HashSet<_> = ordered.iter().copied().collect();
    let mut ordered = VecDeque::from(ordered);
    admit_logic_selects(&mut ordered, &mut members, sim, rules, logic_selects, true);
    for entity in sim.entities().values() {
        if entity.seldom.selection.is_selected() && members.insert(entity.stable_id()) {
            insert_selected_id(&mut ordered, entity.stable_id(), sim, rules);
        }
    }
    ordered.into()
}

pub(super) fn selection_membership_committed(
    sim: &crate::sim::world::Simulation,
    ordered: &[u64],
) -> bool {
    let members: HashSet<_> = ordered.iter().copied().collect();
    let mut committed_count = 0;
    sim.entities()
        .values()
        .filter(|entity| entity.seldom.selection.is_selected())
        .all(|entity| {
            committed_count += 1;
            members.contains(&entity.stable_id())
        })
        && committed_count == ordered.len()
}

/// Synchronize the app ledger after the due selection commands and lifecycle
/// removals have committed for this frame. `outputs` are the frame's
/// lifecycle facts, whose logic Selects enter in call order.
pub(crate) fn reconcile_selection_order_after_sim(
    state: &mut AppState,
    outputs: &[crate::sim::world::LifecycleOutput],
) {
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        state.match_state.input.selection_order.clear();
        state.match_state.input.selection_order_pending = false;
        state.match_state.input.category_navigation = Default::default();
        // RESIDUAL: native keeps the scope byte B0FE64 from one scenario
        // into the next; a new VERA match starts with an on-screen scope.
        state.match_state.input.type_select.reset_scope();
        return;
    };
    let selected = logic_selects(outputs);
    reconcile_selection_order_for_sim(
        &mut state.match_state.input,
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &selected,
    );
    // Phobos "Flashing Technos on selecting" sits in ObjectClass::Select
    // (`0x005F46AE`/`0x005F4718`), so the frame's logic Selects flash too.
    #[cfg(feature = "phobos")]
    if state.match_state.match_presentation.show_flash_on_selecting
        && !selected.is_empty()
        && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command)
    {
        runtime
            .simulation
            .flash_selected_for_current_player(&selected, &runtime.resources.rules);
    }
}

/// Reconcile the existing input owner against its bound simulation. Kept
/// independent of the window so lifecycle boundaries use this same path.
pub(super) fn reconcile_selection_order_for_sim(
    input: &mut crate::app::input::state::MatchInputState,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    logic_selects: &[u64],
) {
    // Navigation733160 consumes actual pointer expiry through LifecycleOutput,
    // independently of this membership ledger. Alive/Health/Deselect alone
    // do not remove an entry from its retained vector.
    let (order, pending) = selection_order_for_world(
        sim,
        rules,
        &input.selection_order,
        input.selection_order_pending,
        logic_selects,
    );
    input.selection_committed = (!pending).then(|| CommittedSelection::of(sim, rules, &order));
    input.selection_order = order;
    input.selection_order_pending = pending;
}

/// Shared reconciliation decision for the live input ledger and a saved
/// local-view record. The simulation's command queue remains authoritative;
/// observing a save boundary neither drains nor applies queued commands.
/// `logic_selects` are the just-finished frame's logic Selects; a save
/// boundary, between frames, has none.
pub(crate) fn selection_order_for_world(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    order: &[u64],
    pending: bool,
    logic_selects: &[u64],
) -> (Vec<u64>, bool) {
    if pending && sim.has_pending_selection_commands() {
        let mut ordered: VecDeque<u64> = order
            .iter()
            .copied()
            .filter(|id| {
                sim.entities()
                    .get(*id)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
            })
            .collect();
        // Native Selects all write the one vector at once, so a logic Select
        // joins the provisional ledger too.
        //
        // RESIDUAL: native commits an input Select at input time, before any
        // later logic Select; here the queued snapshot commits after it.
        // Trigger: a logic Select (crew, escaping passenger, released
        // parasite or their planning partners) while a selection snapshot
        // is queued. Effect: this ledger holds that object beside the
        // input's members, even across owners, and the logic Select's
        // Unselect_All tests and drains the older committed selection; then
        // the snapshot deselects the object, which native keeps (alone,
        // across owners). Frequency: only within a selection input's command
        // latency. Downstream risk: the selection, its Follow and the next
        // orders' recipients.
        let mut members = ordered.iter().copied().collect();
        admit_logic_selects(&mut ordered, &mut members, sim, rules, logic_selects, false);
        return (ordered.into(), true);
    }
    // MainTick55D8AB/55D8B4 commits local Select before Logic55DC9E.
    // A consumed snapshot may have been refused or then deselected by
    // ObjectDestroy/Conceal. Trust committed bits once the queue is drained,
    // rather than waiting forever for those bits to equal the provisional
    // ledger. Deselect5F44A0/expiry733160 do not reset mode or map scope.
    let reconciled: Vec<u64> = order
        .iter()
        .copied()
        .filter(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|entity| entity.seldom.selection.is_selected())
        })
        .collect();
    let reconciled = admit_missing_selected_ids(reconciled, sim, rules, logic_selects);
    debug_assert!(selection_membership_committed(sim, &reconciled));
    (reconciled, false)
}

pub(super) fn apply_selection_mutation(
    state: &mut AppState,
    mutation: SelectionMutation,
    voice_policy: SelectionVoicePolicy,
) -> bool {
    if !mutation.clear && mutation.deselect.is_empty() && mutation.select.is_empty() {
        return false;
    }
    let Some(sim) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
    else {
        return false;
    };
    let current = selected_stable_ids_in_order(
        Some(sim),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let SelectionMutationEffects {
        ordered,
        successful_adds,
        first_select_adds,
        first_attempt_added,
        follow_target,
        native_selection_mode_reset,
        unselected_all,
    } = resolve_selection_mutation(
        sim,
        state.rules(),
        current,
        state.match_state.input.follow_target,
        state.armed_building_type().is_some(),
        state.match_state.input.planning.active(),
        &mutation,
    );
    state.match_state.input.follow_target = follow_target;
    // Phobos "Flashing Technos on selecting": the client switch gates the
    // `[AudioVisual] SelectionFlashDuration` the sim's flash owner reads.
    // ObjectClass_Select flashes each newly selected Techno the current player
    // controls (0x005F46AE/0x005F4718); an already-selected object makes no
    // Select call and therefore no flash.
    #[cfg(feature = "phobos")]
    if state.match_state.match_presentation.show_flash_on_selecting
        && !successful_adds.is_empty()
        && let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command)
    {
        runtime
            .simulation
            .flash_selected_for_current_player(&successful_adds, &runtime.resources.rules);
    }
    if unselected_all {
        // Unselect_All's tail, BeaconManager ClearSelection (0x0048DCB8).
        state
            .match_state
            .match_presentation
            .beacons
            .clear_selection();
    }

    // Select and Unselect_All reset only the selection mode (0x00731D00);
    // nothing but the T and P keys writes the map-scope byte B0FE64.
    if native_selection_mode_reset {
        state
            .match_state
            .input
            .type_select
            .note_successful_selection_mutation(false);
    }
    for id in selection_voice_recipients(
        voice_policy,
        state.match_state.input.selection_voice_enabled,
        &successful_adds,
        first_select_adds,
        first_attempt_added,
    ) {
        emit_selection_voice(state, *id);
    }
    state.match_state.input.selection_order = ordered.clone();
    state.match_state.input.selection_order_pending = true;
    let Some(owner) = preferred_local_owner(state) else {
        return true;
    };
    schedule_command(
        state,
        &owner,
        Command::Select {
            entity_ids: ordered,
            additive: !mutation.clear,
        },
    );
    true
}

/// Object5F44A0/5F4520 effects on the immediate local selection ledger. This
/// is also the read-only native-comparison seam; the command receiver commits
/// the final membership through the same Simulation admission owner.
pub(super) struct SelectionMutationEffects {
    pub(super) ordered: Vec<u64>,
    pub(super) successful_adds: Vec<u64>,
    /// The adds made by the first successful Select call, its planning
    /// expansion included.
    pub(super) first_select_adds: usize,
    /// Whether the first Select call made (an object not already selected)
    /// succeeded; true when none was made.
    pub(super) first_attempt_added: bool,
    pub(super) follow_target: Option<u64>,
    pub(super) native_selection_mode_reset: bool,
    /// Unselect_All ran: the mutation's clear, or a Select draining another
    /// selection (0x005F45F8).
    unselected_all: bool,
}

pub(super) fn resolve_selection_mutation(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    current: Vec<u64>,
    mut follow_target: Option<u64>,
    placement_armed: bool,
    planning_active: bool,
    mutation: &SelectionMutation,
) -> SelectionMutationEffects {
    let deselected: HashSet<_> = mutation.deselect.iter().copied().collect();
    let mut native_selection_mode_reset = mutation.clear;
    let mut ordered: VecDeque<_> = current
        .into_iter()
        .filter(|id| {
            let remove = mutation.clear || deselected.contains(id);
            if remove {
                // Deselect's Follow clear precedes Select, even for a singleton
                // that will immediately be reselected. A final-set diff loses it.
                //5F44A0 itself retains SelectionMode; UnselectAll and a
                // successful new Select are the reset callers.
                if follow_target == Some(*id) {
                    follow_target = None;
                }
            }
            !remove
        })
        .collect();
    let mut members: HashSet<_> = ordered.iter().copied().collect();
    let mut successful_adds = Vec::new();
    let mut first_select_adds = 0;
    let mut first_attempt_added = None;
    let mut unselected_all = mutation.clear;
    // Every input Select is vt+0x14C, TechnoClass::Select (0x006FBFA0 in
    // each Techno vtable; click 0x004ABDBA/0x004ABE72, band 0x004AC2F1,
    // team 0x00731344/0x0073156C, the type, combatant, category and
    // next-object walks): its gate (`techno_select_admits`), then
    // ObjectClass::Select (0x005F4520, called only from 0x006FBFC7): the
    // admission against the newest ledger and placement and, into a
    // non-empty selection, Unselect_All (0x0048DC90 at 0x005F45F8,
    // `select_unselects_all`) against the ledger's first entry, whose
    // Deselects (0x005F44A0) release Follow, before the insert. So the
    // player's object drains another house's selection, and the reverse.
    // VERA-only residual: the trainer's Reveal Map lets the picker reach an
    // enemy the current house has not discovered, and this gate refuses it
    // (gamemd has no presentation-only reveal). Debug builds only.
    let mut select = |id: u64,
                      ordered: &mut VecDeque<u64>,
                      members: &mut HashSet<u64>,
                      adds: &mut Vec<u64>,
                      follow_target: &mut Option<u64>| {
        if placement_armed
            || members.contains(&id)
            || !sim.techno_select_admits(id)
            || !rules.is_some_and(|rules| sim.can_select_object(id, rules))
        {
            return false;
        }
        if ordered
            .front()
            .is_some_and(|&first| sim.select_unselects_all(first, id))
        {
            unselected_all = true;
            for removed in ordered.drain(..) {
                members.remove(&removed);
                if *follow_target == Some(removed) {
                    *follow_target = None;
                }
            }
        }
        members.insert(id);
        adds.push(id);
        insert_selected_id(ordered, id, sim, rules);
        true
    };
    for &id in &mutation.select {
        let attempted = !members.contains(&id);
        let added = select(
            id,
            &mut ordered,
            &mut members,
            &mut successful_adds,
            &mut follow_target,
        );
        if attempted && first_attempt_added.is_none() {
            first_attempt_added = Some(added);
        }
        if !added {
            continue;
        }
        native_selection_mode_reset = true;
        // TechnoClass::Select ends with 0x00637840 (0x006FC020), whose one
        // port names the tail-node partners to Select (vt+0x14C) here; these
        // nested Selects do not expand again (AC4CF6). They run before the
        // band-box callback clears the voice latch (0x004AC2FB), so they
        // speak with their Select. A connect (0x0063AD50) can put another
        // house's object on the player's node; its Select drains the ledger
        // like any other.
        for member in sim.planning_select_partners(id, planning_active) {
            select(
                member,
                &mut ordered,
                &mut members,
                &mut successful_adds,
                &mut follow_target,
            );
        }
        if first_select_adds == 0 {
            first_select_adds = successful_adds.len();
        }
    }
    SelectionMutationEffects {
        ordered: ordered.into(),
        successful_adds,
        first_select_adds,
        first_attempt_added: first_attempt_added.unwrap_or(true),
        follow_target,
        native_selection_mode_reset,
        unselected_all,
    }
}

fn insert_selected_id(
    ordered: &mut VecDeque<u64>,
    id: u64,
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    let positive_damage_primary = sim.entities().get(id).is_some_and(|entity| {
        let type_id = sim.interner().resolve(entity.type_ref());
        rules
            .and_then(|rules| rules.object(type_id))
            .and_then(|object| object.primary())
            .and_then(|weapon_id| rules.and_then(|rules| rules.weapon(weapon_id)))
            .is_some_and(|weapon| weapon.damage > 0)
    });
    insert_selected_id_by_role(ordered, id, positive_damage_primary);
}

// Active-retail ObjectClass::Select5F4520 reads type+C9C at5F461F,
// prepends at5F468C/5F469A, otherwise appends at5F470E. ReadINI715793..7157AE
// derives that byte from Primary(+898)->Damage(+A4)>0. No RNG/timer here;
// successful-add voices remain in candidate order outside this ordering helper.
pub(super) fn insert_selected_id_by_role(
    ordered: &mut VecDeque<u64>,
    id: u64,
    positive_damage_primary: bool,
) {
    if positive_damage_primary {
        ordered.push_front(id);
    } else {
        ordered.push_back(id);
    }
}

pub(super) fn selection_voice_recipients(
    policy: SelectionVoicePolicy,
    voice_enabled: bool,
    successful_adds: &[u64],
    first_select_adds: usize,
    first_attempt_added: bool,
) -> &[u64] {
    if !voice_enabled || policy == SelectionVoicePolicy::Suppressed {
        return &[];
    }
    match policy {
        SelectionVoicePolicy::FirstAttempted if !first_attempt_added => &[],
        SelectionVoicePolicy::FirstAdded | SelectionVoicePolicy::FirstAttempted => {
            &successful_adds[..successful_adds.len().min(first_select_adds)]
        }
        SelectionVoicePolicy::EveryAdded => successful_adds,
        SelectionVoicePolicy::Suppressed => &[],
    }
}

pub(super) fn queue_selection_snapshot_command(
    state: &mut AppState,
    selected_ids: Vec<u64>,
    additive: bool,
) {
    apply_selection_mutation(
        state,
        SelectionMutation {
            clear: !additive,
            select: selected_ids,
            ..Default::default()
        },
        ORDINARY_SELECTION_VOICE_POLICY,
    );
}

/// Apply the selection caller's action-line policy.
///
/// Band-box release, click-select and control-group recall all call the
/// engine's start-timer helper unconditionally, so the freshly selected units
/// flash their current orders for the 25-frame window. TypeSelect tap preserves
/// the prior timer instead.
pub(super) fn apply_selection_action_line_policy(
    state: &mut AppState,
    policy: SelectionActionLinePolicy,
) {
    let Some(frame) = state
        .match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.simulation)
        .map(|sim| sim.session().binary_frame())
    else {
        return;
    };
    apply_selection_action_line_policy_at_frame(
        &mut state.match_state.match_presentation.target_lines,
        frame,
        policy,
    );
}

pub(super) fn apply_selection_action_line_policy_at_frame(
    target_lines: &mut crate::app::presentation::target_lines::TargetLineState,
    frame: u32,
    policy: SelectionActionLinePolicy,
) {
    if policy == SelectionActionLinePolicy::Start {
        target_lines.start_timer(frame);
    }
}

/// Emit the modeled VoiceSelect side effect for one successful Select call.
fn emit_selection_voice(state: &mut AppState, entity_id: u64) {
    let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    if let Some(event) = crate::app::match_runtime::sound_dispatch::selection_voice_event(
        &mut runtime.simulation,
        &runtime.resources.rules,
        entity_id,
        state.match_state.input.selection_voice_enabled,
    ) {
        state.match_state.match_audio.sound_events.push(event);
    }
}
