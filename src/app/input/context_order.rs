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
use crate::app::input::entity_pick::{HoverTargetKindWithId, hover_target_at_point};
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
///   ordinary immediate move that every Foot's Cell resolver receives. Retail
///   Planning Mode uses its existing shared path owner and separate event opcodes.
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
    /// Shift — over a Cell, What_Action answers Move before Can_Enter_Cell
    /// (`0x00700B03`).
    Shift,
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
        OrderModifier::Shift
    } else if alt {
        OrderModifier::ForceMove
    } else {
        OrderModifier::Normal
    }
}

/// The action [`attack_action`] answers, shared by cursor and click. When it
/// is the action of the object that owns the cursor, a click dispatches
/// Attack and NoMove to every receiver; Select, and 0 over an object the
/// player may select and has not, select that object instead
/// (`DisplayClass::BandBox_LeftUp` `0x004ABDD3..0x004ABE11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttackAction {
    None,
    Select,
    Attack,
    NoMove,
    Rally,
}

/// The allied-object part of Techno6FFEC0's unforced attack admission,
/// 700314..7003D4. All consumers ask this through the existing action owner.
/// IsAlly reads the real owner and the asking House's directional mask;
/// display friendliness alone cannot decide whether to attack or select.
fn friendly_attack_is_admitted(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    use crate::sim::combat::combat_weapon;

    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(target) = sim.entities().get(target_id) else {
        // House4F9A90 sees Terrain's null owner (Object4104A0), so
        // the allied-only restrictions do not apply to a real Terrain.
        return crate::sim::movement::ground_pose::target_get_coords(
            TargetKind::Entity(target_id),
            sim,
            None,
        )
        .is_some();
    };
    if !combat_weapon::is_ally_by_object(
        Some(&sim.house_alliances),
        &sim.interner,
        actor.owner(),
        target.owner(),
    ) {
        return true;
    }
    let Some(kind) = sim.interner.object_type(actor.type_ref(), rules) else {
        return false;
    };
    if kind.attacks_friendlies() {
        return true;
    }
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let weapon = combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        kind,
        &combat_weapon::attacker_facts(actor, kind),
        Some(&TargetKind::Entity(target_id)),
        sim,
        &sim.interner,
        cells.as_ref(),
        Some(&sim.house_alliances),
    )
    .and_then(|(_, weapon)| weapon);
    let ivan_transport = weapon
        .as_ref()
        .is_some_and(|weapon| weapon.warhead.ivan_bomb)
        && sim
            .interner
            .object_type(target.type_ref(), rules)
            .is_some_and(|target| target.passengers > 0);
    (kind.attack_cursor_on_friendlies && !ivan_transport)
        || (weapon.as_ref().is_some_and(|weapon| weapon.warhead.sonic)
            && target.category() != EntityCategory::Structure
            && target.parasite.is_infested())
}

/// The attack rung of the actor's What_Action and the tail it falls to.
///
/// Object target (`TechnoClass::What_Action_OnObject @ 0x006FFEC0`): the
/// rung is a human player's (`0x007003ED`), for an armed actor (`Is_Armed`,
/// vt+0x2AC) or an `Infiltrate=` infantryman (`0x00700444..0x00700474`), and
/// asks its control capability (vt+0xA0) or its selected weapon's range
/// (`0x0070047A..0x0070049A`). Unless `forced`, a shot ILLEGAL to the actor
/// leaves the rung as well ([`shot_is_illegal`]); `forced` is the force-fire
/// key, which What_Action reads itself (`0x00700048..0x00700072`): Ctrl,
/// alone or in the Ctrl+Alt chord. Off the rung, the tail
/// (`0x0070056C..0x007005E6`) answers 0 when an armed human player's object
/// points at another house's object, or for a loaner (`+0x3D4`) the player
/// owns (`+0x41A`); otherwise Select for an object the player may select
/// (vt+0x138) that is neither selected (`+0x83`) nor warped out (vt+0x1D4),
/// and 0 for any other.
///
/// Cell target (`TechnoClass::What_Action_OnCell @ 0x00700600`): the rung is
/// a human player's too (`0x007008A6..0x007008B3`), and the caller has asked
/// the weapon ([`armed_action`]); control or range answers Attack
/// (`0x007009F3..0x00700A44`), else 0. Any other house's object answers 0
/// there (`0x00700AB7..0x00700AC4`).
///
/// This asks no ROF or reload. The class post-processing follows below. See
/// the original DetermineAction/Unit/Foot/Techno and Display action corpus
/// in tools/spatial_oracle/bunker_attack_action.json.
///
/// The real-owner ally gate and its shared weapon/type grants are handled by
/// [`friendly_attack_is_admitted`]. A different allied House therefore takes
/// the identity-sensitive tail, then the existing class postprocessing.
///
/// RESIDUAL (#605): the rest of admission (`0x00700255..0x0070043E`) is not
/// ported as one gate. Unforced, native also leaves the rung for:
/// - an object disguised to the actor's house (vt+0xC8), unless that grant
///   holds, `[Rules+0xD6C]` is set, its type is `PermaDisguise=` (`+0xD30`),
///   its `+0x1EC`/`+0x1F4` timer runs, or it wears no house (vt+0xD0) and the
///   weapon may fire at terrain (vt+0x398, `0x0070EF00`, `TerrainFire=`);
/// - a type that is not `LegalTarget=` (`+0x231`), unless that terrain test
///   passes.
///
/// Active YR Battle authors AttackCursorOnDisguise=yes, and SPY also authors
/// PermaDisguise=yes: an ordinary stock Spy/Mirage is not evidence of a missed
/// rejection. A layer disabling those grants or a non-LegalTarget type still
/// requires the remaining admission. Those conditional paths remain open.
pub(crate) fn attack_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> AttackAction {
    let Some(actor) = sim.entities().get(actor_id) else {
        return AttackAction::None;
    };
    let Some(kind) = sim.interner.object_type(actor.type_ref(), rules) else {
        return AttackAction::None;
    };
    let human = sim.house_is_human_player(actor.owner());
    let armed = crate::sim::combat::combat_weapon::is_armed(actor, kind);
    let infiltrator = actor.category() == EntityCategory::Infantry && kind.infiltrate;
    let admitted = match target {
        TargetKind::Entity(target_id) => {
            human
                && (armed || infiltrator)
                && (forced || friendly_attack_is_admitted(sim, rules, actor_id, *target_id))
        }
        TargetKind::Cell(..) => human,
    };
    let mut action = if admitted
        && (sim.techno_player_controllable(actor_id, rules)
            || resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain.as_ref(),
                overlay_registry,
                Some(target),
            ))
        && (forced || !shot_is_illegal(sim, rules, actor_id, target, overlay_registry))
    {
        AttackAction::Attack
    } else if let TargetKind::Entity(target_id) = *target
        && let Some(object) = sim.entities().get(target_id)
        && !(armed && human && object.owner() != actor.owner())
        && !(actor.is_mission_only() && actor.discovery.owned_by_current_house())
        && sim.object_is_selectable(target_id, Some(rules))
        && !object.selection.is_selected()
        && !object.is_warped_out()
    {
        AttackAction::Select
    } else {
        AttackAction::None
    };
    // Unit74077F..7407E5 /740389..7403E8: the fully deployed
    // SimpleDeployer repeats SelectWeapon(NULL), then range. This returns
    // Action2, unlike the Techno control/range Action0 branch above.
    if action == AttackAction::Attack
        && actor.category() == EntityCategory::Unit
        && kind.is_simple_deployer
        && actor.is_fully_deployed()
        && (!kind.deploy_fire
            || !resolved_unit_in_range(
                sim,
                actor_id,
                target,
                Some(rules),
                sim.resolved_terrain.as_ref(),
                overlay_registry,
                None,
            ))
    {
        action = AttackAction::NoMove;
    }
    // Infantry51EE3B..51EEEF: a player's Infiltrate= infantryman's Attack is
    // Capture over a building it may infiltrate, which its producers ask
    // first (Simulation::infiltrator_building_action); otherwise it is 0
    // while he is unarmed (51F04E..51F05C).
    if action == AttackAction::Attack
        && infiltrator
        && human
        && !armed
        && let TargetKind::Entity(target_id) = *target
        && !sim.infiltrator_building_action(actor_id, target_id, rules)
    {
        action = AttackAction::None;
    }
    // Over an object, the Unit and Infantry tails turn 0 into NoMove for a
    // player's object it may control.
    if action == AttackAction::None
        && matches!(target, TargetKind::Entity(_))
        && sim.foot_object_class_action(actor_id, crate::sim::world::MoveAction::None, rules)
            == crate::sim::world::MoveAction::NoMove
    {
        action = AttackAction::NoMove;
    }
    if actor.category() == EntityCategory::Structure {
        // Building447366..4473C7 /4476F3..447749 inspect GetWeapon(0),
        // irrespective of SelectWeapon. These are class post-processing of
        // Attack, not a replacement range owner.
        if action == AttackAction::Attack
            && let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
                actor,
                kind,
                0,
                sim.entities(),
                rules,
                &sim.interner,
            )
            && (weapon
                .projectile
                .as_deref()
                .and_then(|name| rules.projectile(name))
                .is_some_and(|projectile| !projectile.ag)
                || kind.emp_pulse_cannon
                || actor.mission.current().known()
                    == Some(crate::sim::mission::MissionType::Selling))
        {
            action = AttackAction::None;
        }
        if action == AttackAction::Attack
            && let TargetKind::Entity(id) = *target
            && sim
                .entities()
                .get(id)
                .and_then(|target| sim.interner.object_type(target.type_ref(), rules))
                .is_some_and(|target| target.immune)
        {
            // Object447512..447527; the Cell wrapper has no such target.
            action = AttackAction::NoMove;
        }
        if action == AttackAction::None
            && matches!(target, TargetKind::Cell(..))
            && kind.has_rally_line()
            && sim.house_is_human_player(actor.owner())
        {
            // Cell44774B..447768; existing factory destination owner commits it.
            action = AttackAction::Rally;
        }
    }
    action
}

/// [`attack_action`] for an armed actor: the Ctrl branch (`forced`), and a
/// building's unforced one. An unarmed receiver answers `None` and keeps its
/// ordinary action paths. Cell7008BD reads GetCurrentWeapon, which also
/// resolves an occupied building's firing occupant; Object700468 asks
/// virtual IsArmed.
pub(crate) fn armed_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    forced: bool,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Option<AttackAction> {
    let actor = sim.entities().get(actor_id)?;
    let object = sim.interner.object_type(actor.type_ref(), rules)?;
    let armed = match target {
        TargetKind::Cell(..) => crate::sim::combat::combat_weapon::current_weapon(
            actor,
            object,
            sim.entities(),
            rules,
            &sim.interner,
        )
        .is_some(),
        TargetKind::Entity(_) => crate::sim::combat::combat_weapon::is_armed(actor, object),
    };
    armed.then(|| attack_action(sim, rules, actor_id, target, forced, overlay_registry))
}

/// Display4AB51B asks CanFireAtTarget6F7780: ONE SelectWeapon result, not
/// either slot's longest range. The query domain is private to input, across
/// target flight, fire source and InRange, so nested misses never stamp the
/// simulation's canonical Dummy.
pub(crate) fn resolved_unit_in_range(
    sim: &crate::sim::world::Simulation,
    actor_id: u64,
    target: &TargetKind,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    selection_target: Option<&TargetKind>,
) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    if let Some(terrain) = terrain {
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        return crate::sim::combat::can_fire_at_target_in_query(
            sim,
            rules,
            &sim.interner,
            actor_id,
            target,
            &cells,
            Some(&sim.house_alliances),
            &crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: sim.overlay_grid.as_ref(),
                overlay_registry,
                alliances: Some(&sim.house_alliances),
            },
            selection_target,
        );
    }
    // Existing terrain-free test adapter; production maps take the native
    // 3D owner above. No terrain-free parity is claimed.
    let Some(actor) = sim.entities().get(actor_id) else {
        return false;
    };
    let Some(object) = sim.interner.object_type(actor.type_ref(), rules) else {
        return false;
    };
    let Some((index, _)) = crate::sim::combat::combat_weapon::select_weapon_against_in_query(
        rules,
        actor,
        object,
        &crate::sim::combat::combat_weapon::attacker_facts(actor, object),
        selection_target,
        sim,
        &sim.interner,
        None,
        Some(&sim.house_alliances),
    ) else {
        return false;
    };
    let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
        actor,
        object,
        index,
        sim.entities(),
        rules,
        &sim.interner,
    ) else {
        return false;
    };
    let Some(point) = crate::sim::movement::ground_pose::target_get_coords(*target, sim, None)
    else {
        return false;
    };
    let (rx, ry, sub_x, sub_y) = (
        (point.x / 256) as u16,
        (point.y / 256) as u16,
        crate::util::fixed_math::SimFixed::from_num(point.x % 256),
        crate::util::fixed_math::SimFixed::from_num(point.y % 256),
    );
    let distance = crate::sim::combat::lepton_distance_sq_raw(
        actor.position.rx(),
        actor.position.ry(),
        actor.position.sub_x(),
        actor.position.sub_y(),
        rx,
        ry,
        sub_x,
        sub_y,
    );
    crate::sim::combat::is_within_range_leptons(distance, weapon.range)
}

/// Resolved attack receiver batch. These are ephemeral input decisions;
/// source state and mutation authority remain with Simulation.
struct AttackDispatch {
    receivers: std::collections::BTreeSet<u64>,
    commands: Vec<Command>,
    consumed: bool,
    select_fallback: bool,
}

/// Unit73FD7C's post-processing precedes the generic friendly selection and
/// Ctrl attack paths. Selection/keyboard and mission promotion stay with their
/// existing owners. Shift and one selected object's self action precede
/// the native repair arm. Saved caller/output controls: bunker_attack_action
/// repair_rows; Unit738890 converts Repair29 to the ordinary Attack mission.
/// The self gate and DetermineAction read `current_objects`, the whole
/// selection; only `receivers` (`click_order_receivers`) get orders.
fn unit_repair_branch_orders(
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
    use crate::sim::world::UnitRepairAction;
    let mut output = AttackDispatch {
        receivers: Default::default(),
        commands: Vec::new(),
        consumed: false,
        select_fallback: false,
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
    // DetermineAction's one chosen object owns a normal friendly click. A
    // repair-capable member must not turn the whole group's Select into Repair.
    if friendly
        && matches!(modifier, OrderModifier::Normal | OrderModifier::AttackMove)
        && !cursor_owner(sim, rules, current_objects, target.stable_id).is_some_and(|id| {
            sim.unit_repair_action(id, target.stable_id, issuing_house, rules)
                == Some(UnitRepairAction::Repair)
        })
    {
        return output;
    }
    for &id in receivers {
        let Some(action) = sim.unit_repair_action(id, target.stable_id, issuing_house, rules)
        else {
            continue;
        };
        output.receivers.insert(id);
        let payload = match action {
            UnitRepairAction::Select => {
                output.select_fallback = true;
                continue;
            }
            UnitRepairAction::Repair => {
                output.consumed = true;
                // Repair29 replaced Ctrl+Alt's base action before the mission
                // producer; it sends Attack1, not the legacy Guard adapter.
                let attack_modifier = if modifier == OrderModifier::GuardArea {
                    OrderModifier::Normal
                } else {
                    modifier
                };
                object_click_payload(
                    order_mode,
                    attack_modifier,
                    entity_can_attack_move(sim, Some(rules), id),
                    id,
                    target.stable_id,
                )
            }
            UnitRepairAction::Move => {
                output.consumed = true;
                let Some(destination) = crate::app::input::commands::object_move_destination(
                    sim,
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

fn attack_branch_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected_ids: &[u64],
    attack_target: TargetKind,
    force_fire: bool,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> AttackDispatch {
    let mut consumed_attack_action = false;
    let mut queued = Vec::new();
    let mut attack_receivers = std::collections::BTreeSet::new();
    let mut select_fallback = false;
    for &id in selected_ids {
        let is_structure = sim
            .entities()
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure);
        if !(force_fire || is_structure) {
            continue;
        }
        if let TargetKind::Entity(target_id) = attack_target
            && ivan_cannot_bomb(sim, rules, id, target_id)
        {
            attack_receivers.insert(id);
            consumed_attack_action = true;
            continue;
        }
        let Some(action) = armed_action(
            sim,
            rules,
            id,
            &attack_target,
            force_fire,
            Some(overlay_registry),
        ) else {
            continue;
        };
        if action == AttackAction::Rally {
            continue;
        }
        attack_receivers.insert(id);
        match action {
            AttackAction::None | AttackAction::NoMove => consumed_attack_action = true,
            AttackAction::Rally => unreachable!("rally remains with its existing receiver"),
            AttackAction::Select => select_fallback = true,
            AttackAction::Attack => {
                consumed_attack_action = true;
                let payload = match attack_target {
                    TargetKind::Cell(target_rx, target_ry) => Command::ForceAttackCell {
                        attacker_id: id,
                        target_rx,
                        target_ry,
                    },
                    TargetKind::Entity(target_id) => Command::Attack {
                        attacker_id: id,
                        target_id,
                    },
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
/// Deploy/unload's input acknowledgement remains a separate route. A
/// building pack-up speaks at Selling stage0, so UndeployBuilding must not
/// queue a second input acknowledgement here.
///
/// The self-click halt is silent in retail as well: it builds a detonate event
/// directly instead of sending a mission, so the voice dispatch never runs.
fn order_voice_key(command: &Command) -> Option<&'static str> {
    match command {
        Command::Move { .. } | Command::MoveToObject { .. } | Command::AttackMove { .. } => {
            Some("VoiceMove")
        }
        Command::Attack { .. } | Command::ForceAttackCell { .. } => Some("VoiceAttack"),
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
        | Command::MoveToObject { entity_id, .. }
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
    emit_entity_order_voice(state, speaker_id, voice_field, target);
}

/// Play one entity's voice line for the given `Voice*` INI key.
///
/// The superseded app-layer order-voice helper always spoke the lowest-`stable_id` selected
/// entity; retail speaks the object that resolved the order, so order resolution
/// needs to name the speaker explicitly.
fn emit_entity_order_voice(
    state: &mut AppState,
    speaker_id: u64,
    voice_field: &str,
    target: Option<TargetKind>,
) {
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
    let Some(runtime) = state.match_state.sim_runtime.as_mut() else {
        return;
    };
    let Some(id) = runtime.simulation.order_voice_request(
        &runtime.resources.rules,
        speaker_id,
        voice_field,
        target,
        state.match_state.input.selection_voice_enabled,
    ) else {
        return;
    };

    let event = if voice_field == "VoiceAttack" {
        crate::audio::events::GameSoundEvent::UnitAttackOrder {
            speaker_id,
            sound_id: id.to_owned(),
        }
    } else {
        crate::audio::events::GameSoundEvent::UnitMoveOrder {
            speaker_id,
            sound_id: id.to_owned(),
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
    let admitted = if state.match_state.input.planning.active() {
        let mut queued = queued;
        let selected = selected_stable_ids_in_order(
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation),
            state.rules(),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        restore_selection_dispatch_order(&mut queued, &selected);
        crate::app::input::planning_mode::queue_orders(state, &selected, queued)
    } else {
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
        if !queued.is_empty() {
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
        true
    };
    //4ABF94's admitted planning route joins the same70D150 suffix at
    //4ABFAE;4ABF8E's refusal exits4AC294 without restarting this timer.
    if admitted
        && let Some(frame) = state
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

/// Does this object accept an attack-move order? Its `Can_Attack_Move`
/// ([`crate::sim::world::Simulation::techno_can_attack_move`]).
fn entity_can_attack_move(
    sim: &crate::sim::world::Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    stable_id: u64,
) -> bool {
    rules.is_some_and(|rules| sim.techno_can_attack_move(stable_id, rules))
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

/// The order one receiver commits for a Move click's destination.
/// ClickedMission (`0x006FFBE0`) promotes Move to attack-move (mission 29)
/// while the attack-move chord or toggle is on (`0x00731BF0`) and the
/// receiver can attack-move (vt+0x4C0), and the event keeps the click's
/// destination, a Cell or the object (`0x006FFDA5..0x006FFDE6`). The
/// promotion is per object: a unit whose type refuses it keeps the plain Move
/// it committed, even when the rest of the group attack-moves.
fn move_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    order_mode: OrderMode,
    entity_id: u64,
    destination: TargetKind,
) -> Command {
    let attack_move =
        order_mode == OrderMode::AttackMove && entity_can_attack_move(sim, Some(rules), entity_id);
    match destination {
        _ if attack_move => Command::AttackMove {
            entity_id,
            target: None,
            destination: Some(destination),
        },
        TargetKind::Cell(target_rx, target_ry) => Command::Move {
            entity_id,
            target_rx,
            target_ry,
        },
        TargetKind::Entity(target_id) => Command::MoveToObject {
            entity_id,
            target_id,
        },
    }
}

/// The command one selected unit commits for a tactical click on an object.
///
/// ClickedMission (`0x006FFBE0`) promotes the committed **Attack** to
/// attack-move as readily as a committed Move, gated per object on its own
/// type predicate, and the event keeps the clicked object as its target
/// (`0x004D76B1..0x004D76C1`). `Assign_Target_Command` stores it as MegaTarget
/// and answers Attack, so the unit attacks it as an attack-mover; a member
/// whose type refuses attack-move commits the plain attack.
pub(crate) fn object_click_payload(
    order_mode: OrderMode,
    modifier: OrderModifier,
    can_attack_move: bool,
    attacker_id: u64,
    target_id: u64,
) -> Command {
    if modifier == OrderModifier::ForceFire {
        return Command::Attack {
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
            target: Some(TargetKind::Entity(target_id)),
            destination: None,
        },
        _ => Command::Attack {
            attacker_id,
            target_id,
        },
    }
}

/// The order one receiver commits for a click on another house's object:
/// its own What_Action there, which the selection's click loop hands to its
/// ClickedAction (`0x004AE829..0x004AE844`).
///
/// Unforced, that is [`attack_action`]'s answer. NoMove moves the receiver
/// to the object or a Cell near it (Foot `0x004D74E0`), as an attack-move
/// under the chord ([`Simulation::object_move_click`],
/// [`move_click_command`]); Select and 0 order nothing.
///
/// Attack, and any action under force fire, orders
/// [`object_click_payload`]. What_Action decides Attack before
/// ClickedMission promotes it to attack-move, so an unforced shot ILLEGAL to
/// the receiver leaves no Attack to promote: its tail answers 0 or Select
/// there ([`attack_action`]). A Crazy Ivan's Attack over an object he cannot
/// bomb is NoIvanBomb (`0x0051EB24`), which orders nothing.
///
/// RESIDUAL (#605): force fire orders the payload whatever the receiver.
/// Trigger: Ctrl over an object with an unarmed receiver. Effect: VERA
/// queues an Attack where native, whose attack rung admits an unarmed
/// receiver under Ctrl too only as an `Infiltrate=` infantryman
/// (`0x00700444..0x00700474`), selects the object or moves the receiver near
/// it. Rare.
///
/// [`Simulation::object_move_click`]: crate::sim::world::Simulation::object_move_click
fn object_attack_click_command(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    issuing_house: InternedId,
    order_mode: OrderMode,
    modifier: OrderModifier,
    force_move_key: bool,
    attacker_id: u64,
    target_id: u64,
) -> Option<Command> {
    let move_near = |action| {
        let destination = crate::app::input::commands::object_move_destination(
            sim,
            rules,
            issuing_house,
            attacker_id,
            target_id,
            action,
            force_move_key,
        )?;
        Some(move_click_command(
            sim,
            rules,
            order_mode,
            attacker_id,
            destination,
        ))
    };
    if modifier != OrderModifier::ForceFire {
        // Unforced, as the cursor asks it. Native answers the Ctrl+Alt chord
        // before the rung: Guard over an allied object, else Move
        // (`0x00700191..0x00700217`). The legacy classifier (#605) asks the
        // rung instead and orders its Guard payload for an Attack.
        match attack_action(
            sim,
            rules,
            attacker_id,
            &TargetKind::Entity(target_id),
            false,
            Some(overlay_registry),
        ) {
            // NoIvanBomb (`0x0051EB24`) orders nothing.
            AttackAction::Attack if ivan_cannot_bomb(sim, rules, attacker_id, target_id) => {
                return None;
            }
            AttackAction::Attack => {}
            // The legacy Ctrl+Alt classifier (#605) keeps its Guard path.
            AttackAction::NoMove if modifier != OrderModifier::GuardArea => {
                return move_near(crate::sim::world::MoveAction::NoMove);
            }
            AttackAction::NoMove
            | AttackAction::Select
            | AttackAction::None
            | AttackAction::Rally => return None,
        }
    }
    Some(object_click_payload(
        order_mode,
        modifier,
        entity_can_attack_move(sim, Some(rules), attacker_id),
        attacker_id,
        target_id,
    ))
}

/// What `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` does with the action
/// DetermineAction answered ([`left_release`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftRelease {
    /// Every selected object gets the action (`0x004ABE88`); one whose
    /// ClickedAction refuses sends nothing, and the action-line timer still
    /// starts (`0x004ABFA9`).
    Dispatch,
    /// Nothing (`0x004AC294`).
    Nothing,
    /// The ordinary selection click on the object under the cursor
    /// (`0x004ABE18`).
    Select,
}

/// The part of DetermineAction's action that [`left_release`] tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseAction {
    Select,
    Zero,
    Other,
}

impl From<AttackAction> for ReleaseAction {
    fn from(action: AttackAction) -> Self {
        match action {
            AttackAction::Select => Self::Select,
            AttackAction::None => Self::Zero,
            AttackAction::Attack | AttackAction::NoMove | AttackAction::Rally => Self::Other,
        }
    }
}

/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0` for `action`, with `target`
/// the object under the cursor: Select selects it (`0x004ABDD3..0x004ABDDA`),
/// and 0 selects an object the player may select (vt+0x138) and has not
/// (`+0x83`) (`0x004ABDE1..0x004ABE11`); otherwise both do nothing. Every
/// other action dispatches.
fn left_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    action: ReleaseAction,
    target: Option<u64>,
) -> LeftRelease {
    let selects = match action {
        ReleaseAction::Other => return LeftRelease::Dispatch,
        ReleaseAction::Select => target.is_some(),
        ReleaseAction::Zero => target.is_some_and(|id| {
            sim.object_is_selectable(id, Some(rules))
                && sim
                    .entities()
                    .get(id)
                    .is_some_and(|object| !object.selection.is_selected())
        }),
    };
    if selects {
        LeftRelease::Select
    } else {
        LeftRelease::Nothing
    }
}

/// The left release for a selection whose every object is berserk.
/// DetermineAction's best object is then berserk too (rank 0, `0x00535435`),
/// and its What_Action base answers 0 whatever the modifiers (`0x006FFEC9`,
/// cell `0x00700609`). The class rungs decide the rest:
/// - Infantry: NoMove for a human owner (`0x0051F05E`), on the ground too
///   unless `JumpJet=` (`0x0051FAAB`).
/// - Unit: NoMove over an object (`0x007403EB`), 0 on the ground
///   (`0x007404B0`). A healer's repair arm still runs (`0x0073FD7C`), so a
///   berserk repair IFV may answer Select or Repair.
/// - Aircraft: 0, over an object through its cell action (`0x00417D7C`).
///
/// [`left_release`] then decides what the release does.
fn berserk_selection_click(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    hover: Option<&HoverTargetKindWithId>,
    cell: (u16, u16),
    issuing_house: InternedId,
) -> LeftRelease {
    use crate::app::input::cursor::ActionDistanceTarget;
    let target = hover.map_or(ActionDistanceTarget::CellCentre(cell.0, cell.1), |target| {
        ActionDistanceTarget::Object(target.stable_id)
    });
    let Some((best_id, best)) = crate::app::input::cursor::select_best_for_action(
        sim,
        current_objects,
        target,
        Some(rules),
    )
    .and_then(|id| Some((id, sim.entities().get(id)?))) else {
        return LeftRelease::Dispatch;
    };
    let action_zero = match best.category() {
        EntityCategory::Aircraft => true,
        EntityCategory::Unit => hover.is_none(),
        EntityCategory::Infantry => {
            hover.is_none()
                && sim
                    .interner
                    .object_type(best.type_ref(), rules)
                    .is_some_and(|object| object.jumpjet)
        }
        EntityCategory::Structure => false,
    };
    let action = if action_zero {
        ReleaseAction::Zero
    } else if hover.is_some_and(|target| {
        sim.unit_repair_action(best_id, target.stable_id, issuing_house, rules)
            == Some(crate::sim::world::UnitRepairAction::Select)
    }) {
        ReleaseAction::Select
    } else {
        ReleaseAction::Other
    };
    left_release(sim, rules, action, hover.map(|target| target.stable_id))
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
    // Ctrl+Shift = attack move, Ctrl+Alt = guard area. Shift alone only makes a
    // Cell's What_Action answer Move — see `OrderModifier` for the full
    // derivation.
    let mut modifier = resolve_order_modifiers(
        is_ctrl_held(state),
        is_shift_held(state),
        is_alt_held(state),
    );
    // The object receiver 4D74E0 reads the force-move key itself
    // (`0x004D79EC..0x004D7A14`), whatever chord it is part of.
    let force_move_key = is_alt_held(state);
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
    let mut consumed_attack_action = false;

    if let Some(rt) = state.match_state.sim_runtime.as_mut() {
        let resources = &rt.resources;
        let sim = &mut rt.simulation;
        let execute_tick = sim.session.tick;
        if selected_ids.is_empty() {
            return false;
        }
        speaker_id = selected_ids.first().copied();
        // `CurrentObjects`: DetermineAction and the gates that read the
        // selection's size see every selected object. Orders go only to the
        // objects whose ClickedAction can still send one.
        let current_objects = selected_ids;
        let selected_ids = sim.click_order_receivers(&current_objects);

        let mut selected_units: Vec<u64> = Vec::new();
        let mut selected_miner_ids: Vec<u64> = Vec::new();

        for &sid in &selected_ids {
            let Some(entity) = sim.entities().get(sid) else {
                continue;
            };
            if entity.category() != EntityCategory::Structure {
                selected_units.push(sid);
                if entity.miner.is_some() {
                    selected_miner_ids.push(sid);
                }
            }
        }
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
        let cell_is_shrouded = crate::app::presentation::instances::cell_visibility_for_local_owner(
            Some(owner_id),
            Some(&sim.fog),
            target_rx,
            target_ry,
            state.match_state.sandbox_full_visibility,
            sim.resolved_terrain.as_ref(),
        )
            == crate::app::presentation::instances::CellVisibilityState::Shrouded;
        // The chord test fails — and the order resolves normally — unless every
        // selected object can accept an attack-move order. The walk covers the
        // whole selection, so a selected building or aircraft kills the chord.
        if modifier == OrderModifier::AttackMove
            && !selection_can_attack_move(sim, Some(&resources.rules), &current_objects)
        {
            modifier = OrderModifier::Normal;
        }
        // Techno What_Action answers Self for the lone selected object clicking
        // itself (`0x007000B9..0x007000E8`) before Alt's force-move rung
        // (`0x007001E9..0x00700217`), so Alt leaves that click a plain one.
        if modifier == OrderModifier::ForceMove
            && hover
                .as_ref()
                .is_some_and(|target| current_objects == [target.stable_id])
        {
            modifier = OrderModifier::Normal;
        }
        let shift_click: bool = modifier == OrderModifier::Shift;
        let force_fire: bool = modifier == OrderModifier::ForceFire;
        let force_move: bool = modifier == OrderModifier::ForceMove;
        // Force fire, force move and guard area skip the capability branches
        // below (miner return, garrison, C4, engineer capture, depot, bunker,
        // self-click deploy). RESIDUAL (#605): native class What_Action does
        // not always let the key replace them. Infantry `0x0051E3B0` reads
        // Shift and Ctrl but not Alt, and its Engineer rungs over a capturable
        // Building (`0x0051E49E..0x0051E5E5`) answer without the base action,
        // so Alt does not change them natively. The attack-move chord does not
        // skip them: it cancels both its modifiers before the action resolves
        // and only promotes the *committed* mission afterwards, so a chorded
        // click on a capturable building still captures.
        let context_actions_enabled: bool = matches!(
            modifier,
            OrderModifier::Normal | OrderModifier::Shift | OrderModifier::AttackMove
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

        if selected_ids.is_empty() {
            return match berserk_selection_click(
                sim,
                &resources.rules,
                &current_objects,
                hover.as_ref(),
                (target_rx, target_ry),
                owner_id,
            ) {
                LeftRelease::Dispatch => finish_order(state, queued, speaker_id),
                LeftRelease::Nothing => true,
                LeftRelease::Select => false,
            };
        }

        let mut selected_ids = selected_ids;
        let repair_dispatch = hover.as_ref().map(|target| {
            unit_repair_branch_orders(
                sim,
                &resources.rules,
                &current_objects,
                &selected_ids,
                target,
                owner_id,
                modifier,
                order_mode,
                force_move_key,
            )
        });
        if let Some(repair_dispatch) = repair_dispatch {
            consumed_attack_action |= repair_dispatch.consumed;
            queued.extend(
                repair_dispatch
                    .commands
                    .into_iter()
                    .map(|payload| CommandEnvelope::new(owner_id, execute_tick, payload)),
            );
            selected_units.retain(|id| !repair_dispatch.receivers.contains(id));
            selected_miner_ids.retain(|id| !repair_dispatch.receivers.contains(id));
            selected_ids.retain(|id| !repair_dispatch.receivers.contains(id));
            if selected_ids.is_empty() {
                let sent = finish_order(state, queued, speaker_id);
                return sent || (consumed_attack_action && !repair_dispatch.select_fallback);
            }
        }

        // Attack-capable Ctrl receivers, including fixed/occupied buildings,
        // resolve the same action as the cursor before the legacy rally/mobile
        // split. Non-attack capabilities keep their existing handlers below.
        let attack_target = hover
            .as_ref()
            .map_or(TargetKind::Cell(target_rx, target_ry), |target| {
                TargetKind::Entity(target.stable_id)
            });
        let enemy_object = hover.as_ref().is_some_and(|target| {
            matches!(
                target.kind,
                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
            )
        });
        let cell_hidden =
            force_fire && matches!(attack_target, TargetKind::Cell(..)) && cell_is_shrouded;
        let attack_dispatch =
            if !cell_hidden && (force_fire || (enemy_object && context_actions_enabled)) {
                attack_branch_orders(
                    sim,
                    &resources.rules,
                    &selected_ids,
                    attack_target,
                    force_fire,
                    &resources.overlay_registry,
                )
            } else {
                AttackDispatch {
                    receivers: std::collections::BTreeSet::new(),
                    commands: Vec::new(),
                    consumed: false,
                    select_fallback: false,
                }
            };
        consumed_attack_action |= attack_dispatch.consumed;
        queued.extend(
            attack_dispatch
                .commands
                .into_iter()
                .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
        );
        let attack_receivers = attack_dispatch.receivers;
        let select_fallback = attack_dispatch.select_fallback;
        // A blocked stationary actor cannot suppress another selected actor's
        // accepted attack. If only Action7 remains, allow click-selection;
        // Action0 instead consumes the click without issuing a move/attack.
        let selected_ids: Vec<_> = selected_ids
            .into_iter()
            .filter(|id| !attack_receivers.contains(id))
            .collect();
        if selected_ids.is_empty() {
            let sent = finish_order(state, queued, speaker_id);
            return sent || (consumed_attack_action && !select_fallback);
        }
        selected_units.retain(|id| !attack_receivers.contains(id));
        selected_miner_ids.retain(|id| !attack_receivers.contains(id));
        let mobile_count = selected_units.len();
        let structure_selected = selected_ids.iter().any(|&id| {
            sim.entities()
                .get(id)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        });

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
        // Each miner's own What_Action over the refinery
        // (`Selection__DispatchMultiUnitOrder` object loop): Enter only when
        // the refinery's CAN_LOAD admits it (`0x0074017E`). A busy, unpowered
        // or selling refinery leaves Select, and the click selects.
        let entering_miners: Vec<u64> = clicked_friendly_refinery_id.map_or_else(Vec::new, |id| {
            selected_miner_ids
                .iter()
                .copied()
                .filter(|&miner| {
                    sim.unit_building_click_action(miner, id, &resources.rules)
                        == Some(crate::sim::world::BuildingClickAction::Enter)
                })
                .collect()
        });
        let clicked_friendly_refinery = !entering_miners.is_empty();

        // Check if the clicked cell holds tiberium (ore/gems). The Unit
        // harvest arms run only over a Cell the viewer has mapped
        // (`0x007405DB..0x007405E6`); over shroud a miner moves. They are
        // the Cell's What_Action, which DetermineAction asks only with no
        // object under the cursor (`0x0069261B..0x00692655`).
        let clicked_ore = cell_context_enabled
            && hover
                .as_ref()
                .is_none_or(|target| target.kind == HoverTargetKind::HiddenEnemy)
            && sim.clicked_cell_is_shrouded(owner_id, (target_rx, target_ry)) != Ok(true)
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
            for stable_id in entering_miners {
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
                        Some(&resources.overlay_registry),
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    ) else {
                        continue;
                    };
                    queued.push(CommandEnvelope::new(
                        owner_id,
                        execute_tick,
                        Command::Move {
                            entity_id: stable_id,
                            target_rx: goal.0,
                            target_ry: goal.1,
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
                    &current_objects,
                    hover.as_ref(),
                )
            {
                queued.push(CommandEnvelope::new(owner_id, execute_tick, cmd));
                return finish_order(state, queued, speaker_id);
            }
            if select_friendly_clicks && clicked_friendly && context_actions_enabled {
                return if consumed_attack_action {
                    finish_order(state, queued, speaker_id)
                } else {
                    false
                };
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
                                sim.interner
                                    .object_type(entity.type_ref(), &resources.rules)
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
                            Some(&resources.overlay_registry),
                            owner_id,
                            stable_id,
                            (target_rx, target_ry),
                            shift_click,
                        ) else {
                            continue;
                        };
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Move {
                                entity_id: stable_id,
                                target_rx: goal.0,
                                target_ry: goal.1,
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
                            Command::Attack {
                                attacker_id,
                                target_id: target.stable_id,
                            },
                        ));
                    }
                    selected_units.retain(|id| !engineers.contains(id));
                    if friendly || selected_units.is_empty() {
                        return finish_order(state, queued, speaker_id);
                    }
                }
            }

            // Engineers have the first building action. The infiltrator
            // producer checks intervening garrison/C4 actions per actor before
            // consuming a late Capture, keeping mixed selections independent.
            //
            // RESIDUAL: force fire skips these producers, but the infiltration
            // arm reads no modifier. Its one action test is base action Attack
            // (0x0051EE62), which force fire keeps. Trigger: a Ctrl+click with
            // a spy on a building it may enter. Effect: VERA's force attack with
            // its MakeupKit where native enters. Rare.
            let mut consumed_building_action = false;
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                for producer in [engineer_capture_orders, infiltrator_capture_orders] {
                    let Some(orders) = producer(sim, &resources.rules, &mut selected_units, target)
                    else {
                        continue;
                    };
                    consumed_building_action = true;
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                }
                if consumed_building_action && selected_units.is_empty() {
                    return finish_order(state, queued, speaker_id);
                }
            }

            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let orders = transport_entry_orders(
                    sim,
                    &resources.rules,
                    &selected_units,
                    target.stable_id,
                );
                if !orders.is_empty() {
                    queued.extend(
                        orders
                            .into_iter()
                            .map(|command| CommandEnvelope::new(owner_id, execute_tick, command)),
                    );
                    return finish_order(state, queued, speaker_id);
                }
            }

            // C4 precedes the late infiltration action (51E9DA before51EE54).
            if context_actions_enabled && let Some(target) = hover.as_ref() {
                let c4_attackers: Vec<_> = selected_units
                    .iter()
                    .copied()
                    .filter(|&id| {
                        infantry_c4_action(sim, &resources.rules, id, target)
                            == Some(InfantryC4Action::Sabotage)
                    })
                    .collect();
                if !c4_attackers.is_empty() {
                    for attacker_id in c4_attackers {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::PlantC4 {
                                attacker_id,
                                target_building_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id);
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
                    let obj = sim.interner.object_type(building.type_ref(), rules)?;
                    obj.unit_repair.then_some(target.stable_id)
                });
                if let Some(depot_id) = depot_target {
                    let repair_ids: Vec<u64> = selected_units
                        .iter()
                        .copied()
                        .filter(|&sid| {
                            sim.entities().get(sid).is_some_and(|e| {
                                e.category() == EntityCategory::Unit
                                    && sim
                                        .interner
                                        .object_type(e.type_ref(), &resources.rules)
                                        .is_some_and(|obj| e.health.current() < obj.strength)
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
                    (building.bunker_runtime.is_some() && building.bunker.occupant().is_none())
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
                    &current_objects,
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
                let ivans = friendly_bomb_ivans(
                    sim,
                    &resources.rules,
                    &selected_units,
                    current_objects.len(),
                    target.stable_id,
                    Some(&resources.overlay_registry),
                );
                if cursor_owner(sim, &resources.rules, &selected_ids, target.stable_id)
                    .is_some_and(|best| ivans.contains(&best))
                {
                    for attacker_id in ivans {
                        queued.push(CommandEnvelope::new(
                            owner_id,
                            execute_tick,
                            Command::Attack {
                                attacker_id,
                                target_id: target.stable_id,
                            },
                        ));
                    }
                    return finish_order(state, queued, speaker_id);
                }
            }
            // Ordinary Unit What_Action distinguishes a different allied
            // House from an own object. Preserve Shift's earlier toggle-select
            // answer and the existing other-class capability routes. The
            // shared release below, not display friendliness, decides Select
            // versus dispatch for this Unit route.
            let friendly_unit_action = clicked_friendly
                && hover.as_ref().is_some_and(|target| {
                    cursor_owner(sim, &resources.rules, &current_objects, target.stable_id)
                        .and_then(|id| sim.entities().get(id))
                        .is_some_and(|actor| actor.category() == EntityCategory::Unit)
                });
            if select_friendly_clicks
                && clicked_friendly
                && context_actions_enabled
                && (!friendly_unit_action
                    || modifier == OrderModifier::Shift
                    || consumed_building_action
                    || consumed_attack_action)
            {
                return if consumed_building_action || consumed_attack_action {
                    finish_order(state, queued, speaker_id)
                } else {
                    false
                };
            }

            // Alt over an object: Techno's force-move rung answers Move for a
            // player's controllable receiver (`0x007001E9..0x00700217`), so
            // nothing under the cursor is a target, and the object receiver
            // 4D74E0 orders the object or a Cell near it
            // ([`Simulation::object_move_click`]). Rungs that answer first
            // (Self, handled above) or rewrite it are not asked here (#605):
            // the class rungs noted above, and Foot's shroud rung
            // (`0x004DDF45..0x004DDF82`), which only GameMode 0 reaches. An
            // enemy the player cannot see is no object to click, as for the
            // cursor: the Cell under it takes the click.
            let force_move_target = hover
                .as_ref()
                .filter(|target| force_move && target.kind != HoverTargetKind::HiddenEnemy)
                .map(|target| target.stable_id);

            // Scroll693271..69328B gives the same picked object to action
            // and release. Re-picking with an empty viewer for Ctrl lost the
            // local shroud and sensor policy; retain the original pick.
            let attack_target = hover
                .as_ref()
                .filter(|target| {
                    !force_move
                        && (force_fire
                            || (select_friendly_clicks
                                && friendly_unit_action
                                && matches!(
                                    modifier,
                                    OrderModifier::Normal | OrderModifier::AttackMove
                                ))
                            || matches!(
                                target.kind,
                                HoverTargetKind::EnemyUnit | HoverTargetKind::EnemyStructure
                            ))
                })
                .map(|target| target.stable_id);
            // Foot4DDDE0's hidden-cell wrapper rewrites a nonzero action
            // except GuardArea0x33 to Move when MoveToShroud and the playfield
            // admit it, otherwise NoMove (4DDE8F..4DDEB0). It does not preserve
            // Attack5. The ordinary MTNK fall-through below takes Move; other
            // class/shroud combinations remain part of the shared-cell audit.
            if let Some(target_id) = attack_target {
                if matches!(modifier, OrderModifier::Normal | OrderModifier::AttackMove) {
                    match object_click_release(
                        sim,
                        &resources.rules,
                        &current_objects,
                        &selected_units,
                        target_id,
                        &resources.overlay_registry,
                    ) {
                        LeftRelease::Select => return false,
                        LeftRelease::Nothing => return true,
                        LeftRelease::Dispatch => {}
                    }
                }
                // A dispatched release selects nothing, whatever each
                // receiver sends (`0x004ABE88`).
                consumed_attack_action = true;
            }
            for stable_id in selected_units {
                let payload = if let Some(target_id) = attack_target {
                    let Some(payload) = object_attack_click_command(
                        sim,
                        &resources.rules,
                        &resources.overlay_registry,
                        owner_id,
                        order_mode,
                        modifier,
                        force_move_key,
                        stable_id,
                        target_id,
                    ) else {
                        continue;
                    };
                    payload
                } else if force_fire && !cell_is_shrouded {
                    // Unarmed Cell-click fall-through resolves the original
                    // clicked Cell before committing its Move event.
                    let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal(
                        sim,
                        &resources.rules,
                        Some(&resources.overlay_registry),
                        owner_id,
                        stable_id,
                        (target_rx, target_ry),
                        shift_click,
                    ) else {
                        continue;
                    };
                    Command::Move {
                        entity_id: stable_id,
                        target_rx: goal.0,
                        target_ry: goal.1,
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
                            let destination = match force_move_target {
                                Some(target_id) => {
                                    crate::app::input::commands::object_move_destination(
                                        sim,
                                        &resources.rules,
                                        owner_id,
                                        stable_id,
                                        target_id,
                                        crate::sim::world::MoveAction::Move,
                                        force_move_key,
                                    )
                                }
                                None => crate::app::input::commands::ordinary_cell_move_goal(
                                    sim,
                                    &resources.rules,
                                    Some(&resources.overlay_registry),
                                    owner_id,
                                    stable_id,
                                    (target_rx, target_ry),
                                    shift_click,
                                )
                                .map(|(rx, ry)| TargetKind::Cell(rx, ry)),
                            };
                            let Some(destination) = destination else {
                                continue;
                            };
                            move_click_command(
                                sim,
                                &resources.rules,
                                order_mode,
                                stable_id,
                                destination,
                            )
                        }
                    }
                };
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
    if rally_announce && !state.match_state.input.planning.active() {
        crate::app::input::dispatch::push_local_eva(
            state,
            crate::app::match_runtime::eva_producers::EVA_NEW_RALLY_POINT_ESTABLISHED,
        );
    }
    finish_order(state, queued, speaker_id) || consumed_attack_action
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
    entity.category() == EntityCategory::Unit && object.is_simple_deployer
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
/// is not ported. The factory primary and simple-deployer decisions share
/// their simulation owners with the cursor; other self-hover capabilities
/// remain separate (issue #605).
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
    let obj = sim.interner.object_type(entity.type_ref(), rules);
    if let Some(command) = super::transport_orders::unload_command(entity, obj, Some(local_owner)) {
        return Some(command);
    }
    if entity.category() == EntityCategory::Structure {
        // A tank bunker ejects its unit, and a building that undeploys
        // (a Construction Yard) packs up.
        return if entity.bunker.occupant().is_some() {
            Some(Command::EjectBunker {
                bunker_id: target.stable_id,
            })
        } else if sim.should_show_undeploy_building_command(target.stable_id, rules) {
            Some(Command::UndeployBuilding {
                entity_id: target.stable_id,
            })
        } else if crate::sim::production::can_select_factory_primary(sim, rules, target.stable_id) {
            // Building4472EC preserves Action4 only for a nonprimary
            // factory; Click443450..443494 queues this target's Event17.
            Some(Command::TogglePrimaryFactory {
                entity_id: target.stable_id,
            })
        } else {
            None
        };
    }
    if entity.category() == EntityCategory::Infantry && obj.is_some_and(|o| o.deploy_fire) {
        return Some(Command::ToggleInfantryDeploy {
            entity_id: target.stable_id,
        });
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

/// Object entry orders use the same admission owner as the hover cursor.
/// Infantry What_Action51E668..51E6B1 answers action9 for an admitted
/// garrison; Foot ClickedAction4D7716..4D773A sends Capture8 with NULL
/// Target and the building as Destination. Actual arrival belongs to the
/// existing Capture/PerCell owners, as for computer occupiers (#1400).
fn transport_entry_orders(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    transport_id: u64,
) -> Vec<Command> {
    let garrison = sim
        .entities()
        .get(transport_id)
        .filter(|target| target.category() == EntityCategory::Structure)
        .and_then(|target| sim.interner.object_type(target.type_ref(), rules))
        .is_some_and(|object| object.can_be_occupied);
    selected
        .iter()
        .copied()
        .filter(|&id| crate::sim::passenger::can_enter_transport(sim, rules, id, transport_id))
        .map(|passenger_id| {
            if garrison {
                Command::CaptureBuilding {
                    engineer_id: passenger_id,
                    target_building_id: transport_id,
                }
            } else {
                Command::EnterTransport {
                    passenger_id,
                    transport_id,
                }
            }
        })
        .collect()
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
        if actor.category() != EntityCategory::Infantry
            || !sim
                .interner
                .object_type(actor.type_ref(), rules)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InfantryC4Action {
    Attack,
    Sabotage,
}

/// The type-C4 arm of Infantry What_Action51E9C1..51EB21, after the
/// ordinary enemy-building hover admission. A non-vehicle building
/// terminates this arm even when CanC4/InvisibleInGame leave action Attack;
/// it must not fall through to the later infiltration arm (51EE54).
/// Shared by click dispatch and cursor feedback. Rank-granted C4 remains
/// outside the existing type-C4 input path.
pub(super) fn infantry_c4_action(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    hover: &HoverTargetKindWithId,
) -> Option<InfantryC4Action> {
    if hover.kind != HoverTargetKind::EnemyStructure {
        return None;
    }
    let actor = sim.entities().get(actor_id)?;
    let target = sim.entities().get(hover.stable_id)?;
    if actor.category() != EntityCategory::Infantry
        || target.category() != EntityCategory::Structure
        || !sim.interner.object_type(actor.type_ref(), rules)?.c4
    {
        return None;
    }
    let building = sim.interner.object_type(target.type_ref(), rules)?;
    // Building virtual+80: 457620 -> BuildingType465D40, not invulnerability.
    if building.is_1x1_with_undeploy() {
        return None;
    }
    Some(if building.can_c4 && !building.invisible_in_game {
        InfantryC4Action::Sabotage
    } else {
        InfantryC4Action::Attack
    })
}

/// The selected spies whose action over `hover` is Capture
/// (`Simulation::infiltrator_building_action`): `ClickedAction_Object`
/// (0x0051F190) keeps action 9, and `FootClass::ClickedAction_Object`
/// (0x004D74E0) queues it as it does an Engineer's capture. None when no
/// selected object takes the arm.
fn infiltrator_capture_orders(
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
        if infantry_c4_action(sim, rules, id, hover).is_some()
            || crate::sim::passenger::can_enter_transport(sim, rules, id, hover.stable_id)
            || !sim.infiltrator_building_action(id, hover.stable_id, rules)
        {
            return true;
        }
        consumed = true;
        // Foot4D7716 calls Infantry+A0/Techno700C40 before enqueue.
        if sim.techno_player_controllable(id, rules) {
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
                    e.category() == EntityCategory::Infantry
                        && sim
                            .interner
                            .object_type(e.type_ref(), rules)
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

/// The left release over another house's object: [`left_release`] of the
/// [`attack_action`] of the object that owns the cursor ([`cursor_owner`]),
/// when it is one of the `receivers` the capability arms left. An owner
/// those arms took answered there, receiver by receiver, so the release
/// dispatches.
///
/// RESIDUAL (#605): `attack_branch_orders` decides each receiver's release
/// itself, and consumes a 0 where native selects an object the player may
/// select and has not. Trigger: a selected armed building alone, over
/// another house's object out of its range. Effect: VERA does nothing where
/// native selects the object. Rare.
///
/// RESIDUAL: a release that selects hands the click to the selection
/// picker, which picks again. Both pickers admit what
/// `Tactical::PickObjectAtScreenPoint` admits
/// ([`crate::app::input::entity_pick::pick_admits_object`]), but they break
/// ties apart. Both use ProcessClickCoords' floor-projected shroud gate
/// (692480, spatial_oracle/click_shroud.json).
/// Trigger: two objects under the click point.
/// Effect: the selection takes the other object, or is cleared,
/// where native selects the object DetermineAction saw (one pick for both,
/// `0x00693271` → `0x0069328B`). Rare.
fn object_click_release(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    current_objects: &[u64],
    receivers: &[u64],
    target_id: u64,
    overlay_registry: &crate::rules::overlay_types::OverlayTypeRegistry,
) -> LeftRelease {
    let Some(best) = cursor_owner(sim, rules, current_objects, target_id)
        .filter(|best| receivers.contains(best))
    else {
        return LeftRelease::Dispatch;
    };
    let action = attack_action(
        sim,
        rules,
        best,
        &TargetKind::Entity(target_id),
        false,
        Some(overlay_registry),
    );
    left_release(sim, rules, action.into(), Some(target_id))
}

/// The selected Crazy Ivans whose click on the own or allied `target` plants a
/// bomb. The canonical Techno grant and ILLEGAL test must already answer
/// Attack ([`attack_action`], which the cursor asks too). Infantry51EB24..7E
/// consumes that answer without repeating its type/weapon grants; the Ivan makes it
/// IvanBomb ([`ivan_bomb_refused`], 0x0051EB24), and
/// `FootClass::ClickedAction_Object` (0x004D74E0) sends that as Attack. A
/// bombed friend stays a selection click. A lone selected Ivan over himself
/// has the self action (4) instead; in a larger selection nothing stops him
/// bombing himself, as natively. The count is the whole selection's
/// (`[0xA8ECC8]`), berserk objects included.
fn friendly_bomb_ivans(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    selected: &[u64],
    selection_count: usize,
    target: u64,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> Vec<u64> {
    let bombable = sim.entities().get(target).is_some_and(|entity| {
        let obj = sim.interner.object_type(entity.type_ref(), rules);
        !ivan_bomb_refused(entity, obj)
    });
    if !bombable {
        return Vec::new();
    }
    selected
        .iter()
        .copied()
        .filter(|&id| {
            (id != target || selection_count > 1)
                && sim.entities().get(id).is_some_and(|e| {
                    sim.interner
                        .object_type(e.type_ref(), rules)
                        .is_some_and(|o| o.ivan)
                })
                && attack_action(
                    sim,
                    rules,
                    id,
                    &TargetKind::Entity(target),
                    false,
                    overlay_registry,
                ) == AttackAction::Attack
        })
        .collect()
}

/// `InfantryClass::What_Action_OnObject`'s Ivan arm
/// (`0x0051EB24..0x0051EB7E`): a player's Crazy Ivan (`Ivan=`, `+0xEAE`)
/// whose action is Attack answers NoIvanBomb over an object that carries a
/// bomb (`+0x38`) or is not `Bombable=` (`+0x22E`), else IvanBomb.
pub(crate) fn ivan_bomb_refused(
    target: &crate::sim::game_entity::GameEntity,
    target_obj: Option<&crate::rules::object_type::ObjectType>,
) -> bool {
    target.bomb.is_some() || !target_obj.is_some_and(|o| o.bombable)
}

/// Whether `actor_id` is a Crazy Ivan whose Attack over `target_id` is
/// NoIvanBomb ([`ivan_bomb_refused`]), which
/// `FootClass::ClickedAction_Object` (0x004D74E0) ignores. An unforced click
/// on a bombed target never gets that far: its GetFireError is ILLEGAL
/// (`0x006FCBAD`, [`shot_is_illegal`]), so the Ivan's What_Action answers 0
/// or Select there ([`attack_action`]).
fn ivan_cannot_bomb(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target_id: u64,
) -> bool {
    let is_ivan = sim
        .entities()
        .get(actor_id)
        .and_then(|e| sim.interner.object_type(e.type_ref(), rules))
        .is_some_and(|o| o.ivan);
    is_ivan
        && sim.entities().get(target_id).is_some_and(|target| {
            ivan_bomb_refused(target, sim.interner.object_type(target.type_ref(), rules))
        })
}

/// The ILLEGAL test of `TechnoClass::What_Action_OnObject`'s attack rung
/// (`0x00700536..0x0070055B`): GetFireError (vt+0x3C0, with the selected
/// weapon and range asked; [`Simulation::selected_weapon_fire_error`])
/// answers ILLEGAL (5) for the object. The Infiltrate arm
/// (`0x007004A0..0x00700531`) keeps the rung for an `Infiltrate=`
/// infantryman over a building that is `Capturable=` when he is not
/// `Agent=`, `Spyable=` when he is, or `CanC4=` when he is `C4=`. A Cell
/// target asks no such test.
///
/// Input asks it with the Dummy's coordinate word detached
/// ([`SharedCellDummy::detached`]), so GetFireError's missing-cell lookups
/// stamp a private copy and a cursor or click leaves the simulation's
/// retained Dummy as it was. [`resolved_unit_in_range`] isolates its query
/// for the same reason.
///
/// [`Simulation::selected_weapon_fire_error`]: crate::sim::world::Simulation::selected_weapon_fire_error
/// [`SharedCellDummy::detached`]: crate::map::resolved_terrain::SharedCellDummy::detached
fn shot_is_illegal(
    sim: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    actor_id: u64,
    target: &TargetKind,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let TargetKind::Entity(target_id) = *target else {
        return false;
    };
    let infiltrate_arm = sim
        .entities()
        .get(actor_id)
        .filter(|actor| actor.category() == EntityCategory::Infantry)
        .and_then(|actor| sim.interner.object_type(actor.type_ref(), rules))
        .filter(|infantry| infantry.infiltrate)
        .zip(
            sim.entities()
                .get(target_id)
                .filter(|object| object.category() == EntityCategory::Structure)
                .and_then(|object| sim.interner.object_type(object.type_ref(), rules)),
        )
        .is_some_and(|(infantry, building)| {
            (if infantry.agent {
                building.spyable
            } else {
                building.capturable
            }) || (infantry.c4 && building.can_c4)
        });
    !infiltrate_arm
        && sim.effective_shared_cell_dummy().detached(|| {
            sim.selected_weapon_fire_error(rules, actor_id, *target, true, overlay_registry)
        }) == crate::sim::combat::fire_error::FireError::Illegal
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
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    /// An all-berserk selection's left release follows the best object's
    /// class rung: Infantry NoMove (`0x0051F05E`, cell `0x0051FAAB`), Unit
    /// NoMove over an object (`0x007403EB`) but 0 on the ground
    /// (`0x007404B0`), JumpJet infantry 0 on the ground, and Aircraft 0
    /// (`0x00417D7C`). The best is the nearest berserk object.
    #[test]
    fn an_all_berserk_click_follows_the_best_objects_class() {
        use LeftRelease::{Dispatch, Nothing, Select};
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=GI\n1=JUMPJET\n[VehicleTypes]\n0=MTNK\n\
                 [AircraftTypes]\n0=ORCA\n[GI]\nStrength=125\n\
                 [JUMPJET]\nStrength=125\nJumpJet=yes\n[MTNK]\nStrength=300\n\
                 [ORCA]\nStrength=200\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut spawn = |kind: &str, rx: u16, berserk: bool| {
            let id = sim
                .spawn_object(kind, "Americans", rx, 5, 0, &rules)
                .unwrap();
            sim.entities_mut()
                .get_mut(id)
                .unwrap()
                .berserk
                .set_active_for_test(berserk);
            id
        };
        let gi = spawn("GI", 3, true);
        let jumpjet = spawn("JUMPJET", 4, true);
        let tank = spawn("MTNK", 6, true);
        let orca = spawn("ORCA", 8, true);
        let friend = spawn("GI", 10, false);
        let over_friend = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyUnit,
            stable_id: friend,
        };
        let click = |selection: &[u64], hover: Option<&HoverTargetKindWithId>, cell| {
            berserk_selection_click(&sim, &rules, selection, hover, cell, owner)
        };
        assert_eq!(click(&[gi], None, (3, 5)), Dispatch);
        assert_eq!(click(&[gi], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[jumpjet], None, (3, 5)), Nothing);
        assert_eq!(click(&[jumpjet], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[tank], None, (3, 5)), Nothing);
        assert_eq!(click(&[tank], Some(&over_friend), (3, 5)), Dispatch);
        assert_eq!(click(&[orca], None, (3, 5)), Nothing);
        assert_eq!(click(&[orca], Some(&over_friend), (3, 5)), Select);
        // Equal rank 0: the object nearer the clicked cell decides.
        assert_eq!(click(&[tank, gi], None, (3, 5)), Dispatch);
        assert_eq!(click(&[gi, tank], None, (6, 5)), Nothing);
        // 0 over an object already selected does nothing (`0x004ABE0B`).
        sim.entities_mut()
            .get_mut(friend)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        assert_eq!(
            berserk_selection_click(&sim, &rules, &[orca], Some(&over_friend), (3, 5), owner),
            Nothing
        );
    }

    /// The Ivan's self-bomb gate counts the whole selection (`[0xA8ECC8]`):
    /// a calm Ivan clicked with a berserk escort still bombs himself, though
    /// he is the only object that can send the order.
    #[test]
    fn the_ivan_self_bomb_gate_counts_the_whole_selection() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n[IVAN]\nStrength=100\nIvan=yes\n\
                 AttackCursorOnFriendlies=yes\nBombable=yes\nPrimary=IvanBomber\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Invisible]\nInviso=yes\n[IvanBomb]\nIvanBomb=yes\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let americans = sim.interner.intern("Americans");
        sim.session.house_order.push(americans);
        // The rung is a human player's (`0x007003ED`).
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(americans);
        let ivan = sim
            .spawn_object("IVAN", "Americans", 3, 5, 0, &rules)
            .unwrap();
        assert!(friendly_bomb_ivans(&sim, &rules, &[ivan], 1, ivan, None).is_empty());
        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &[ivan], 2, ivan, None),
            [ivan]
        );
    }

    /// Role-derived Infiltrate must preserve native C4-before-Capture order,
    /// including terminal ordinary Attack when the building refuses C4.
    #[test]
    fn specialist_infiltration_keeps_c4_click_and_cursor_priority() {
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=GHOST\n1=SPY\n[BuildingTypes]\n0=PLANT\n1=BUNK\n2=FOLD\n[VehicleTypes]\n0=TRUCK\n\
                 [GHOST]\nStrength=100\nC4=yes\nPrimary=Rifle\n\
                 [SPY]\nStrength=100\nAgent=yes\n\
                 [PLANT]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=yes\n\
                 [BUNK]\nStrength=750\nCapturable=yes\nSpyable=yes\nCanC4=no\n\
                 [FOLD]\nStrength=750\nCapturable=yes\nSpyable=yes\nUndeploysInto=TRUCK\n\
                 [TRUCK]\nStrength=100\n\
                 [Rifle]\nDamage=10\nRange=8\nProjectile=Invisible\nWarhead=AP\n\
                 [Invisible]\nInviso=yes\n\
                 [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            ),
        ).unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (name, human) in [("Americans", true), ("Soviets", false)] {
            let owner = sim.interner.intern(name);
            sim.houses.insert(
                owner,
                crate::sim::house_state::HouseState::new(owner, 0, None, human, 0, 10),
            );
            sim.session.house_order.push(owner);
        }
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = sim.interner.get("Americans");
        let seal = sim
            .spawn_object("GHOST", "Americans", 3, 5, 0, &rules)
            .unwrap();
        let spy = sim
            .spawn_object("SPY", "Americans", 4, 5, 0, &rules)
            .unwrap();
        let plant = sim
            .spawn_object("PLANT", "Soviets", 6, 5, 0, &rules)
            .unwrap();
        let bunker = sim
            .spawn_object("BUNK", "Soviets", 8, 5, 0, &rules)
            .unwrap();
        let fold = sim
            .spawn_object("FOLD", "Soviets", 10, 5, 0, &rules)
            .unwrap();
        assert!(rules.object("GHOST").unwrap().infiltrate);
        assert!(rules.object("SPY").unwrap().infiltrate);
        for (target, expected_action, expected_cursor) in [
            (
                plant,
                InfantryC4Action::Sabotage,
                crate::app::types::CursorFeedbackKind::Demolish,
            ),
            (
                bunker,
                InfantryC4Action::Attack,
                crate::app::types::CursorFeedbackKind::EnemyStructure,
            ),
        ] {
            let hover = HoverTargetKindWithId {
                kind: HoverTargetKind::EnemyStructure,
                stable_id: target,
            };
            assert!(sim.infiltrator_building_action(seal, target, &rules));
            let mut selected = vec![seal, spy];
            assert_eq!(
                infiltrator_capture_orders(&sim, &rules, &mut selected, &hover),
                Some(vec![Command::CaptureBuilding {
                    engineer_id: spy,
                    target_building_id: target
                }])
            );
            assert_eq!(selected, [seal]);
            assert_eq!(
                infantry_c4_action(&sim, &rules, seal, &hover),
                Some(expected_action)
            );
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[seal],
                    Some(seal),
                    &hover,
                    Some(&rules),
                    None
                ),
                expected_cursor
            );
        }
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyStructure,
            stable_id: fold,
        };
        assert!(rules.object("FOLD").unwrap().is_1x1_with_undeploy());
        assert_eq!(infantry_c4_action(&sim, &rules, seal, &hover), None);
        assert_eq!(
            infiltrator_capture_orders(&sim, &rules, &mut vec![seal], &hover),
            Some(vec![Command::CaptureBuilding {
                engineer_id: seal,
                target_building_id: fold
            }])
        );
        assert_eq!(
            crate::app::input::cursor::capability_cursor_for_hover(
                &sim,
                &[seal],
                Some(seal),
                &hover,
                Some(&rules),
                None
            ),
            crate::app::types::CursorFeedbackKind::Enter
        );
    }

    /// A spy's click takes its Capture order and leaves the other selected
    /// actors for their own actions. An inadmissible building consumes none.
    #[test]
    fn a_spy_click_on_a_building_it_may_enter_captures() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=SPY\n1=GI\n[BuildingTypes]\n0=PLANT\n1=DEPOT\n\
                 [SPY]\nStrength=100\nInfiltrate=yes\nAgent=yes\n\
                 [GI]\nStrength=125\n\
                 [PLANT]\nStrength=750\nSpyable=yes\n[DEPOT]\nStrength=750\nCapturable=yes\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (house, human) in [("Americans", true), ("Russians", false)] {
            let id = sim.interner.intern(house);
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, 0, None, human, 0, 10),
            );
            sim.session.house_order.push(id);
        }
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = sim.interner.get("Americans");
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let spy = spawn("SPY", "Americans", 3);
        let gi = spawn("GI", "Americans", 4);
        let plant = spawn("PLANT", "Russians", 10);
        let depot = spawn("DEPOT", "Russians", 14);
        let hover = |stable_id| HoverTargetKindWithId {
            kind: HoverTargetKind::EnemyStructure,
            stable_id,
        };

        let mut selected = vec![gi, spy];
        assert_eq!(
            infiltrator_capture_orders(&sim, &rules, &mut selected, &hover(plant)),
            Some(vec![Command::CaptureBuilding {
                engineer_id: spy,
                target_building_id: plant,
            }])
        );
        assert_eq!(selected, [gi]);
        let mut selected = vec![spy];
        assert_eq!(
            infiltrator_capture_orders(&sim, &rules, &mut selected, &hover(depot)),
            None,
            "a building that is not Spyable="
        );
        assert_eq!(selected, [spy]);
    }

    /// `TechnoClass::What_Action_OnObject @ 0x00700548`: unforced, the
    /// attack rung answers Attack only while the actor's GetFireError for the
    /// object is not ILLEGAL; otherwise the tail answers, and a player's
    /// Infantry turns its 0 into NoMove. Force fire keeps the rung
    /// (`0x00700555`). Out of range is RANGE (T61, the last test), not
    /// ILLEGAL. A Crazy Ivan's bombed target is ILLEGAL at any distance (T55,
    /// `0x006FCBAD`, comes first). ClickedMission promotes only an Attack
    /// that passed, so an attack-move click needs it too.
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
        // The attack rung is a human player's (`0x007003ED`).
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = sim.interner.get("Americans");
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let tank = spawn("HTNK", "Americans", 3);
        let gi = spawn("GI", "Americans", 4);
        let ivan = spawn("IVAN", "Americans", 5);
        let near = spawn("HTNK", "Soviets", 6);
        let far = spawn("HTNK", "Soviets", 20);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |sim: &Simulation, actor: u64, target: u64, forced: bool| {
            attack_action(
                sim,
                &rules,
                actor,
                &TargetKind::Entity(target),
                forced,
                Some(&overlays),
            )
        };

        assert_eq!(
            action(&sim, gi, near, false),
            AttackAction::NoMove,
            "0% Verses against heavy armour"
        );
        assert_eq!(action(&sim, gi, near, true), AttackAction::Attack);
        assert_eq!(action(&sim, tank, near, false), AttackAction::Attack);
        assert_eq!(
            action(&sim, tank, far, false),
            AttackAction::Attack,
            "out of range"
        );
        assert_eq!(action(&sim, ivan, near, false), AttackAction::Attack);
        let americans = sim.interner.get("Americans").unwrap();
        assert_eq!(
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                americans,
                OrderMode::AttackMove,
                OrderModifier::Normal,
                false,
                tank,
                near,
            ),
            Some(Command::AttackMove {
                entity_id: tank,
                target: Some(TargetKind::Entity(near)),
                destination: None,
            })
        );

        sim.bomb_attach(ivan, Some(near), &rules);
        sim.bomb_attach(ivan, Some(far), &rules);
        assert_eq!(
            action(&sim, ivan, near, false),
            AttackAction::NoMove,
            "a bombed target"
        );
        assert_eq!(
            action(&sim, ivan, far, false),
            AttackAction::NoMove,
            "the bomb gate precedes the range test"
        );
    }

    /// The Infiltrate arm keeps an unforced ILLEGAL shot on the rung
    /// (`0x007004A0..0x00700531`): an `Agent=` infantryman over a `Spyable=`
    /// building, another over a `Capturable=` one, and a `C4=` one over a
    /// building that is `CanC4=` (a building's default). Any other building
    /// takes the tail. The reader also enables Infiltrate for C4, Engineer
    /// and Agent types without an explicit Infiltrate key.
    #[test]
    fn the_infiltrate_arm_keeps_an_illegal_shot_on_the_rung() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=SPY\n1=ENGI\n2=SEAL\n\
                 [BuildingTypes]\n0=LAB\n1=DEPOT\n2=BUNK\n\
                 [SPY]\nStrength=100\nInfiltrate=yes\nAgent=yes\nPrimary=Pea\n\
                 [ENGI]\nStrength=100\nInfiltrate=yes\nPrimary=Pea\n\
                 [SEAL]\nStrength=100\nInfiltrate=yes\nC4=yes\nPrimary=Pea\n\
                 [LAB]\nStrength=750\nArmor=heavy\nSpyable=yes\n\
                 [DEPOT]\nStrength=750\nArmor=heavy\nCapturable=yes\n\
                 [BUNK]\nStrength=750\nArmor=heavy\nCanC4=no\n\
                 [Pea]\nDamage=10\nRange=5\nProjectile=Invisible\nWarhead=Soft\n\
                 [Invisible]\nInviso=yes\n\
                 [Soft]\nVerses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (house, human) in [("Americans", true), ("Soviets", false)] {
            let id = sim.interner.intern(house);
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, 0, None, human, 0, 10),
            );
            sim.session.house_order.push(id);
        }
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = sim.interner.get("Americans");
        let mut spawn = |kind: &str, owner: &str, rx: u16, ry: u16| {
            sim.spawn_object(kind, owner, rx, ry, 0, &rules).unwrap()
        };
        let spy = spawn("SPY", "Americans", 4, 5);
        let engineer = spawn("ENGI", "Americans", 5, 5);
        let seal = spawn("SEAL", "Americans", 5, 8);
        let lab = spawn("LAB", "Soviets", 7, 5);
        let depot = spawn("DEPOT", "Soviets", 7, 8);
        let bunker = spawn("BUNK", "Soviets", 10, 8);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |actor, target| {
            attack_action(
                &sim,
                &rules,
                actor,
                &TargetKind::Entity(target),
                false,
                Some(&overlays),
            )
        };
        assert_eq!(action(spy, lab), AttackAction::Attack);
        assert_eq!(action(spy, depot), AttackAction::NoMove);
        assert_eq!(action(engineer, depot), AttackAction::Attack);
        assert_eq!(action(engineer, lab), AttackAction::NoMove);
        assert_eq!(action(seal, lab), AttackAction::Attack);
        assert_eq!(action(seal, bunker), AttackAction::NoMove);
    }

    /// A shot ILLEGAL to an armed receiver leaves its What_Action 0
    /// (`0x0070056C..0x007005E6`), which a player's Infantry or Unit it may
    /// control turns into NoMove (`0x0051F05E..0x0051F08B`,
    /// `0x007403EB..0x00740414`): the click moves it near the object, as an
    /// attack-move under the chord, and the cursor shows NoMove. A retail
    /// Crazy Ivan's bombed target is ILLEGAL to him (`0x006FCBAD`), and a
    /// retail Grizzly is ILLEGAL to an attack dog.
    #[test]
    fn a_refused_shot_moves_the_receiver_near_the_object() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner.get("Local").unwrap();
        let ivan = sim.spawn_object("IVAN", "Local", 6, 4, 0, &rules).unwrap();
        let dog = sim.spawn_object("DOG", "Local", 5, 4, 0, &rules).unwrap();
        let tank = sim.spawn_object("MTNK", "Enemy", 6, 6, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        sim.fog = crate::sim::vision::FogState::new(16, 16);
        sim.fog
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        sim.bomb_attach(ivan, Some(tank), &rules);
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        for actor in [ivan, dog] {
            assert_eq!(
                attack_action(
                    &sim,
                    &rules,
                    actor,
                    &TargetKind::Entity(tank),
                    false,
                    Some(&overlays)
                ),
                AttackAction::NoMove
            );
            // The cursor shows that NoMove too.
            assert_eq!(
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[actor],
                    Some(actor),
                    &HoverTargetKindWithId {
                        kind: HoverTargetKind::EnemyUnit,
                        stable_id: tank,
                    },
                    Some(&rules),
                    None,
                ),
                crate::app::types::CursorFeedbackKind::Invalid
            );
        }
        let click = |order_mode| {
            object_attack_click_command(
                &sim,
                &rules,
                &overlays,
                owner,
                order_mode,
                OrderModifier::Normal,
                false,
                ivan,
                tank,
            )
        };
        let near = crate::app::input::commands::object_move_destination(
            &sim,
            &rules,
            owner,
            ivan,
            tank,
            crate::sim::world::MoveAction::NoMove,
            false,
        );
        let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
            panic!("NoMove searches near the tank: {near:?}");
        };
        assert_eq!(
            click(OrderMode::Move),
            Some(Command::Move {
                entity_id: ivan,
                target_rx,
                target_ry,
            })
        );
        assert_eq!(
            click(OrderMode::AttackMove),
            Some(Command::AttackMove {
                entity_id: ivan,
                target: None,
                destination: near,
            })
        );
    }

    /// An unarmed object's What_Action over another house's object skips the
    /// attack rung (`0x00700464..0x00700474`) for the tail
    /// (`0x0070059D..0x007005E6`): Select over an object the player may
    /// select and has not, else 0, which a player's Unit it may control
    /// turns into NoMove (`0x007403EB`). A loaner the player owns answers 0.
    /// The click selects the object when the object that owns the cursor
    /// answers Select (`0x004ABDD3..0x004ABDDA`), and an armed object
    /// outranks unarmed ones (`0x00535435..0x0053549C`); NoMove moves the
    /// receiver near the object. Retail `[CMIN]` (`Primary=none`) and
    /// `[AMCV]` are unarmed. Another house's armed object asks no rung
    /// (`0x007003ED`) and no owner test, so it answers Select.
    #[test]
    fn an_unarmed_receiver_selects_another_houses_object() {
        use crate::sim::world::MoveAction;
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let owner = sim.interner.get("Local").unwrap();
        let miner = sim.spawn_object("CMIN", "Local", 2, 4, 0, &rules).unwrap();
        let mcv = sim.spawn_object("AMCV", "Local", 2, 6, 0, &rules).unwrap();
        let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
        let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        sim.fog = crate::sim::vision::FogState::new(16, 16);
        sim.fog
            .reveal_cells_for_owner(owner, (0..16).flat_map(|y| (0..16).map(move |x| (x, y))));
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::empty();
        let action = |sim: &Simulation, actor| {
            attack_action(sim, &rules, actor, &TargetKind::Entity(enemy), false, None)
        };
        let release = |sim: &Simulation, selection: &[u64]| {
            object_click_release(sim, &rules, selection, selection, enemy, &overlays)
        };
        let click = |sim: &Simulation, actor| {
            object_attack_click_command(
                sim,
                &rules,
                &overlays,
                owner,
                OrderMode::Move,
                OrderModifier::Normal,
                false,
                actor,
                enemy,
            )
        };
        let cursor = |sim: &Simulation, actor| {
            crate::app::input::cursor::capability_cursor_for_hover(
                sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind: HoverTargetKind::EnemyUnit,
                    stable_id: enemy,
                },
                Some(&rules),
                None,
            )
        };

        assert_eq!(action(&sim, miner), AttackAction::Select);
        assert_eq!(action(&sim, mcv), AttackAction::Select);
        assert_eq!(action(&sim, tank), AttackAction::Attack);
        assert_eq!(
            cursor(&sim, miner),
            crate::app::types::CursorFeedbackKind::FriendlyUnit
        );
        assert_eq!(release(&sim, &[miner, mcv]), LeftRelease::Select);
        assert_eq!(
            release(&sim, &[miner, tank]),
            LeftRelease::Dispatch,
            "the armed tank owns the cursor"
        );
        assert_eq!(
            object_click_release(&sim, &rules, &[mcv], &[], enemy, &overlays),
            LeftRelease::Dispatch,
            "a capability arm took the object that owns the cursor"
        );
        assert_eq!(click(&sim, mcv), None);

        sim.entities_mut()
            .get_mut(enemy)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        assert_eq!(action(&sim, mcv), AttackAction::NoMove);
        assert_eq!(
            cursor(&sim, mcv),
            crate::app::types::CursorFeedbackKind::Invalid
        );
        assert_eq!(release(&sim, &[mcv]), LeftRelease::Dispatch);
        let near = crate::app::input::commands::object_move_destination(
            &sim,
            &rules,
            owner,
            mcv,
            enemy,
            MoveAction::NoMove,
            false,
        );
        let Some(TargetKind::Cell(target_rx, target_ry)) = near else {
            panic!("NoMove searches near the tank: {near:?}");
        };
        assert_eq!(
            click(&sim, mcv),
            Some(Command::Move {
                entity_id: mcv,
                target_rx,
                target_ry,
            })
        );
        sim.entities_mut()
            .get_mut(enemy)
            .unwrap()
            .selection
            .set_selected_for_test(false);

        let loaner = sim.entities_mut().get_mut(mcv).unwrap();
        loaner.mission_only.mark();
        loaner.discovery.set_owned_by_current_house_for_test(true);
        assert_eq!(action(&sim, mcv), AttackAction::NoMove);

        assert_eq!(
            attack_action(&sim, &rules, enemy, &TargetKind::Entity(tank), false, None),
            AttackAction::Select
        );
    }

    /// What_Action_OnCell's force-fire rung is a human player's too
    /// (`0x007008A6..0x007008B3`): another house's armed object, which the
    /// player may select to inspect, answers 0 over a cell in its range
    /// (`0x00700AB7..0x00700AC4`), where the player's answers Attack.
    #[test]
    fn force_fire_on_a_cell_is_a_human_players() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let (mut sim, rules, _) = Simulation::walk_cell_input_test_scene_with(retail.rules, false);
        let tank = sim.spawn_object("MTNK", "Local", 4, 4, 0, &rules).unwrap();
        let enemy = sim.spawn_object("MTNK", "Enemy", 6, 5, 0, &rules).unwrap();
        sim.resolve_type_handles(&rules);
        let over_cell =
            |actor| armed_action(&sim, &rules, actor, &TargetKind::Cell(5, 6), true, None);
        assert_eq!(over_cell(tank), Some(AttackAction::Attack));
        assert_eq!(over_cell(enemy), Some(AttackAction::None));
    }

    /// An unarmed `Infiltrate=` infantryman keeps the attack rung
    /// (`0x00700444..0x00700474`), but his class turns its Attack into 0 over
    /// an object he may not infiltrate (`0x0051F04E..0x0051F05C`), and its
    /// tail turns 0 into NoMove (`0x0051F05E..0x0051F08B`). Retail's SPY
    /// carries its MakeupKit, so only modded data reaches this.
    #[test]
    fn an_unarmed_infiltrator_moves_near_an_object_it_may_not_enter() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=AGENT\n[VehicleTypes]\n0=MTNK\n\
                 [AGENT]\nStrength=100\nInfiltrate=yes\nAgent=yes\n[MTNK]\nStrength=300\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for (house, human) in [("Americans", true), ("Russians", false)] {
            let id = sim.interner.intern(house);
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, 0, None, human, 0, 10),
            );
            sim.session.house_order.push(id);
        }
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = sim.interner.get("Americans");
        let agent = sim
            .spawn_object("AGENT", "Americans", 3, 5, 0, &rules)
            .unwrap();
        let tank = sim
            .spawn_object("MTNK", "Russians", 6, 5, 0, &rules)
            .unwrap();
        assert_eq!(
            attack_action(&sim, &rules, agent, &TargetKind::Entity(tank), false, None),
            AttackAction::NoMove
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
                    Health::new(300),
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
            OrderModifier::Shift
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
            OrderModifier::Shift
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
            }),
            Some("VoiceMove")
        );
        // Attack-move commits through the Move voice slot, not the Attack one.
        assert_eq!(
            order_voice_key(&Command::AttackMove {
                entity_id: 1,
                target: None,
                destination: Some(crate::sim::combat::TargetKind::Cell(0, 0)),
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

    #[test]
    fn capture_orders_dispatch_the_capture_voice_key() {
        assert_eq!(
            order_voice_key(&Command::CaptureBuilding {
                engineer_id: 1,
                target_building_id: 2,
            }),
            Some("VoiceCapture")
        );
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
                Health::new(300),
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

    /// A chorded click on an enemy *object* attack-moves with the object as its
    /// target, because ClickedMission promotes a committed Attack just as it
    /// promotes a committed Move (`0x006FFC09..0x006FFC1E`). The promotion is
    /// per object: a member whose type refuses attack-move still commits the
    /// plain attack.
    #[test]
    fn chorded_click_on_an_enemy_object_attack_moves() {
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::Normal, true, 1, 2),
            Command::AttackMove {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
                destination: None,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::Normal, false, 1, 2),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        // Plain click, force fire and guard area are untouched by the promotion.
        assert_eq!(
            object_click_payload(OrderMode::Move, OrderModifier::Normal, true, 1, 2),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::AttackMove, OrderModifier::ForceFire, true, 1, 2),
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            }
        );
        assert_eq!(
            object_click_payload(OrderMode::Move, OrderModifier::GuardArea, true, 1, 2),
            Command::Guard {
                entity_id: 1,
                target: Some(TargetKind::Entity(2)),
            }
        );
    }

    /// The attack-move promotion keeps a Move click's destination token
    /// (`0x006FFDA5..0x006FFDE6`), so an object stays the destination; a
    /// receiver whose type refuses attack-move commits the plain Move.
    #[test]
    fn attack_move_keeps_the_move_click_destination() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=AMCV\n\
                 [E1]\nStrength=125\nPrimary=M60\n\
                 [AMCV]\nStrength=1000\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let gi = sim
            .spawn_object("E1", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let mcv = sim
            .spawn_object("AMCV", "Americans", 9, 5, 0, &rules)
            .unwrap();
        let command =
            |id, mode, destination| move_click_command(&sim, &rules, mode, id, destination);

        for destination in [TargetKind::Entity(mcv), TargetKind::Cell(12, 5)] {
            assert_eq!(
                command(gi, OrderMode::AttackMove, destination),
                Command::AttackMove {
                    entity_id: gi,
                    target: None,
                    destination: Some(destination),
                }
            );
        }
        assert_eq!(
            command(mcv, OrderMode::AttackMove, TargetKind::Entity(gi)),
            Command::MoveToObject {
                entity_id: mcv,
                target_id: gi,
            }
        );
        assert_eq!(
            command(gi, OrderMode::Move, TargetKind::Cell(12, 5)),
            Command::Move {
                entity_id: gi,
                target_rx: 12,
                target_ry: 5,
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

    /// Building447210 keeps Action4 for a lone selected own nonprimary
    /// factory. Click443410 then queues TargetEvent17 for that same building;
    /// its receiver toggles Building448160. This covers the input boundary,
    /// with existing FindFactory proving the initial primary independently.
    #[test]
    fn retail_nonprimary_war_factory_self_click_queues_its_primary_selection() {
        use crate::rules::object_type::FactoryType;
        use crate::sim::house_state::HouseState;

        let Some(battle) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let rules = &battle.rules;
        let factory = rules.object("GAWEAP").expect("stock Allied War Factory");
        assert_eq!(factory.factory_type(), Some(FactoryType::UnitType));
        assert!(!factory.naval && !factory.invisible_in_game);
        let mut sim = Simulation::with_seed(0x0044_8160);
        sim.intern_rule_type_ids(rules);
        sim.resolve_type_handles(rules);
        sim.session.game_mode_nonzero = true;
        for name in ["Americans", "Russians"] {
            let owner = sim.intern(name);
            let side = rules
                .country_side_index(name)
                .expect("stock country Side")
                .0;
            let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
            house.project_country_mults(rules, &sim.interner);
            sim.houses.insert(owner, house);
            sim.session.house_order.push(owner);
        }
        let owner = sim.interner.get("Americans").unwrap();
        sim.session.current_house = Some(owner);
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        let first = sim
            .spawn_object("GAWEAP", "Americans", 6, 6, 0, rules)
            .expect("first own retail War Factory");
        let second = sim
            .spawn_object("GAWEAP", "Americans", 20, 6, 0, rules)
            .expect("second own retail War Factory");
        sim.spawn_object("GAWEAP", "Russians", 20, 20, 0, rules)
            .expect("foreign owner's initial primary War Factory");
        let foreign = sim
            .spawn_object("GAWEAP", "Russians", 6, 20, 0, rules)
            .expect("foreign owner's nonprimary War Factory");
        let product = sim
            .spawn_object_limbo_at_height("MTNK", "Americans", 0, 0, 0, 0, rules)
            .expect("actual retail tank for the existing producer query");
        assert_eq!(
            crate::sim::production::object_find_factory(&sim, rules, product, false, false),
            Some(first),
            "automatic initialization must preserve the first primary; without one FindFactory returns the last eligible building"
        );
        let hover = |stable_id| HoverTargetKindWithId {
            kind: if stable_id == foreign {
                HoverTargetKind::EnemyStructure
            } else {
                HoverTargetKind::FriendlyStructure
            },
            stable_id,
        };
        let click = |selected: &[u64], target| {
            self_click_command(&sim, rules, owner, selected, Some(&hover(target)))
        };
        assert_eq!(click(&[first], first), None, "already primary is Action0");
        assert_eq!(click(&[first, second], second), None, "a group selects");
        assert_eq!(click(&[first], second), None, "not the selected object");
        assert_eq!(
            click(&[foreign], foreign),
            None,
            "not the player's building"
        );
        let command = click(&[second], second)
            .expect("native nonprimary GAWEAP self-click queues its target-bearing Event17");
        assert_eq!(command, Command::TogglePrimaryFactory { entity_id: second });
        assert_eq!(
            order_voice_key(&command),
            None,
            "Event17 has no input voice"
        );

        use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
        use crate::app::types::{CursorFeedbackKind, CursorId};

        let cursor = |selected: &[u64], target| {
            capability_cursor_for_hover(
                &sim,
                selected,
                selected.first().copied(),
                &hover(target),
                Some(rules),
                None,
            )
        };
        let nonprimary_cursor = cursor(&[second], second);
        assert_eq!(nonprimary_cursor, CursorFeedbackKind::Deploy);
        assert_eq!(
            cursor_id_for_feedback(nonprimary_cursor),
            Some(CursorId::Deploy),
            "native main-map Action4 uses the existing row27"
        );
        let primary_cursor = cursor(&[first], first);
        assert_eq!(primary_cursor, CursorFeedbackKind::Default);
        assert_eq!(
            cursor_id_for_feedback(primary_cursor),
            Some(CursorId::Default),
            "already-primary Action0 uses row0"
        );
        assert_eq!(
            cursor(&[first, second], second),
            CursorFeedbackKind::FriendlyStructure,
            "a group hover selects instead of choosing a primary"
        );
        assert_eq!(
            cursor(&[first], second),
            CursorFeedbackKind::FriendlyStructure,
            "the hovered factory must be the selected object"
        );
        assert_ne!(
            cursor(&[foreign], foreign),
            CursorFeedbackKind::Deploy,
            "a foreign nonprimary must not offer the local self action"
        );
    }

    #[test]
    fn a_loaded_bio_reactor_self_click_and_hover_cursor_offer_unload() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=YAPOWR\n[YAPOWR]\nStrength=1000\n\
                 InfantryAbsorb=yes\nPassengers=5\n",
            ))
            .expect("Bio Reactor input rules");
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.session.current_house = Some(owner);
        let reactor = sim
            .spawn_object_limbo_at_height("YAPOWR", "Americans", 10, 10, 0, 0, &rules)
            .expect("Bio Reactor builds");
        let mut cargo = crate::sim::passenger::PassengerCargo::new(5, 0);
        cargo.board(99, 1);
        sim.entities_mut().get_mut(reactor).unwrap().passenger_role =
            crate::sim::passenger::PassengerRole::transport(cargo);
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: reactor,
        };

        let command = self_click_command(&sim, &rules, owner, &[reactor], Some(&hover));
        let cursor = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[reactor],
            Some(reactor),
            &hover,
            Some(&rules),
            None,
        );

        assert_eq!(
            command,
            Some(Command::UnloadPassengers {
                transport_id: reactor
            })
        );
        assert_eq!(cursor, crate::app::types::CursorFeedbackKind::Deploy);
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
            crate::sim::movement::tube_movement::LowBridgeTubeMovementState::for_test(
                crate::map::tube_facts::TubeId(0),
                0,
                crate::sim::components::DriveCoord::cell(10, 10, 0),
            ),
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

    /// A deploying or deployed receiver takes no Move click on a Cell,
    /// whatever its locomotor, so no Event clears its target. Retail SCHP is a
    /// JumpJet IsSimpleDeployer Unit: over an open Cell, Unit7404B0 makes Move
    /// action0 while 6E0, 6E1 or 6E2 is set (740709..740727). A retail E1
    /// Deployer in Doing28 answers NoMove (51F8ED..51F94E), which its Cell
    /// click refuses (51F25D..51F285). Over an object their What_Action turns
    /// Move into NoMove only for 6E0 or Doing27..30 (`0x0074037A`,
    /// `0x0051EB81`), and the object receiver 4D74E0 still sends NoMove, as a
    /// Cell near the object. A non-player House orders neither: its Cell
    /// What_Action answers 0, and 4D74E0 asks IsControllable (`0x004D77FF`).
    #[test]
    fn retail_deployment_answers_move_clicks_through_the_class_what_action() {
        use crate::app::input::commands::{object_move_destination, ordinary_cell_move_goal};
        use crate::sim::world::MoveAction;
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &battle.rules;
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, rules);
        // Successful Unlimbo now publishes sight immediately; establish the
        // viewer-plane dimensions before creating any sources.
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        let chopper = sim
            .spawn_object("SCHP", "Americans", 10, 10, 0, rules)
            .expect("retail Siege Chopper spawns");
        let gi = sim
            .spawn_object("E1", "Americans", 12, 10, 0, rules)
            .expect("retail GI spawns");
        let tank = sim
            .spawn_object("MTNK", "Americans", 14, 10, 0, rules)
            .expect("retail tank spawns");
        let owner = sim.interner.get("Americans").unwrap();
        sim.fog.reveal_all_for_owner(owner);
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(owner);
        let goals = |sim: &Simulation, id| {
            (
                ordinary_cell_move_goal(sim, rules, None, owner, id, (14, 12), false),
                object_move_destination(sim, rules, owner, id, tank, MoveAction::Move, false),
            )
        };
        let moves = Some((14, 12));
        let onto_tank = Some(TargetKind::Entity(tank));
        assert_eq!(goals(&sim, chopper), (moves, onto_tank));
        assert_eq!(goals(&sim, gi), (moves, onto_tank));
        for (deployed, begin, reverse) in [
            (false, true, false),
            (true, false, false),
            (true, false, true),
        ] {
            sim.entities_mut()
                .get_mut(chopper)
                .unwrap()
                .set_unit_simple_deploy_for_test(deployed, begin, reverse);
            let (on_cell, over_object) = goals(&sim, chopper);
            let state = format!("6E0={deployed} 6E1={begin} 6E2={reverse}");
            assert_eq!(on_cell, None, "{state}");
            if deployed {
                assert!(
                    matches!(over_object, Some(TargetKind::Cell(..))),
                    "{state}: {over_object:?}"
                );
            } else {
                assert_eq!(over_object, onto_tank, "{state}");
            }
        }
        assert!(rules.object("E1").unwrap().deployer);
        sim.entities_mut()
            .get_mut(gi)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(28)
            .unwrap();
        let (on_cell, over_object) = goals(&sim, gi);
        assert_eq!(on_cell, None);
        assert!(
            matches!(over_object, Some(TargetKind::Cell(..))),
            "{over_object:?}"
        );
        sim.entities_mut()
            .get_mut(gi)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(0)
            .unwrap();
        sim.session.current_house = None;
        assert_eq!(goals(&sim, gi), (None, None));
    }

    /// The click side of the bomb actions: an Engineer's DisarmBomb and a
    /// Crazy Ivan's IvanBomb on a friend are Attack orders. An Ivan never
    /// attacks a bombed target, nor a
    /// friend his shot cannot hit (the rung's ILLEGAL test), where the cursor
    /// shows Select too.
    #[test]
    fn bomb_click_orders() {
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n0=IVAN\n1=ENGINEER\n[VehicleTypes]\n0=HTNK\n1=HVY\n\
                 [CombatDamage]\nIvanTimedDelay=450\n\
                 [IVAN]\nStrength=125\nIvan=yes\nAttackCursorOnFriendlies=yes\n\
                 Primary=IvanBomber\n\
                 [ENGINEER]\nStrength=75\nEngineer=yes\nBombSight=4\n\
                 [HTNK]\nStrength=900\n[HVY]\nStrength=900\nArmor=heavy\n\
                 [IvanBomber]\nRange=1.5\nProjectile=Invisible\nWarhead=IvanBomb\n\
                 [Invisible]\nInviso=yes\n\
                 [IvanBomb]\nIvanBomb=yes\n\
                 Verses=100%,100%,100%,100%,100%,0%,100%,100%,100%,100%,100%\n",
            ))
            .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        for house in ["Americans", "Soviets"] {
            let id = sim.interner.intern(house);
            sim.session.house_order.push(id);
        }
        let americans = sim.interner.get("Americans").unwrap();
        // The rung is a human player's (`0x007003ED`).
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(americans);
        let mut spawn = |kind: &str, owner: &str, rx: u16| {
            sim.spawn_object(kind, owner, rx, 5, 0, &rules).unwrap()
        };
        let ivan = spawn("IVAN", "Americans", 5);
        let engineer = spawn("ENGINEER", "Americans", 7);
        let own_tank = spawn("HTNK", "Americans", 9);
        let spare_tank = spawn("HTNK", "Americans", 11);
        let armoured = spawn("HVY", "Americans", 13);
        let enemy_ivan = spawn("IVAN", "Soviets", 20);
        let far_tank = spawn("HTNK", "Soviets", 22);
        let selected = [ivan, engineer];

        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), own_tank, None),
            vec![ivan]
        );
        // 0% Verses: ILLEGAL to the Ivan, so the friend stays a selection
        // click, as the cursor shows it.
        assert!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), armoured, None).is_empty()
        );
        {
            let cursor = |target| {
                crate::app::input::cursor::capability_cursor_for_hover(
                    &sim,
                    &[ivan],
                    Some(ivan),
                    &HoverTargetKindWithId {
                        kind: HoverTargetKind::FriendlyUnit,
                        stable_id: target,
                    },
                    Some(&rules),
                    None,
                )
            };
            assert_eq!(
                cursor(armoured),
                crate::app::types::CursorFeedbackKind::FriendlyUnit
            );
            assert_eq!(
                cursor(spare_tank),
                crate::app::types::CursorFeedbackKind::IvanBomb
            );
        }
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
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), own_tank, None).is_empty(),
            "a bombed friend stays a selection click"
        );
        assert_eq!(
            friendly_bomb_ivans(&sim, &rules, &selected, selected.len(), spare_tank, None),
            vec![ivan]
        );
        assert!(ivan_cannot_bomb(&sim, &rules, ivan, own_tank));
        assert!(!ivan_cannot_bomb(&sim, &rules, engineer, own_tank));

        // The Engineer carrying a bomb itself is no target of its own click.
        sim.bomb_attach(ivan, Some(engineer), &rules);
        sim.bomb_list_update(&rules);
        sim.bomb_list_update(&rules);
        assert!(disarm_bomb_engineers(&sim, &rules, &[engineer], engineer, americans).is_empty());
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
            &crate::sim::arena_fixture::clear_ground_ini(&format!(
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
        crate::sim::arena_fixture::supply_native_map(&mut sim);
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
        sim.entities_mut()
            .get_mut(hut)
            .unwrap()
            .health
            .set_for_test(150);
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
        use crate::sim::spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState};
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
                sim.entities_mut()
                    .get_mut(engineer)
                    .unwrap()
                    .bunker
                    .set_bunker_link_for_test(
                        crate::sim::docking::bunker_link::BunkerLink::Installed(hover.stable_id),
                    );
            }
            if gates["paralysis_start"].is_number() {
                sim.entities_mut()
                    .get_mut(engineer)
                    .unwrap()
                    .parasite
                    .set_paralysis_for_test(CdTimer::from_raw(
                        gates["paralysis_start"].as_i64().unwrap() as i32,
                        gates["paralysis_duration"].as_i64().unwrap() as i32,
                    ));
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
                            .set_in_limbo_for_test(native_slot["limbo"] == true);
                        Some(child)
                    } else {
                        None
                    };
                    slots.push(SpawnSlot::for_test(
                        child,
                        match native_slot["state"].as_i64().unwrap() {
                            0 => SpawnSlotState::ReadyDocked,
                            1 => SpawnSlotState::KamikazeWait,
                            2 => SpawnSlotState::InFlight,
                            other => panic!("unrepresented native slot state {other}"),
                        },
                        CdTimer::default(),
                        false,
                    ));
                }
                let spawn_type = sim.interner.intern("ENGINEER");
                sim.entities_mut().get_mut(engineer).unwrap().spawn_manager =
                    Some(SpawnManagerState::for_test(spawn_type, slots));
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

#[cfg(test)]
#[path = "context_order_retail_garrison_tests.rs"]
mod retail_garrison_tests;

#[cfg(test)]
#[path = "context_order_retail_ifv_repair_tests.rs"]
pub(in crate::app) mod retail_ifv_repair_tests;

#[cfg(test)]
mod stationary_attack_dispatch_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::rules::overlay_types::OverlayTypeRegistry;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::docking::bunker_link::BunkerLink;
    use crate::sim::world::Simulation;

    #[test]
    fn bunker_attack_batch_preserves_mobile_order_and_select_fallback() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            r#"
[VehicleTypes]
0=TANK
[TANK]
Strength=300
Primary=GUN
Speed=6
SpeedType=Track
MovementZone=Normal
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[GUN]
Damage=60
Range=5
Projectile=SHOT
Warhead=AP
[SHOT]
AG=yes
Inviso=yes
SubjectToElevation=no
[Warheads]
0=AP
[AP]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
"#,
        ))
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolve_type_handles(&rules);
        let stationary = sim
            .spawn_object("TANK", "Americans", 2, 2, 0, &rules)
            .unwrap();
        let mobile = sim
            .spawn_object("TANK", "Americans", 2, 4, 0, &rules)
            .unwrap();
        let friend = sim
            .spawn_object("TANK", "Americans", 20, 2, 0, &rules)
            .unwrap();
        let enemy = sim
            .spawn_object("TANK", "Soviets", 20, 4, 0, &rules)
            .unwrap();
        sim.session.game_mode_nonzero = true;
        sim.session.current_house = Some(sim.entities().get(stationary).unwrap().owner());
        sim.entities_mut()
            .get_mut(stationary)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(BunkerLink::Installed(99));
        sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| {
                    (0..32).map(move |x| crate::map::resolved_terrain::test_flat_cell(x, y))
                })
                .collect(),
        ));
        let registry = OverlayTypeRegistry::empty();
        let mixed = attack_branch_orders(
            &sim,
            &rules,
            &[stationary, mobile],
            TargetKind::Cell(20, 2),
            true,
            &registry,
        );
        assert!(mixed.consumed);
        assert!(!mixed.select_fallback);
        assert_eq!(mixed.receivers, [stationary, mobile].into_iter().collect());
        assert_eq!(mixed.commands.len(), 1);
        assert!(
            matches!(mixed.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==mobile)
        );
        let none = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(enemy),
            true,
            &registry,
        );
        assert!(none.commands.is_empty());
        assert!(
            none.consumed && !none.select_fallback,
            "Action0 consumes without selection or a hidden attack event"
        );
        let select = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Entity(friend),
            true,
            &registry,
        );
        assert!(select.commands.is_empty());
        assert!(
            !select.consumed && select.select_fallback,
            "Action7 leaves the click for ordinary selection"
        );
        let near = attack_branch_orders(
            &sim,
            &rules,
            &[stationary],
            TargetKind::Cell(4, 2),
            true,
            &registry,
        );
        assert_eq!(near.commands.len(), 1);
        assert!(
            matches!(near.commands[0], Command::ForceAttackCell { attacker_id, .. } if attacker_id==stationary)
        );
    }
}

#[cfg(test)]
#[path = "allied_object_action_tests.rs"]
mod allied_object_action_tests;
