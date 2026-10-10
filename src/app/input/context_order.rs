//! Context-sensitive order resolution — translates a screen click into game commands.
//!
//! Given a click position and the current selection, determines what command to issue:
//! move, attack, garrison, deploy, harvest, rally point, etc. This is the decision tree
//! that maps player intent to `Command` envelopes.
//!
//! Split from the input dispatcher to separate order resolution from raw input handling.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner;
use crate::app::input::dispatch::{
    is_alt_held, is_ctrl_held, is_shift_held, selected_stable_ids_in_order,
};
use crate::app::input::entity_pick::{
    HoverTargetKindWithId, hover_target_at_point, pick_any_target_stable_id,
    pick_enemy_target_stable_id,
};
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;

/// The verb a Ctrl / Shift / Alt chord selects for a tactical click.
///
/// Retail contract, read from `TechnoClass::What_Action_OnCell`,
/// `TechnoClass::What_Action_OnObject` and `TechnoClass::Player_Send_Command`:
///
/// * **Ctrl alone → force fire.** The cell path takes the attack branch with no
///   enemy under the cursor; the object path drops the ally guard.
/// * **Alt alone → force move.** The object path returns the plain Move action
///   instead of whatever context action the object would resolve, and the cell
///   path returns Move without running its occupancy probe.
/// * **Ctrl+Shift → attack move.** `What_Action_OnCell` cancels Shift and Ctrl
///   against each other, so the action itself resolves as an ordinary
///   Move/Attack; `Player_Send_Command` then promotes a committed Move or Attack
///   mission to attack-move whenever the chord test passes. That test reads the
///   raw key state, so the chord still fires when Alt is also held — and because
///   the cancel has already cleared Ctrl, the Ctrl+Alt guard-area gate cannot.
/// * **Ctrl+Alt → guard area** (patrol when the cell carries a waypoint).
/// * **Shift alone.** On an object it returns the
///   add-to-selection action, which the object click handler has no case for and
///   therefore sends no mission; on a cell it returns the plain Move action, an
///   ordinary immediate move. Represented Walk Infantry and Drive Unit Cell
///   producers resolve and encode that Move through the shared owner. Other
///   receiver and object contexts retain VERA's queue adapter. Retail Planning
///   Mode has its own event opcodes and remains unimplemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderModifier {
    /// No modifier held — the object/cell context action stands.
    Normal,
    /// Ctrl — force fire.
    ForceFire,
    /// Alt — force move.
    ForceMove,
    /// Ctrl+Shift — attack move.
    AttackMove,
    /// Ctrl+Alt — guard area.
    GuardArea,
    /// Shift — the producer normalizes represented Infantry/Drive Unit Cell
    /// Moves; other contexts retain VERA's queue adapter.
    Queue,
}

/// Resolve the retail modifier verb from the three held-key states.
///
/// Ordering mirrors the binary: the Ctrl+Shift chord is tested against the raw
/// key state before anything else, then the Ctrl+Alt guard-area gate, then the
/// single-modifier branches. Shift outranks Alt because the cell path returns on
/// Shift before it ever reaches the Alt test, and the object path returns the
/// add-to-selection action before the Alt branch.
pub(crate) fn resolve_order_modifiers(ctrl: bool, shift: bool, alt: bool) -> OrderModifier {
    if ctrl && shift {
        OrderModifier::AttackMove
    } else if ctrl && alt {
        OrderModifier::GuardArea
    } else if ctrl {
        OrderModifier::ForceFire
    } else if shift {
        OrderModifier::Queue
    } else if alt {
        OrderModifier::ForceMove
    } else {
        OrderModifier::Normal
    }
}

/// INI voice key for the mission a command commits.
///
/// `TechnoClass::Player_Send_Command` dispatches the order-ack line by mission
/// number: Harvest, Attack, Move (shared with attack-move), Enter, Capture and
/// Unload each have their own slot, and every other mission falls to a default
/// branch that draws a random entry from the type's `VoiceSpecialAttack` list.
///
/// Capture has its **own** slot, and it is wired here now. The dispatcher at
/// `0x00708DC0` reads the type's `VoiceCapture=` (`TechnoTypeClass+0x55C`) and
/// queues it (`0x00708DE8 CALL [EDI+0x354]` = `TechnoClass::Queue_Voice @
/// 0x00708D90`); only when that slot is the `-1` sentinel (`0x00708DCB CMP
/// [EAX+0x55C],-1`) does it fall through to the Enter-voice method
/// (`0x00708DF5 CALL [EDX+0x358]` = `0x00709020`, which reads
/// `VoiceEnter=` at `+0x558` and itself falls through to `[+0x368]` when that
/// is absent). Every stock engineer ships `VoiceCapture=`, so before this arm
/// existed every engineer capture spoke the move line (Allied `EngAllMove`
/// instead of `EngAllAttackCommand`, Soviet `EngSovMove` instead of
/// `EngSovAttackCommand`; Yuri's two keys hold the same sound, so Yuri was
/// unaffected).
///
/// One retail slot still has no VERA counterpart:
/// * Deploy/unload plays `VoiceDeploy` / `VoiceUndeploy`, neither of which VERA
///   parses — those orders stay silent.
///
/// The self-click halt is silent in retail as well: it builds a detonate event
/// directly instead of sending a mission, so the voice dispatch never runs.
fn order_voice_key(command: &Command) -> Option<&'static str> {
    match command {
        Command::Move { .. } | Command::AttackMove { .. } => Some("VoiceMove"),
        Command::Attack { .. } | Command::ForceAttack { .. } | Command::ForceAttackCell { .. } => {
            Some("VoiceAttack")
        }
        Command::HarvestCell { .. } => Some("VoiceHarvest"),
        Command::CaptureBuilding { .. } => Some("VoiceCapture"),
        Command::MinerReturn { .. }
        | Command::EnterTransport { .. }
        | Command::RepairAtDepot { .. }
        | Command::EnterBunker { .. } => Some("VoiceEnter"),
        // Sabotage and area guard have no dedicated slot, so retail takes the
        // default branch and speaks a VoiceSpecialAttack line.
        Command::PlantC4 { .. } | Command::Guard { .. } => Some("VoiceSpecialAttack"),
        _ => None,
    }
}

/// The entity a command acts on, when the command targets exactly one.
///
/// Used to find which queued order belongs to the speaking object.
fn command_actor_id(command: &Command) -> Option<u64> {
    match command {
        Command::Move { entity_id, .. }
        | Command::AttackMove { entity_id, .. }
        | Command::Guard { entity_id, .. }
        | Command::HarvestCell { entity_id, .. }
        | Command::MinerReturn { entity_id, .. }
        | Command::RepairAtDepot { entity_id, .. }
        | Command::ToggleInfantryDeploy { entity_id }
        | Command::DeployMcv { entity_id }
        | Command::UndeployBuilding { entity_id }
        | Command::Stop { entity_id } => Some(*entity_id),
        Command::Attack { attacker_id, .. }
        | Command::ForceAttack { attacker_id, .. }
        | Command::ForceAttackCell { attacker_id, .. }
        | Command::PlantC4 { attacker_id, .. } => Some(*attacker_id),
        Command::EnterTransport { passenger_id, .. } => Some(*passenger_id),
        Command::EnterBunker { unit_id, .. } => Some(*unit_id),
        Command::CaptureBuilding { engineer_id, .. } => Some(*engineer_id),
        Command::UnloadPassengers { transport_id } => Some(*transport_id),
        Command::EjectBunker { bunker_id } => Some(*bunker_id),
        Command::SetRally { producer_ids, .. } if producer_ids.len() == 1 => Some(producer_ids[0]),
        _ => None,
    }
}

/// Selection4AE750 object loop4AE829..4AE85A and ground loop4AE95B..4AE990
/// resolve each object's action in selection-array order.
/// Capability batches may remove handled actors, but must not reorder their
/// single-object events, including each prepared factory rally. Legacy aggregate
/// commands cannot be placed at one actor's index and keep dispatcher order.
/// This index derives from input selection.
fn restore_selection_dispatch_order(queued: &mut [CommandEnvelope], selected: &[u64]) {
    if queued
        .iter()
        .any(|envelope| command_actor_id(&envelope.payload).is_none())
    {
        return;
    }
    let positions: std::collections::HashMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(index, &id)| (id, index))
        .collect();
    queued.sort_by_key(|envelope| {
        command_actor_id(&envelope.payload)
            .and_then(|id| positions.get(&id).copied())
            .unwrap_or(usize::MAX)
    });
}

/// Play the single order-ack line for a batch of freshly queued orders.
///
/// Retail's dispatch loop clears the voice-enable flag at the end of *every*
/// iteration and restores it once after the loop, so exactly one object speaks —
/// the first entry of the selection array — and it speaks the line for the
/// mission *it* resolved, not an order-wide line. If that first object resolved
/// no order (its action mapped to a cursor-only code) nothing is spoken.
fn emit_resolved_order_voice(state: &mut AppState, speaker_id: u64, queued: &[CommandEnvelope]) {
    let Some(voice_field) = queued
        .iter()
        .find(|env| command_actor_id(&env.payload) == Some(speaker_id))
        .and_then(|env| order_voice_key(&env.payload))
    else {
        return;
    };
    emit_entity_order_voice(state, speaker_id, voice_field);
}

/// Resolve one `Voice*` slot on an object type, with the native fallbacks.
///
/// The capture and enter slots are the two with a fallback in gamemd, and they
/// chain: `0x00708DCB CMP [type+0x55C],-1 ; JZ 0x00708DF1` sends an absent
/// `VoiceCapture=` to the Enter-voice method at `0x00709020`, which reads
/// `VoiceEnter=` (`TechnoTypeClass+0x558`) and, when *that* is the `-1`
/// sentinel too (`0x0070902B CMP [EAX+0x558],-1 ; JZ 0x00709051`), calls
/// `[vtable+0x368]` — `0x00708FC0` for the BuildingClass-family vtable at
/// `0x007E3EBC` (`read_memory 0x007E4224` → `0x00708FC0`). That body is **not**
/// silence: it reads the `VoiceMove=` sound list at `TechnoTypeClass+0x468`
/// (`0x00712AB6 LEA EDI,[EBP+0x468]` in `TechnoTypeClass::ReadINI` passes key
/// `"VoiceMove"` at `0x008442C0` to `CCINIClass::ReadSoundList @ 0x00525430`),
/// skips only when its count at `+0x478` is zero
/// (`0x00708FD6 TEST ECX,ECX ; JZ 0x00709014`), and otherwise queues
/// `items[rand % count]` through `[vtable+0x354]`.
///
/// VERA models `VoiceMove=` as one id, not a list. No stock type authors a
/// comma-separated `VoiceMove=` (0 of 148 occurrences in `ini/rulesmd.ini`),
/// so the pick is observationally identical on retail; the `rand % count` draw
/// native spends on RNG instance `0x00886B88` (`0x00708FE1 CALL 0x0065C780`)
/// has no VERA counterpart. Recorded, not landed.
fn voice_id_for_key<'a>(
    obj: &'a crate::rules::object_type::ObjectType,
    voice_field: &str,
) -> Option<&'a String> {
    match voice_field {
        "VoiceMove" => obj.voice_move.as_ref(),
        "VoiceAttack" => obj.voice_attack.as_ref(),
        "VoiceHarvest" => obj.voice_harvest.as_ref(),
        "VoiceCapture" => obj
            .voice_capture
            .as_ref()
            .or(obj.voice_enter.as_ref())
            .or(obj.voice_move.as_ref()),
        "VoiceEnter" => obj.voice_enter.as_ref().or(obj.voice_move.as_ref()),
        _ => None,
    }
}

/// Play one entity's voice line for the given `Voice*` INI key.
///
/// The superseded app-layer order-voice helper always spoke the lowest-`stable_id` selected
/// entity; retail speaks the object that resolved the order, so order resolution
/// needs to name the speaker explicitly.
fn emit_entity_order_voice(state: &mut AppState, speaker_id: u64, voice_field: &str) {
    if voice_field == "VoiceSpecialAttack" {
        if let Some(runtime) = state.match_state.sim_runtime.as_mut()
            && let Some(event) =
                crate::app::match_runtime::sound_dispatch::default_order_voice_event(
                    &mut runtime.simulation,
                    &runtime.resources.rules,
                    speaker_id,
                    state.match_state.input.selection_voice_enabled,
                )
        {
            state.match_state.match_audio.sound_events.push(event);
        }
        return;
    }
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return;
    };
    let Some(rules) = state.rules().map(|r| r) else {
        return;
    };
    let Some(entity) = sim.entities().get(speaker_id) else {
        return;
    };
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return;
    };
    let Some(id) = voice_id_for_key(obj, voice_field) else {
        return;
    };
    let event = if voice_field == "VoiceAttack" {
        crate::audio::events::GameSoundEvent::UnitAttackOrder {
            speaker_id,
            sound_id: id.clone(),
        }
    } else {
        crate::audio::events::GameSoundEvent::UnitMoveOrder {
            speaker_id,
            sound_id: id.clone(),
        }
    };
    state.match_state.match_audio.sound_events.push(event);
}

/// Commit a resolved context dispatch: one voice line, the queue, then its timer.
///
/// Every exit from order resolution goes through here so the single-speaker rule
/// holds for the capability branches (garrison, C4, capture, depot, bunker,
/// deploy) as well as for the move/attack tail.
/// Display4ABFA9 calls Selection4AE750, then unconditionally starts the global
/// action-line timer at4ABFAE, including Move1/NoMove2 with no admitted event.
/// Both take the common4AC294 exit and RET4AC2A7, without visiting Select7.
/// The return value means that dispatch consumed the click, not that it
/// admitted an event; an empty batch must not fall through to selection clear.
/// Friendly click-selection and an empty selection never enter this boundary.
/// Evidence: tools/procedural_drawing_oracle/action_lines.
fn finish_order(
    state: &mut AppState,
    queued: Vec<CommandEnvelope>,
    speaker_id: Option<u64>,
) -> bool {
    let mut queued = if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    {
        queued
            .into_iter()
            .filter_map(|envelope| {
                crate::app::input::commands::roundtrip_ordinary_local_megamission(sim, envelope)
            })
            .collect::<Vec<_>>()
    } else {
        queued
    };
    let queued_any = !queued.is_empty();
    if queued_any {
        restore_selection_dispatch_order(
            &mut queued,
            &selected_stable_ids_in_order(
                state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .map(|rt| &rt.simulation),
                state.rules(),
                &state.match_state.input.selection_order,
                state.match_state.input.selection_order_pending,
            ),
        );
        if let Some(speaker_id) = speaker_id {
            emit_resolved_order_voice(state, speaker_id, &queued);
        }
        if let Some(sim) = state
            .match_state
            .sim_runtime
            .as_mut()
            .map(|rt| &mut rt.simulation)
        {
            sim.queue_commands(queued);
        }
    }
    if let Some(frame) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| rt.simulation.session.binary_frame)
    {
        state
            .match_state
            .match_presentation
            .target_lines
            .start_timer(frame);
    }
    true
}

/// The two spellings a weapon reference uses to mean "no weapon".
///
/// The retail weapon lookup compares the INI value against both before it ever
/// searches the weapon table and answers null for either, so `Primary=none` is
/// exactly the same as having no `Primary=` line at all.
const NO_WEAPON_NAMES: [&str; 2] = ["none", "<none>"];

/// Does this object accept an attack-move order?
///
/// Retail asks the object, and the object forwards the question straight to its
/// *type*, where three answers live:
///
/// * a **building** type answers no, unconditionally;
/// * an **aircraft** type answers no, unconditionally;
/// * every other type answers "the `Primary` weapon field names a real weapon
///   **and** `PreventAttackMove=` is off".
///
/// The `Secondary` slot is never consulted — a secondary-only type is refused —
/// and there is no harvester clause anywhere. The Soviet War Miner and the Slave
/// Miner both carry a real primary, so retail lets them attack-move along with
/// the rest of a defended-expansion group; the Chrono Miner is refused by its
/// `Primary=none` on its own.
///
/// gamemd-derived: `TechnoTypeClass::Can_Attack_Move @ 0x00711E90` (vtable
/// `+0xA4`) — `Primary(+0x898) != NULL && PreventAttackMove(+0x6C8) == 0`.
/// Native reads the raw slot-0 *field*, not `GetWeapon`, so the elite tier does
/// not apply here and `obj.primary()` is the exact read: `+0x898` is the storage
/// `Weapon1=` writes as well (`TechnoTypeClass::ReadINI @ 0x0071294A`, cursor
/// seeded at `0x007128D6`), which `ObjectType::read_weapon_arrays` reproduces.
/// That is what lets a Prism Tank (`[SREF]`, whose `Primary=Comet` is commented
/// out but whose `Weapon1=Comet` is not) attack-move here as it does in gamemd —
/// and, because `selection_can_attack_move` below is an `.all(..)`, keeps one
/// Prism Tank in a mixed selection from disabling the chord for the whole group.
///
/// Stock YR sets `PreventAttackMove=yes` on eleven types and `=no` on two. Two
/// of the eleven are refused anyway by the aircraft rule above — `ORCA` and
/// `BEAG`, the sole `[AircraftTypes]` members of the set. The other nine used to
/// slip through, each carrying a real `Primary=` and so passing the weapon
/// half: the three Engineers and the Spy (`DefuseKit`/`MakeupKit`), Boris
/// (`CCOMAND`, an infantry type), and the four `[VehicleTypes]` helicopters
/// `SHAD`, `HIND`, `SCHP` and `SCHD` (`BlackHawkCannon`), which `ObjectCategory`
/// derives from list membership and so classifies as ordinary units, not
/// aircraft. An Engineer walked into fire and a Nighthawk full of infantry flew
/// at the enemy where retail leaves the chord inert for them.
///
/// Note also that the sim side is confirmed complete for this row:
/// `MissionClass::Mission_Dispatch @ 0x005B3060` has no case for mission 29, so
/// Attack Move is an assign-side selector with no dispatcher handler, exactly
/// as modelled.
fn entity_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    stable_id: u64,
) -> bool {
    let Some(entity) = sim.entities().get(stable_id) else {
        return false;
    };
    if matches!(
        entity.category,
        EntityCategory::Structure | EntityCategory::Aircraft
    ) {
        return false;
    }
    let Some(obj) = rules.and_then(|r| r.object(sim.interner.resolve(entity.type_ref()))) else {
        return false;
    };
    if obj.prevent_attack_move {
        return false;
    }
    obj.primary().is_some_and(|primary| {
        let primary = primary.trim();
        !primary.is_empty()
            && !NO_WEAPON_NAMES
                .iter()
                .any(|none| primary.eq_ignore_ascii_case(none))
    })
}

/// Every selected object has to accept an attack-move order for the chord to fire.
///
/// The chord test walks the *whole* current selection — buildings and aircraft
/// included, not just the mobiles that would receive the order — and fails
/// outright the moment one member answers no.
fn selection_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    selected_ids: &[u64],
) -> bool {
    if selected_ids.is_empty() {
        return false;
    }
    selected_ids
        .iter()
        .all(|&sid| entity_can_attack_move(sim, rules, sid))
}

/// How far the passable-cell fallback searches when the clicked cell is blocked.
const GOAL_FALLBACK_RADIUS: u16 = 12;

/// Resolve a clicked cell to the cell a mover can actually stand on.
///
/// Retail resolves the click to a cell and then walks outward for a passable one
/// rather than refusing the order, so an unwalkable goal — water, a cliff, a
/// building's own footprint — becomes the nearest cell that works.
fn nearest_reachable_goal(
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
    goal: (u16, u16),
) -> (u16, u16) {
    let Some(grid) = path_grid else {
        return goal;
    };
    if crate::app::match_runtime::sim_tick::is_any_layer_walkable(grid, goal.0, goal.1) {
        return goal;
    }
    crate::app::match_runtime::sim_tick::nearest_walkable_cell_layered(
        grid,
        goal,
        GOAL_FALLBACK_RADIUS,
    )
    .unwrap_or(goal)
}

/// The command one selected unit commits for a tactical click on an object.
///
/// Retail resolves the object action, commits the mission, and only then
/// promotes it: a committed **Attack** is promoted to attack-move exactly as
/// readily as a committed **Move** is, and the promotion is gated per object on
/// that object's own type predicate. So a chorded click on an enemy tank sends
/// the selection walking toward it in fighting order rather than charging it,
/// while a member whose type refuses attack-move still commits the plain attack.
pub(crate) fn object_click_payload(
    order_mode: OrderMode,
    modifier: OrderModifier,
    can_attack_move: bool,
    attacker_id: u64,
    target_id: u64,
    target_rx: u16,
    target_ry: u16,
    queue: bool,
) -> Command {
    if modifier == OrderModifier::ForceFire {
        return Command::ForceAttack {
            attacker_id,
            target_id,
        };
    }
    if modifier == OrderModifier::GuardArea {
        return Command::Guard {
            entity_id: attacker_id,
            target: Some(TargetKind::Entity(target_id)),
        };
    }
    match order_mode {
        OrderMode::AttackMove if can_attack_move => Command::AttackMove {
            entity_id: attacker_id,
            target_rx,
            target_ry,
            queue,
        },
        _ => Command::Attack {
            attacker_id,
            target_id,
        },
    }
}

/// Attempt to issue a context-sensitive order at the given screen point.
///
/// Returns `true` when context dispatch consumed the click, including a refused
/// event, and `false` when the click should fall through to selection handling.
///
/// When `select_friendly_clicks` is true, clicks on friendly units/structures
/// return `false` so the caller can treat them as selection clicks instead.
pub(crate) fn try_queue_context_order_at_screen_point(
    state: &mut AppState,
    screen_x: f32,
    screen_y: f32,
    select_friendly_clicks: bool,
) -> bool {
    if state.match_state.sim_runtime.is_none() {
        return false;
    }
    let (world_x, world_y) =
        crate::app::match_runtime::sim_tick::screen_point_to_world(state, screen_x, screen_y);
    let (target_rx, target_ry) =
        crate::app::match_runtime::sim_tick::screen_point_to_world_cell(state, screen_x, screen_y);
    // Retail modifier map: Ctrl = force fire, Alt = force move,
    // Ctrl+Shift = attack move, Ctrl+Alt = guard area. Shift alone has no retail
    // order semantics at all and carries VERA's order queue instead — see
    // `OrderModifier` for the full derivation.
    let mut modifier = resolve_order_modifiers(
        is_ctrl_held(state),
        is_shift_held(state),
        is_alt_held(state),
    );
    let order_mode = state.match_state.input.queued_order_mode;
    let owner: String = preferred_local_owner(state).unwrap_or_else(|| "Americans".to_string());
    let owner_id: InternedId = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .and_then(|s| s.interner.get(&owner))
        .unwrap_or_default();

    let mut queued: Vec<CommandEnvelope> = Vec::new();
    let mut consumed_order_mode = false;
    // The one object that speaks the order-ack line. Retail lets only the first
    // entry of the selection array speak.
    let mut speaker_id: Option<u64> = None;
    let selected_ids = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    // `EVA_NewRallyPointEstablished` is spoken by the click handler itself,
    // after the sim borrow below ends.
    let mut rally_announce = false;

    if let Some(rt) = state.match_state.sim_runtime.as_mut() {
        let resources = &rt.resources;
        let sim = &mut rt.simulation;
        let execute_tick = sim.session.tick;
        if selected_ids.is_empty() {
            return false;
        }
        speaker_id = selected_ids.first().copied();

        let mut selected_units: Vec<u64> = Vec::new();
        let mut selected_miner_ids: Vec<u64> = Vec::new();
        let mut structure_selected = false;
        let mut mobile_count: usize = 0;

        for &sid in &selected_ids {
            let Some(entity) = sim.entities().get(sid) else {
                continue;
            };
            if entity.category == EntityCategory::Structure {
                structure_selected = true;
            } else {
                mobile_count += 1;
                selected_units.push(sid);
                if entity.miner.is_some() {
                    selected_miner_ids.push(sid);
                }
            }
        }
        // The chord test fails — and the order resolves normally — unless every
        // selected object can accept an attack-move order. The walk covers the
        // whole selection, so a selected building or aircraft kills the chord.
        if modifier == OrderModifier::AttackMove
            && !selection_can_attack_move(sim, Some(&resources.rules), &selected_ids)
        {
            modifier = OrderModifier::Normal;
        }
        let queue_mode: bool = modifier == OrderModifier::Queue;
        let force_fire: bool = modifier == OrderModifier::ForceFire;
        let force_move: bool = modifier == OrderModifier::ForceMove;
        // Force fire, force move and guard area each replace the object's own
        // context action outright, so the capability branches below (miner
        // return, garrison, C4, engineer capture, depot, bunker, self-click
        // deploy) are skipped for them. The attack-move chord does not: it
        // cancels both its modifiers before the action resolves and only
        // promotes the *committed* mission afterwards, so a chorded click on a
        // capturable building still captures.
        let context_actions_enabled: bool = matches!(
            modifier,
            OrderModifier::Normal | OrderModifier::Queue | OrderModifier::AttackMove
        );
        // The ore/gem harvest action hangs off the *cell* action, which stays
        // Move under Alt and under the cancelled chord but is replaced by force
        // fire and guard area.
        let cell_context_enabled: bool = !force_fire && modifier != OrderModifier::GuardArea;
        // RESIDUAL (#605): this legacy Ctrl+Alt classifier does not execute
        // Techno object700191..700217 or Cell700830..70089A's complete gates
        // and waypoint alternatives. Occasional chorded clicks may therefore
        // request Guard where native chooses another action. The shared Guard
        // post below preserves the accepted target, but cannot correct that
        // earlier choice; full click/cursor classification has its own owner.
        // A held chord overrides the sticky sidebar order mode for this click.
        let order_mode = match modifier {
            OrderModifier::AttackMove => OrderMode::AttackMove,
            _ => order_mode,
        };

        let hover = hover_target_at_point(
            sim,
            world_x,
            world_y,
            &owner,
            state.match_state.sandbox_full_visibility,
            Some(&resources.rules),
            &state.match_state.match_presentation.height_map,
            crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
        );

        let only_miners_selected = mobile_count > 0 && selected_miner_ids.len() == mobile_count;
        let clicked_friendly_refinery_id = context_actions_enabled
            .then(|| {
                hover.as_ref().and_then(|target| {
                    if target.kind != HoverTargetKind::FriendlyStructure {
                        return None;
                    }
                    let rules = Some(&resources.rules)?;
                    sim.entities().get(target.stable_id).and_then(|e| {
                        rules
                            .is_refinery_type(sim.interner.resolve(e.type_ref()))
                            .then_some(target.stable_id)
                    })
                })
            })
            .flatten();
        let clicked_friendly_refinery = clicked_friendly_refinery_id.is_some();

        // Check if the clicked cell holds tiberium (ore/gems).
        let clicked_ore = cell_context_enabled
            && match (
                sim.overlay_grid.as_ref(),
                Some(&resources.overlay_registry),
                Some(&resources.rules),
            ) {
                (Some(grid), Some(registry), Some(rules)) if !rules.tiberium_types.is_empty() => {
                    crate::sim::tiberium::tiberium_cell_view(
                        grid,
                        registry,
                        &rules.tiberium_types,
                        (target_rx, target_ry),
                    )
                    .is_some()
                }
                _ => false,
            };

        if clicked_friendly_refinery && only_miners_selected {
            for stable_id in selected_miner_ids {
                queued.push(CommandEnvelope::new(
                    owner_id,
                    execute_tick,
                    Command::MinerReturn {
                        entity_id: stable_id,
                        target_refinery_id: clicked_friendly_refinery_id,
                    },
                ));
            }
            // The manual return order commits mission Enter, so its ack is the
            // VoiceEnter slot (e.g. CMIN ChronoMinerReturn) — resolved from the
            // command itself by `order_voice_key`.
        } else if clicked_ore && !selected_miner_ids.is_empty() {
            // Direct miners to harvest the clicked ore cell.
            for &stable_id in &selected_miner_ids {
                queued.push(CommandEnvelope::new(
                    owner_id,
                    execute_tick,
                    Command::HarvestCell {
                        entity_id: stable_id,
                        target_rx,
                        target_ry,
                    },
                ));
            }
            // The harvest order commits mission Harvest, so its ack is the
            // VoiceHarvest slot (e.g. CMIN ChronoMinerHarvest); a non-miner in
            // the same selection commits Move and would speak VoiceMove.
            // Non-miner units in selection just move to that cell, through the
            // same 4DE1D0 cell receiver as an ordinary Move click.
            for &stable_id in &selected_units {
                if !selected_miner_ids.contains(&stable_id) {
                    let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                        sim,
                        &resources.rules,
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        true,
                        queue_mode,
                    ) else {
                        continue;
                    };
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::Move {
                            entity_id: stable_id,
                            target_rx: goal.cell.0,
                            target_ry: goal.cell.1,
                            queue: goal.queue,
                        },
                    ));
                }
            }
        } else if structure_selected {
            let clicked_friendly = hover.as_ref().is_some_and(|target| {
                matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            });
            if context_actions_enabled
                && let Some(cmd) = self_click_command(
                    sim,
                    &resources.rules,
                    owner_id,
                    &selected_ids,
                    hover.as_ref(),
                )
            {
                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                return finish_order(state, queued, speaker_id);
            }
            if select_friendly_clicks && clicked_friendly && context_actions_enabled {
                return false;
            }
            {
                // Set rally point for the structures.
                {
                    // `SetRallyPoint 0x00443860` sends the rally event as the
                    // building's owner; the click names only the local
                    // player's buildings that take a rally, so that owner is
                    // the local player. A building without a rally point gets
                    // `ACTION_NONE` on a cell (`0x0044762A`, `0x0044774F`):
                    // with no such building the click orders nothing here.
                    let producer_ids =
                        selected_rally_producer_ids(sim, &resources.rules, &selected_ids, owner_id);
                    // `BuildingClass::SetRallyPoint 0x00443A2B..0x00443A69`,
                    // called per selected factory with announce = 1 by the
                    // map-click handlers `FUN_00443410` / `FUN_004436F0`:
                    // after the rally `EventClass` is pushed, the
                    // clicking machine speaks `EVA_NewRallyPointEstablished`
                    // when the factory's owner is the local player
                    // (`0x00443A3C CALL 0x0050B6F0`) and the type is neither
                    // a `ConstructionYard=` nor a `ResourceDestination=`.
                    // One `PlayEVA` per factory; VoxClass drops same-entry
                    // duplicates, so one request per click is equivalent.
                    for id in producer_ids {
                        let target = match sim.factory_rally_cell_input(
                            id,
                            (target_rx, target_ry),
                            &resources.rules,
                        ) {
                            Ok(target) => target,
                            Err(error) => {
                                log::warn!("factory rally input: {error}");
                                None
                            }
                        };
                        let Some((rx, ry)) = target else {
                            continue;
                        };
                        // Event1E carries this factory's already prepared cell.
                        // Different selected factories can resolve different zones.
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::SetRally {
                                rx,
                                ry,
                                producer_ids: vec![id],
                            },
                        ));
                        rally_announce |= sim
                            .entities()
                            .get(id)
                            .and_then(|entity| {
                                resources
                                    .rules
                                    .object(sim.interner.resolve(entity.type_ref()))
                            })
                            .is_some_and(
                                crate::app::match_runtime::eva_producers::rally_point_announces,
                            );
                    }
                }
                // Also issue Move commands for any mobile units in the
                // selection — RA2 moves units AND sets rally when both
                // are selected.
                if mobile_count > 0 {
                    for &stable_id in &selected_units {
                        // Each mobile resolves the clicked cell through its
                        // own 4DE1D0 receiver, as for an ordinary Move click.
                        let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                            sim,
                            &resources.rules,
                            owner_id,
                            stable_id,
                            (target_rx, target_ry),
                            true,
                            queue_mode,
                        ) else {
                            continue;
                        };
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Move {
                                entity_id: stable_id,
                                target_rx: goal.cell.0,
                                target_ry: goal.cell.1,
                                queue: goal.queue,
                            },
                        ));
                    }
                }
            }
        } else {
            // An Engineer's DisarmBomb is its first action over a bombed object.
            // Every selected object takes its own action
            // (`Selection__DispatchMultiUnitOrder` 0x004AE750, object loop
            // 0x004AE829..0x004AE85A): the Engineers defuse, and over an enemy the rest go on to
            // their own orders below. Over a friend the rest have nothing to do,
            // and the dispatch happens only when the object that owns the cursor
            // is one of the Engineers; otherwise the click selects.
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let friendly = matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                );
                let engineers = disarm_bomb_engineers(
                    sim,
                    &resources.rules,
                    &selected_units,
                    target.stable_id,
                    owner_id,
                );
                let dispatched = !engineers.is_empty()
                    && (!friendly
                        || cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                            .is_some_and(|best| engineers.contains(&best)));
                if dispatched {
                    for &attacker_id in &engineers {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            bomb_order(attacker_id, target.stable_id, friendly),
                        ));
                    }
                    selected_units.retain(|id| !engineers.contains(id));
                    if friendly || selected_units.is_empty() {
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            // Object51E3B0 Engineer decisions precede CanBeOccupiedBy and C4.
            // Consume each Engineer's terminal action while retaining the
            // other selected actors for their own actions below.
            let mut consumed_engineer_action = false;
            if context_actions_enabled
                && let Some(orders) = hover.as_ref().and_then(|target| {
                    engineer_capture_orders(sim, &resources.rules, &mut selected_units, target)
                })
            {
                consumed_engineer_action = true;
                queued.extend(
                    orders
                        .into_iter()
                        .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                );
                if selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id);
                }
            }

            // Garrison entry uses the shared CanBeOccupiedBy-equivalent predicate before
            // issuing EnterTransport commands.
            // are classified as EnemyStructure but are still garrisonable —
            if context_actions_enabled {
                let garrison_target = hover.as_ref().map(|target| target.stable_id);
                if let Some(transport_id) = garrison_target {
                    let infantry_ids: Vec<u64> = selected_units
                        .iter()
                        .copied()
                        .filter(|&sid| {
                            Some(&resources.rules).is_some_and(|rules| {
                                crate::sim::passenger::can_entity_enter_garrison(
                                    sim,
                                    rules,
                                    sid,
                                    transport_id,
                                )
                            })
                        })
                        .collect();
                    if !infantry_ids.is_empty() {
                        for pax_id in infantry_ids {
                            queued.push(CommandEnvelope::new(
                                owner_id,
                                execute_tick,
                                Command::EnterTransport {
                                    passenger_id: pax_id,
                                    transport_id,
                                },
                            ));
                        }
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            // C4 plant: SEAL / Tanya / Psi-Corp Trooper clicking a CanC4 enemy
            // structure. Engineers already resolved their higher-priority
            // native action; the remaining C4 actors take their own action.
            if context_actions_enabled {
                let c4_target = hover.as_ref().and_then(|target| {
                    if !matches!(target.kind, HoverTargetKind::EnemyStructure) {
                        return None;
                    }
                    let rules = Some(&resources.rules)?;
                    let building = sim.entities().get(target.stable_id)?;
                    let obj = rules.object(sim.interner.resolve(building.type_ref()))?;
                    if !obj.can_c4 || obj.invisible_in_game {
                        return None;
                    }
                    // Reject IC'd target at issue time (matches gamemd's
                    // What_Action_OnObject vtable[+0x80] check).
                    if crate::sim::superweapon::invulnerability::is_invulnerable(
                        building.invulnerability.as_ref(),
                        sim.session.tick as u32,
                    ) {
                        return None;
                    }
                    Some(target.stable_id)
                });
                if let Some(building_id) = c4_target {
                    let c4_attackers: Vec<u64> = selected_units
                        .iter()
                        .copied()
                        .filter(|&sid| {
                            sim.entities().get(sid).is_some_and(|e| {
                                e.category == EntityCategory::Infantry
                                    && Some(&resources.rules)
                                        .and_then(|r| r.object(sim.interner.resolve(e.type_ref())))
                                        .map_or(false, |o| o.c4)
                            })
                        })
                        .collect();
                    if !c4_attackers.is_empty() {
                        for attacker_id in c4_attackers {
                            queued.push(CommandEnvelope::new(
                                owner_id,
                                execute_tick,
                                Command::PlantC4 {
                                    attacker_id,
                                    target_building_id: building_id,
                                },
                            ));
                        }
                        // Sabotage has no dedicated voice slot in retail, so
                        // `order_voice_key` routes it to VoiceSpecialAttack.
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            // Service depot: damaged own vehicles clicking an own UnitRepair
            // building drive to the depot and auto-repair. Ordered before the
            // friendly-fallthrough so the click isn't consumed as re-selection.
            if context_actions_enabled {
                let depot_target = hover.as_ref().and_then(|target| {
                    if !matches!(target.kind, HoverTargetKind::FriendlyStructure) {
                        return None;
                    }
                    let rules = Some(&resources.rules)?;
                    let building = sim.entities().get(target.stable_id)?;
                    let obj = rules.object(sim.interner.resolve(building.type_ref()))?;
                    obj.unit_repair.then_some(target.stable_id)
                });
                if let Some(depot_id) = depot_target {
                    let repair_ids: Vec<u64> = selected_units
                        .iter()
                        .copied()
                        .filter(|&sid| {
                            sim.entities().get(sid).is_some_and(|e| {
                                e.category == EntityCategory::Unit
                                    && resources
                                        .rules
                                        .object(sim.interner.resolve(e.type_ref()))
                                        .is_some_and(|obj| e.health.current < obj.strength)
                                    && !e.is_deployed()
                            })
                        })
                        .collect();
                    if !repair_ids.is_empty() {
                        for unit_id in repair_ids {
                            queued.push(CommandEnvelope::new(
                                owner_id,
                                execute_tick,
                                Command::RepairAtDepot {
                                    entity_id: unit_id,
                                    depot_id,
                                },
                            ));
                        }
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            // Tank bunker: an own bunkerable vehicle clicking an own EMPTY tank
            // bunker installs into it. The bunker holds one unit, so only the
            // first eligible vehicle is sent. Occupied bunkers are ejected via
            // the self-click path below.
            if context_actions_enabled {
                let bunker_target = hover.as_ref().and_then(|target| {
                    if !matches!(target.kind, HoverTargetKind::FriendlyStructure) {
                        return None;
                    }
                    let building = sim.entities().get(target.stable_id)?;
                    (building.bunker_runtime.is_some() && building.bunker_occupant.is_none())
                        .then_some(target.stable_id)
                });
                if let Some(bunker_id) = bunker_target {
                    let unit_id = selected_units.iter().copied().find(|&sid| {
                        sim.entities().get(sid).is_some_and(|e| !e.is_deployed())
                            && Some(&resources.rules).is_some_and(|rules| {
                                crate::sim::docking::bunker_link::can_auto_deploy_here(
                                    sim, sid, rules,
                                )
                            })
                    });
                    if let Some(unit_id) = unit_id {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::EnterBunker { unit_id, bunker_id },
                        ));
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            let clicked_friendly = hover.as_ref().is_some_and(|target| {
                matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            });
            if context_actions_enabled
                && let Some(cmd) = self_click_command(
                    sim,
                    &resources.rules,
                    owner_id,
                    &selected_ids,
                    hover.as_ref(),
                )
            {
                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                return finish_order(state, queued, speaker_id);
            }
            // A Crazy Ivan bombs a friend by a plain click, when the object that
            // owns the cursor is one of the bombing Ivans.
            if clicked_friendly
                && context_actions_enabled
                && !force_move
                && let Some(target) = hover.as_ref()
            {
                let ivans =
                    friendly_bomb_ivans(sim, &resources.rules, &selected_units, target.stable_id);
                if cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                    .is_some_and(|best| ivans.contains(&best))
                {
                    for attacker_id in ivans {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            bomb_order(attacker_id, target.stable_id, true),
                        ));
                    }
                    return finish_order(state, queued, speaker_id);
                }
            }
            if select_friendly_clicks && clicked_friendly && context_actions_enabled {
                return if consumed_engineer_action {
                    finish_order(state, queued, speaker_id)
                } else {
                    false
                };
            }

            // Alt force-move: the object path returns the plain Move action, so
            // nothing under the cursor is treated as a target and the
            // destination becomes the clicked object's own cell (retail resolves
            // that cell, then falls back to a nearby passable one — the Move
            // payload below already routes through the same fallback).
            // Object action1/2 uses its own coordinate/zone receiver (4D77FB,
            //4D7816/4D78BA), not Cell-click4DE1D0. Keep that prior adapter.
            let ordinary_cell_receiver = !(force_move && hover.is_some());
            let (target_rx, target_ry) = if force_move {
                hover
                    .as_ref()
                    .and_then(|t| sim.entities().get(t.stable_id))
                    .map_or((target_rx, target_ry), |e| (e.position.rx, e.position.ry))
            } else {
                (target_rx, target_ry)
            };

            let attack_target: Option<u64> = if force_move {
                None
            } else if force_fire {
                pick_any_target_stable_id(
                    sim,
                    world_x,
                    world_y,
                    state.match_state.sandbox_full_visibility,
                    Some(&resources.rules),
                    &state.match_state.match_presentation.height_map,
                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                )
            } else {
                pick_enemy_target_stable_id(
                    sim,
                    world_x,
                    world_y,
                    &owner,
                    state.match_state.sandbox_full_visibility,
                    Some(&resources.rules),
                    &state.match_state.match_presentation.height_map,
                    crate::app::match_runtime::sim_tick::tactical_bridge_cells(sim),
                )
            };
            // VERA-internal: force-fire on a shrouded cell is rejected here.
            // gamemd equivalent CONTRADICTS this — the FootClass shroud wrapper
            // around What_Action_OnCell explicitly preserves the force-fire
            // action code through the shroud, and the function this gate's old
            // comment cited as a "shroud check" is in fact the waypoint lookup.
            // Left in place because removing it changes click routing; recorded
            // as a DRIFT for its own slice. Computed once outside the loop.
            let cell_is_shrouded: bool = if force_fire && !state.match_state.sandbox_full_visibility
            {
                let owner_id_for_fog = sim.interner.get(&owner).unwrap_or_default();
                !sim.fog
                    .is_cell_revealed(owner_id_for_fog, target_rx, target_ry)
                    || sim
                        .fog
                        .is_cell_gap_covered(owner_id_for_fog, target_rx, target_ry)
            } else {
                false
            };

            for stable_id in selected_units {
                if let Some(target_id) = attack_target
                    && ivan_cannot_bomb(sim, &resources.rules, stable_id, target_id)
                {
                    continue;
                }
                let payload = if let Some(target_id) = attack_target {
                    // Retail promotes the *committed* mission and keeps the
                    // object as the destination, so the attack-move goal is the
                    // object's own cell — routed through the same passable-cell
                    // fallback the Move payload uses, since a building's own
                    // cell is never walkable.
                    let (goal_rx, goal_ry) = sim
                        .entities()
                        .get(target_id)
                        .map_or((target_rx, target_ry), |e| (e.position.rx, e.position.ry));
                    let (goal_rx, goal_ry) =
                        nearest_reachable_goal(sim.path_grid(), (goal_rx, goal_ry));
                    object_click_payload(
                        order_mode,
                        modifier,
                        entity_can_attack_move(sim, Some(&resources.rules), stable_id),
                        stable_id,
                        target_id,
                        goal_rx,
                        goal_ry,
                        queue_mode,
                    )
                } else if force_fire && !cell_is_shrouded {
                    // Force-fire on empty terrain: per-unit dispatch matching
                    // gamemd `TechnoClass::What_Action_OnCell @ 0x00700600` —
                    // armed mobile units fire at the cell, unarmed
                    // (Engineer/Harvester/MCV) fall through to plain Move. The
                    // armed test is the inlined `TechnoClass::Is_Armed` at
                    // `0x007008BD` (one weapon slot, via `GetCurrentWeapon`),
                    // not `Primary=`/`Secondary=`: those keys are never parsed
                    // for a `TurretCount>0` type, so the Prism Tank and the
                    // Gattling Cannon were routed to Move instead of firing.
                    let unit_armed = sim
                        .entities()
                        .get(stable_id)
                        .and_then(|e| {
                            let type_str = sim.interner.resolve(e.type_ref());
                            Some(&resources.rules)
                                .and_then(|r| r.object(type_str))
                                .map(|obj| crate::sim::combat::combat_weapon::is_armed(e, obj))
                        })
                        .unwrap_or(false);
                    let is_harvester = sim
                        .entities()
                        .get(stable_id)
                        .is_some_and(|e| e.miner.is_some());

                    if unit_armed && !is_harvester {
                        Command::ForceAttackCell {
                            attacker_id: stable_id,
                            target_rx,
                            target_ry,
                        }
                    } else {
                        // Unarmed Cell-click fall-through resolves the original
                        // clicked Cell before committing its Move event.
                        let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                            sim,
                            &resources.rules,
                            owner_id,
                            stable_id,
                            (target_rx, target_ry),
                            ordinary_cell_receiver,
                            queue_mode,
                        ) else {
                            continue;
                        };
                        Command::Move {
                            entity_id: stable_id,
                            target_rx: goal.cell.0,
                            target_ry: goal.cell.1,
                            queue: goal.queue,
                        }
                    }
                } else if modifier == OrderModifier::GuardArea {
                    // An accepted Guard cell action carries the clicked Cell
                    // in MegaMission's target slot. Classification remains the
                    // existing Ctrl+Alt path; the native What_Action gates and
                    // waypoint/patrol alternatives belong to that separate
                    // input mechanism, not to the Guard key's 730D60 loop.
                    Command::Guard {
                        entity_id: stable_id,
                        target: Some(TargetKind::Cell(target_rx, target_ry)),
                    }
                } else {
                    match order_mode {
                        OrderMode::Move | OrderMode::AttackMove => {
                            let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                                sim,
                                &resources.rules,
                                owner_id,
                                stable_id,
                                (target_rx, target_ry),
                                order_mode == OrderMode::Move && ordinary_cell_receiver,
                                queue_mode,
                            ) else {
                                continue;
                            };
                            // The promotion to attack-move is per object: a unit
                            // whose type refuses it keeps the plain Move it
                            // committed, even when the rest of the group
                            // attack-moves.
                            if order_mode == OrderMode::AttackMove
                                && entity_can_attack_move(sim, Some(&resources.rules), stable_id)
                            {
                                Command::AttackMove {
                                    entity_id: stable_id,
                                    target_rx: goal.cell.0,
                                    target_ry: goal.cell.1,
                                    queue: goal.queue,
                                }
                            } else {
                                Command::Move {
                                    entity_id: stable_id,
                                    target_rx: goal.cell.0,
                                    target_ry: goal.cell.1,
                                    queue: goal.queue,
                                }
                            }
                        }
                    }
                };
                if let Command::Attack {
                    attacker_id,
                    target_id,
                } = payload
                    && click_attack_refused(
                        sim,
                        &resources.rules,
                        &resources.overlay_registry,
                        attacker_id,
                        target_id,
                    )
                {
                    continue;
                }
                queued.push(CommandEnvelope::new(owner_id, execute_tick, payload));
            }
            if !queued.is_empty() {
                consumed_order_mode = true;
            }
        }
    }

    if !queued.is_empty()
        && consumed_order_mode
        && state.match_state.input.queued_order_mode != OrderMode::Move
    {
        state.match_state.input.queued_order_mode = OrderMode::Move;
    }
    if rally_announce {
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::match_runtime::eva_producers::EVA_NEW_RALLY_POINT_ESTABLISHED,
        );
    }
    finish_order(state, queued, speaker_id)
}

/// The data capability shared by self-click, self-hover and the deploy key.
/// `UnitClass::What_Action_OnObject @ 0x0074000B` admits `IsSimpleDeployer`
/// to the deploy action; `CanDeploySlashUnload @ 0x00700E81` reads that same
/// UnitType flag. Deploy/undeploy transitions do not remove the capability.
/// The simulation owns command admission and the deployment lifecycle.
pub(super) fn is_simple_deploy_unit(
    entity: &crate::sim::game_entity::GameEntity,
    object: &crate::rules::object_type::ObjectType,
) -> bool {
    entity.category == EntityCategory::Unit && object.is_simple_deployer
}

/// The command a click on the one selected object issues when it clicks
/// itself. `TechnoClass::What_Action_OnObject` returns ACTION_SELF only when
/// the target is the acting object, exactly one object is selected
/// (`[0xA8ECC8] == 1`, `0x007000C1`) and its owner is the current player
/// (`0x007000B9..0x007000DF`). With more selected, a click on one of them
/// resolves to ACTION_SELECT, which the left-up handler turns into a fresh
/// selection of the clicked object (`0x004ABE18..`); the caller's friendly-click
/// path does that.
///
/// VERA picks the command from the clicked object's own capabilities. The
/// class overrides' full post-processing of ACTION_SELF (`UnitClass`
/// `0x0073FD50`, `InfantryClass` `0x0051E3B0`, `BuildingClass` `0x00447210`)
/// is not ported. Simple-deployer capability is shared with the cursor and
/// deploy key; the other self-hover capabilities remain separate (issue #605).
fn self_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    local_owner: InternedId,
    selected_ids: &[u64],
    hover: Option<&HoverTargetKindWithId>,
) -> Option<Command> {
    let target = hover?;
    if selected_ids != [target.stable_id] {
        return None;
    }
    let entity = sim
        .entities()
        .get(target.stable_id)
        .filter(|entity| entity.owner() == local_owner)?;
    let obj = rules.object(sim.interner.resolve(entity.type_ref()));
    if entity.category == EntityCategory::Structure {
        // A garrisoned building unloads its occupants, a tank bunker ejects
        // its unit, and a building that undeploys (a Construction Yard)
        // packs up.
        return if obj.is_some_and(|o| o.can_be_occupied)
            && entity.passenger_role.cargo().is_some_and(|c| !c.is_empty())
        {
            Some(Command::UnloadPassengers {
                transport_id: target.stable_id,
            })
        } else if entity.bunker_occupant.is_some() {
            Some(Command::EjectBunker {
                bunker_id: target.stable_id,
            })
        } else if sim.should_show_undeploy_building_command(target.stable_id, rules) {
            Some(Command::UndeployBuilding {
                entity_id: target.stable_id,
            })
        } else {
            None
        };
    }
    if entity.category == EntityCategory::Infantry && obj.is_some_and(|o| o.deploy_fire) {
        return Some(Command::ToggleInfantryDeploy {
            entity_id: target.stable_id,
        });
    }
    if let Some(cmd) = super::transport_orders::transport_unload_command(entity, obj) {
        return Some(cmd);
    }
    if obj.is_some_and(|object| is_simple_deploy_unit(entity, object)) {
        return sim
            .can_simple_deploy(target.stable_id, rules)
            .then_some(Command::DeployMcv {
                entity_id: target.stable_id,
            });
    }
    obj.is_some_and(|o| o.deploys_into.is_some() || o.deployer)
        .then_some(Command::DeployMcv {
            entity_id: target.stable_id,
        })
}

/// Engineer object actions before event enqueue. No duplicate geometry query.
/// Some(empty) is a consumed no-order hut click; None allows other actions.
fn engineer_capture_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &mut Vec<u64>,
    hover: &HoverTargetKindWithId,
) -> Option<Vec<Command>> {
    if !matches!(
        hover.kind,
        HoverTargetKind::FriendlyStructure | HoverTargetKind::EnemyStructure
    ) {
        return None;
    }
    let mut consumed = false;
    let mut orders = Vec::new();
    selected.retain(|&id| {
        let Some(actor) = sim.entities().get(id) else {
            return true;
        };
        if actor.category != EntityCategory::Infantry
            || !rules
                .object(sim.interner.resolve(actor.type_ref()))
                .is_some_and(|o| o.engineer)
        {
            return true;
        }
        let Some(action) = sim.engineer_building_action(id, hover.stable_id, rules) else {
            return true;
        };
        use crate::sim::world::EngineerBuildingAction;
        let admitted = matches!(
            action,
            EngineerBuildingAction::Capture
                | EngineerBuildingAction::Damage
                | EngineerBuildingAction::Repair(true)
        );
        //Hospital/grinder Enter producers still require their own entry
        //mechanism. They must not silently enqueue ordinary capture/repair.
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if admitted && sim.techno_player_controllable(id, rules) {
            orders.push(Command::CaptureBuilding {
                engineer_id: id,
                target_building_id: hover.stable_id,
            });
        }
        false
    });
    consumed.then_some(orders)
}

/// The selected Engineers whose click on `target` is DisarmBomb
/// (`InfantryClass::What_Action_OnObject`, 0x0051E462): their first action
/// over an object carrying a bomb their player sees, whoever owns it.
/// `InfantryClass::ClickedAction_Object` (0x0051F190) sends it as Attack, so
/// the DefuseKit shot defuses the bomb; it refuses the Engineer's own body.
fn disarm_bomb_engineers(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
    player: InternedId,
) -> Vec<u64> {
    let bombed = sim
        .entities()
        .get(target)
        .is_some_and(|entity| entity.bomb.is_some());
    if !bombed || !sim.bomb_seen_by(target, player) {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| {
            id != target
                && sim.entities().get(id).is_some_and(|e| {
                    e.category == EntityCategory::Infantry
                        && rules
                            .object(sim.interner.resolve(e.type_ref()))
                            .is_some_and(|o| o.engineer)
                })
        })
        .collect()
}

/// The object whose action owns the cursor over `target` for the selection
/// (gamemd's DetermineAction, shared with the cursor): the click dispatches
/// its orders only when that object's action is one.
fn cursor_owner(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
) -> Option<u64> {
    crate::app::input::cursor::select_best_for_action(
        sim,
        selected,
        crate::app::input::cursor::ActionDistanceTarget::Object(target),
        Some(rules),
    )
}

/// The selected Crazy Ivans whose click on the own or allied `target` plants a
/// bomb. AttackCursorOnFriendlies (stock: IVAN and CIVAN only) grants Attack
/// on a friend without passengers (`TechnoClass::What_Action_OnObject`,
/// 0x00700314..0x007003D6), the Ivan makes it IvanBomb on a Bombable target
/// without a bomb (0x0051EB24), and `FootClass::ClickedAction_Object`
/// (0x004D74E0) sends that as Attack. A bombed friend stays a selection click.
/// A lone selected Ivan over himself has the self action (4) instead; in a
/// larger selection nothing stops him bombing himself, as natively.
fn friendly_bomb_ivans(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    target: u64,
) -> Vec<u64> {
    let bombable = sim.entities().get(target).is_some_and(|entity| {
        entity.bomb.is_none()
            && rules
                .object(sim.interner.resolve(entity.type_ref()))
                .is_some_and(|o| o.bombable && o.passengers == 0)
    });
    if !bombable {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| {
            (id != target || selected.len() > 1)
                && sim.entities().get(id).is_some_and(|e| {
                    rules
                        .object(sim.interner.resolve(e.type_ref()))
                        .is_some_and(|o| o.ivan && o.attack_cursor_on_friendlies)
                })
        })
        .collect()
}

/// The Attack order an Engineer's DisarmBomb or a Crazy Ivan's IvanBomb click
/// sends. Native's Attack event has no alliance test; VERA's plain `Attack`
/// command refuses friends, so an own or allied target goes as `ForceAttack`,
/// the same Attack mission without that gate.
fn bomb_order(attacker_id: u64, target_id: u64, friendly: bool) -> Command {
    if friendly {
        Command::ForceAttack {
            attacker_id,
            target_id,
        }
    } else {
        Command::Attack {
            attacker_id,
            target_id,
        }
    }
}

/// A Crazy Ivan's Attack over a target it cannot bomb becomes NoIvanBomb
/// (`InfantryClass::What_Action_OnObject`, `0x0051EB24`: the target carries
/// a bomb (`+0x38`) or is `Bombable=no`), which
/// `FootClass::ClickedAction_Object` (0x004D74E0) ignores. An unforced click
/// on a bombed target never gets that far: its GetFireError is ILLEGAL
/// (`0x006FCBAD`), which [`click_attack_refused`] answers.
fn ivan_cannot_bomb(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    let is_ivan = sim
        .entities()
        .get(actor_id)
        .and_then(|e| rules.object(sim.interner.resolve(e.type_ref())))
        .is_some_and(|o| o.ivan);
    is_ivan
        && sim.entities().get(target_id).is_some_and(|target| {
            target.bomb.is_some()
                || !rules
                    .object(sim.interner.resolve(target.type_ref()))
                    .is_some_and(|o| o.bombable)
        })
}

/// `TechnoClass::What_Action_OnObject @ 0x00700548`: an unforced click on
/// an object stays Attack only while the object's GetFireError for it
/// ([`Simulation::selected_weapon_fire_error`]) is not ILLEGAL. Otherwise the action
/// is None or Select (`0x0070056C`), and the object takes no order.
///
/// RESIDUAL: the Infiltrate arm (`0x007004A0..0x00700531`) keeps Attack
/// despite ILLEGAL for an `Infiltrate=` infantryman over a building its flags
/// admit. VERA issues capture and C4 through their own orders before this,
/// and has no spy infiltration order; a spy's click on a building it cannot
/// shoot gets no order here, where retail enters it.
fn click_attack_refused(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor_id: u64,
    target_id: u64,
) -> bool {
    sim.selected_weapon_fire_error(
        rules,
        actor_id,
        crate::sim::combat::TargetKind::Entity(target_id),
        Some(overlay_registry),
    ) == crate::sim::combat::fire_error::FireError::Illegal
}

fn selected_rally_producer_ids(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    owner: InternedId,
) -> Vec<u64> {
    let owner = sim.interner.resolve(owner);
    let mut seen = std::collections::HashSet::with_capacity(selected_ids.len());
    selected_ids
        .iter()
        .copied()
        .filter(|&stable_id| {
            seen.insert(stable_id) && sim.takes_rally_point(stable_id, owner, rules)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    /// `TechnoClass::What_Action_OnObject @ 0x00700548`: an unforced click
    /// orders Attack only to the objects whose GetFireError for the target is
    /// not ILLEGAL. Out of range is RANGE (T61, the last test), not ILLEGAL.
    /// A Crazy Ivan's bombed target is ILLEGAL at any distance (T55,
    /// `0x006FCBAD`, comes first).
    #[test]
    fn an_unforced_click_refuses_only_an_illegal_shot() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n1=GI\n[VehicleTypes]\n0=HTNK\n\
                 [CombatDamage]\nIvanTimedDelay=450\n\
                 [IVAN]\nStrength=125\nPrimary=IvanBomber\nIvan=yes\n\
                 [GI]\nStrength=125\nPrimary=Pea\n\
                 [HTNK]\nStrength=900\nArmor=heavy\nPrimary=120mm\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
                 [120mm]\nDamage=90\nRange=5.75\nProjectile=Invisible\nWarhead=AP\n\
                 [Invisible]\nInviso=yes\n\
                 [IvanBomb]\nIvanBomb=yes\n\
                 [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.interner.intern(house);
            sim.session.house_order.push(id);
        }
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let tank = spawn("HTNK", "Americans", 3);
        let gi = spawn("GI", "Americans", 4);
        let ivan = spawn("IVAN", "Americans", 5);
        let near = spawn("HTNK", "Soviets", 6);
        let far = spawn("HTNK", "Soviets", 20);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let refused = |sim: &Simulation, actor: u64, target: u64| {
            click_attack_refused(sim, &rules, &overlays, actor, target)
        };

        assert!(refused(&sim, gi, near), "0% Verses against heavy armour");
        assert!(!refused(&sim, tank, near));
        assert!(!refused(&sim, tank, far), "out of range");
        assert!(!refused(&sim, ivan, near));

        sim.bomb_attach(ivan, Some(near), &rules);
        sim.bomb_attach(ivan, Some(far), &rules);
        assert!(refused(&sim, ivan, near), "a bombed target");
        assert!(
            refused(&sim, ivan, far),
            "the bomb gate precedes the range test"
        );
    }

    /// The rally click names the local player's selected buildings that take
    /// a rally: not their power plant, not another house's factory, not units.
    #[test]
    fn rally_click_names_only_the_players_rally_buildings() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=GAWEAP\n1=GAPOWR\n[VehicleTypes]\n0=MTNK\n\
                 [GAWEAP]\nFactory=UnitType\n[GAPOWR]\nPower=100\n[MTNK]\nStrength=300\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let other = sim.interner.intern("Russians");
        let factory_type = sim.interner.intern("GAWEAP");
        let power_type = sim.interner.intern("GAPOWR");
        let tank_type = sim.interner.intern("MTNK");
        for (id, rx, house, kind, category) in [
            (1, 10, owner, factory_type, EntityCategory::Structure),
            (2, 11, owner, tank_type, EntityCategory::Unit),
            (3, 12, owner, power_type, EntityCategory::Structure),
            (4, 13, other, factory_type, EntityCategory::Structure),
            (5, 14, owner, factory_type, EntityCategory::Structure),
        ] {
            sim.entities_mut()
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    rx,
                    10,
                    0,
                    0,
                    house,
                    Health { current: 300 },
                    kind,
                    category,
                    0,
                    5,
                    category == EntityCategory::Unit,
                ));
        }

        let producer_ids = selected_rally_producer_ids(&sim, &rules, &[5, 4, 3, 2, 1, 5], owner);

        assert_eq!(producer_ids, vec![5, 1]);
    }

    /// The retail modifier map, one row per chord. Ctrl force-fires, Alt forces
    /// a move, Ctrl+Shift is attack move and Ctrl+Alt is guard area.
    #[test]
    fn modifier_map_matches_retail_chords() {
        // (ctrl, shift, alt) -> verb
        assert_eq!(
            resolve_order_modifiers(false, false, false),
            OrderModifier::Normal
        );
        assert_eq!(
            resolve_order_modifiers(true, false, false),
            OrderModifier::ForceFire
        );
        assert_eq!(
            resolve_order_modifiers(false, false, true),
            OrderModifier::ForceMove
        );
        assert_eq!(
            resolve_order_modifiers(true, true, false),
            OrderModifier::AttackMove
        );
        assert_eq!(
            resolve_order_modifiers(true, false, true),
            OrderModifier::GuardArea
        );
        assert_eq!(
            resolve_order_modifiers(false, true, false),
            OrderModifier::Queue
        );
    }

    /// The chord test reads the raw key state, so Alt does not defeat it — and
    /// because Shift and Ctrl have already cancelled each other by the time the
    /// guard-area gate is evaluated, Ctrl+Shift+Alt is attack move, not guard.
    #[test]
    fn attack_move_chord_survives_a_held_alt() {
        assert_eq!(
            resolve_order_modifiers(true, true, true),
            OrderModifier::AttackMove
        );
    }

    /// Shift outranks Alt: the cell path returns on Shift before it reaches the
    /// Alt test, and the object path returns the add-to-selection action first.
    #[test]
    fn shift_outranks_alt_without_ctrl() {
        assert_eq!(
            resolve_order_modifiers(false, true, true),
            OrderModifier::Queue
        );
    }

    /// Each order speaks the voice slot of the mission it commits, and missions
    /// with no dedicated slot fall to the VoiceSpecialAttack default branch.
    #[test]
    fn order_voice_key_follows_the_committed_mission() {
        assert_eq!(
            order_voice_key(&Command::Move {
                entity_id: 1,
                target_rx: 0,
                target_ry: 0,
                queue: false,
            }),
            Some("VoiceMove")
        );
        // Attack-move commits through the Move voice slot, not the Attack one.
        assert_eq!(
            order_voice_key(&Command::AttackMove {
                entity_id: 1,
                target_rx: 0,
                target_ry: 0,
                queue: false,
            }),
            Some("VoiceMove")
        );
        assert_eq!(
            order_voice_key(&Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }),
            Some("VoiceAttack")
        );
        assert_eq!(
            order_voice_key(&Command::HarvestCell {
                entity_id: 1,
                target_rx: 0,
                target_ry: 0,
            }),
            Some("VoiceHarvest")
        );
        assert_eq!(
            order_voice_key(&Command::EnterTransport {
                passenger_id: 1,
                transport_id: 2,
            }),
            Some("VoiceEnter")
        );
        // Area guard and sabotage have no slot of their own.
        assert_eq!(
            order_voice_key(&Command::Guard {
                entity_id: 1,
                target: None,
            }),
            Some("VoiceSpecialAttack")
        );
        assert_eq!(
            order_voice_key(&Command::PlantC4 {
                attacker_id: 1,
                target_building_id: 2,
            }),
            Some("VoiceSpecialAttack")
        );
    }

    /// A capture order takes the type's own `VoiceCapture=` slot
    /// (`0x00708DC0` reads `TechnoTypeClass+0x55C` and queues it through
    /// `TechnoClass::Queue_Voice @ 0x00708D90`) and only falls through to
    /// `VoiceEnter=` when the key is the `-1` sentinel. Before this, every
    /// stock engineer capture spoke the Enter line.
    ///
    /// The chain does not end at `VoiceEnter=`: an absent one sends
    /// `0x00709020` to `[vtable+0x368]` = `0x00708FC0`, which draws from the
    /// `VoiceMove=` list at `TechnoTypeClass+0x468` and is silent only when
    /// that list is empty.
    #[test]
    fn capture_speaks_the_capture_slot_and_falls_back_to_enter() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::object_type::{ObjectCategory, ObjectType};

        assert_eq!(
            order_voice_key(&Command::CaptureBuilding {
                engineer_id: 1,
                target_building_id: 2,
            }),
            Some("VoiceCapture"),
            "capture has its own slot, it does not share the Enter slot"
        );

        let object = |body: &str| {
            let ini = IniFile::from_str(body);
            ObjectType::from_ini_section(
                "ENGINEER",
                ini.section("ENGINEER").unwrap(),
                ObjectCategory::Infantry,
            )
        };

        // Stock Allied engineer: both keys present, capture wins.
        let stock = object(
            "[ENGINEER]\nVoiceMove=EngAllMove\nVoiceEnter=EngAllMove\n\
             VoiceCapture=EngAllAttackCommand\n",
        );
        assert_eq!(
            voice_id_for_key(&stock, "VoiceCapture").map(String::as_str),
            Some("EngAllAttackCommand")
        );
        assert_eq!(
            voice_id_for_key(&stock, "VoiceEnter").map(String::as_str),
            Some("EngAllMove"),
            "an ordinary Enter order is unaffected"
        );

        // `VoiceCapture=` absent: `0x00708DCB` falls through to the Enter slot.
        let no_capture = object("[ENGINEER]\nVoiceMove=EngAllMove\nVoiceEnter=EngAllMove\n");
        assert_eq!(
            voice_id_for_key(&no_capture, "VoiceCapture").map(String::as_str),
            Some("EngAllMove")
        );

        // Neither key: `0x0070902B` sends the Enter slot to `[vtable+0x368]`
        // = `0x00708FC0`, which draws from the `VoiceMove=` list at
        // `TechnoTypeClass+0x468`. Native does NOT go silent here.
        let neither = object("[ENGINEER]\nVoiceMove=EngAllMove\n");
        assert_eq!(
            voice_id_for_key(&neither, "VoiceCapture").map(String::as_str),
            Some("EngAllMove")
        );
        assert_eq!(
            voice_id_for_key(&neither, "VoiceEnter").map(String::as_str),
            Some("EngAllMove"),
            "the Enter slot itself falls through to the move list"
        );

        // Silence needs the move list empty too — `0x00708FD6 TEST ECX,ECX ;
        // JZ 0x00709014` is the only exit that plays nothing.
        let nothing = object("[ENGINEER]\nStrength=100\n");
        assert_eq!(voice_id_for_key(&nothing, "VoiceCapture"), None);
        assert_eq!(voice_id_for_key(&nothing, "VoiceEnter"), None);
    }

    fn chord_rules() -> crate::rules::ruleset::RuleSet {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             0=MTNK\n\
             1=HARV\n\
             2=SECONLY\n\
             3=CMIN\n\
             4=SHAD\n\
             5=SREF\n\
             [AircraftTypes]\n\
             0=ORCA\n\
             [BuildingTypes]\n\
             0=GAWEAP\n\
             [MTNK]\n\
             Strength=300\n\
             Primary=105mm\n\
             [HARV]\n\
             Strength=1000\n\
             Harvester=yes\n\
             Primary=105mm\n\
             [CMIN]\n\
             Strength=1000\n\
             Harvester=yes\n\
             Primary=none\n\
             [SHAD]\n\
             Strength=200\n\
             Primary=105mm\n\
             PreventAttackMove=yes\n\
             [SECONLY]\n\
             Strength=200\n\
             Secondary=105mm\n\
             [SREF]\n\
             Strength=200\n\
             TurretCount=4\n\
             WeaponCount=1\n\
             Weapon1=105mm\n\
             [ORCA]\n\
             Strength=200\n\
             Primary=105mm\n\
             [GAWEAP]\n\
             Strength=1000\n\
             Primary=105mm\n\
             [WeaponTypes]\n\
             0=105mm\n\
             [105mm]\n\
             Damage=60\n\
             Range=5\n",
        );
        crate::rules::ruleset::RuleSet::from_ini(&ini).expect("chord rules")
    }

    /// Insert an entity of an explicit category, so the building and aircraft
    /// halves of the type rule can be exercised directly.
    fn insert_typed(
        sim: &mut Simulation,
        stable_id: u64,
        type_name: &str,
        category: EntityCategory,
    ) -> u64 {
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_name);
        sim.entities_mut()
            .insert(GameEntity::new_at_frame_zero_for_test(
                stable_id,
                5,
                5,
                0,
                0,
                owner,
                Health { current: 300 },
                type_ref,
                category,
                0,
                5,
                true,
            ));
        stable_id
    }

    /// The retail per-type rule: a real `Primary=` and nothing else.
    ///
    /// `Secondary=` is not consulted, `Primary=none` counts as unarmed, and
    /// being a harvester is irrelevant — the armed War Miner accepts the order
    /// while the Chrono Miner is refused for having no primary weapon.
    #[test]
    fn attack_move_eligibility_follows_the_primary_weapon() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
            .expect("unarmed miner");
        let arty = sim
            .spawn_object("SECONLY", "Americans", 7, 5, 0, &rules)
            .expect("secondary-only unit");

        assert!(entity_can_attack_move(&sim, Some(&rules), tank));
        // An armed harvester is an ordinary member of a defended-expansion group.
        assert!(entity_can_attack_move(&sim, Some(&rules), war_miner));
        // `Primary=none` resolves to no weapon at all.
        assert!(!entity_can_attack_move(&sim, Some(&rules), chrono_miner));
        // A secondary-only type is refused: the rule reads Primary only.
        assert!(!entity_can_attack_move(&sim, Some(&rules), arty));
    }

    /// GSI-08.02 regression: a `TurretCount>0` type carries its weapon in the
    /// `Primary` field even though it authors no `Primary=` key, because
    /// `Weapon1=` and `Primary=` are the same storage
    /// (`TechnoTypeClass::ReadINI`: cursor `0x007128D6 LEA EDI,[EBP+0xA94]`,
    /// base store `0x0071294A MOV [EDI-0x1FC],EAX`, and `0xA94-0x1FC = 0x898`
    /// which is the `Primary` field `Can_Attack_Move @ 0x00711E90` reads).
    ///
    /// The group case is the one a player feels: `selection_can_attack_move` is
    /// an `.all(..)`, so a single Prism Tank answering "no" used to silently
    /// disable the attack-move chord for every other unit selected with it.
    #[test]
    fn gsi_08_02_turret_count_type_attack_moves_through_weapon_one() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let prism = sim
            .spawn_object("SREF", "Americans", 7, 6, 0, &rules)
            .expect("TurretCount type");
        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");

        assert!(entity_can_attack_move(&sim, Some(&rules), prism));
        assert!(selection_can_attack_move(
            &sim,
            Some(&rules),
            &[prism, tank]
        ));
    }

    /// `TechnoTypeClass::Can_Attack_Move @ 0x00711E90` reads BOTH halves: a real
    /// `Primary=` and `PreventAttackMove=` off. The nine stock types that carry
    /// the key and are not aircraft — the three Engineers, the Spy, Boris and
    /// the four helicopters — all have a real primary, so the weapon half alone
    /// let every one of them through.
    #[test]
    fn gsi_07_34_prevent_attack_move_refuses_an_armed_type() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let nighthawk = sim
            .spawn_object("SHAD", "Americans", 5, 6, 0, &rules)
            .expect("helicopter carrying PreventAttackMove=yes");
        assert!(!entity_can_attack_move(&sim, Some(&rules), nighthawk));
    }

    /// Buildings and aircraft answer no unconditionally, whatever they are armed
    /// with.
    #[test]
    fn buildings_and_aircraft_never_attack_move() {
        let rules = chord_rules();
        let mut sim = Simulation::new();

        let factory = insert_typed(&mut sim, 1, "GAWEAP", EntityCategory::Structure);
        let orca = insert_typed(&mut sim, 2, "ORCA", EntityCategory::Aircraft);
        let tank = insert_typed(&mut sim, 3, "MTNK", EntityCategory::Unit);

        assert!(!entity_can_attack_move(&sim, Some(&rules), factory));
        assert!(!entity_can_attack_move(&sim, Some(&rules), orca));
        assert!(entity_can_attack_move(&sim, Some(&rules), tank));
    }

    /// The chord walks the whole selection — buildings included — and dies on
    /// the first member that refuses.
    #[test]
    fn attack_move_chord_requires_every_selected_object() {
        let rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules)
            .expect("unarmed miner");
        let factory = insert_typed(&mut sim, 900, "GAWEAP", EntityCategory::Structure);

        assert!(selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, war_miner]
        ));
        assert!(!selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, chrono_miner]
        ));
        // A selected structure kills the chord for the whole group.
        assert!(!selection_can_attack_move(
            &sim,
            Some(&rules),
            &[tank, factory]
        ));
        // An empty selection cannot attack-move either.
        assert!(!selection_can_attack_move(&sim, Some(&rules), &[]));
    }

    /// A chorded click on an enemy *object* attack-moves, because retail
    /// promotes a committed Attack mission just as it promotes a committed Move.
    /// The promotion is per object: a member whose type refuses attack-move
    /// still commits the plain attack.
    #[test]
    fn chorded_click_on_an_enemy_object_attack_moves() {
        assert_eq!(
            object_click_payload(
                OrderMode::AttackMove,
                OrderModifier::Normal,
                true,
                1,
                2,
                9,
                11,
                false
            ),
            Command::AttackMove {
                entity_id: 1,
                target_rx: 9,
                target_ry: 11,
                queue: false,
            }
        );
        assert_eq!(
            object_click_payload(
                OrderMode::AttackMove,
                OrderModifier::Normal,
                false,
                1,
                2,
                9,
                11,
                false
            ),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        // Plain click, force fire and guard area are untouched by the promotion.
        assert_eq!(
            object_click_payload(
                OrderMode::Move,
                OrderModifier::Normal,
                true,
                1,
                2,
                9,
                11,
                false
            ),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(
                OrderMode::AttackMove,
                OrderModifier::ForceFire,
                true,
                1,
                2,
                9,
                11,
                false
            ),
            Command::ForceAttack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(
                OrderMode::Move,
                OrderModifier::GuardArea,
                true,
                1,
                2,
                9,
                11,
                false
            ),
            Command::Guard {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
            }
        );
    }
    /// A click on the selected object itself is its self action only while it
    /// is the one selected object and the local player's
    /// (`TechnoClass::What_Action_OnObject`, `0x007000B9..0x007000DF`); a click
    /// on one of several selected objects is a plain selection click.
    #[test]
    fn self_click_needs_the_one_selected_own_object() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
                 [BuildingTypes]\n0=GACNST\n\
                 [E1]\nStrength=125\nDeployFire=yes\nDeployer=yes\n\
                 [AMCV]\nStrength=1000\nDeploysInto=GACNST\n\
                 [GACNST]\nStrength=1000\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.interner.intern(house);
            sim.session.house_order.push(id);
        }
        let americans = sim.interner.get("Americans").unwrap();
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let gi = spawn("E1", "Americans", 5);
        let other_gi = spawn("E1", "Americans", 7);
        let mcv = spawn("AMCV", "Americans", 9);
        let enemy_mcv = spawn("AMCV", "Soviets", 20);
        let hover = |stable_id| HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id,
        };
        let click = |selected: &[u64], target: u64| {
            self_click_command(&sim, &rules, americans, selected, Some(&hover(target)))
        };

        assert_eq!(
            click(&[gi], gi),
            Some(Command::ToggleInfantryDeploy { entity_id: gi })
        );
        assert_eq!(
            click(&[mcv], mcv),
            Some(Command::DeployMcv { entity_id: mcv })
        );
        assert_eq!(click(&[gi, other_gi], gi), None, "a group click selects");
        assert_eq!(click(&[mcv, gi], mcv), None, "a group click selects");
        assert_eq!(click(&[gi], other_gi), None, "not the selected object");
        assert_eq!(click(&[enemy_mcv], enemy_mcv), None, "not the player's");
    }

    /// Retail SCHP has IsSimpleDeployer without DeploysInto/Deployer. Native
    /// Unit WhatAction74000B and CanDeploy700E81 still offer its deploy action,
    /// including while the deploy animation owns the visible body.
    #[test]
    fn retail_siege_chopper_self_click_and_cursor_offer_the_same_deploy_action() {
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &battle.rules;
        let object = rules.object("SCHP").expect("retail Siege Chopper");
        assert!(object.is_simple_deployer);
        assert!(object.deploys_into.is_none());
        assert!(!object.deployer);
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object_limbo_at_height("SCHP", "Americans", 10, 10, 0, 0, rules)
            .expect("retail Siege Chopper builds");
        let owner = sim.interner.get("Americans").unwrap();
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: id,
        };
        for (deployed, begin, reverse) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, false, true),
        ] {
            sim.entities_mut()
                .get_mut(id)
                .unwrap()
                .set_unit_simple_deploy_for_test(deployed, begin, reverse);
            assert_eq!(
                self_click_command(&sim, rules, owner, &[id], Some(&hover)),
                Some(Command::DeployMcv { entity_id: id }),
            );
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[id],
                    Some(id),
                    &hover,
                    Some(rules),
                    None,
                ),
                crate::app::types::CursorFeedbackKind::Deploy,
            );
        }
        assert_eq!(
            self_click_command(&sim, rules, owner, &[id, id + 1], Some(&hover)),
            None,
            "a group self-click remains selection"
        );
        sim.entities_mut()
            .get_mut(id)
            .unwrap()
            .low_bridge_tube_state = Some(
            crate::sim::movement::tube_movement::LowBridgeTubeMovementState {
                tube_id: crate::map::tube_facts::TubeId(0),
                cursor: 0,
                target: crate::sim::components::DriveCoord::cell(10, 10, 0),
            },
        );
        assert_eq!(
            self_click_command(&sim, rules, owner, &[id], Some(&hover)),
            None
        );
        let feedback = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[id],
            Some(id),
            &hover,
            Some(rules),
            None,
        );
        assert_eq!(feedback, crate::app::types::CursorFeedbackKind::NoDeploy);
        assert_eq!(
            crate::app::input::cursor::cursor_id_for_feedback(feedback),
            Some(crate::app::types::CursorId::NoDeploy),
            "native action30 must display the blocked deploy cursor"
        );
    }

    /// The click side of the bomb actions: an Engineer's DisarmBomb and a
    /// Crazy Ivan's IvanBomb on a friend are Attack orders (ForceAttack past
    /// VERA's alliance gate), and an Ivan never attacks a bombed target.
    #[test]
    fn bomb_click_orders() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n1=ENGINEER\n[VehicleTypes]\n0=HTNK\n\
                 [CombatDamage]\nIvanTimedDelay=450\n\
                 [IVAN]\nStrength=125\nIvan=yes\nAttackCursorOnFriendlies=yes\n\
                 [ENGINEER]\nStrength=75\nEngineer=yes\nBombSight=4\n\
                 [HTNK]\nStrength=900\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.interner.intern(house);
            sim.session.house_order.push(id);
        }
        let americans = sim.interner.get("Americans").unwrap();
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let ivan = spawn("IVAN", "Americans", 5);
        let engineer = spawn("ENGINEER", "Americans", 7);
        let own_tank = spawn("HTNK", "Americans", 9);
        let spare_tank = spawn("HTNK", "Americans", 11);
        let enemy_ivan = spawn("IVAN", "Soviets", 20);
        let far_tank = spawn("HTNK", "Soviets", 22);
        let selected = [ivan, engineer];

        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, own_tank),
            vec![ivan]
        );
        assert!(disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans).is_empty());
        assert!(!ivan_cannot_bomb(&sim, &rules, ivan, own_tank));

        sim.bomb_attach(ivan, Some(own_tank), &rules);
        sim.bomb_attach(enemy_ivan, Some(far_tank), &rules);
        sim.bomb_list_update(&rules);
        sim.bomb_list_update(&rules);

        assert_eq!(
            disarm_bomb_engineers(&sim, &rules, &selected, own_tank, americans),
            vec![engineer]
        );
        assert!(
            disarm_bomb_engineers(&sim, &rules, &selected, far_tank, americans).is_empty(),
            "an unseen bomb"
        );
        assert!(
            friendly_bomb_ivans(&sim, &rules, &selected, own_tank).is_empty(),
            "a bombed friend stays a selection click"
        );
        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, spare_tank),
            vec![ivan]
        );
        assert!(ivan_cannot_bomb(&sim, &rules, ivan, own_tank));
        assert!(!ivan_cannot_bomb(&sim, &rules, engineer, own_tank));

        // The Engineer carrying a bomb itself is no target of its own click.
        sim.bomb_attach(ivan, Some(engineer), &rules);
        sim.bomb_list_update(&rules);
        sim.bomb_list_update(&rules);
        assert!(disarm_bomb_engineers(&sim, &rules, &[engineer], engineer, americans).is_empty());

        assert!(matches!(
            bomb_order(engineer, own_tank, true),
            Command::ForceAttack { .. }
        ));
        assert!(matches!(
            bomb_order(engineer, far_tank, false),
            Command::Attack { .. }
        ));
    }
    /// Supplies the ordinary object-route prerequisites, through production
    /// object constructors and the existing terrain/type authorities.
    fn engineer_hut_click_fixture(
        relation: &str,
        collapsed: bool,
        repairable: bool,
        capturable: bool,
        extra_hut_flags: &str,
        extra_actor_flags: &str,
    ) -> (
        Simulation,
        crate::rules::ruleset::RuleSet,
        u64,
        HoverTargetKindWithId,
    ) {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[InfantryTypes]\n0=ENGINEER\n[VehicleTypes]\n0=TRUCK\n[BuildingTypes]\n0=CABHUT\n\
                 [TRUCK]\nStrength=100\nSpeed=4\n\
                 [ENGINEER]\nStrength=75\nEngineer=yes\n{}\n\
                 [CABHUT]\nStrength=200\nFoundation=1x1\nBridgeRepairHut=yes\nRepairable={}\nCapturable={}\n{}\n",
                extra_actor_flags,
                if repairable { "yes" } else { "no" },
                if capturable { "yes" } else { "no" },
                extra_hut_flags,
            )),
        ).unwrap();
        let mut sim = Simulation::with_seed(0x587410);
        sim.resolve_type_handles(&rules);
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(sim.interner.intern("Americans"));
        let mut terrain = crate::map::resolved_terrain::test_grid(20, 20, |rx, ry| {
            crate::map::resolved_terrain::ResolvedTerrainCell {
                speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
                base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
                ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
            }
        });
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for y in 9..=11 {
            terrain.cell_mut(12, y).unwrap().bridge_facts.overlay_id =
                Some(if collapsed { 0xE7 } else { 0xD4 });
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.bridge_state = Some(crate::sim::bridge_state::BridgeRuntimeState::default());
        let owner = if relation == "self" {
            "Americans"
        } else {
            "Soviets"
        };
        let engineer = sim
            .spawn_object("ENGINEER", "Americans", 8, 10, 0, &rules)
            .unwrap();
        let hut = sim
            .spawn_object("CABHUT", owner, 10, 10, 0, &rules)
            .unwrap();
        sim.entities_mut().get_mut(hut).unwrap().health.current = 150;
        if relation == "allied" {
            sim.house_alliances
                .entry("AMERICANS".into())
                .or_default()
                .insert("SOVIETS".into());
        }
        let kind = if relation == "hostile" {
            HoverTargetKind::EnemyStructure
        } else {
            HoverTargetKind::FriendlyStructure
        };
        (
            sim,
            rules,
            engineer,
            HoverTargetKindWithId {
                kind,
                stable_id: hut,
            },
        )
    }

    /// Original object-click51F190 maps action29 to Capture with Destination
    /// hut; action32 consumes the click without queueing. Owner friendship is
    /// absent from the admitted hut arm, including damaged/capturable traps.
    #[test]
    fn engineer_hut_context_orders_match_native_terminal_action_decisions() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        for relation in ["hostile", "allied", "self"] {
            for collapsed in [false, true] {
                let (sim, rules, engineer, hover) =
                    engineer_hut_click_fixture(relation, collapsed, true, true, "", "");
                let before = sim.state_hash();
                let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
                let rng = (
                    sim.scenario_rng.logical_state(),
                    sim.main_rng.logical_state(),
                    sim.mapgen_rng.logical_state(),
                );
                let commands = engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover)
                    .expect("an admitted hut arm always consumes the click, including false query");
                let row = native["object_clicks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["name"]
                            == format!("{relation}_{}", if collapsed { "True" } else { "False" })
                            && row["input"]["emp_blocked"] == false
                    })
                    .unwrap();
                assert_eq!(
                    commands.len() as u64,
                    row["output"]["queue_calls"].as_u64().unwrap()
                );
                let expected = if collapsed {
                    vec![Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id,
                    }]
                } else {
                    vec![]
                };
                assert_eq!(
                    commands, expected,
                    "relation={relation} collapsed={collapsed}"
                );
                assert_eq!(
                    sim.state_hash(),
                    before,
                    "input producer must not mutate gameplay"
                );
                assert_eq!(
                    serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                    actor
                );
                assert_eq!(
                    (
                        sim.scenario_rng.logical_state(),
                        sim.main_rng.logical_state(),
                        sim.mapgen_rng.logical_state()
                    ),
                    rng
                );
            }
        }
    }

    /// The native object route checks Repairable before entering the hut
    /// branch. Native repairable_false establishes exclusion/query_calls=0;
    /// its later ordinary WhatAction continuation is outside those goldens.
    #[test]
    fn nonrepairable_noncapturable_hut_does_not_produce_hut_capture_order() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        let row = native["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["input"]["route"] == "object" && row["input"]["name"] == "repairable_false"
            })
            .unwrap();
        assert_eq!(row["output"]["query_calls"], 0);
        for relation in ["hostile", "allied", "self"] {
            let (sim, rules, engineer, hover) =
                engineer_hut_click_fixture(relation, true, false, false, "", "");
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                None
            );
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
                None
            );
            assert_eq!(sim.state_hash(), before);
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor
            );
            assert_eq!(
                (
                    sim.scenario_rng.logical_state(),
                    sim.main_rng.logical_state(),
                    sim.mapgen_rng.logical_state()
                ),
                rng
            );
        }
    }
    /// Executed local_false and undeploy_True_foundation_0 object rows stop
    /// before the hut query. Both assertions are hut-arm exclusion, without
    /// claiming the later ordinary WhatAction continuation's final action.
    #[test]
    fn engineer_hut_object_prerequisites_exclude_nonlocal_and_undeploy_targets() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        for gate in ["nonlocal", "undeploy"] {
            let flags = if gate == "undeploy" {
                "UndeploysInto=TRUCK"
            } else {
                ""
            };
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", true, true, false, flags, "");
            if gate == "nonlocal" {
                sim.session.current_house = Some(sim.interner.intern("Soviets"));
            } else {
                assert!(rules.object("CABHUT").unwrap().is_1x1_with_undeploy());
            }
            if gate == "nonlocal" {
                let row = native["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["route"] == "object" && row["input"]["name"] == "local_false"
                    })
                    .unwrap();
                assert_eq!(row["output"]["query_calls"], 0);
            }
            if gate == "undeploy" {
                let row = native["undeploy_controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["input"]["route"] == "object"
                            && row["input"]["name"] == "undeploy_True_foundation_0"
                    })
                    .unwrap();
                assert_eq!(row["output"]["query_calls"], 0);
            }
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                None,
                "gate={gate}"
            );
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut vec![engineer], &hover),
                None,
                "gate={gate}"
            );
            assert_eq!(sim.state_hash(), before);
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor
            );
            assert_eq!(
                (
                    sim.scenario_rng.logical_state(),
                    sim.main_rng.logical_state(),
                    sim.mapgen_rng.logical_state()
                ),
                rng
            );
        }
    }
    /// Original700C40 and its object-click caller are independently executed
    /// in each paired golden group. These adapters supply represented owner
    /// state; they do not assert that retail constructs missile-spawn Engineers
    /// or imports arbitrary native EMP/Robot latch bytes.
    #[test]
    fn represented_infantry_controllability_gates_match_native_hut_clicks() {
        use crate::sim::spawn_manager::{
            SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState,
        };
        use crate::sim::timer::CdTimer;
        let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
        ))
        .unwrap();
        let mut compared = 0;
        let mut unrepresented = Vec::new();
        for row in packet["controllability_controls"].as_array().unwrap() {
            let input = &row["native"]["input"];
            let name = input["name"].as_str().unwrap();
            if matches!(
                name,
                "robot_native_save_control" | "emp_native_save_positive" | "emp_negative"
            ) {
                unrepresented.push(name);
                continue;
            }
            let gates = &input["gates"];
            let spawned = gates["spawned"].as_bool().unwrap_or(false);
            let capacity = gates["spawns_number"].as_i64().unwrap_or(1);
            let missile_child = gates["spawn_slots"]
                .as_array()
                .is_some_and(|slots| slots.iter().any(|slot| slot["missile_spawn"] == true));
            let actor_flags = format!(
                "Spawned={}\nSpawnsNumber={capacity}\nMissileSpawn={}\n",
                if spawned { "yes" } else { "no" },
                if missile_child { "yes" } else { "no" }
            );
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", true, true, false, "", &actor_flags);
            sim.session.binary_frame = gates["frame"].as_u64().unwrap_or(0) as u32;
            if gates["bunker"] == true {
                // Typed existing owner state: positive native pointer control,
                // not an assertion of ordinary Infantry bunker installation.
                sim.entities_mut().get_mut(engineer).unwrap().bunker_link =
                    crate::sim::game_entity::BunkerLink::Installed(hover.stable_id);
            }
            if gates["paralysis_start"].is_number() {
                sim.entities_mut()
                    .get_mut(engineer)
                    .unwrap()
                    .paralysis_timer = CdTimer::from_raw(
                    gates["paralysis_start"].as_i64().unwrap() as i32,
                    gates["paralysis_duration"].as_i64().unwrap() as i32,
                );
            }
            if gates["warp_out"] == true {
                sim.entities_mut().get_mut(engineer).unwrap().temporal =
                    crate::sim::temporal::TemporalState::warped_by_for_test(hover.stable_id);
            }
            if gates["warp_in"] == true {
                sim.entities_mut()
                    .get_mut(engineer)
                    .unwrap()
                    .install_teleport_state_for_test(Some(
                        crate::sim::movement::teleport_movement::TeleportState::for_test(
                            crate::sim::movement::teleport_movement::TeleportPhase::ChronoDelay,
                            8,
                            10,
                            1,
                        ),
                    ));
            }
            if gates["slave_owner"] == true {
                sim.entities_mut().get_mut(engineer).unwrap().slave =
                    crate::sim::slave_manager::SlaveLink::for_test(Some(hover.stable_id), vec![]);
            }
            if let Some(native_slots) = gates["spawn_slots"].as_array() {
                let mut slots = Vec::new();
                for (index, native_slot) in native_slots.iter().enumerate() {
                    let child = if native_slot["state"] == 2 && native_slot["child"] != false {
                        let child = sim
                            .spawn_object("ENGINEER", "Americans", 5 + index as u16, 12, 0, &rules)
                            .unwrap();
                        sim.entities_mut()
                            .get_mut(child)
                            .unwrap()
                            .lifecycle
                            .in_limbo = native_slot["limbo"] == true;
                        Some(child)
                    } else {
                        None
                    };
                    slots.push(SpawnSlot {
                        spawn: child,
                        state: match native_slot["state"].as_i64().unwrap() {
                            0 => SpawnSlotState::ReadyDocked,
                            1 => SpawnSlotState::KamikazeWait,
                            2 => SpawnSlotState::InFlight,
                            other => panic!("unrepresented native slot state {other}"),
                        },
                        timer: CdTimer::default(),
                        is_missile_spawn: false,
                    });
                }
                let spawn_type = sim.interner.intern("ENGINEER");
                sim.entities_mut().get_mut(engineer).unwrap().spawn_manager =
                    Some(SpawnManagerState {
                        spawn_type,
                        missile_family: None,
                        regen_rate: 0,
                        reload_rate: 0,
                        kamikaze_wait_frames: 0,
                        slots,
                        update_timer: CdTimer::default(),
                        spawn_timer: CdTimer::default(),
                        current_target: None,
                        queued_target: None,
                        mode: SpawnManagerMode::Idle,
                    });
            }
            let before = sim.state_hash();
            let actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
            let rng = (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            );
            assert_eq!(
                sim.techno_player_controllable(engineer, &rules),
                row["native"]["output"]["returned_al"] == 1,
                "{name}"
            );
            assert_eq!(
                sim.engineer_building_action(engineer, hover.stable_id, &rules),
                Some(crate::sim::world::EngineerBuildingAction::Repair(true)),
                "control admission must not change WhatAction: {name}"
            );
            let mut selected = vec![engineer];
            let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover)
                .expect("hut arm consumes blocked and allowed clicks");
            assert_eq!(
                commands.len() as u64,
                row["object_click"]["output"]["queue_calls"]
                    .as_u64()
                    .unwrap(),
                "{name}"
            );
            assert!(
                selected.is_empty(),
                "handled Engineer must not reach a later input branch: {name}"
            );
            if !commands.is_empty() {
                assert_eq!(
                    commands,
                    [Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id
                    }],
                    "{name}"
                );
            }
            assert_eq!(sim.state_hash(), before, "{name}");
            assert_eq!(
                serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
                actor,
                "{name}"
            );
            assert_eq!(
                (
                    sim.scenario_rng.logical_state(),
                    sim.main_rng.logical_state(),
                    sim.mapgen_rng.logical_state()
                ),
                rng,
                "{name}"
            );
            compared += 1;
        }
        assert_eq!(compared, 21);
        assert_eq!(
            unrepresented,
            [
                "robot_native_save_control",
                "emp_native_save_positive",
                "emp_negative"
            ]
        );
    }

    /// A hut's terminal action consumes each handled Engineer, while other
    /// selected actors continue through their own original action routes.
    #[test]
    fn engineer_hut_context_keeps_unhandled_actors_in_selection_order() {
        for collapsed in [false, true] {
            let (mut sim, rules, engineer, hover) =
                engineer_hut_click_fixture("hostile", collapsed, true, true, "", "C4=yes");
            // Unit Unlimbo requires configured playfield bounds; the hut
            // action fixture otherwise only constructs Infantry/Buildings.
            sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
                base: 0,
                off_fc: -100,
                off_100: -100,
                off_104: 200,
                off_108: 200,
            });
            let first = sim
                .spawn_object("TRUCK", "Americans", 4, 10, 0, &rules)
                .unwrap();
            let last = sim
                .spawn_object("TRUCK", "Americans", 6, 10, 0, &rules)
                .unwrap();
            let second_engineer = sim
                .spawn_object("ENGINEER", "Americans", 7, 10, 0, &rules)
                .unwrap();
            let mut selected = vec![first, engineer, last, second_engineer];
            let before = sim.state_hash();
            let commands = engineer_capture_orders(&sim, &rules, &mut selected, &hover).unwrap();
            assert_eq!(selected, [first, last]);
            let expected = if collapsed {
                vec![
                    Command::CaptureBuilding {
                        engineer_id: engineer,
                        target_building_id: hover.stable_id,
                    },
                    Command::CaptureBuilding {
                        engineer_id: second_engineer,
                        target_building_id: hover.stable_id,
                    },
                ]
            } else {
                vec![]
            };
            assert_eq!(commands, expected);
            assert_eq!(sim.state_hash(), before);
            let before_selected = selected.clone();
            assert_eq!(
                engineer_capture_orders(&sim, &rules, &mut selected, &hover),
                None
            );
            assert_eq!(
                selected, before_selected,
                "non-Engineers retain their next action route"
            );
        }
    }
}

#[cfg(test)]
#[path = "context_order_selection_tests.rs"]
mod selection_dispatch_tests;

#[cfg(test)]
#[path = "context_order_retail_bridge_tests.rs"]
mod retail_bridge_tests;

#[cfg(test)]
#[path = "context_order_retail_engineer_tests.rs"]
mod retail_engineer_tests;
