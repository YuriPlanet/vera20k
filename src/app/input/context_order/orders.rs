//! The command lists for the attack and repair branches of a click, and finishing
//! a queued order: selection dispatch order, order voice and context-order filtering.

use super::{
    AttackAction, AttackDispatch, HealerAction, InfantryAttackAction, OrderModifier,
    armed_action_with, attack_action_with, attack_support_command, context_actor_viewer,
    context_order_receiver, cursor_owner, entity_can_attack_move, guard_area_admitted,
    healer_object_action_with, infantry_attack_action_with, ivan_cannot_bomb, move_click_command,
    object_click_payload, object_guard_command,
};
use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::types::{HoverTargetKind, OrderMode};
use crate::audio::sfx::SfxPlayer;
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::ControlProjection;

/// Unit73FD7C's post-processing precedes the generic friendly selection and
/// Ctrl attack paths. Selection/keyboard and mission promotion stay with their
/// existing owners. Shift and one selected object's self action precede
/// the native repair arm. Saved caller/output controls: bunker_attack_action
/// repair_rows; Unit738890 converts Repair29 to the ordinary Attack mission.
/// The self gate and DetermineAction read `current_objects`, the whole
/// selection; only `receivers` (`click_order_receivers`) get orders.
pub(super) fn unit_repair_branch_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    receivers: &[u64],
    target: &HoverTargetKindWithId,
    issuing_house: InternedId,
    modifier: OrderModifier,
    order_mode: OrderMode,
    force_move_key: bool,
) -> AttackDispatch {
    use crate::sim::world::{InfantryHealAction, UnitRepairAction};
    let mut output = AttackDispatch {
        receivers: Default::default(),
        commands: Vec::new(),
        consumed: false,
        select_fallback: false,
        selects: false,
    };
    if modifier == OrderModifier::Shift
        || current_objects == [target.stable_id]
        || target.kind == HoverTargetKind::HiddenEnemy
    {
        return output;
    }
    let friendly = matches!(
        target.kind,
        HoverTargetKind::FriendlyUnit | HoverTargetKind::FriendlyStructure
    );
    let control = sim.control_projection();
    let heal_action = |id| {
        healer_object_action_with(
            sim,
            &control,
            rules,
            id,
            target.stable_id,
            issuing_house,
            modifier,
            force_move_key,
        )
    };
    let owner_action =
        cursor_owner(sim, rules, current_objects, target.stable_id).and_then(heal_action);
    // An Infantryman's Select ends its What_Action (`0x0051E78B`), where a
    // Unit's goes on to its building arm (`0x00740122`), so the release
    // selects.
    if owner_action == Some(HealerAction::Infantry(InfantryHealAction::Select)) {
        output.selects = true;
        return output;
    }
    // DetermineAction's one chosen object owns a normal friendly click. A
    // repair-capable member must not turn the whole group's Select into Repair.
    if friendly
        && matches!(modifier, OrderModifier::Normal | OrderModifier::AttackMove)
        && !matches!(
            owner_action,
            Some(
                HealerAction::Unit(UnitRepairAction::Repair)
                    | HealerAction::Infantry(InfantryHealAction::Heal { .. })
            )
        )
    {
        return output;
    }
    for &id in receivers {
        let Some(action) = heal_action(id) else {
            continue;
        };
        output.receivers.insert(id);
        let payload = match action {
            HealerAction::Unit(UnitRepairAction::Select)
            | HealerAction::Infantry(InfantryHealAction::Select) => {
                output.select_fallback = true;
                continue;
            }
            HealerAction::Infantry(InfantryHealAction::AttackSupport) => {
                output.consumed = true;
                let Some(command) = attack_support_command(sim, rules, id) else {
                    continue;
                };
                command
            }
            // Infantry ClickedAction (`0x0051F190`) sends Heal as Attack.
            HealerAction::Unit(UnitRepairAction::Repair)
            | HealerAction::Infantry(InfantryHealAction::Heal { .. }) => {
                output.consumed = true;
                // Repair29 replaced Ctrl+Alt's base action before the mission
                // producer; it sends Attack1, not a Guard.
                let Some(command) =
                    object_click_payload(sim, rules, order_mode, id, target.stable_id)
                else {
                    continue;
                };
                command
            }
            HealerAction::Unit(UnitRepairAction::Move) => {
                output.consumed = true;
                let Some(destination) = crate::app::input::commands::object_move_destination_with(
                    sim,
                    &control,
                    rules,
                    issuing_house,
                    id,
                    target.stable_id,
                    crate::sim::world::MoveAction::NoMove,
                    force_move_key,
                ) else {
                    continue;
                };
                move_click_command(sim, rules, order_mode, id, destination)
            }
        };
        output.commands.push(payload);
    }
    output
}

pub(super) fn attack_branch_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    attack_target: TargetKind,
    force_fire: bool,
    order_mode: OrderMode,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> AttackDispatch {
    let mut consumed_attack_action = false;
    let mut queued = Vec::new();
    let mut attack_receivers = std::collections::BTreeSet::new();
    let mut select_fallback = false;
    // Over a Terrain object every receiver answers its own What_Action on
    // the object, armed or not: there is no other object route to take.
    let terrain =
        matches!(attack_target, TargetKind::Entity(id) if sim.terrain_objects().get(id).is_some());
    let control = sim.control_projection();
    for &id in selected_ids {
        let is_structure = sim
            .entities()
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure);
        if !(force_fire || is_structure || terrain) {
            continue;
        }
        let action = if terrain {
            Some(attack_action_with(
                sim,
                &control,
                rules,
                id,
                &attack_target,
                force_fire,
                Some(overlay_registry),
            ))
        } else {
            armed_action_with(
                sim,
                &control,
                rules,
                id,
                &attack_target,
                force_fire,
                Some(overlay_registry),
            )
        };
        let Some(action) = action else {
            continue;
        };
        if action == AttackAction::Rally {
            continue;
        }
        // NoIvanBomb (`0x0051EB24`) is only reached from an Attack answer
        // (`0x0051EA6F`, `0x0051EB33`); it orders nothing.
        if action == AttackAction::Attack
            && let TargetKind::Entity(target_id) = attack_target
            && ivan_cannot_bomb(sim, rules, id, target_id)
        {
            attack_receivers.insert(id);
            consumed_attack_action = true;
            continue;
        }
        if action == AttackAction::None && matches!(attack_target, TargetKind::Cell(..)) {
            // Rejection of the attack rung still reaches Techno's Cell
            // movement tail700AB7..700C08. Leave this receiver to the
            // existing cell-move/harvest path below, including under Ctrl.
            continue;
        }
        attack_receivers.insert(id);
        // Unit 0x738890, Infantry 0x51F190 and Aircraft 0x417BD0
        // ClickedAction_Object hand an object click's 1 or 2 to Foot
        // 0x4D74E0, which moves near the object, as the unforced click does
        // (`object_attack_click_command`).
        if let Some(move_action) = match action {
            AttackAction::NoMove => Some(crate::sim::world::MoveAction::NoMove),
            AttackAction::Move => Some(crate::sim::world::MoveAction::Move),
            _ => None,
        } && !is_structure
            && let TargetKind::Entity(target_id) = attack_target
        {
            consumed_attack_action = true;
            queued.extend(move_near_object(
                sim,
                &control,
                rules,
                order_mode,
                id,
                target_id,
                move_action,
            ));
            continue;
        }
        match action {
            // A healer infantryman's AttackSupport over a Terrain object.
            AttackAction::AttackSupport if matches!(attack_target, TargetKind::Entity(_)) => {
                consumed_attack_action = true;
                queued.extend(attack_support_command(sim, rules, id));
            }
            AttackAction::None
            | AttackAction::NoMove
            | AttackAction::Move
            | AttackAction::AttackSupport => consumed_attack_action = true,
            AttackAction::Rally => unreachable!("rally remains with its existing receiver"),
            AttackAction::Select => select_fallback = true,
            AttackAction::GuardArea => {
                consumed_attack_action = true;
                // gamemd.exe FootClass::ClickedAction_Cell 0x004D7E35..0x004D7E8B:
                // an armed Infantry's admitted GuardArea sends mission 11 with
                // the clicked Cell as target and no destination. ClickedMission
                // 0x006FFC09..0x006FFC23 only promotes Move/Attack, not AreaGuard.
                if matches!(attack_target, TargetKind::Cell(..))
                    && guard_area_admitted(sim, &control, rules, id, None)
                {
                    queued.push(Command::Guard {
                        entity_id: id,
                        target: Some(attack_target),
                    });
                } else if let TargetKind::Entity(target_id) = attack_target {
                    // A healer infantryman over itself (`0x0051E748`).
                    queued.extend(object_guard_command(sim, &control, rules, id, target_id));
                }
            }
            AttackAction::Attack => {
                consumed_attack_action = true;
                let payload = match attack_target {
                    // Under the attack-move chord or mode ClickedMission
                    // promotes this Attack whatever its target
                    // (`0x006FFC09..0x006FFC1E`): an attack-move with the
                    // Cell as its target and no destination, the tokens Foot
                    // ClickedAction_Cell sends (`0x004D80E9..0x004D80FF`).
                    TargetKind::Cell(target_rx, target_ry)
                        if order_mode == OrderMode::AttackMove
                            && entity_can_attack_move(sim, Some(rules), id) =>
                    {
                        Command::AttackMove {
                            entity_id: id,
                            target: Some(TargetKind::Cell(target_rx, target_ry)),
                            destination: None,
                        }
                    }
                    TargetKind::Cell(target_rx, target_ry) => Command::ForceAttackCell {
                        attacker_id: id,
                        target_rx,
                        target_ry,
                    },
                    TargetKind::Entity(target_id) => {
                        match infantry_attack_action_with(sim, &control, rules, id, target_id) {
                            Some(InfantryAttackAction::NoMove) => {
                                let Some(command) = move_near_object(
                                    sim,
                                    &control,
                                    rules,
                                    order_mode,
                                    id,
                                    target_id,
                                    crate::sim::world::MoveAction::NoMove,
                                ) else {
                                    continue;
                                };
                                command
                            }
                            Some(InfantryAttackAction::Infiltrate) => Command::CaptureBuilding {
                                engineer_id: id,
                                target_building_id: target_id,
                            },
                            Some(InfantryAttackAction::Sabotage) => Command::PlantC4 {
                                attacker_id: id,
                                target_building_id: target_id,
                            },
                            Some(InfantryAttackAction::Select) => {
                                select_fallback = true;
                                continue;
                            }
                            _ => {
                                let Some(command) =
                                    object_click_payload(sim, rules, order_mode, id, target_id)
                                else {
                                    continue;
                                };
                                command
                            }
                        }
                    }
                };
                queued.push(payload);
            }
        }
    }
    AttackDispatch {
        receivers: attack_receivers,
        commands: queued,
        consumed: consumed_attack_action,
        select_fallback,
        selects: false,
    }
}

/// Foot `0x004D74E0` case 1 or 2 (`action`) for a forced object click: a
/// Move near the object.
fn move_near_object(
    sim: &crate::sim::world::Simulation,
    control: &ControlProjection<'_>,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    id: u64,
    target_id: u64,
    action: crate::sim::world::MoveAction,
) -> Option<Command> {
    let viewer = context_actor_viewer(sim, sim.entities().get(id)?.owner())?;
    let destination = crate::app::input::commands::object_move_destination_with(
        sim, control, rules, viewer, id, target_id, action, false,
    )?;
    Some(move_click_command(sim, rules, order_mode, id, destination))
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
/// A building pack-up speaks at Selling stage0, so UndeployBuilding must not
/// queue a second input acknowledgement here.
///
/// The self-click halt is silent in retail as well: it builds a detonate event
/// directly instead of sending a mission, so the voice dispatch never runs.
pub(super) fn order_voice_key(command: &Command) -> Option<&'static str> {
    match command {
        Command::Move { .. } | Command::MoveToObject { .. } | Command::AttackMove { .. } => {
            Some("VoiceMove")
        }
        Command::Attack { .. } | Command::ForceAttackCell { .. } => Some("VoiceAttack"),
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Harvest,
            ..
        } => Some("VoiceHarvest"),
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Enter,
            ..
        } => Some("VoiceEnter"),
        // Eaten9 takes QueueMegaMission6FFD42's default voice branch.
        Command::MegaMission {
            mission: crate::sim::mission::MissionType::Eaten,
            ..
        } => Some("VoiceSpecialAttack"),
        Command::CaptureBuilding { .. }
        | Command::MegaMission {
            mission: crate::sim::mission::MissionType::Capture,
            ..
        } => Some("VoiceCapture"),
        // Sabotage and area guard have no dedicated slot, so retail takes the
        // default branch and speaks a VoiceSpecialAttack line.
        Command::PlantC4 { .. } | Command::Guard { .. } => Some("VoiceSpecialAttack"),
        // A self-click's deploy, unload or deploy-fire is the Unload mission
        // (`FootClass::ClickedAction_Object @ 0x004D74E0` actions 4/0x34/0x38;
        // `AircraftClass @ 0x00417BD0` action 4; a Building's garrison or
        // absorber arm `0x004434F2..0x00443562`), whose ClickedMission arm
        // speaks VoiceDeploy (vt+0x36C, `0x006FFD3A`). EjectBunker and the
        // factory primary toggle are not mapped.
        Command::DeployMcv { .. }
        | Command::ToggleInfantryDeploy { .. }
        | Command::UnloadPassengers { .. } => Some("VoiceDeploy"),
        _ => None,
    }
}

/// The entity a command acts on, when the command targets exactly one.
///
/// Used to find which queued order belongs to the speaking object.
pub(super) fn command_actor_id(command: &Command) -> Option<u64> {
    match command {
        Command::Move { entity_id, .. }
        | Command::MegaMission { entity_id, .. }
        | Command::MoveToObject { entity_id, .. }
        | Command::AttackMove { entity_id, .. }
        | Command::Guard { entity_id, .. }
        | Command::ToggleInfantryDeploy { entity_id }
        | Command::DeployMcv { entity_id }
        | Command::UndeployBuilding { entity_id }
        // BuildingClass::ClickedAction_Cell @ 0x004436F0 emits SELL16 after
        // ARCHIVE1E; Selection @ 0x004AE750 keeps both at this actor's index.
        | Command::SellBuilding { entity_id }
        | Command::ArchiveObject { entity_id, .. }
        | Command::Stop { entity_id } => Some(*entity_id),
        Command::Attack { attacker_id, .. }
        | Command::ForceAttackCell { attacker_id, .. }
        | Command::PlantC4 { attacker_id, .. } => Some(*attacker_id),
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
/// This index derives from input selection. A MoveFlash belongs to the first
/// receiver and stays ahead of its order ([`push_move_flash`]).
pub(super) fn restore_selection_dispatch_order(queued: &mut [CommandEnvelope], selected: &[u64]) {
    if queued.iter().any(|envelope| {
        command_actor_id(&envelope.payload).is_none()
            && !matches!(envelope.payload, Command::MoveFlash { .. })
    }) {
        return;
    }
    let positions: std::collections::HashMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(index, &id)| (id, index))
        .collect();
    queued.sort_by_key(|envelope| match envelope.payload {
        Command::MoveFlash { .. } => (0, false),
        _ => (
            command_actor_id(&envelope.payload)
                .and_then(|id| positions.get(&id).copied())
                .unwrap_or(usize::MAX),
            true,
        ),
    });
}

/// Play the single order-ack line for a batch of freshly queued orders.
///
/// Retail's dispatch loop clears the voice-enable flag at the end of *every*
/// iteration and restores it once after the loop, so exactly one object speaks —
/// the first entry of the selection array — and it speaks the line for the
/// mission *it* resolved, not an order-wide line. If that first object resolved
/// no order (its action mapped to a cursor-only code) nothing is spoken.
fn emit_resolved_order_voice(
    match_state: &mut MatchState,
    speaker_id: u64,
    queued: &[CommandEnvelope],
) {
    let Some(envelope) = queued
        .iter()
        .find(|env| command_actor_id(&env.payload) == Some(speaker_id))
    else {
        return;
    };
    let Some(voice_field) = order_voice_key(&envelope.payload) else {
        return;
    };
    //6FFCEA..6FFCEF passes the committed Attack argument to VoiceAttack,
    // before event encoding. A Move destination and the actor's old TarCom
    // are different owners; neither supplies this argument.
    let target = match &envelope.payload {
        Command::Attack { target_id, .. } => Some(TargetKind::Entity(*target_id)),
        Command::ForceAttackCell {
            target_rx,
            target_ry,
            ..
        } => Some(TargetKind::Cell(*target_rx, *target_ry)),
        _ => None,
    };
    emit_entity_order_voice(match_state, speaker_id, voice_field, target);
}

/// Play one entity's voice line for the given `Voice*` INI key.
///
/// The superseded app-layer order-voice helper always spoke the lowest-`stable_id` selected
/// entity; retail speaks the object that resolved the order, so order resolution
/// needs to name the speaker explicitly.
fn emit_entity_order_voice(
    match_state: &mut MatchState,
    speaker_id: u64,
    voice_field: &str,
    target: Option<TargetKind>,
) {
    if voice_field == "VoiceDeploy" {
        if let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command)
            && let Some(event) = crate::app::match_runtime::sound_dispatch::deploy_order_voice_event(
                &mut runtime.simulation,
                &runtime.resources.rules,
                speaker_id,
                match_state.input.selection_voice_enabled,
            )
        {
            match_state.match_audio.sound_events.push(event);
        }
        return;
    }
    if voice_field == "VoiceSpecialAttack" {
        if let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command)
            && let Some(event) =
                crate::app::match_runtime::sound_dispatch::default_order_voice_event(
                    &mut runtime.simulation,
                    &runtime.resources.rules,
                    speaker_id,
                    match_state.input.selection_voice_enabled,
                )
        {
            match_state.match_audio.sound_events.push(event);
        }
        return;
    }
    let Some(runtime) = match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return;
    };
    let Some(id) = runtime.simulation.order_voice_request(
        &runtime.resources.rules,
        speaker_id,
        voice_field,
        target,
        match_state.input.selection_voice_enabled,
    ) else {
        return;
    };

    let event = if voice_field == "VoiceAttack" {
        crate::audio::events::GameSoundEvent::UnitAttackOrder {
            speaker_id,
            sound_id: runtime.resources.rules.sound_name(id),
        }
    } else {
        crate::audio::events::GameSoundEvent::UnitMoveOrder {
            speaker_id,
            sound_id: runtime.resources.rules.sound_name(id),
        }
    };
    match_state.match_audio.sound_events.push(event);
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
pub(super) fn finish_order(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    mut queued: Vec<CommandEnvelope>,
    speaker_id: Option<u64>,
    rally_announce: bool,
    clicked_cell: Option<(u16, u16)>,
) -> bool {
    let planning = match_state.input.planning.active();
    if let Some(runtime) = match_state.sim_runtime.runtime() {
        let proposed = !queued.is_empty();
        let sim = &runtime.simulation;
        retain_context_orders(sim, &runtime.resources.rules, &mut queued);
        if sim.session().has_custom_control_grants() {
            let Some(primary) = sim.control_projection().local_command_house() else {
                return false;
            };
            for order in &mut queued {
                order.owner = primary;
            }
        }
        if proposed && queued.is_empty() {
            return true;
        }
    }
    let sim = match_state.sim_runtime.runtime().map(|rt| &rt.simulation);
    let mut selected = selected_stable_ids_in_order(
        sim,
        crate::app::state::active_rules(frontend, match_state),
        &match_state.input.selection_order,
        match_state.input.selection_order_pending,
    );
    if let Some(sim) = sim {
        let control = sim.control_projection();
        selected.retain(|&actor| context_order_receiver(sim, &control, actor));
    }
    restore_selection_dispatch_order(&mut queued, &selected);
    let admitted = if planning {
        crate::app::input::planning_mode::queue_orders(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            persistence,
            &selected,
            queued,
            clicked_cell,
        )
    } else {
        // The local acknowledgement reads the resolved input order before
        // encoding. Only the native record enters the gameplay queue.
        let (encoded, feedback): (Vec<_>, Vec<_>) =
            if let Some(sim) = match_state.sim_runtime.runtime().map(|rt| &rt.simulation) {
                queued
                    .into_iter()
                    .filter_map(|envelope| {
                        crate::app::input::commands::encode_ordinary_local_command(
                            sim,
                            envelope.clone(),
                        )
                        .map(|encoded| (encoded, envelope))
                    })
                    .unzip()
            } else {
                (queued.clone(), queued)
            };
        let mut accepted = true;
        if !encoded.is_empty() {
            if let Some(sim) = match_state
                .sim_runtime
                .write_sim(SimWrite::Command)
                .map(|rt| &mut rt.simulation)
            {
                accepted = sim.issue_local_commands(encoded);
            }
            if accepted {
                if let Some(speaker_id) = speaker_id {
                    emit_resolved_order_voice(match_state, speaker_id, &feedback);
                }
            }
        }
        if !accepted {
            super::super::messages::post_command_backpressure(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
            );
        }
        accepted
    };
    // Building4437A3 passes announce=true to443860 in planning too.
    // LeftUp4ABF78..4ABF8E refuses before dispatch; admitted clicks announce
    // after the append attempt, including a full OutList or a missed search
    // (4439DC/443A89 ->443A2B..443A69). Source: original gamemd.exe.
    // Sim-scale deviation: a click VERA's larger queue refuses is not
    // admitted, so it does not announce.
    if admitted && rally_announce {
        crate::app::input::dispatch::push_local_eva(
            &mut match_state.match_audio,
            crate::app::match_runtime::eva_producers::EVA_NEW_RALLY_POINT_ESTABLISHED,
        );
    }
    //4ABF94's admitted planning route joins the same70D150 suffix at
    //4ABFAE;4ABF8E's refusal exits4AC294 without restarting this timer.
    if admitted
        && let Some(frame) = match_state
            .sim_runtime
            .runtime()
            .map(|rt| rt.simulation.session().binary_frame())
    {
        match_state
            .match_presentation
            .target_lines
            .start_timer(frame);
    }
    true
}

pub(super) fn retain_context_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    queued: &mut Vec<CommandEnvelope>,
) {
    let control = sim.control_projection();
    queued.retain(|order| sim.can_issue_local_command_with(&control, &order.payload, rules));
}
