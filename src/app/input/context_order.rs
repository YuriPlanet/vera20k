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
    hover_target_at_point, pick_any_target_stable_id, pick_enemy_target_stable_id,
};
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::map::entities::EntityCategory;
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
/// * **Shift alone → no order at all in retail.** On an object it returns the
///   add-to-selection action, which the object click handler has no case for and
///   therefore sends no mission; on a cell it returns the plain Move action, an
///   ordinary immediate move. Retail's *deferred*-order verb is Planning Mode —
///   a separate bindable command class with its own event opcodes — not a
///   modifier. VERA has no Planning Mode, so Shift keeps VERA's order-queue
///   verb: VERA-internal, gamemd equivalent (Planning Mode) UNIMPLEMENTED.
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
    /// Shift — VERA's order queue. No retail equivalent on this modifier.
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
        _ => None,
    }
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
        "VoiceSpecialAttack" => obj.voice_special_attack.as_ref(),
        _ => None,
    }
}

/// Play one entity's voice line for the given `Voice*` INI key.
///
/// The superseded app-layer order-voice helper always spoke the lowest-`stable_id` selected
/// entity; retail speaks the object that resolved the order, so order resolution
/// needs to name the speaker explicitly.
fn emit_entity_order_voice(state: &mut AppState, speaker_id: u64, voice_field: &str) {
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

/// Commit a resolved order batch: one voice line, the action lines, the queue.
///
/// Every exit from order resolution goes through here so the single-speaker rule
/// holds for the capability branches (garrison, C4, capture, depot, bunker,
/// deploy) as well as for the move/attack tail.
fn finish_order(
    state: &mut AppState,
    queued: Vec<CommandEnvelope>,
    speaker_id: Option<u64>,
) -> bool {
    let queued = if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    {
        queued
            .into_iter()
            .filter_map(|envelope| {
                crate::app::input::commands::roundtrip_ordinary_local_move(sim, envelope)
            })
            .collect::<Vec<_>>()
    } else {
        queued
    };
    if queued.is_empty() {
        return false;
    }
    if let Some(speaker_id) = speaker_id {
        emit_resolved_order_voice(state, speaker_id, &queued);
    }
    let current_tick = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
        .map_or(0, |s| s.session.tick);
    crate::app::presentation::target_lines::record_command_lines(
        &mut state.match_state.match_presentation.target_lines,
        &queued,
        current_tick,
    );
    if let Some(sim) = state
        .match_state
        .sim_runtime
        .as_mut()
        .map(|rt| &mut rt.simulation)
    {
        sim.queue_commands(queued);
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
/// not apply here and `obj.primary` is the exact read: `+0x898` is the storage
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
    obj.primary.as_deref().is_some_and(|primary| {
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
    force_fire: bool,
    can_attack_move: bool,
    attacker_id: u64,
    target_id: u64,
    target_rx: u16,
    target_ry: u16,
    queue: bool,
) -> Command {
    if force_fire {
        return Command::ForceAttack {
            attacker_id,
            target_id,
        };
    }
    match order_mode {
        OrderMode::AttackMove if can_attack_move => Command::AttackMove {
            entity_id: attacker_id,
            target_rx,
            target_ry,
            queue,
        },
        OrderMode::Guard => Command::Guard {
            entity_id: attacker_id,
            target_id: Some(target_id),
        },
        _ => Command::Attack {
            attacker_id,
            target_id,
        },
    }
}

/// Attempt to issue a context-sensitive order at the given screen point.
///
/// Returns `true` if a command was queued (consuming the click), `false` if the
/// click should fall through to selection handling.
///
/// When `select_friendly_clicks` is true, clicks on friendly units/structures
/// return `false` so the caller can treat them as selection clicks instead.
pub(crate) fn try_queue_context_order_at_screen_point(
    state: &mut AppState,
    screen_x: f32,
    screen_y: f32,
    select_friendly_clicks: bool,
) -> bool {
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
    let selected_ids = selected_stable_ids_in_order(state);
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
        let mut structure_owner: Option<String> = None;
        let mut mobile_count: usize = 0;
        let mut _structure_count: usize = 0;

        for &sid in &selected_ids {
            let Some(entity) = sim.entities().get(sid) else {
                continue;
            };
            if entity.category == EntityCategory::Structure {
                _structure_count += 1;
                if structure_owner.is_none() {
                    structure_owner = Some(sim.interner.resolve(entity.owner()).to_string());
                }
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
        // A held chord overrides the sticky sidebar order mode for this click.
        let order_mode = match modifier {
            OrderModifier::AttackMove => OrderMode::AttackMove,
            OrderModifier::GuardArea => OrderMode::Guard,
            _ => order_mode,
        };

        let hover = hover_target_at_point(
            sim,
            world_x,
            world_y,
            &owner,
            state.match_state.sandbox_full_visibility,
            Some(&resources.rules),
            &resources.height_map,
            Some(
                &state
                    .match_state
                    .match_presentation
                    .tactical_bridge_inverse_map,
            ),
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
            // Non-miner units in selection just move to that cell.
            for &stable_id in &selected_units {
                if !selected_miner_ids.contains(&stable_id) {
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::Move {
                            entity_id: stable_id,
                            target_rx,
                            target_ry,
                            queue: queue_mode,
                            group_id: None,
                        },
                    ));
                }
            }
        } else if let Some(struct_own) = structure_owner {
            let clicked_friendly = hover.as_ref().is_some_and(|target| {
                matches!(
                    target.kind,
                    HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
                )
            });
            // Self-click on a deployable structure (garrisoned building → unload,
            // ConYard → undeploy). Must run before the friendly-click fallthrough
            // below — otherwise the click is treated as plain re-selection and the
            // deploy cursor's action is lost.
            if context_actions_enabled && clicked_friendly {
                if let Some(target) = hover.as_ref() {
                    if selected_ids.contains(&target.stable_id) {
                        if let Some(entity) = sim.entities().get(target.stable_id) {
                            // Self-click on a loaded transport → unload.
                            if let Some(cmd) = super::transport_orders::transport_unload_command(
                                entity,
                                resources
                                    .rules
                                    .object(sim.interner.resolve(entity.type_ref())),
                            ) {
                                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                                return finish_order(state, queued, speaker_id);
                            }
                            if entity.category == EntityCategory::Structure {
                                let obj = Some(&resources.rules).and_then(|r| {
                                    r.object(sim.interner.resolve(entity.type_ref()))
                                });
                                let cmd = if obj.map_or(false, |o| o.can_be_occupied)
                                    && entity.passenger_role.cargo().is_some_and(|c| !c.is_empty())
                                {
                                    Some(Command::UnloadPassengers {
                                        transport_id: target.stable_id,
                                    })
                                } else if entity.bunker_occupant.is_some() {
                                    // Own occupied tank bunker → eject the installed unit.
                                    Some(Command::EjectBunker {
                                        bunker_id: target.stable_id,
                                    })
                                } else if Some(&resources.rules).is_some_and(|rules| {
                                    sim.should_show_undeploy_building_command(
                                        target.stable_id,
                                        rules,
                                    )
                                }) {
                                    Some(Command::UndeployBuilding {
                                        entity_id: target.stable_id,
                                    })
                                } else {
                                    None
                                };
                                if let Some(cmd) = cmd {
                                    queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                                    return finish_order(state, queued, speaker_id);
                                }
                            }
                        }
                    }
                }
            }
            if select_friendly_clicks && clicked_friendly && context_actions_enabled {
                return false;
            }
            {
                // Set rally point for the structures.
                {
                    let struct_owner_id = sim.interner.get(&struct_own).unwrap_or(owner_id);
                    let producer_ids =
                        selected_rally_producer_ids(sim, &selected_ids, struct_owner_id);
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
                    rally_announce = struct_owner_id == owner_id
                        && producer_ids.iter().any(|id| {
                            sim.entities().get(*id).is_some_and(|entity| {
                                Some(&resources.rules)
                                .and_then(|r| r.object(sim.interner.resolve(entity.type_ref())))
                                .is_some_and(
                                    crate::app::match_runtime::eva_producers::rally_point_announces,
                                )
                            })
                        });
                    queued.push(CommandEnvelope::new(
                        struct_owner_id,
                        execute_tick,
                        Command::SetRally {
                            owner: struct_owner_id,
                            rx: target_rx,
                            ry: target_ry,
                            producer_ids,
                        },
                    ));
                }
                // Also issue Move commands for any mobile units in the
                // selection — RA2 moves units AND sets rally when both
                // are selected.
                if mobile_count > 0 {
                    for &stable_id in &selected_units {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Move {
                                entity_id: stable_id,
                                target_rx,
                                target_ry,
                                queue: queue_mode,
                                group_id: None,
                            },
                        ));
                    }
                }
            }
        } else {
            // Garrison entry uses the shared CanDock-equivalent predicate before
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
                                    sim.path_grid(),
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
            // structure. Ordered before the engineer-capture branch so C4 takes
            // priority for any unit with both flags.
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

            // Engineer capture: engineer clicking a capturable enemy building.
            if context_actions_enabled {
                let capture_target = hover.as_ref().and_then(|target| {
                    if !matches!(target.kind, HoverTargetKind::EnemyStructure) {
                        return None;
                    }
                    let rules = Some(&resources.rules)?;
                    let building = sim.entities().get(target.stable_id)?;
                    let btype_str = sim.interner.resolve(building.type_ref());
                    let bowner_str = sim.interner.resolve(building.owner());
                    let obj = rules.object(btype_str)?;
                    if !obj.capturable && !obj.bridge_repair_hut {
                        return None;
                    }
                    // Don't capture neutral garrisonable buildings — those use garrison entry.
                    if obj.can_be_occupied
                        && (bowner_str.eq_ignore_ascii_case("neutral")
                            || bowner_str.eq_ignore_ascii_case("special"))
                    {
                        return None;
                    }
                    Some(target.stable_id)
                });
                if let Some(building_id) = capture_target {
                    let engineer_ids: Vec<u64> = selected_units
                        .iter()
                        .copied()
                        .filter(|&sid| {
                            sim.entities().get(sid).is_some_and(|e| {
                                e.category == EntityCategory::Infantry
                                    && Some(&resources.rules)
                                        .and_then(|r| r.object(sim.interner.resolve(e.type_ref())))
                                        .map_or(false, |o| o.engineer)
                            })
                        })
                        .collect();
                    if !engineer_ids.is_empty() {
                        for eng_id in engineer_ids {
                            queued.push(CommandEnvelope::new(
                                owner_id,
                                execute_tick,
                                Command::CaptureBuilding {
                                    engineer_id: eng_id,
                                    target_building_id: building_id,
                                },
                            ));
                        }
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
            // Deploy-on-self-click: clicking a selected deployable entity deploys/undeploys it.
            if clicked_friendly && context_actions_enabled {
                if let Some(target) = hover.as_ref() {
                    if selected_ids.contains(&target.stable_id) {
                        if let Some(entity) = sim.entities().get(target.stable_id) {
                            let obj = Some(&resources.rules)
                                .and_then(|r| r.object(sim.interner.resolve(entity.type_ref())));
                            let cmd = if entity.category == EntityCategory::Structure {
                                // Garrisoned building → unload occupants.
                                if obj.map_or(false, |o| o.can_be_occupied)
                                    && entity.passenger_role.cargo().is_some_and(|c| !c.is_empty())
                                {
                                    Some(Command::UnloadPassengers {
                                        transport_id: target.stable_id,
                                    })
                                // ConYard → MCV
                                } else if Some(&resources.rules).is_some_and(|rules| {
                                    sim.should_show_undeploy_building_command(
                                        target.stable_id,
                                        rules,
                                    )
                                }) {
                                    Some(Command::UndeployBuilding {
                                        entity_id: target.stable_id,
                                    })
                                } else {
                                    None
                                }
                            } else if entity.category == EntityCategory::Infantry
                                && obj.map_or(false, |o| o.deploy_fire)
                            {
                                // Deploy-fire infantry (GI, GGI, etc.) → toggle deploy.
                                Some(Command::ToggleInfantryDeploy {
                                    entity_id: target.stable_id,
                                })
                            } else if let Some(cmd) =
                                super::transport_orders::transport_unload_command(entity, obj)
                            {
                                // Loaded transport → unload passengers.
                                Some(cmd)
                            } else {
                                // MCV → ConYard
                                if obj.map_or(false, |o| o.deploys_into.is_some() || o.deployer) {
                                    Some(Command::DeployMcv {
                                        entity_id: target.stable_id,
                                    })
                                } else {
                                    None
                                }
                            };
                            if let Some(cmd) = cmd {
                                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                                return finish_order(state, queued, speaker_id);
                            }
                        }
                    }
                }
            }
            if select_friendly_clicks && clicked_friendly && context_actions_enabled {
                return false;
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
                    &resources.height_map,
                    Some(
                        &state
                            .match_state
                            .match_presentation
                            .tactical_bridge_inverse_map,
                    ),
                )
            } else {
                pick_enemy_target_stable_id(
                    sim,
                    world_x,
                    world_y,
                    &owner,
                    state.match_state.sandbox_full_visibility,
                    Some(&resources.rules),
                    &resources.height_map,
                    Some(
                        &state
                            .match_state
                            .match_presentation
                            .tactical_bridge_inverse_map,
                    ),
                )
            };
            // Assign a shared group_id when multiple units move together.
            // The movement system uses this to sync speed to the slowest member.
            let move_group_id: Option<u32> = if selected_units.len() > 1 && attack_target.is_none()
            {
                Some(execute_tick as u32)
            } else {
                None
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
                        force_fire,
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
                            !queue_mode && ordinary_cell_receiver,
                        ) else {
                            continue;
                        };
                        Command::Move {
                            entity_id: stable_id,
                            target_rx: goal.0,
                            target_ry: goal.1,
                            queue: queue_mode,
                            group_id: None,
                        }
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
                                order_mode == OrderMode::Move
                                    && !queue_mode
                                    && ordinary_cell_receiver,
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
                                    target_rx: goal.0,
                                    target_ry: goal.1,
                                    queue: queue_mode,
                                }
                            } else {
                                Command::Move {
                                    entity_id: stable_id,
                                    target_rx: goal.0,
                                    target_ry: goal.1,
                                    queue: queue_mode,
                                    group_id: move_group_id,
                                }
                            }
                        }
                        // DRIFT, recorded not fixed: gamemd's Ctrl+Alt area-guard
                        // carries the CLICKED cell (0x00700830 -> mission 0x1A /
                        // 0x33), while `Command::Guard` has no cell field, so the
                        // sim anchors the guard at the actor's own position.
                        // Trigger: Ctrl+Alt-clicking a cell away from the unit.
                        // Player effect: the unit guards where it stands instead
                        // of where the player pointed, so the order looks like it
                        // did nothing. Frequency: occasional — a real habit for
                        // holding ground, but not a reflex. Downstream risk:
                        // closing it widens a sim command's payload, so it lands
                        // with the Guard mission's own row (95), not here.
                        OrderMode::Guard => Command::Guard {
                            entity_id: stable_id,
                            target_id: None,
                        },
                    }
                };
                queued.push(CommandEnvelope::new(owner_id, execute_tick, payload));
            }
            if !queued.is_empty() {
                consumed_order_mode = true;
            }
        }
    }

    if queued.is_empty() {
        return false;
    }
    if consumed_order_mode && state.match_state.input.queued_order_mode != OrderMode::Move {
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

fn selected_rally_producer_ids(
    sim: &crate::sim::world::Simulation,
    selected_ids: &[u64],
    owner: InternedId,
) -> Vec<u64> {
    let mut producer_ids: Vec<u64> = selected_ids
        .iter()
        .copied()
        .filter(|stable_id| {
            sim.entities().get(*stable_id).is_some_and(|entity| {
                entity.category == EntityCategory::Structure && entity.owner() == owner
            })
        })
        .collect();
    producer_ids.sort_unstable();
    producer_ids.dedup();
    producer_ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    #[test]
    fn right_click_structure_selection_sends_rally_producer_ids() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let factory_type = sim.interner.intern("GAWEAP");
        let tank_type = sim.interner.intern("MTNK");
        sim.entities_mut()
            .insert(GameEntity::new_at_frame_zero_for_test(
                1,
                10,
                10,
                0,
                0,
                owner,
                Health { current: 1000 },
                factory_type,
                EntityCategory::Structure,
                0,
                5,
                false,
            ));
        sim.entities_mut()
            .insert(GameEntity::new_at_frame_zero_for_test(
                2,
                11,
                10,
                0,
                0,
                owner,
                Health { current: 300 },
                tank_type,
                EntityCategory::Unit,
                0,
                5,
                true,
            ));

        let producer_ids = selected_rally_producer_ids(&sim, &[2, 1], owner);

        assert_eq!(producer_ids, vec![1]);
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
                group_id: None,
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
                target_id: None,
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
        let mut rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let height_map: std::collections::BTreeMap<(u16, u16), u8> =
            std::collections::BTreeMap::new();

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules, &height_map)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules, &height_map)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules, &height_map)
            .expect("unarmed miner");
        let arty = sim
            .spawn_object("SECONLY", "Americans", 7, 5, 0, &rules, &height_map)
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
        let height_map: std::collections::BTreeMap<(u16, u16), u8> =
            std::collections::BTreeMap::new();

        let prism = sim
            .spawn_object("SREF", "Americans", 7, 6, 0, &rules, &height_map)
            .expect("TurretCount type");
        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules, &height_map)
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
        let height_map: std::collections::BTreeMap<(u16, u16), u8> =
            std::collections::BTreeMap::new();

        let nighthawk = sim
            .spawn_object("SHAD", "Americans", 5, 6, 0, &rules, &height_map)
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
        let mut rules = chord_rules();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let height_map: std::collections::BTreeMap<(u16, u16), u8> =
            std::collections::BTreeMap::new();

        let tank = sim
            .spawn_object("MTNK", "Americans", 5, 5, 0, &rules, &height_map)
            .expect("tank");
        let war_miner = sim
            .spawn_object("HARV", "Americans", 6, 5, 0, &rules, &height_map)
            .expect("armed miner");
        let chrono_miner = sim
            .spawn_object("CMIN", "Americans", 8, 5, 0, &rules, &height_map)
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
            object_click_payload(OrderMode::AttackMove, false, true, 1, 2, 9, 11, false),
            Command::AttackMove {
                entity_id: 1,
                target_rx: 9,
                target_ry: 11,
                queue: false,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, false, false, 1, 2, 9, 11, false),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        // Plain click, force fire and guard area are untouched by the promotion.
        assert_eq!(
            object_click_payload(OrderMode::Move, false, true, 1, 2, 9, 11, false),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, true, true, 1, 2, 9, 11, false),
            Command::ForceAttack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::Guard, false, true, 1, 2, 9, 11, false),
            Command::Guard {
                entity_id: 1,
                target_id: Some(2),
            }
        );
    }
}
